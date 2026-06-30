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
    /// Seconds with no heartbeat before a session is stale.
    pub stale_ttl: u64,
    /// Claim overlap policy.
    pub overlap_policy: OverlapPolicy,
    /// P1 capability-strict edit guard (folds in the `<coord>/strict-leases` marker).
    pub strict: bool,
}

/// `[daemon]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonConfig {
    /// Whether the CLI routes consequential ops through the daemon.
    pub enabled: bool,
}

/// `[launcher]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherConfig {
    /// Extra flags passed to `claude` at launch.
    pub claude_flags: String,
    /// Worktree naming convention; `{repo}`/`{id}` are substituted.
    pub worktree_pattern: String,
}

/// `[escalation]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EscalationConfig {
    /// The default escalation target.
    pub coordinator: String,
    /// Seconds before an un-ACK'd directive is re-delivered.
    pub ack_ttl: u64,
    /// Re-deliveries before auto-escalation.
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

/// `[telemetry]` — the F4 hub-telemetry layer (consumes Claude Code's native OTel stream).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetryConfig {
    /// Whether the launcher enables telemetry + the daemon runs the OTLP receiver.
    pub enabled: bool,
    /// The local OTLP/HTTP-JSON receiver port (a Concord-specific default so it never
    /// collides with a user's real collector on 4317/4318).
    pub port: u16,
    /// Minutes with no telemetry datapoint before a session counts as idle.
    pub idle_min: u64,
    /// Token-usage rate (tokens per minute) above which a session is flagged BURN.
    pub burn_warn: u64,
    /// Edit-tool reject/deny decisions within `loop_window` that flag a REJECT storm.
    pub reject_storm: u32,
    /// The look-back window (seconds) for burn / reject / loop heuristics.
    pub loop_window: u64,
}

/// The full resolved configuration.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Config {
    pub leases: LeasesConfig,
    pub daemon: DaemonConfig,
    pub launcher: LauncherConfig,
    pub escalation: EscalationConfig,
    pub resources: ResourcesConfig,
    pub telemetry: TelemetryConfig,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        TelemetryConfig {
            enabled: false,
            port: 4319,
            idle_min: 15,
            burn_warn: 20_000,
            reject_storm: 5,
            loop_window: 600,
        }
    }
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

