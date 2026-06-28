//! The typed coordination state model and its byte-exact on-disk representation.
//!
//! Every (de)serialization here is pure (string in / string out) so it can be unit-
//! tested without touching the filesystem; [`crate::store`] does the actual I/O.
//!
//! Parity is the contract: the strings produced must be byte-identical to what
//! `bin/coord.sh` writes for the same inputs (modulo wall-clock timestamps).

/// A registered session. `started`/`heartbeat` are kept as the raw on-disk strings
/// so a `heartbeat` rewrite preserves `focus`/`started` verbatim, exactly as the
/// shell does (it re-`sed`s those two fields and only replaces `heartbeat`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub focus: String,
    pub started: String,
    pub heartbeat: String,
}

impl Session {
    /// A freshly-registered session: `started == heartbeat == now`.
    pub fn fresh(id: &str, focus: &str, now: u64) -> Session {
        Session {
            id: id.to_string(),
            focus: focus.to_string(),
            started: now.to_string(),
            heartbeat: now.to_string(),
        }
    }

    /// Parse a session-file body. Mirrors the shell's `sed -n 's/^FIELD=//p'`:
    /// for each line, the first `KEY=` prefix wins; the value is the rest of the
    /// line verbatim. Unknown lines are ignored; missing fields become empty.
    pub fn parse(id: &str, body: &str) -> Session {
        let mut focus = String::new();
        let mut started = String::new();
        let mut heartbeat = String::new();
        for line in body.lines() {
            if let Some(v) = line.strip_prefix("focus=") {
                if focus.is_empty() {
                    focus = v.to_string();
                }
            } else if let Some(v) = line.strip_prefix("started=") {
                if started.is_empty() {
                    started = v.to_string();
                }
            } else if let Some(v) = line.strip_prefix("heartbeat=") {
                if heartbeat.is_empty() {
                    heartbeat = v.to_string();
                }
            }
        }
        Session {
            id: id.to_string(),
            focus,
            started,
            heartbeat,
        }
    }

    /// Serialize to the exact 3-line on-disk body
    /// (`printf 'focus=%s\nstarted=%s\nheartbeat=%s\n'`).
    pub fn to_body(&self) -> String {
        format!(
            "focus={}\nstarted={}\nheartbeat={}\n",
            self.focus, self.started, self.heartbeat
        )
    }

    /// The parsed heartbeat seconds, or `None` if absent/empty/non-numeric — any of
    /// which the shell treats as stale.
    pub fn heartbeat_secs(&self) -> Option<u64> {
        let t = self.heartbeat.trim();
        if t.is_empty() {
            None
        } else {
            t.parse::<u64>().ok()
        }
    }

    /// Is this session stale at `now` given `ttl`? `None` heartbeat ⇒ always stale,
    /// else `now - hb > ttl`. Matches `session_stale()`.
    pub fn is_stale(&self, now: u64, ttl: u64) -> bool {
        match self.heartbeat_secs() {
            None => true,
            Some(hb) => now.saturating_sub(hb) > ttl,
        }
    }
}

/// A held lease over an area. The on-disk form is a directory whose mere existence
/// is the lock (atomic `mkdir`), containing `holder`/`since`/`why` files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lease {
    /// The slugged area (on-disk directory name).
    pub area_slug: String,
    pub holder: String,
    pub since: String,
    pub why: String,
}

/// The singleton merge gate. On disk: `merge.lock/` dir with `holder`/`since`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeLock {
    pub holder: String,
    pub since: String,
}

/// One append-only ledger record in `intents.jsonl`.
///
/// **Parity quirk preserved:** the shell builds the JSON with a bare `printf` and
/// appends a trailing space inside the event field (`"event":"<text> "`). We
/// reproduce that byte-for-byte in [`LedgerEntry::to_jsonl`] — the differential
/// harness compares these lines, so the space must stay. Escaping of `"`/`\` is
/// deliberately NOT done here (the shell does not escape either); hardening that is
/// a tracked M1.x follow-up, kept out of the parity default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerEntry {
    pub t: u64,
    pub session: String,
    /// The event text WITHOUT the trailing space (the writer adds it).
    pub event: String,
}

impl LedgerEntry {
    pub fn new(t: u64, session: &str, event: &str) -> LedgerEntry {
        LedgerEntry {
            t,
            session: session.to_string(),
            event: event.to_string(),
        }
    }

    /// The exact JSONL line, trailing `\n` included, byte-equal to the shell's
    /// `printf '{"t":%s,"session":"%s","event":"%s"}\n' "$t" "$id" "$event "`.
    pub fn to_jsonl(&self) -> String {
        format!(
            "{{\"t\":{},\"session\":\"{}\",\"event\":\"{} \"}}\n",
            self.t, self.session, self.event
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_roundtrips_through_disk_form() {
        let body = "focus=B15.3 baseline\nstarted=1782685936\nheartbeat=1782686539\n";
        let s = Session::parse("a", body);
        assert_eq!(s.focus, "B15.3 baseline");
        assert_eq!(s.started, "1782685936");
        assert_eq!(s.heartbeat, "1782686539");
        assert_eq!(s.to_body(), body);
    }

    #[test]
    fn session_preserves_started_and_focus_on_rewrite() {
        let s = Session::parse(
            "a",
            "focus=(session start)\nstarted=100\nheartbeat=200\n",
        );
        let beat = Session {
            heartbeat: "999".to_string(),
            ..s
        };
        assert_eq!(
            beat.to_body(),
            "focus=(session start)\nstarted=100\nheartbeat=999\n"
        );
    }

    #[test]
    fn staleness_follows_ttl_window() {
        let s = Session::fresh("a", "", 1000);
        assert!(!s.is_stale(1000 + 1800, 1800)); // exactly TTL ⇒ not yet stale (> only)
        assert!(s.is_stale(1000 + 1801, 1800));
    }

    #[test]
    fn empty_or_missing_heartbeat_is_stale() {
        assert!(Session::parse("a", "focus=x\nstarted=1\n").is_stale(5, 1800));
        assert!(Session::parse("a", "focus=x\nstarted=1\nheartbeat=\n").is_stale(5, 1800));
    }

    #[test]
    fn ledger_line_keeps_trailing_space_quirk() {
        let e = LedgerEntry::new(1782686531, "D", "release: x86-auditd-spawn");
        assert_eq!(
            e.to_jsonl(),
            "{\"t\":1782686531,\"session\":\"D\",\"event\":\"release: x86-auditd-spawn \"}\n"
        );
    }
}
