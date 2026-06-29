//! Prose-channel directive parsing and demultiplexing (M2 inbox substrate).
//!
//! The prose channel (`*-SESSION-SYNC.md`) carries directed messages as Markdown
//! blocks headed by a directive line:
//!
//! ```text
//! ### <from> → <to>  (<topic>)
//! <body lines…>
//! ```
//!
//! The M2 daemon **demultiplexes** these: it parses each new header, determines the
//! target session(s), and appends the whole block to a per-recipient inbox
//! (`inbox/<id>`). Posters are unchanged — they keep writing `### …` to the prose
//! channel; only *consumers* gain the cheap, directed inbox (the §9/WP7 token lever).
//! This module is the pure, filesystem-free core of that demux so it can be unit
//! tested; [`crate::store`]/the daemon do the I/O.
//!
//! Scope (per coordinator arbitration): this is the inbox *substrate* only. The full
//! typed inbox protocol (`{from,to,type,ref,body}` structured messages) is M3.

/// A parsed directive header. `targets` is empty when `broadcast` is true (the
/// daemon resolves a broadcast to the set of registered sessions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directive {
    pub from: String,
    /// Explicit recipients (e.g. `["D", "E"]`); empty for a broadcast.
    pub targets: Vec<String>,
    /// True for `→ ALLE` / `→ ALL` broadcasts.
    pub broadcast: bool,
}

const ALL_TOKENS: [&str; 3] = ["ALLE", "ALL", "ALLES"];

/// Parse a single line as a directive header, or `None` if it is not one.
///
/// Accepts either arrow form (`→` U+2192 or ASCII `->`). The recipient spec is
/// everything between the arrow and an optional `(topic)` suffix; it is split on
/// `,` and `+` into multiple targets (`D,E` / `C + B`). A recipient of `ALLE`/`ALL`
/// (case-insensitive) marks a broadcast.
pub fn parse_header(line: &str) -> Option<Directive> {
    let rest = line.strip_prefix("###")?;
    let rest = rest.trim_start();

    // Locate the arrow (prefer the Unicode arrow; fall back to ASCII "->").
    let (idx, arrow_len) = if let Some(i) = rest.find('→') {
        (i, '→'.len_utf8())
    } else if let Some(i) = rest.find("->") {
        (i, 2)
    } else {
        return None;
    };

    let from = rest[..idx].trim().to_string();
    if from.is_empty() {
        return None;
    }

    // Recipient spec = text after the arrow up to an optional "(topic)".
    let after = &rest[idx + arrow_len..];
    let to_spec = match after.find('(') {
        Some(p) => &after[..p],
        None => after,
    };
    let to_spec = to_spec.trim();
    if to_spec.is_empty() {
        return None;
    }

    let raw_targets: Vec<String> = to_spec
        .split([',', '+'])
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect();

    let broadcast = raw_targets
        .iter()
        .any(|t| ALL_TOKENS.contains(&t.to_uppercase().as_str()));

    Some(Directive {
        from,
        targets: if broadcast { Vec::new() } else { raw_targets },
        broadcast,
    })
}

/// One demultiplexed block: a directive and the verbatim Markdown block it heads
/// (the header line plus every following line up to the next header).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub directive: Directive,
    /// Header + body, exactly as it appeared (no trailing newline).
    pub text: String,
}

/// Split a chunk of newly-appended prose-channel text into directed blocks. Lines
/// before the first header (and non-directive headers) are ignored. Each block runs
/// from its header up to — but not including — the next directive header.
pub fn demux(new_text: &str) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    let mut cur: Option<(Directive, Vec<&str>)> = None;

    for line in new_text.lines() {
        if let Some(dir) = parse_header(line) {
            if let Some((d, lines)) = cur.take() {
                blocks.push(Block {
                    directive: d,
                    text: lines.join("\n"),
                });
            }
            cur = Some((dir, vec![line]));
        } else if let Some((_, lines)) = cur.as_mut() {
            lines.push(line);
        }
        // else: preamble before the first header — ignore.
    }
    if let Some((d, lines)) = cur.take() {
        blocks.push(Block {
            directive: d,
            text: lines.join("\n"),
        });
    }
    blocks
}

/// Route demultiplexed blocks to recipient inboxes. Returns `(recipient_id, text)`
/// pairs in block order. A broadcast fans out to every `registered` session except
/// its own sender (you don't get your own broadcast echoed back). Directed blocks go
/// to their explicit targets verbatim. Pure — the daemon turns each pair into an
/// append to `inbox/<recipient_id>`.
pub fn route(blocks: &[Block], registered: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for b in blocks {
        if b.directive.broadcast {
            for id in registered {
                if *id != b.directive.from {
                    out.push((id.clone(), b.text.clone()));
                }
            }
        } else {
            for t in &b.directive.targets {
                out.push((t.clone(), b.text.clone()));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_unicode_arrow_directed() {
        let d = parse_header("### hub → concord-w  (GO: M2 …)").unwrap();
        assert_eq!(d.from, "hub");
        assert_eq!(d.targets, vec!["concord-w"]);
        assert!(!d.broadcast);
    }

    #[test]
    fn parses_ascii_arrow() {
        let d = parse_header("### concord-w -> hub (ADR-Draft fertig)").unwrap();
        assert_eq!(d.from, "concord-w");
        assert_eq!(d.targets, vec!["hub"]);
    }

    #[test]
    fn broadcast_alle_and_all() {
        for line in ["### hub → ALLE  (x)", "### K -> ALL (y)"] {
            let d = parse_header(line).unwrap();
            assert!(d.broadcast, "{line}");
            assert!(d.targets.is_empty());
        }
    }

    #[test]
    fn multi_target_comma_and_plus() {
        let d = parse_header("### hub → D,E  (wind-down)").unwrap();
        assert_eq!(d.targets, vec!["D", "E"]);
        let d2 = parse_header("### hub → C + B  (xhci)").unwrap();
        assert_eq!(d2.targets, vec!["C", "B"]);
    }

    #[test]
    fn rejects_non_headers() {
        assert!(parse_header("plain text").is_none());
        assert!(parse_header("## heading").is_none());
        assert!(parse_header("### no arrow here").is_none());
        assert!(parse_header("### → missing from (t)").is_none());
    }

    #[test]
    fn demux_groups_blocks_and_bodies() {
        let text = "\
### a → b  (one)
body line 1
body line 2
### c → ALLE  (two)
broadcast body
not a header line
### d → e (three)";
        let blocks = demux(text);
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].directive.targets, vec!["b"]);
        assert_eq!(blocks[0].text, "### a → b  (one)\nbody line 1\nbody line 2");
        assert!(blocks[1].directive.broadcast);
        assert_eq!(blocks[1].text, "### c → ALLE  (two)\nbroadcast body\nnot a header line");
        assert_eq!(blocks[2].directive.targets, vec!["e"]);
    }

    #[test]
    fn demux_ignores_preamble() {
        let blocks = demux("leading noise\nmore noise\n### x → y (t)\nbody");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "### x → y (t)\nbody");
    }

    #[test]
    fn route_directed_and_broadcast_fanout() {
        let blocks = demux(
            "### hub → concord-w (go)\nbody1\n### concord-w → ALLE (status)\nbody2",
        );
        let registered = vec!["a".to_string(), "concord-w".to_string(), "hub".to_string()];
        let routed = route(&blocks, &registered);
        // Directed → concord-w; broadcast from concord-w → a + hub (not concord-w).
        assert_eq!(routed.len(), 3);
        assert_eq!(routed[0].0, "concord-w");
        assert!(routed[0].1.starts_with("### hub → concord-w"));
        assert_eq!(routed[1].0, "a");
        assert_eq!(routed[2].0, "hub");
        assert!(routed[1].1.starts_with("### concord-w → ALLE"));
    }
}
