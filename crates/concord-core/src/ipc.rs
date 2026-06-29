//! The tiny line protocol between the `concord` CLI and the `concordd` daemon (M2.3).
//!
//! When the daemon is up it is the **single serialization point** for mediated
//! consequential writes (merge-lock / merge-unlock): the CLI sends a one-line request
//! over a Unix socket, the daemon's single handler thread does the check-and-apply
//! atomically, and replies with a one-line response. That closes the Floor's residual
//! TOCTOU window (check-then-commit is one step inside the daemon). When the daemon is
//! down, the CLI falls back to the Floor (direct FS, [`crate::store`]).
//!
//! The wire format is deliberately trivial newline-delimited text — no serde, no
//! dependency — so both crates share it and it is greppable on the wire. Fields are
//! space-separated; the trailing free-text field (a lock reason) may contain spaces
//! and is taken verbatim to end-of-line.

use std::fmt;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;

/// Socket file name under the coordination dir.
pub const SOCKET_NAME: &str = "concordd.sock";

/// Client side of the mediation protocol: connect to the daemon's socket at
/// `sock_path`, send `req`, and return the parsed response. Returns `None` if the
/// socket is absent/unreachable or the daemon answered with an error — in all of
/// which the caller falls back to the Floor (direct FS). Shared by the CLI and the
/// MCP server so both get the same airtight Strong-tier path when the daemon is up.
pub fn mediate(sock_path: &Path, req: &Request) -> Option<Response> {
    if !sock_path.exists() {
        return None;
    }
    let mut stream = UnixStream::connect(sock_path).ok()?;
    writeln!(stream, "{}", req.to_line()).ok()?;
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line).ok()?;
    match Response::parse_line(&line) {
        Some(Response::Err(_)) | None => None, // daemon hiccup ⇒ fall back to Floor
        other => other,
    }
}

/// A request from the CLI to the daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Acquire the singleton merge lock for `id` (reason `why`, may be empty).
    MergeLock { id: String, why: String },
    /// Release the merge lock held by `id`.
    MergeUnlock { id: String },
    /// Liveness probe.
    Ping,
}

/// A response from the daemon to the CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    Acquired { fence: u64 },
    Reacquired { fence: u64 },
    Held { holder: String },
    Released,
    NotHeld,
    NotYours { holder: String },
    Pong,
    Err(String),
}

impl Request {
    /// Serialize to a single line (no trailing newline).
    pub fn to_line(&self) -> String {
        match self {
            Request::MergeLock { id, why } => format!("MERGE-LOCK {id} {why}"),
            Request::MergeUnlock { id } => format!("MERGE-UNLOCK {id}"),
            Request::Ping => "PING".to_string(),
        }
    }

    /// Parse a request line, or `None` if malformed.
    pub fn parse_line(line: &str) -> Option<Request> {
        let line = line.trim_end_matches(['\n', '\r']);
        let (verb, rest) = split_first(line);
        match verb {
            "MERGE-LOCK" => {
                let (id, why) = split_first(rest);
                if id.is_empty() {
                    return None;
                }
                Some(Request::MergeLock {
                    id: id.to_string(),
                    why: why.to_string(),
                })
            }
            "MERGE-UNLOCK" => {
                let (id, _) = split_first(rest);
                if id.is_empty() {
                    return None;
                }
                Some(Request::MergeUnlock { id: id.to_string() })
            }
            "PING" => Some(Request::Ping),
            _ => None,
        }
    }
}

impl Response {
    pub fn to_line(&self) -> String {
        match self {
            Response::Acquired { fence } => format!("ACQUIRED fence={fence}"),
            Response::Reacquired { fence } => format!("REACQUIRED fence={fence}"),
            Response::Held { holder } => format!("HELD {holder}"),
            Response::Released => "RELEASED".to_string(),
            Response::NotHeld => "NOTHELD".to_string(),
            Response::NotYours { holder } => format!("NOTYOURS {holder}"),
            Response::Pong => "PONG".to_string(),
            Response::Err(m) => format!("ERR {m}"),
        }
    }

    pub fn parse_line(line: &str) -> Option<Response> {
        let line = line.trim_end_matches(['\n', '\r']);
        let (verb, rest) = split_first(line);
        match verb {
            "ACQUIRED" => parse_fence(rest).map(|fence| Response::Acquired { fence }),
            "REACQUIRED" => parse_fence(rest).map(|fence| Response::Reacquired { fence }),
            "HELD" => Some(Response::Held {
                holder: rest.to_string(),
            }),
            "RELEASED" => Some(Response::Released),
            "NOTHELD" => Some(Response::NotHeld),
            "NOTYOURS" => Some(Response::NotYours {
                holder: rest.to_string(),
            }),
            "PONG" => Some(Response::Pong),
            "ERR" => Some(Response::Err(rest.to_string())),
            _ => None,
        }
    }
}

impl fmt::Display for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_line())
    }
}

/// Split a string into (first whitespace-delimited token, remainder-after-one-space).
fn split_first(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    match s.find(' ') {
        Some(i) => (&s[..i], s[i + 1..].trim_start()),
        None => (s, ""),
    }
}

/// Parse a `fence=<n>` token.
fn parse_fence(s: &str) -> Option<u64> {
    s.trim().strip_prefix("fence=")?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req_roundtrip(r: Request) {
        assert_eq!(Request::parse_line(&r.to_line()), Some(r));
    }
    fn resp_roundtrip(r: Response) {
        assert_eq!(Response::parse_line(&r.to_line()), Some(r));
    }

    #[test]
    fn requests_roundtrip() {
        req_roundtrip(Request::MergeLock {
            id: "hub".into(),
            why: "merge #1 with spaces".into(),
        });
        req_roundtrip(Request::MergeUnlock { id: "a".into() });
        req_roundtrip(Request::Ping);
    }

    #[test]
    fn responses_roundtrip() {
        resp_roundtrip(Response::Acquired { fence: 42 });
        resp_roundtrip(Response::Reacquired { fence: 7 });
        resp_roundtrip(Response::Held { holder: "hub".into() });
        resp_roundtrip(Response::Released);
        resp_roundtrip(Response::NotHeld);
        resp_roundtrip(Response::NotYours { holder: "a".into() });
        resp_roundtrip(Response::Pong);
        resp_roundtrip(Response::Err("bad thing".into()));
    }

    #[test]
    fn merge_lock_empty_why_is_ok() {
        let r = Request::parse_line("MERGE-LOCK hub").unwrap();
        assert_eq!(
            r,
            Request::MergeLock {
                id: "hub".into(),
                why: String::new()
            }
        );
    }

    #[test]
    fn malformed_requests_rejected() {
        assert!(Request::parse_line("MERGE-LOCK").is_none()); // no id
        assert!(Request::parse_line("BOGUS x").is_none());
        assert!(Request::parse_line("").is_none());
    }
}
