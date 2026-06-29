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
use std::path::Path;

/// Socket file name under the coordination dir.
pub const SOCKET_NAME: &str = "concordd.sock";

/// Client side of the mediation protocol: connect to the daemon's Unix socket at
/// `sock_path`, send `req`, and return the parsed response. Returns `None` if the
/// socket is absent/unreachable or the daemon answered with an error — in all of which
/// the caller falls back to the Floor (direct FS). Shared by the CLI and the MCP server
/// so both get the same airtight Strong-tier path when the daemon is up.
///
/// Unix only — the daemon mediation uses a Unix-domain socket. Off Unix (Windows) there
/// is no daemon, so this returns `None` and every consequential op uses the enforced
/// Floor (FS-authoritative). A Windows named-pipe Strong tier is a backlog item.
#[cfg(unix)]
pub fn mediate(sock_path: &Path, req: &Request) -> Option<Response> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
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

/// Off Unix: no daemon mediation — always fall back to the Floor.
#[cfg(not(unix))]
pub fn mediate(_sock_path: &Path, _req: &Request) -> Option<Response> {
    None
}

/// A request from a client (CLI or MCP server) to the daemon — the consequential
/// operations the daemon mediates at its single serialization point (Strong tier).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Acquire the singleton merge lock for `id` (reason `why`, may be empty).
    MergeLock { id: String, why: String },
    /// Release the merge lock held by `id`.
    MergeUnlock { id: String },
    /// Claim a lease on `area` (the daemon applies the enforced overlap policy).
    Claim { id: String, area: String, why: String },
    /// Release the lease on `area`; with `fence`, only if the lease still carries it.
    Release {
        id: String,
        area: String,
        fence: Option<u64>,
    },
    /// Liveness probe.
    Ping,
}

/// A response from the daemon to a client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    // merge-lock
    Acquired { fence: u64 },
    Reacquired { fence: u64 },
    Held { holder: String },
    Released,
    NotHeld,
    NotYours { holder: String },
    // claim
    Claimed,
    AlreadyYours,
    Reclaimed { previous: String },
    ClaimConflict { holder: String },
    Overlap { area: String, holder: String },
    // release
    NoLease,
    FenceStale { current: u64 },
    // misc
    Pong,
    Err(String),
}

// The wire format is TAB-delimited (fields may contain spaces — an area like
// "merge #411" or a free-text `why`), one record per line. Internal IPC only, so the
// encoding is a private contract between the daemon and its clients.
const SEP: char = '\t';

impl Request {
    /// Serialize to a single tab-delimited line (no trailing newline).
    pub fn to_line(&self) -> String {
        match self {
            Request::MergeLock { id, why } => format!("MERGE-LOCK{SEP}{id}{SEP}{why}"),
            Request::MergeUnlock { id } => format!("MERGE-UNLOCK{SEP}{id}"),
            Request::Claim { id, area, why } => {
                format!("CLAIM{SEP}{id}{SEP}{area}{SEP}{why}")
            }
            Request::Release { id, area, fence } => {
                let f = fence.map(|n| n.to_string()).unwrap_or_default();
                format!("RELEASE{SEP}{id}{SEP}{area}{SEP}{f}")
            }
            Request::Ping => "PING".to_string(),
        }
    }

    /// Parse a request line, or `None` if malformed.
    pub fn parse_line(line: &str) -> Option<Request> {
        let f: Vec<&str> = line.trim_end_matches(['\n', '\r']).split(SEP).collect();
        match f[0] {
            "MERGE-LOCK" => nonempty(f.get(1)).map(|id| Request::MergeLock {
                id,
                why: f.get(2).unwrap_or(&"").to_string(),
            }),
            "MERGE-UNLOCK" => nonempty(f.get(1)).map(|id| Request::MergeUnlock { id }),
            "CLAIM" => {
                let id = nonempty(f.get(1))?;
                let area = nonempty(f.get(2))?;
                Some(Request::Claim {
                    id,
                    area,
                    why: f.get(3).unwrap_or(&"").to_string(),
                })
            }
            "RELEASE" => {
                let id = nonempty(f.get(1))?;
                let area = nonempty(f.get(2))?;
                let fence = f.get(3).and_then(|s| s.parse::<u64>().ok());
                Some(Request::Release { id, area, fence })
            }
            "PING" => Some(Request::Ping),
            _ => None,
        }
    }
}

impl Response {
    pub fn to_line(&self) -> String {
        match self {
            Response::Acquired { fence } => format!("ACQUIRED{SEP}{fence}"),
            Response::Reacquired { fence } => format!("REACQUIRED{SEP}{fence}"),
            Response::Held { holder } => format!("HELD{SEP}{holder}"),
            Response::Released => "RELEASED".to_string(),
            Response::NotHeld => "NOTHELD".to_string(),
            Response::NotYours { holder } => format!("NOTYOURS{SEP}{holder}"),
            Response::Claimed => "CLAIMED".to_string(),
            Response::AlreadyYours => "ALREADY-YOURS".to_string(),
            Response::Reclaimed { previous } => format!("RECLAIMED{SEP}{previous}"),
            Response::ClaimConflict { holder } => format!("CLAIM-CONFLICT{SEP}{holder}"),
            Response::Overlap { area, holder } => format!("OVERLAP{SEP}{area}{SEP}{holder}"),
            Response::NoLease => "NO-LEASE".to_string(),
            Response::FenceStale { current } => format!("FENCE-STALE{SEP}{current}"),
            Response::Pong => "PONG".to_string(),
            Response::Err(m) => format!("ERR{SEP}{m}"),
        }
    }

    pub fn parse_line(line: &str) -> Option<Response> {
        let f: Vec<&str> = line.trim_end_matches(['\n', '\r']).split(SEP).collect();
        match f[0] {
            "ACQUIRED" => f.get(1)?.parse().ok().map(|fence| Response::Acquired { fence }),
            "REACQUIRED" => f.get(1)?.parse().ok().map(|fence| Response::Reacquired { fence }),
            "HELD" => Some(Response::Held {
                holder: f.get(1).unwrap_or(&"").to_string(),
            }),
            "RELEASED" => Some(Response::Released),
            "NOTHELD" => Some(Response::NotHeld),
            "NOTYOURS" => Some(Response::NotYours {
                holder: f.get(1).unwrap_or(&"").to_string(),
            }),
            "CLAIMED" => Some(Response::Claimed),
            "ALREADY-YOURS" => Some(Response::AlreadyYours),
            "RECLAIMED" => Some(Response::Reclaimed {
                previous: f.get(1).unwrap_or(&"").to_string(),
            }),
            "CLAIM-CONFLICT" => Some(Response::ClaimConflict {
                holder: f.get(1).unwrap_or(&"").to_string(),
            }),
            "OVERLAP" => Some(Response::Overlap {
                area: f.get(1).unwrap_or(&"").to_string(),
                holder: f.get(2).unwrap_or(&"").to_string(),
            }),
            "NO-LEASE" => Some(Response::NoLease),
            "FENCE-STALE" => f.get(1)?.parse().ok().map(|current| Response::FenceStale { current }),
            "PONG" => Some(Response::Pong),
            "ERR" => Some(Response::Err(f.get(1).unwrap_or(&"").to_string())),
            _ => None,
        }
    }
}

impl fmt::Display for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_line())
    }
}

/// `Some(owned)` if the optional field is present and non-empty, else `None`.
fn nonempty(s: Option<&&str>) -> Option<String> {
    match s {
        Some(v) if !v.is_empty() => Some(v.to_string()),
        _ => None,
    }
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
        // area + why both contain spaces — tab-delimited keeps them intact.
        req_roundtrip(Request::Claim {
            id: "a".into(),
            area: "merge #411 area".into(),
            why: "why with spaces".into(),
        });
        req_roundtrip(Request::Release {
            id: "a".into(),
            area: "kernel/src/main.rs".into(),
            fence: Some(7),
        });
        req_roundtrip(Request::Release {
            id: "a".into(),
            area: "x/y".into(),
            fence: None,
        });
        req_roundtrip(Request::Ping);
    }

    #[test]
    fn responses_roundtrip() {
        for r in [
            Response::Acquired { fence: 42 },
            Response::Reacquired { fence: 7 },
            Response::Held { holder: "hub".into() },
            Response::Released,
            Response::NotHeld,
            Response::NotYours { holder: "a".into() },
            Response::Claimed,
            Response::AlreadyYours,
            Response::Reclaimed { previous: "b".into() },
            Response::ClaimConflict { holder: "b".into() },
            Response::Overlap {
                area: "kernel/src/embedded".into(),
                holder: "a".into(),
            },
            Response::NoLease,
            Response::FenceStale { current: 9 },
            Response::Pong,
            Response::Err("bad thing".into()),
        ] {
            resp_roundtrip(r);
        }
    }

    #[test]
    fn merge_lock_empty_why_is_ok() {
        let r = Request::parse_line("MERGE-LOCK\thub").unwrap();
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
        assert!(Request::parse_line("CLAIM\ta").is_none()); // no area
        assert!(Request::parse_line("BOGUS\tx").is_none());
        assert!(Request::parse_line("").is_none());
    }
}
