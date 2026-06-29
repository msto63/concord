//! `config.toml` loader for Concord (F-config).
//!
//! This crate **isolates** the `toml`/`serde` dependency — exactly as `concord-ast`
//! isolates tree-sitter and `concord-mcp` isolates rmcp — so `concord-core` stays
//! dependency-free. It parses the project (`<coord>/config.toml`) and user-global
//! (`~/.config/concord/config.toml`) files, applies precedence, and returns a plain
//! [`concord_core::config::Config`]. It also resolves the two bootstrap values config
//! cannot define (coord dir / session id) from convention, flags, the user-global
//! `[projects]` map, or — deprecated, with a warning — a legacy env var.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use concord_core::config::Config;
use concord_core::escalation::Severity;
use concord_core::paths::Overrides;
use concord_core::store::OverlapPolicy;
use serde::Deserialize;

/// A serde mirror of the TOML — every field optional so a partial file merges cleanly
/// onto the built-in defaults.
#[derive(Debug, Default, Deserialize)]
struct RawConfig {
    #[serde(default)]
    leases: RawLeases,
    #[serde(default)]
    daemon: RawDaemon,
    #[serde(default)]
    launcher: RawLauncher,
    #[serde(default)]
    escalation: RawEscalation,
    #[serde(default)]
    resources: RawResources,
    /// User-global only: project-root → coord-dir bootstrap map.
    #[serde(default)]
    projects: HashMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawLeases {
    stale_ttl: Option<u64>,
    overlap_policy: Option<String>,
    strict: Option<bool>,
}
#[derive(Debug, Default, Deserialize)]
struct RawDaemon {
    enabled: Option<bool>,
}
#[derive(Debug, Default, Deserialize)]
struct RawLauncher {
    claude_flags: Option<String>,
    worktree_pattern: Option<String>,
}
#[derive(Debug, Default, Deserialize)]
struct RawEscalation {
    coordinator: Option<String>,
    ack_ttl: Option<u64>,
    redeliver_max: Option<u32>,
    auto_severity: Option<String>,
}
#[derive(Debug, Default, Deserialize)]
struct RawResources {
    port_base: Option<u32>,
    default_slots: Option<u32>,
}

impl RawConfig {
    /// Parse a TOML string, or `Err(message)` for a malformed file (the caller warns and
    /// falls back rather than crashing — a human-edited file must never brick the tool).
    fn parse(s: &str) -> Result<RawConfig, String> {
        toml::from_str(s).map_err(|e| e.to_string())
    }

    /// Layer this (higher-precedence) raw config onto an accumulating [`Config`].
    fn apply(&self, c: &mut Config) {
        if let Some(v) = self.leases.stale_ttl {
            c.leases.stale_ttl = v;
        }
        if let Some(v) = &self.leases.overlap_policy {
            c.leases.overlap_policy = match v.trim().to_ascii_lowercase().as_str() {
                "shell" | "parity" | "parityshell" => OverlapPolicy::ParityShell,
                _ => OverlapPolicy::RejectOverlap,
            };
        }
        if let Some(v) = self.leases.strict {
            c.leases.strict = v;
        }
        if let Some(v) = self.daemon.enabled {
            c.daemon.enabled = v;
        }
        if let Some(v) = &self.launcher.claude_flags {
            c.launcher.claude_flags = v.clone();
        }
        if let Some(v) = &self.launcher.worktree_pattern {
            c.launcher.worktree_pattern = v.clone();
        }
        if let Some(v) = &self.escalation.coordinator {
            c.escalation.coordinator = v.clone();
        }
        if let Some(v) = self.escalation.ack_ttl {
            c.escalation.ack_ttl = v;
        }
        if let Some(v) = self.escalation.redeliver_max {
            c.escalation.redeliver_max = v;
        }
        if let Some(v) = &self.escalation.auto_severity {
            if let Some(s) = Severity::parse(v) {
                c.escalation.auto_severity = s;
            }
        }
        if let Some(v) = self.resources.port_base {
            c.resources.port_base = v;
        }
        if let Some(v) = self.resources.default_slots {
            c.resources.default_slots = v;
        }
    }
}

/// The path to the user-global config (`~/.config/concord/config.toml`), honoring
/// `XDG_CONFIG_HOME` then `HOME`. `None` if neither is set.
pub fn user_global_path() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("concord/config.toml"));
        }
    }
    std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .map(|h| PathBuf::from(h).join(".config/concord/config.toml"))
}

/// Read + parse a config file if it exists; on a parse error, print a warning to stderr
/// and return `None` (fall back to lower-precedence layers / defaults).
fn read_raw(path: &Path) -> Option<RawConfig> {
    let body = std::fs::read_to_string(path).ok()?;
    match RawConfig::parse(&body) {
        Ok(raw) => Some(raw),
        Err(e) => {
            eprintln!("concord: warning: ignoring malformed {} ({e})", path.display());
            None
        }
    }
}

/// Load the effective [`Config`] for a coordination dir: built-in defaults ← user-global
/// ← project (`<coord>/config.toml`), each layer overriding the previous per field.
pub fn load(coord: &Path) -> Config {
    let mut cfg = Config::default();
    if let Some(path) = user_global_path() {
        if let Some(raw) = read_raw(&path) {
            raw.apply(&mut cfg);
        }
    }
    if let Some(raw) = read_raw(&coord.join("config.toml")) {
        raw.apply(&mut cfg);
    }
    cfg
}

/// The user-global `[projects]` bootstrap map (project-root → coord-dir).
pub fn projects_map() -> HashMap<String, String> {
    user_global_path()
        .and_then(|p| read_raw(&p))
        .map(|r| r.projects)
        .unwrap_or_default()
}

/// Detect retired environment variables (F-config). Returns the bootstrap [`Overrides`]
/// they imply plus a deprecation warning per variable seen. Env is **honored for one
/// release** so the live self-hosting keeps working; full removal follows once all
/// callers use config/flags/convention.
pub fn legacy_env_overrides() -> (Overrides, Vec<String>) {
    let mut ov = Overrides::default();
    let mut warns = Vec::new();
    let mut take = |names: &[&str], slot: &mut Option<PathBuf>, what: &str| {
        for n in names {
            if let Ok(v) = std::env::var(n) {
                if !v.is_empty() {
                    warns.push(format!(
                        "concord: warning: ${n} is deprecated (F-config) — use {what}; honored for now, removed next release"
                    ));
                    if slot.is_none() {
                        *slot = Some(PathBuf::from(v));
                    }
                }
            }
        }
    };
    take(&["CONCORD_DIR", "AIS_COORD_DIR"], &mut ov.coord, "--coord or the <repo>-coord convention");
    take(&["CONCORD_SYNC", "AIS_SYNC_FILE"], &mut ov.sync, "the <repo>-SESSION-SYNC.md convention");
    take(&["CONCORD_PROJECT", "AIS_PROJECT_DIR"], &mut ov.project, "--project or the git toplevel");
    (ov, warns)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_empty() {
        let mut c = Config::default();
        RawConfig::parse("").unwrap().apply(&mut c);
        assert_eq!(c, Config::default());
    }

    #[test]
    fn project_overrides_fields() {
        let mut c = Config::default();
        let raw = RawConfig::parse(
            r#"
            [leases]
            stale_ttl = 600
            overlap_policy = "shell"
            strict = true
            [escalation]
            coordinator = "K"
            ack_ttl = 120
            redeliver_max = 5
            auto_severity = "critical"
            [resources]
            port_base = 6000
            "#,
        )
        .unwrap();
        raw.apply(&mut c);
        assert_eq!(c.leases.stale_ttl, 600);
        assert_eq!(c.leases.overlap_policy, OverlapPolicy::ParityShell);
        assert!(c.leases.strict);
        assert_eq!(c.escalation.coordinator, "K");
        assert_eq!(c.escalation.ack_ttl, 120);
        assert_eq!(c.escalation.redeliver_max, 5);
        assert_eq!(c.escalation.auto_severity, Severity::Critical);
        assert_eq!(c.resources.port_base, 6000);
        // Untouched field keeps its default.
        assert_eq!(c.resources.default_slots, 1);
    }

    #[test]
    fn malformed_is_rejected_not_panicked() {
        assert!(RawConfig::parse("this is = = not toml [[[").is_err());
    }
}
