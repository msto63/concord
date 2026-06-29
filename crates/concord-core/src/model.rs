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
/// is the lock (atomic `mkdir`), containing `holder`/`since`/`why` files — and, in
/// the Rust port, two additive files the shell ignores but that fix real bugs:
///  - `area`  — the ORIGINAL (un-slugged) area string. The shell stores only the
///    lossy slug (`tr '/ ' '__'`), which conflates `a/b` with `a b`; keeping the
///    original lets overlap/identity checks reason about the true area.
///  - `fence` — a monotonic fencing token (WP12 research §1). Recorded in M1
///    (design-for); enforcement lands in M2 with the daemon + stale-reclaim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lease {
    /// The slugged area (on-disk directory name).
    pub area_slug: String,
    /// The original area string (from the `area` file, or de-slugged for a
    /// shell-created lease that has none).
    pub area: String,
    pub holder: String,
    pub since: String,
    pub why: String,
    /// Monotonic fencing token at acquisition (0 if absent, e.g. shell-created).
    pub fence: u64,
}

/// The singleton merge gate. On disk: `merge.lock/` dir with `holder`/`since`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeLock {
    pub holder: String,
    pub since: String,
}

/// One append-only ledger record in `intents.jsonl`.
///
/// Two shell bugs are FIXED here (coordinator STEER: parity = semantic equivalence,
/// not byte-identity — don't port the bugs):
///  - **Proper JSON escaping.** The shell's bare `printf` emits invalid JSON the
///    moment an event contains `"` or `\`; [`json_escape`] makes every line valid.
///  - **No trailing-space quirk.** The shell appended a stray space inside the event
///    field (`"event":"<text> "`); that was cosmetic, not a format contract, so it
///    is dropped. The differential harness compares semantic state, not bytes.
///
/// A monotonic `fence` token is recorded (WP12 research §1, design-for-M2). The
/// shell never reads `intents.jsonl`, so enriching it is safe for coexistence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerEntry {
    pub t: u64,
    /// Monotonic fencing token for this action (0 if not assigned).
    pub fence: u64,
    pub session: String,
    pub event: String,
}

impl LedgerEntry {
    pub fn new(t: u64, fence: u64, session: &str, event: &str) -> LedgerEntry {
        LedgerEntry {
            t,
            fence,
            session: session.to_string(),
            event: event.to_string(),
        }
    }

    /// A single valid JSONL line (trailing `\n` included). Field order is stable
    /// (`t, fence, session, event`) for readable diffs; strings are JSON-escaped.
    pub fn to_jsonl(&self) -> String {
        format!(
            "{{\"t\":{},\"fence\":{},\"session\":\"{}\",\"event\":\"{}\"}}\n",
            self.t,
            self.fence,
            json_escape(&self.session),
            json_escape(&self.event)
        )
    }
}

/// Escape a string for embedding in a JSON double-quoted value (RFC 8259): the two
/// mandatory escapes `"` and `\`, the short forms for common control characters, and
/// `\u00XX` for any remaining C0 control byte. Everything else (incl. UTF-8) passes
/// through unchanged.
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
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
    fn ledger_line_is_valid_escaped_json_without_trailing_space() {
        let e = LedgerEntry::new(1782686531, 7, "D", "release: x86-auditd-spawn");
        assert_eq!(
            e.to_jsonl(),
            "{\"t\":1782686531,\"fence\":7,\"session\":\"D\",\"event\":\"release: x86-auditd-spawn\"}\n"
        );
    }

    #[test]
    fn ledger_escapes_quotes_and_backslashes_and_controls() {
        let e = LedgerEntry::new(1, 2, "x", "say \"hi\"\tpath C:\\a\nnext");
        assert_eq!(
            e.to_jsonl(),
            "{\"t\":1,\"fence\":2,\"session\":\"x\",\"event\":\"say \\\"hi\\\"\\tpath C:\\\\a\\nnext\"}\n"
        );
    }

    #[test]
    fn json_escape_passes_utf8_through() {
        assert_eq!(json_escape("→ hub — café"), "→ hub — café");
    }
}
