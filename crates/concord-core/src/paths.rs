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
    /// `$COORD/intents.jsonl`.
    pub log: PathBuf,
    /// `$COORD/merge.lock` (singleton merge gate).
    pub merge_lock: PathBuf,
    /// The prose channel (`<repo>-SESSION-SYNC.md` by default).
    pub sync: PathBuf,
    /// Stale window in seconds.
    pub ttl: u64,
}

impl Paths {
    /// Resolve paths from the environment exactly as the shell does, using `start`
    /// (typically the current directory) as the basis for git-toplevel discovery.
    pub fn resolve(start: &Path) -> Paths {
        let top = git_toplevel(start).unwrap_or_else(|| start.to_path_buf());

        let coord = first_env(&["CONCORD_DIR", "AIS_COORD_DIR"])
            .map(PathBuf::from)
            .unwrap_or_else(|| sibling(&top, "-coord"));

        let sync = first_env(&["CONCORD_SYNC", "AIS_SYNC_FILE"])
            .map(PathBuf::from)
            .unwrap_or_else(|| sibling(&top, "-SESSION-SYNC.md"));

        let ttl = env::var("AIS_COORD_TTL")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_TTL);

        Paths {
            sessions: coord.join("sessions"),
            leases: coord.join("leases"),
            log: coord.join("intents.jsonl"),
            merge_lock: coord.join("merge.lock"),
            coord,
            sync,
            ttl,
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
}

/// Return the first set, non-empty environment variable from `names`.
fn first_env(names: &[&str]) -> Option<String> {
    names
        .iter()
        .filter_map(|n| env::var(n).ok())
        .find(|v| !v.is_empty())
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
/// success. Mirrors `git rev-parse --show-toplevel 2>/dev/null || pwd`.
fn git_toplevel(start: &Path) -> Option<PathBuf> {
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
            log: PathBuf::from("/x/ais-coord/intents.jsonl"),
            merge_lock: PathBuf::from("/x/ais-coord/merge.lock"),
            sync: PathBuf::from("/x/ais-SESSION-SYNC.md"),
            ttl: DEFAULT_TTL,
        };
        assert_eq!(p.lease_dir("a_b"), Path::new("/x/ais-coord/leases/a_b"));
        assert_eq!(p.session_file("hub"), Path::new("/x/ais-coord/sessions/hub"));
    }
}
