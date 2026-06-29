//! F3 escalation + ack types.
//!
//! A **tracked escalation** (E2) is a persistent record (`<coord>/escalations/<seq>/`,
//! one dir per escalation, mirroring the lease/merge-lock on-disk shape) that survives
//! until explicitly resolved — so a blocker cannot silently vanish. The coordinator's
//! forwarding queue to the operator is simply "the open escalations". **Ack-tracking**
//! (E3) records which directives a recipient has not yet acknowledged so the daemon can
//! re-deliver and, after K misses, auto-escalate.

use std::fmt;

/// How urgent an escalation is. `Critical` is reserved for explicit
/// vision-critical-path blockers; the daemon's auto-escalation of an un-ACK'd directive
/// is `High` (a real coordination breach, but not necessarily critical).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Low => "low",
            Severity::Medium => "medium",
            Severity::High => "high",
            Severity::Critical => "critical",
        }
    }

    /// Parse a severity token (case-insensitive; common abbreviations accepted).
    pub fn parse(s: &str) -> Option<Severity> {
        match s.trim().to_ascii_lowercase().as_str() {
            "low" | "l" => Some(Severity::Low),
            "medium" | "med" | "m" => Some(Severity::Medium),
            "high" | "hi" | "h" => Some(Severity::High),
            "critical" | "crit" | "c" => Some(Severity::Critical),
            _ => None,
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The lifecycle status of an escalation record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscStatus {
    /// Raised, not yet seen by the target.
    Open,
    /// The target acknowledged it (seen, not yet resolved).
    Acked,
    /// Closed.
    Resolved,
}

impl EscStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            EscStatus::Open => "open",
            EscStatus::Acked => "acked",
            EscStatus::Resolved => "resolved",
        }
    }
    pub fn parse(s: &str) -> EscStatus {
        match s.trim() {
            "acked" => EscStatus::Acked,
            "resolved" => EscStatus::Resolved,
            _ => EscStatus::Open,
        }
    }
}

/// A tracked escalation record (E2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Escalation {
    pub seq: u64,
    pub from: String,
    /// The routed target (default: the coordinator — workers cannot reach the operator).
    pub to: String,
    pub severity: Severity,
    pub about: String,
    pub created: u64,
    pub status: EscStatus,
    /// `(resolved_ts, resolver/note)` once closed.
    pub resolved: Option<(u64, String)>,
    /// Optional back-reference (e.g. a backlog AP, or the directive that triggered an
    /// auto-escalation).
    pub reference: Option<String>,
}

/// One un-acknowledged directive tracked for a recipient (E3). Persisted as a TSV line
/// in `<coord>/acks/<id>.pending`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub seq: u64,
    pub from: String,
    pub first_seen: u64,
    pub redelivers: u32,
    pub escalated: bool,
}

impl Pending {
    /// Serialize to one tab-delimited line (internal IPC; `from` is slug-safe — a session
    /// id has no tabs).
    pub fn to_line(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}",
            self.seq,
            self.from,
            self.first_seen,
            self.redelivers,
            if self.escalated { 1 } else { 0 }
        )
    }

    pub fn parse_line(line: &str) -> Option<Pending> {
        let f: Vec<&str> = line.trim_end().split('\t').collect();
        if f.len() < 5 {
            return None;
        }
        Some(Pending {
            seq: f[0].parse().ok()?,
            from: f[1].to_string(),
            first_seen: f[2].parse().ok()?,
            redelivers: f[3].parse().ok()?,
            escalated: f[4] == "1",
        })
    }
}

/// The outcome of resolving an escalation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveOutcome {
    Resolved,
    NotFound,
    AlreadyResolved,
}

/// A directive that became overdue and should be re-delivered to its recipient's inbox
/// (the daemon performs the actual inbox append + mtime bump).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redeliver {
    pub to: String,
    pub from: String,
    pub seq: u64,
}

/// What one ack-timeout tick did (F3 daemon active layer).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AckTickReport {
    /// Directives re-delivered this tick (overdue, under the K-miss threshold).
    pub redelivered: Vec<Redeliver>,
    /// Escalation seqs auto-raised this tick (overdue past K misses).
    pub escalated: Vec<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_roundtrip_and_order() {
        for s in [Severity::Low, Severity::Medium, Severity::High, Severity::Critical] {
            assert_eq!(Severity::parse(s.as_str()), Some(s));
        }
        assert!(Severity::Critical > Severity::High);
        assert_eq!(Severity::parse("HI"), Some(Severity::High));
        assert_eq!(Severity::parse("bogus"), None);
    }

    #[test]
    fn pending_line_roundtrip() {
        let p = Pending { seq: 7, from: "hub".into(), first_seen: 1000, redelivers: 2, escalated: true };
        assert_eq!(Pending::parse_line(&p.to_line()), Some(p));
    }
}
