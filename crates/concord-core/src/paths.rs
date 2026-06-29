//! Resolution of the coordination directory and prose channel, byte-compatible with
//! `bin/coord.sh`. This is what makes the Rust binary a *drop-in*: run in the same
//! repo with the same environment, it reads and writes the exact same locations.
//!
//! Shell logic mirrored (coord.sh:27-34):
//! ```text
//!   _top     = git rev-parse --show-toplevel   (fallback: cwd)
//!   COORD    = $CONCORD_DIR : $AIS_COORD_DIR : <dirname _top>/<basename _top>-coord
//!   SESSIONS = $COORD/sessions
//!   LEASES   = $COORD/leases
//!   LOG      = $COORD/intents.jsonl
//!   SYNC     = $CONCORD_SYNC : $AIS_SYNC_FILE : <dirname _top>/<basename _top>-SESSION-SYNC.md
//!   TTL      = $AIS_COORD_TTL : 1800
//! ```

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The default "no heartbeat within this many seconds ⇒ stale" window (30 min).
pub const DEFAULT_TTL: u64 = 1800;

/// Fully-resolved coordination paths for one invocation.
#[derive(Debug, Clone)]
pub struct Paths {
    /// The coordination state root (`<repo>-coord/` by default).
    pub coord: PathBuf,
    /// `$COORD/sessions`.
    pub sessions: PathBuf,
    /// `$COORD/leases`.
    pub leases: PathBuf,
    /// `$COORD/resources` — the F2 named-resource / build-slot semaphore namespace,
    /// kept separate from `leases/` so resource locks never touch the path/symbol
    /// overlap logic (structurally orthogonal).
    pub resources: PathBuf,
    /// `$COORD/acks` — the F3 per-recipient pending-directive tracking (`<id>.pending`).
    pub acks: PathBuf,
    /// `$COORD/escalations` — the F3 tracked-escalation records (one dir per escalation,
    /// persisted until resolved).
    pub escalations: PathBuf,
    /// `$COORD/intents.jsonl`.
    pub log: PathBuf,
    /// `$COORD/merge.lock` (singleton merge gate).
    pub merge_lock: PathBuf,
    /// The prose channel (`<repo>-SESSION-SYNC.md` by default).
    pub sync: PathBuf,
    /// The project root this coordination state belongs to (`CONCORD_PROJECT` env, or
    /// the git toplevel). Surfaced by `concord paths` for multi-project tooling.
    pub project: PathBuf,
    /// Stale window in seconds.
    pub ttl: u64,
}

/// Bootstrap overrides for [`Paths::resolve_with`] — the two values config cannot define
/// (they locate the config itself). Each is `None` to fall back to the git-toplevel
/// convention. The binary computes these from `--coord`/`--project` flags, the user-global
/// `[projects]` map, or (deprecated, with a warning) a legacy env var.
#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub coord: Option<PathBuf>,
    pub sync: Option<PathBuf>,
    pub project: Option<PathBuf>,
}

impl Paths {
    /// Resolve paths by **convention only** (no environment): `<repo>-coord/` and
    /// `<repo>-SESSION-SYNC.md` as siblings of the git toplevel discovered from `start`.
    /// F-config: env reading is retired — the binary passes explicit overrides via
    /// [`Paths::resolve_with`].
    pub fn resolve(start: &Path) -> Paths {
        Paths::resolve_with(start, &Overrides::default())
    }

    /// Resolve paths from the git-toplevel convention, with explicit `overrides` taking
    /// precedence. `ttl` defaults to [`DEFAULT_TTL`] (the binary sets it from config).
    pub fn resolve_with(start: &Path, overrides: &Overrides) -> Paths {
        // The convention basis is the project root: an explicit `--project` override if
        // given (so `--project P` derives `P-coord`), else the git toplevel from `start`.
        let top = overrides
            .project
            .clone()
            .unwrap_or_else(|| git_toplevel(start).unwrap_or_else(|| start.to_path_buf()));

        let coord = overrides
            .coord
            .clone()
            .unwrap_or_else(|| sibling(&top, "-coord"));
        let sync = overrides
            .sync
            .clone()
            .unwrap_or_else(|| sibling(&top, "-SESSION-SYNC.md"));
        let project = top.clone();

        Paths {
            sessions: coord.join("sessions"),
            leases: coord.join("leases"),
            resources: coord.join("resources"),
            acks: coord.join("acks"),
            escalations: coord.join("escalations"),
            log: coord.join("intents.jsonl"),
            merge_lock: coord.join("merge.lock"),
            coord,
            sync,
            project,
            ttl: DEFAULT_TTL,
        }
    }

    /// Convenience: resolve from the process's current working directory.
    pub fn from_cwd() -> Paths {
        let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Paths::resolve(&cwd)
    }

    /// The lease directory for a slugged area.
    pub fn lease_dir(&self, slug: &str) -> PathBuf {
        self.leases.join(slug)
    }

    /// The session file for an id.
    pub fn session_file(&self, id: &str) -> PathBuf {
        self.sessions.join(id)
    }

    /// The directory for a named resource (F2), keyed by the slugged name.
    pub fn resource_dir(&self, name: &str) -> PathBuf {
        self.resources.join(crate::slug::slug(name))
    }

    /// The directory for slot `i` of a named resource.
    pub fn resource_slot_dir(&self, name: &str, i: u32) -> PathBuf {
        self.resource_dir(name).join("slots").join(i.to_string())
    }

    /// The capacity marker file for a named resource (declared N, persisted on first
    /// acquire and validated thereafter).
    pub fn resource_capacity_file(&self, name: &str) -> PathBuf {
        self.resource_dir(name).join("capacity")
    }

    /// The pending-directive tracking file for a recipient (F3).
    pub fn pending_file(&self, id: &str) -> PathBuf {
        self.acks.join(format!("{}.pending", crate::slug::slug(id)))
    }

    /// The directory for escalation record `seq` (F3).
    pub fn escalation_dir(&self, seq: u64) -> PathBuf {
        self.escalations.join(seq.to_string())
    }
}

/// `<dirname top>/<basename top><suffix>` — e.g. `~/Projects/ais` + `-coord`
/// ⇒ `~/Projects/ais-coord`. Matches `$(dirname "$_top")/$(basename "$_top")<suffix>`.
fn sibling(top: &Path, suffix: &str) -> PathBuf {
    let parent = top.parent().unwrap_or_else(|| Path::new("."));
    let base = top
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    parent.join(format!("{base}{suffix}"))
}

/// Run `git rev-parse --show-toplevel` from `start`, returning the toplevel path on
/// success. Mirrors `git rev-parse --show-toplevel 2>/dev/null || pwd`. Public so the
/// binary can resolve the convention coord dir / `[projects]` map key (F-config).
pub fn git_toplevel(start: &Path) -> Option<PathBuf> {
    let out = Command::new("git")
        .arg("-C")
        .arg(start)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let trimmed = s.trim_end_matches(['\n', '\r']);
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sibling_appends_suffix_to_basename() {
        let top = Path::new("/home/u/Projects/ais");
        assert_eq!(sibling(top, "-coord"), Path::new("/home/u/Projects/ais-coord"));
        assert_eq!(
            sibling(top, "-SESSION-SYNC.md"),
            Path::new("/home/u/Projects/ais-SESSION-SYNC.md")
        );
    }

    #[test]
    fn derived_paths_hang_off_coord() {
        // Force the default branch by clearing overrides for this resolution.
        let p = Paths {
            coord: PathBuf::from("/x/ais-coord"),
            sessions: PathBuf::from("/x/ais-coord/sessions"),
            leases: PathBuf::from("/x/ais-coord/leases"),
            resources: PathBuf::from("/x/ais-coord/resources"),
            acks: PathBuf::from("/x/ais-coord/acks"),
            escalations: PathBuf::from("/x/ais-coord/escalations"),
            log: PathBuf::from("/x/ais-coord/intents.jsonl"),
            merge_lock: PathBuf::from("/x/ais-coord/merge.lock"),
            sync: PathBuf::from("/x/ais-SESSION-SYNC.md"),
            project: PathBuf::from("/x/ais"),
            ttl: DEFAULT_TTL,
        };
        assert_eq!(p.lease_dir("a_b"), Path::new("/x/ais-coord/leases/a_b"));
        assert_eq!(p.session_file("hub"), Path::new("/x/ais-coord/sessions/hub"));
    }
}
