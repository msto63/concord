//! WP7 — the typed inbox message model (M3.1).
//!
//! M2 demultiplexed raw prose blocks into per-recipient inboxes. WP7 layers a *type*
//! over each: a [`Message`] `{ts, from, to, kind, ref, body}` persisted as one JSON
//! object per line in `inbox/<id>.jsonl`. The prose channel stays the human audit log;
//! the typed inbox is the machine-readable, per-recipient delta a consumer (or the M3
//! board / MCP tools) reads.
//!
//! Two creation paths (coordinator-arbitrated, mirroring M2's "derived + opt-in"):
//!  - **Derived** (default, zero-migration): the daemon classifies the `(topic)` of an
//!    existing `### from → to (topic)` directive into a [`MessageKind`].
//!  - **First-class**: `concord send <from> <to> <kind> [--ref R] <body>` writes a typed
//!    message directly.
//!
//! The classifier is **conservative** (coordinator refinement): it matches only a
//! confident leading-token keyword and falls back to [`MessageKind::Note`] on any
//! ambiguity — never mis-labelling.

use crate::directive::Block;
use crate::model::json_escape;

/// The kind of a coordination message — the lived vocabulary of the prose channel
/// (frequencies measured over the real log: Ack ≫ MergeReady > Idle/Done > Ready …).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Go,
    Ack,
    Design,
    Arbitration,
    Status,
    Decision,
    Blocked,
    Done,
    Ready,
    Idle,
    MergeReady,
    StandDown,
    /// Catch-all: anything not confidently classified (keeps the classifier honest).
    Note,
}

impl MessageKind {
    /// Lowercase wire token (used in JSON and on the `send` CLI).
    pub fn as_str(self) -> &'static str {
        match self {
            MessageKind::Go => "go",
            MessageKind::Ack => "ack",
            MessageKind::Design => "design",
            MessageKind::Arbitration => "arbitration",
            MessageKind::Status => "status",
            MessageKind::Decision => "decision",
            MessageKind::Blocked => "blocked",
            MessageKind::Done => "done",
            MessageKind::Ready => "ready",
            MessageKind::Idle => "idle",
            MessageKind::MergeReady => "merge-ready",
            MessageKind::StandDown => "stand-down",
            MessageKind::Note => "note",
        }
    }

    /// Parse an explicit kind token (for `concord send`). Unknown ⇒ `None` (the CLI
    /// rejects an unknown kind rather than silently downgrading).
    pub fn parse(token: &str) -> Option<MessageKind> {
        let t = token.trim().to_lowercase();
        let k = match t.as_str() {
            "go" => MessageKind::Go,
            "ack" => MessageKind::Ack,
            "design" => MessageKind::Design,
            "arbitration" => MessageKind::Arbitration,
            "status" => MessageKind::Status,
            "decision" => MessageKind::Decision,
            "blocked" => MessageKind::Blocked,
            "done" => MessageKind::Done,
            "ready" => MessageKind::Ready,
            "idle" => MessageKind::Idle,
            "merge-ready" | "mergeready" => MessageKind::MergeReady,
            "stand-down" | "standdown" => MessageKind::StandDown,
            "note" => MessageKind::Note,
            _ => return None,
        };
        Some(k)
    }

    /// Conservatively classify a directive `(topic)` into a kind. Matches only the
    /// leading keyword token (text up to the first space / `:` / `(`), upper-cased,
    /// against the known vocabulary; anything else ⇒ [`MessageKind::Note`]. This
    /// avoids mis-classifying (e.g. "feedback" must NOT become Ack) — when unsure, Note.
    pub fn classify(topic: &str) -> MessageKind {
        let lead: String = topic
            .trim_start()
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != ':' && *c != '(')
            .collect();
        match lead.to_uppercase().as_str() {
            "GO" => MessageKind::Go,
            "ACK" => MessageKind::Ack,
            "DESIGN" => MessageKind::Design,
            "ARBITRIERUNG" | "ARBITRATION" => MessageKind::Arbitration,
            "STATUS" | "MEILENSTEIN" | "MILESTONE" | "UPDATE" => MessageKind::Status,
            "ENTSCHEIDUNG" | "DECISION" => MessageKind::Decision,
            "BLOCKED" | "BLOCK" => MessageKind::Blocked,
            "FERTIG" | "DONE" => MessageKind::Done,
            "READY" => MessageKind::Ready,
            "IDLE" => MessageKind::Idle,
            "MERGE-READY" => MessageKind::MergeReady,
            "STAND-DOWN" => MessageKind::StandDown,
            _ => MessageKind::Note,
        }
    }
}

/// A typed inbox message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub ts: u64,
    pub from: String,
    pub to: String,
    pub kind: MessageKind,
    /// An AP/backlog reference (e.g. `B15.3`, `WP7`) extracted or supplied — the WP7
    /// hook for the ADR's AP-linkage. `None` when absent.
    pub reference: Option<String>,
    /// The verbatim block (header + body) — lossless human content.
    pub body: String,
}

impl Message {
    /// Build a derived message from a routed directive block to `to`.
    pub fn from_block(block: &Block, to: &str, ts: u64) -> Message {
        Message {
            ts,
            from: block.directive.from.clone(),
            to: to.to_string(),
            kind: MessageKind::classify(&block.directive.topic),
            reference: extract_ap_ref(&block.text),
            body: block.text.clone(),
        }
    }

    /// A first-class message (the `send` path) with an explicit kind and reference.
    pub fn new(
        ts: u64,
        from: &str,
        to: &str,
        kind: MessageKind,
        reference: Option<String>,
        body: &str,
    ) -> Message {
        Message {
            ts,
            from: from.to_string(),
            to: to.to_string(),
            kind,
            reference,
            body: body.to_string(),
        }
    }

    /// One JSONL line (trailing `\n` included), fields JSON-escaped. `ref` is the
    /// reference string or `null`.
    pub fn to_jsonl(&self) -> String {
        let reference = match &self.reference {
            Some(r) => format!("\"{}\"", json_escape(r)),
            None => "null".to_string(),
        };
        format!(
            "{{\"ts\":{},\"from\":\"{}\",\"to\":\"{}\",\"kind\":\"{}\",\"ref\":{},\"body\":\"{}\"}}\n",
            self.ts,
            json_escape(&self.from),
            json_escape(&self.to),
            self.kind.as_str(),
            reference,
            json_escape(&self.body)
        )
    }
}

/// Extract the first AP/backlog id from `text`, or `None`. An AP id is 1–3 uppercase
/// letters followed by a digit, then any run of digits / dots / lowercase letters —
/// e.g. `B15.3`, `DS1`, `K6.8a`, `PS1.4`, `WP7`, `M2.1`. Conservative: plain words
/// (`GO`, `PR`) and hyphenated tags (`ADR-0028`) do not match.
pub fn extract_ap_ref(text: &str) -> Option<String> {
    for raw in text.split(|c: char| c.is_whitespace() || "()[]{},;:\"'".contains(c)) {
        let tok = raw.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.');
        if is_ap_id(tok) {
            return Some(tok.to_string());
        }
    }
    None
}

fn is_ap_id(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_uppercase() {
        i += 1;
    }
    let letters = i;
    if !(1..=3).contains(&letters) {
        return false;
    }
    if i >= b.len() || !b[i].is_ascii_digit() {
        return false; // must be letters then a digit
    }
    // Remainder: digits, dots, or lowercase only.
    while i < b.len() {
        let c = b[i];
        if !(c.is_ascii_digit() || c == b'.' || c.is_ascii_lowercase()) {
            return false;
        }
        i += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::directive::demux;

    #[test]
    fn classify_real_vocabulary() {
        assert_eq!(MessageKind::classify("GO: M2 concordd"), MessageKind::Go);
        assert_eq!(MessageKind::classify("ACK + erster Befund"), MessageKind::Ack);
        assert_eq!(MessageKind::classify("DESIGN: M3 …"), MessageKind::Design);
        assert_eq!(
            MessageKind::classify("ARBITRIERUNG M2: …"),
            MessageKind::Arbitration
        );
        assert_eq!(MessageKind::classify("MEILENSTEIN M2.1"), MessageKind::Status);
        assert_eq!(
            MessageKind::classify("ENTSCHEIDUNG: overlap"),
            MessageKind::Decision
        );
        assert_eq!(MessageKind::classify("STAND-DOWN für Pause"), MessageKind::StandDown);
        assert_eq!(MessageKind::classify("MERGE-READY: #408"), MessageKind::MergeReady);
    }

    #[test]
    fn classify_is_conservative() {
        // Ambiguous / unknown ⇒ Note, never a wrong label.
        assert_eq!(MessageKind::classify("feedback on the thing"), MessageKind::Note);
        assert_eq!(MessageKind::classify("REBASED+MERGE-READY: #408"), MessageKind::Note);
        assert_eq!(MessageKind::classify(""), MessageKind::Note);
        assert_eq!(MessageKind::classify("Koordinator-Neustart"), MessageKind::Note);
    }

    #[test]
    fn ap_ref_extraction() {
        assert_eq!(extract_ap_ref("work on B15.3 baseline"), Some("B15.3".into()));
        assert_eq!(extract_ap_ref("DS1.1p.4 done"), Some("DS1.1p.4".into()));
        assert_eq!(extract_ap_ref("the WP7 substrate"), Some("WP7".into()));
        assert_eq!(extract_ap_ref("no ap here, just GO and PR"), None);
        assert_eq!(extract_ap_ref("ADR-0028 persistence"), None);
    }

    #[test]
    fn message_jsonl_is_valid_and_escaped() {
        let m = Message::new(
            42,
            "hub",
            "concord-w",
            MessageKind::Go,
            Some("B15.3".into()),
            "go \"now\"",
        );
        assert_eq!(
            m.to_jsonl(),
            "{\"ts\":42,\"from\":\"hub\",\"to\":\"concord-w\",\"kind\":\"go\",\"ref\":\"B15.3\",\"body\":\"go \\\"now\\\"\"}\n"
        );
    }

    #[test]
    fn message_null_ref() {
        let m = Message::new(1, "a", "b", MessageKind::Note, None, "x");
        assert!(m.to_jsonl().contains("\"ref\":null"));
    }

    #[test]
    fn from_block_classifies_and_extracts() {
        let blocks = demux("### hub → concord-w  (GO: build B15.3)\nplease build it");
        let m = Message::from_block(&blocks[0], "concord-w", 100);
        assert_eq!(m.kind, MessageKind::Go);
        assert_eq!(m.from, "hub");
        assert_eq!(m.to, "concord-w");
        assert_eq!(m.reference, Some("B15.3".into()));
        assert!(m.body.contains("please build it"));
    }
}
