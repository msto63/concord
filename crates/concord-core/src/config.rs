//! The typed configuration (F-config).
//!
//! `Config` is a **plain struct** — no `serde`, no `toml` — so `concord-core` stays
//! dependency-free. The `concord-config` crate parses `config.toml` (project +
//! user-global) into this struct, applying precedence and these built-in defaults; the
//! core and the binaries then read values from here instead of the environment. Env vars
//! are retired (honored-with-deprecation-warning by the loader for one release).

use crate::escalation::Severity;
use crate::store::OverlapPolicy;

/// `[leases]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeasesConfig {
    /// Seconds with no heartbeat before a session is stale (was `AIS_COORD_TTL`).
    pub stale_ttl: u64,
    /// Claim overlap policy (was `CONCORD_STRICT_OVERLAP`).
    pub overlap_policy: OverlapPolicy,
    /// P1 capability-strict edit guard (folds in the `<coord>/strict-leases` marker).
    pub strict: bool,
}

/// `[daemon]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonConfig {
    /// Whether the CLI routes consequential ops through the daemon (was `CONCORD_NO_DAEMON`).
    pub enabled: bool,
}

/// `[launcher]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherConfig {
    /// Extra flags passed to `claude` at launch (was `CONCORD_CLAUDE_FLAGS`).
    pub claude_flags: String,
    /// Worktree naming convention; `{repo}`/`{id}` are substituted.
    pub worktree_pattern: String,
}

/// `[escalation]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EscalationConfig {
    /// The default escalation target (was `CONCORD_COORDINATOR_ID` / `<coord>/coordinator`).
    pub coordinator: String,
    /// Seconds before an un-ACK'd directive is re-delivered (was `TTL_ACK`).
    pub ack_ttl: u64,
    /// Re-deliveries before auto-escalation (was `K_REDELIVER`).
    pub redeliver_max: u32,
    /// Severity of an auto-escalation.
    pub auto_severity: Severity,
}

/// `[resources]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourcesConfig {
    /// Base port for a `qemu-port` pool (slot i → port_base + i).
    pub port_base: u32,
    /// Default semaphore capacity when `--slots` is omitted.
    pub default_slots: u32,
}

/// The full resolved configuration.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Config {
    pub leases: LeasesConfig,
    pub daemon: DaemonConfig,
    pub launcher: LauncherConfig,
    pub escalation: EscalationConfig,
    pub resources: ResourcesConfig,
}

impl Default for LeasesConfig {
    fn default() -> Self {
        LeasesConfig {
            stale_ttl: crate::paths::DEFAULT_TTL,
            overlap_policy: OverlapPolicy::RejectOverlap,
            strict: false,
        }
    }
}

impl Default for DaemonConfig {
    fn default() -> Self {
        DaemonConfig { enabled: true }
    }
}

impl Default for LauncherConfig {
    fn default() -> Self {
        LauncherConfig {
            claude_flags: "--dangerously-skip-permissions".to_string(),
            worktree_pattern: "{repo}-{id}".to_string(),
        }
    }
}

impl Default for EscalationConfig {
    fn default() -> Self {
        EscalationConfig {
            coordinator: "hub".to_string(),
            ack_ttl: 15 * 60,
            redeliver_max: 2,
            auto_severity: Severity::High,
        }
    }
}

impl Default for ResourcesConfig {
    fn default() -> Self {
        ResourcesConfig {
            port_base: 5900,
            default_slots: 1,
        }
    }
}

