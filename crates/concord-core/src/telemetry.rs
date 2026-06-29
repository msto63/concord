//! F4 telemetry types + the health heuristic (kept in `concord-core` so it is testable
//! and dependency-free).
//!
//! The daemon's OTLP/HTTP-JSON receiver normalizes each Claude Code metric datapoint into
//! a [`TelemetryPoint`] (a tiny tab-delimited record) appended to `<coord>/telemetry/
//! <id>.jsonl`. The health verdict ([`SessionHealth`]) is then computed from those points
//! against the `[telemetry]` thresholds — making `hub` telemetry-*driven* rather than
//! prose-reading. Privacy: only metric attributes are ingested, never prompt content.

use crate::config::TelemetryConfig;

/// One normalized telemetry datapoint. `metric` is a small Concord tag (not the raw OTLP
/// name): `token` (value = tokens), `reject` (an edit-tool reject/deny, value = 1),
/// `commit` / `lines` (progress, value = count), or `activity` (any other datapoint —
/// counts only toward "not idle"). `attr` is a compact label (e.g. the token type), or "".
#[derive(Debug, Clone, PartialEq)]
pub struct TelemetryPoint {
    pub ts: u64,
    pub metric: String,
    pub value: f64,
    pub attr: String,
}

impl TelemetryPoint {
    pub fn to_line(&self) -> String {
        // Tab-delimited; metric/attr are Concord-controlled tags with no tabs.
        format!("{}\t{}\t{}\t{}", self.ts, self.metric, self.value, self.attr)
    }
    pub fn parse_line(line: &str) -> Option<TelemetryPoint> {
        let f: Vec<&str> = line.trim_end().split('\t').collect();
        if f.len() < 4 {
            return None;
        }
        Some(TelemetryPoint {
            ts: f[0].parse().ok()?,
            metric: f[1].to_string(),
            value: f[2].parse().ok()?,
            attr: f[3].to_string(),
        })
    }
}

/// A session's health verdict (F4 / B3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthFlag {
    /// Healthy (recent activity, progressing, no reject storm / runaway burn).
    Ok,
    /// No telemetry datapoint within `idle_min` — possibly going dark.
    Idle,
    /// Token-usage rate over the window exceeds `burn_warn`.
    Burn,
    /// `reject_storm`+ edit-tool reject/deny decisions in the window.
    Reject,
    /// Sustained tool activity with zero commit/line progress — likely looping.
    Loop,
}

impl HealthFlag {
    pub fn as_str(self) -> &'static str {
        match self {
            HealthFlag::Ok => "OK",
            HealthFlag::Idle => "IDLE",
            HealthFlag::Burn => "BURN",
            HealthFlag::Reject => "REJECT",
            HealthFlag::Loop => "LOOP",
        }
    }
}

/// The computed health of one session over the look-back window.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionHealth {
    pub id: String,
    /// Seconds since the last telemetry datapoint (`u64::MAX` if none).
    pub idle_secs: u64,
    /// Token-usage rate (tokens/min) over the window.
    pub burn_per_min: u64,
    /// Edit-tool reject/deny decisions in the window.
    pub reject_count: u32,
    /// Commits + lines changed in the window (progress signal).
    pub commits: u64,
    pub lines: u64,
    pub flag: HealthFlag,
}

/// Compute a session's health from its telemetry `points` against `cfg`, as of `now`.
/// Pure — the store reads the points and calls this; tests drive it directly.
pub fn health(id: &str, points: &[TelemetryPoint], cfg: &TelemetryConfig, now: u64) -> SessionHealth {
    let window_start = now.saturating_sub(cfg.loop_window);
    let last_ts = points.iter().map(|p| p.ts).max();
    let idle_secs = last_ts.map(|t| now.saturating_sub(t)).unwrap_or(u64::MAX);

    let mut tokens = 0.0f64;
    let mut reject_count = 0u32;
    let mut commits = 0u64;
    let mut lines = 0u64;
    let mut activity = 0u32;
    for p in points.iter().filter(|p| p.ts >= window_start) {
        activity += 1;
        match p.metric.as_str() {
            "token" => tokens += p.value,
            "reject" => reject_count += p.value as u32,
            "commit" => commits += p.value as u64,
            "lines" => lines += p.value as u64,
            _ => {}
        }
    }
    let minutes = (cfg.loop_window as f64 / 60.0).max(1.0);
    let burn_per_min = (tokens / minutes) as u64;

    // Precedence: idle (dark) dominates; then a reject storm; then runaway burn; then a
    // loop (sustained activity, no progress). A session can only carry one flag.
    let flag = if idle_secs > cfg.idle_min * 60 {
        HealthFlag::Idle
    } else if reject_count >= cfg.reject_storm {
        HealthFlag::Reject
    } else if burn_per_min > cfg.burn_warn {
        HealthFlag::Burn
    } else if activity >= 6 && commits == 0 && lines == 0 {
        HealthFlag::Loop
    } else {
        HealthFlag::Ok
    };

    SessionHealth { id: id.to_string(), idle_secs, burn_per_min, reject_count, commits, lines, flag }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(ts: u64, metric: &str, value: f64) -> TelemetryPoint {
        TelemetryPoint { ts, metric: metric.into(), value, attr: String::new() }
    }

    #[test]
    fn point_roundtrip() {
        let p = TelemetryPoint { ts: 10, metric: "token".into(), value: 1234.0, attr: "output".into() };
        assert_eq!(TelemetryPoint::parse_line(&p.to_line()), Some(p));
    }

    #[test]
    fn flags_by_precedence() {
        let cfg = TelemetryConfig::default(); // idle_min=15, burn_warn=20000, reject_storm=5, loop_window=600
        let now = 100_000;

        // Idle: last point older than idle_min (15 min).
        let h = health("a", &[pt(now - 16 * 60, "token", 100.0)], &cfg, now);
        assert_eq!(h.flag, HealthFlag::Idle);

        // Reject storm: 5 rejects in the window.
        let rejects: Vec<_> = (0..5).map(|i| pt(now - 10 - i, "reject", 1.0)).collect();
        assert_eq!(health("a", &rejects, &cfg, now).flag, HealthFlag::Reject);

        // Burn: > burn_warn (20000) tokens/min over the 10-min window ⇒ need > 200000.
        let h = health("a", &[pt(now - 5, "token", 250_000.0)], &cfg, now);
        assert_eq!(h.flag, HealthFlag::Burn);

        // Loop: sustained activity, zero commit/line progress.
        let busy: Vec<_> = (0..8).map(|i| pt(now - 10 - i, "activity", 1.0)).collect();
        assert_eq!(health("a", &busy, &cfg, now).flag, HealthFlag::Loop);

        // Ok: recent activity WITH progress.
        let mut good = vec![pt(now - 5, "token", 100.0)];
        good.push(pt(now - 5, "commit", 1.0));
        assert_eq!(health("a", &good, &cfg, now).flag, HealthFlag::Ok);
    }
}
