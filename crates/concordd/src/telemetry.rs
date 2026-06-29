//! F4 telemetry receiver — a minimal OTLP/HTTP-JSON sink (Unix-only, like the rest of the
//! daemon). Claude Code, launched with `OTEL_EXPORTER_OTLP_PROTOCOL=http/json`, POSTs
//! metric batches to `http://127.0.0.1:<port>/v1/metrics`; we parse the JSON (serde_json,
//! no protobuf / no otel-collector dependency — Policy 3: native, not a SaaS dep),
//! normalize each datapoint to a `concord_core::telemetry::TelemetryPoint`, and append it
//! to `<coord>/telemetry/<concord.id>.jsonl`. Privacy: only metric *attributes* are read —
//! never prompt content (`OTEL_LOG_USER_PROMPTS` is deliberately not set).

use concord_core::telemetry::TelemetryPoint;
use serde_json::Value;

/// Map an OTLP metric batch (already JSON-parsed) to `(concord_id, point)` pairs. Pure +
/// testable; the receiver does the I/O. Datapoints whose resource has no `concord.id`
/// resource attribute are skipped (we only track Concord-launched sessions).
pub fn otlp_to_points(v: &Value) -> Vec<(String, TelemetryPoint)> {
    let mut out = Vec::new();
    let Some(rms) = v.get("resourceMetrics").and_then(Value::as_array) else {
        return out;
    };
    for rm in rms {
        let id = resource_attr(rm, "concord.id");
        let Some(id) = id else { continue };
        for sm in rm.get("scopeMetrics").and_then(Value::as_array).into_iter().flatten() {
            for m in sm.get("metrics").and_then(Value::as_array).into_iter().flatten() {
                let name = m.get("name").and_then(Value::as_str).unwrap_or("");
                // dataPoints live under sum (counters) or gauge.
                let dps = m
                    .get("sum")
                    .or_else(|| m.get("gauge"))
                    .and_then(|s| s.get("dataPoints"))
                    .and_then(Value::as_array);
                for dp in dps.into_iter().flatten() {
                    if let Some(p) = datapoint_to_point(name, dp) {
                        out.push((id.clone(), p));
                    }
                }
            }
        }
    }
    out
}

/// A resource attribute's string value (e.g. `concord.id`), if present.
fn resource_attr(rm: &Value, key: &str) -> Option<String> {
    let attrs = rm.get("resource")?.get("attributes")?.as_array()?;
    attr_str(attrs, key)
}

/// Find string attribute `key` in an OTLP attribute array.
fn attr_str(attrs: &[Value], key: &str) -> Option<String> {
    attrs.iter().find_map(|a| {
        if a.get("key").and_then(Value::as_str) == Some(key) {
            a.get("value")?.get("stringValue")?.as_str().map(String::from)
        } else {
            None
        }
    })
}

/// Normalize one OTLP datapoint of metric `name` into a Concord [`TelemetryPoint`].
fn datapoint_to_point(name: &str, dp: &Value) -> Option<TelemetryPoint> {
    let ts_ns: u64 = dp
        .get("timeUnixNano")
        .and_then(|t| t.as_str().and_then(|s| s.parse().ok()).or_else(|| t.as_u64()))
        .unwrap_or(0);
    let ts = ts_ns / 1_000_000_000;
    let value = dp
        .get("asInt")
        .and_then(|x| x.as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| x.as_f64()))
        .or_else(|| dp.get("asDouble").and_then(Value::as_f64))
        .unwrap_or(0.0);
    let empty = Vec::new();
    let attrs = dp.get("attributes").and_then(Value::as_array).unwrap_or(&empty);

    // Map the raw OTLP metric name → a small Concord tag (see telemetry::TelemetryPoint).
    let (metric, val, attr) = match name {
        "claude_code.token.usage" => {
            ("token", value, attr_str(attrs, "type").unwrap_or_default())
        }
        "claude_code.commit.count" => ("commit", value, String::new()),
        "claude_code.lines_of_code.count" => {
            ("lines", value, attr_str(attrs, "type").unwrap_or_default())
        }
        "claude_code.code_edit_tool.decision" => {
            let decision = attr_str(attrs, "decision").unwrap_or_default();
            if decision == "reject" || decision == "deny" {
                ("reject", 1.0, attr_str(attrs, "tool_name").unwrap_or_default())
            } else {
                ("activity", 1.0, String::new())
            }
        }
        _ => ("activity", 1.0, String::new()),
    };
    Some(TelemetryPoint { ts, metric: metric.to_string(), value: val, attr })
}

#[cfg(unix)]
pub fn run_receiver(paths: &concord_core::Paths, port: u16) {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[concordd] telemetry: cannot bind 127.0.0.1:{port} ({e}) — telemetry off");
            return;
        }
    };
    eprintln!("[concordd] telemetry: OTLP/HTTP-JSON receiver on 127.0.0.1:{port}");
    for conn in listener.incoming() {
        let Ok(mut stream) = conn else { continue };
        // Read the full request (headers + body) up to Content-Length.
        let mut buf = Vec::new();
        let mut tmp = [0u8; 8192];
        let body = loop {
            match stream.read(&mut tmp) {
                Ok(0) => break extract_body(&buf),
                Ok(n) => {
                    buf.extend_from_slice(&tmp[..n]);
                    if let Some(b) = complete_body(&buf) {
                        break Some(b);
                    }
                }
                Err(_) => break None,
            }
        };
        if let Some(body) = body {
            if let Ok(v) = serde_json::from_slice::<Value>(&body) {
                let store = concord_core::Store::open(paths.clone());
                if let Ok(store) = store {
                    for (id, point) in otlp_to_points(&v) {
                        let _ = store.record_telemetry(&id, &point);
                    }
                }
            }
        }
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
    }
}

/// If the buffer holds full headers + a Content-Length body, return the body bytes.
fn complete_body(buf: &[u8]) -> Option<Vec<u8>> {
    let sep = find_subslice(buf, b"\r\n\r\n")?;
    let headers = &buf[..sep];
    let body_start = sep + 4;
    let len = content_length(headers)?;
    if buf.len() - body_start >= len {
        Some(buf[body_start..body_start + len].to_vec())
    } else {
        None
    }
}

/// Best-effort body extraction when the peer closed the connection (no Content-Length).
fn extract_body(buf: &[u8]) -> Option<Vec<u8>> {
    let sep = find_subslice(buf, b"\r\n\r\n")?;
    Some(buf[sep + 4..].to_vec())
}

fn content_length(headers: &[u8]) -> Option<usize> {
    let s = String::from_utf8_lossy(headers);
    for line in s.lines() {
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            return v.trim().parse().ok();
        }
    }
    None
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_otlp_token_and_reject() {
        let v: Value = serde_json::from_str(
            r#"{"resourceMetrics":[{
              "resource":{"attributes":[{"key":"concord.id","value":{"stringValue":"w"}}]},
              "scopeMetrics":[{"metrics":[
                {"name":"claude_code.token.usage","sum":{"dataPoints":[
                  {"attributes":[{"key":"type","value":{"stringValue":"output"}}],"timeUnixNano":"1700000000000000000","asInt":"512"}]}},
                {"name":"claude_code.code_edit_tool.decision","sum":{"dataPoints":[
                  {"attributes":[{"key":"decision","value":{"stringValue":"reject"}},{"key":"tool_name","value":{"stringValue":"Edit"}}],"timeUnixNano":"1700000001000000000","asInt":"1"}]}}
              ]}]
            }]}"#,
        )
        .unwrap();
        let pts = otlp_to_points(&v);
        assert_eq!(pts.len(), 2);
        assert_eq!(pts[0].0, "w");
        assert_eq!(pts[0].1.metric, "token");
        assert_eq!(pts[0].1.value, 512.0);
        assert_eq!(pts[0].1.attr, "output");
        assert_eq!(pts[0].1.ts, 1_700_000_000);
        assert_eq!(pts[1].1.metric, "reject");
        assert_eq!(pts[1].1.attr, "Edit");
    }

    #[test]
    fn skips_when_no_concord_id() {
        let v: Value = serde_json::from_str(
            r#"{"resourceMetrics":[{"resource":{"attributes":[]},"scopeMetrics":[{"metrics":[
               {"name":"claude_code.token.usage","sum":{"dataPoints":[{"timeUnixNano":"1000000000","asInt":"1"}]}}]}]}]}"#,
        )
        .unwrap();
        assert!(otlp_to_points(&v).is_empty());
    }

    #[test]
    fn content_length_body_framing() {
        let req = b"POST /v1/metrics HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello";
        assert_eq!(complete_body(req), Some(b"hello".to_vec()));
        let partial = b"POST / HTTP/1.1\r\nContent-Length: 5\r\n\r\nhel";
        assert_eq!(complete_body(partial), None);
    }
}
