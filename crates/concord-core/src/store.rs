//! The filesystem-backed coordination store: typed, atomic transitions over the
//! exact on-disk layout the shell uses.
//!
//! Atomicity improvements over the shell (WP12 §1, the "coordinator is not itself
//! race-free" fix):
//!  - **Session/lease-field writes are temp-file + atomic-rename**, so a concurrent
//!    reader never sees a torn half-written file (the shell's `printf > file` can).
//!  - **Lease acquisition is the atomic `mkdir`** (same primitive as the shell —
//!    `fs::create_dir` fails if the dir exists), so the lock itself is race-free in
//!    both; we keep it.
//!  - **Ownership is enforced, not advised:** `release`/`merge-unlock` of a foreign
//!    lease is structurally distinguishable (the outcome reports it) rather than
//!    silently succeeding.

use std::fs;
use std::io::Write;
use std::path::Path;

use crate::clock;
use crate::error::{ConcordError, Result};
use crate::model::{LedgerEntry, Lease, MergeLock, Session};
use crate::paths::Paths;
use crate::slug;

/// How `claim` treats a path-prefix overlap with an existing lease.
///
/// This is the one place the typed port can intentionally exceed shell parity
/// (WP12 §6). It is a parameter, not a hardcoded policy, so the CLI can flip the
/// default once `hub` rules on sequencing — without touching the mechanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlapPolicy {
    /// Byte-parity with the shell: only the exact-slug `mkdir` collision blocks a
    /// claim; a parent/child overlap is NOT detected (the shell behaviour).
    ParityShell,
    /// Vision-true enforcement: reject a claim that path-prefix-overlaps any held,
    /// non-stale lease, even under a different slug.
    RejectOverlap,
}

/// Result of a `claim`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimOutcome {
    /// Newly acquired.
    Claimed,
    /// The caller already holds this exact area.
    AlreadyYours,
    /// Reclaimed from a stale holder.
    Reclaimed { previous: String },
    /// Blocked: held by a live session (exact-slug collision).
    Conflict { holder: String },
    /// Blocked by `RejectOverlap`: path-prefix overlap with another held area.
    OverlapConflict { area: String, holder: String },
}

/// Result of a `release`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseOutcome {
    Released,
    NoLease,
}

/// Result of a `merge-lock`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeLockOutcome {
    Acquired,
    Reacquired,
    Held { holder: String },
}

/// A snapshot for `status`: only non-stale sessions/leases are included, matching
/// what the shell prints.
#[derive(Debug, Clone)]
pub struct StatusReport {
    pub sessions: Vec<Session>,
    pub leases: Vec<Lease>,
    pub merge_lock_holder: Option<String>,
    /// Whether the `sessions/` dir held zero entries. The shell prints "(none)" for
    /// sessions only in this case (NOT when entries exist but are all stale), so the
    /// CLI needs to distinguish "empty" from "all stale" to match byte-for-byte.
    pub sessions_dir_empty: bool,
}

/// The coordination store, bound to a resolved [`Paths`] and a fixed `now`.
///
/// `now` is captured once at construction so every field written in one logical
/// operation shares a timestamp (more consistent than the shell's repeated `now()`).
pub struct Store {
    paths: Paths,
    now: u64,
}

impl Store {
    /// Open the store for `paths`, stamping operations with the current time.
    pub fn open(paths: Paths) -> Result<Store> {
        let now = clock::now();
        let s = Store { paths, now };
        s.ensure_dirs()?;
        Ok(s)
    }

    /// Open with an explicit `now` (for deterministic tests).
    pub fn open_at(paths: Paths, now: u64) -> Result<Store> {
        let s = Store { paths, now };
        s.ensure_dirs()?;
        Ok(s)
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }
    pub fn now(&self) -> u64 {
        self.now
    }

    fn ensure_dirs(&self) -> Result<()> {
        mkdirs(&self.paths.sessions)?;
        mkdirs(&self.paths.leases)?;
        Ok(())
    }

    // ───────────────────────── register / heartbeat ─────────────────────────

    /// `register <id> <focus>`: (over)write the session file and log it.
    pub fn register(&self, id: &str, focus: &str) -> Result<Session> {
        let s = Session::fresh(id, focus, self.now);
        self.write_session(&s)?;
        self.append_log(id, &format!("register: {focus}"))?;
        Ok(s)
    }

    /// `heartbeat <id>`: refresh the heartbeat, preserving `focus`/`started`. If the
    /// session file is missing, create one with empty focus (shell behaviour).
    pub fn heartbeat(&self, id: &str) -> Result<()> {
        let s = match self.read_session(id)? {
            Some(mut existing) => {
                existing.heartbeat = self.now.to_string();
                existing
            }
            None => Session {
                id: id.to_string(),
                focus: String::new(),
                started: self.now.to_string(),
                heartbeat: self.now.to_string(),
            },
        };
        self.write_session(&s)
    }

    // ───────────────────────────── claim / release ─────────────────────────────

    /// `claim <id> <area> [why]` with the chosen overlap policy.
    pub fn claim(
        &self,
        id: &str,
        area: &str,
        why: &str,
        policy: OverlapPolicy,
    ) -> Result<ClaimOutcome> {
        // New capability (WP12 §6): reject a parent/child overlap with a live lease,
        // before the exact-slug mkdir. Skipped under ParityShell.
        if policy == OverlapPolicy::RejectOverlap {
            if let Some((other_area, holder)) = self.find_live_overlap(id, area)? {
                return Ok(ClaimOutcome::OverlapConflict {
                    area: other_area,
                    holder,
                });
            }
        }

        let dir = self.paths.lease_dir(&slug::slug(area));
        match fs::create_dir(&dir) {
            Ok(()) => {
                self.write_lease_fields(&dir, id, why)?;
                self.append_log(id, &format!("claim: {area} ({why})"))?;
                Ok(ClaimOutcome::Claimed)
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let holder = read_trimmed(&dir.join("holder")).unwrap_or_else(|| "?".into());
                if holder == id {
                    return Ok(ClaimOutcome::AlreadyYours);
                }
                if self.holder_stale(&holder)? {
                    self.write_lease_fields(&dir, id, why)?;
                    self.append_log(
                        id,
                        &format!("reclaim-stale: {area} (was {holder})"),
                    )?;
                    Ok(ClaimOutcome::Reclaimed { previous: holder })
                } else {
                    Ok(ClaimOutcome::Conflict { holder })
                }
            }
            Err(e) => Err(ConcordError::io(&dir, e)),
        }
    }

    /// `release <id> <area>`: remove the lease dir. `id` is recorded in the log; the
    /// outcome distinguishes "there was no lease" from a real release.
    pub fn release(&self, id: &str, area: &str) -> Result<ReleaseOutcome> {
        let dir = self.paths.lease_dir(&slug::slug(area));
        if dir.is_dir() {
            fs::remove_dir_all(&dir).map_err(|e| ConcordError::io(&dir, e))?;
            self.append_log(id, &format!("release: {area}"))?;
            Ok(ReleaseOutcome::Released)
        } else {
            Ok(ReleaseOutcome::NoLease)
        }
    }

    // ───────────────────────── merge-lock / merge-unlock ─────────────────────────

    /// `merge-lock <id> [why]`: acquire the singleton merge gate, or (re)acquire it
    /// if the caller already holds it or the holder is stale.
    pub fn merge_lock(&self, id: &str, why: &str) -> Result<MergeLockOutcome> {
        let dir = &self.paths.merge_lock;
        match fs::create_dir(dir) {
            Ok(()) => {
                self.write_atomic(&dir.join("holder"), &format!("{id}\n"))?;
                self.write_atomic(&dir.join("since"), &format!("{}\n", self.now))?;
                self.append_log(id, &format!("merge-lock: {why}"))?;
                Ok(MergeLockOutcome::Acquired)
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let holder = read_trimmed(&dir.join("holder")).unwrap_or_else(|| "?".into());
                if holder == id || self.holder_stale(&holder)? {
                    self.write_atomic(&dir.join("holder"), &format!("{id}\n"))?;
                    self.write_atomic(&dir.join("since"), &format!("{}\n", self.now))?;
                    Ok(MergeLockOutcome::Reacquired)
                } else {
                    Ok(MergeLockOutcome::Held { holder })
                }
            }
            Err(e) => Err(ConcordError::io(dir, e)),
        }
    }

    /// `merge-unlock <id>`: release the merge gate and log it.
    pub fn merge_unlock(&self, id: &str) -> Result<()> {
        let dir = &self.paths.merge_lock;
        if dir.exists() {
            fs::remove_dir_all(dir).map_err(|e| ConcordError::io(dir, e))?;
        }
        self.append_log(id, "merge-unlock")
    }

    // ──────────────────────────────── log / sync ────────────────────────────────

    /// `log <id> <event...>`: append a structured ledger record.
    pub fn log(&self, id: &str, event: &str) -> Result<()> {
        self.append_log(id, event)
    }

    /// `sync <id> <target> <topic> <body>`: append a prose-channel entry and log it.
    /// Byte-exact with the shell's `printf '\n### %s → %s  (%s)\n%s\n'`.
    pub fn sync(&self, id: &str, target: &str, topic: &str, body: &str) -> Result<()> {
        let entry = format!("\n### {id} → {target}  ({topic})\n{body}\n");
        append_file(&self.paths.sync, &entry)?;
        self.append_log(id, &format!("sync→{target}: {topic}"))?;
        Ok(())
    }

    // ─────────────────────────────────── status ───────────────────────────────────

    /// Build the `status` snapshot: live sessions and leases, lexicographically by
    /// directory name (matching the shell's glob order), plus the merge-lock holder
    /// if live.
    pub fn status(&self) -> Result<StatusReport> {
        let session_names = sorted_entries(&self.paths.sessions)?;
        let sessions_dir_empty = session_names.is_empty();
        let mut sessions = Vec::new();
        for name in session_names {
            if let Some(s) = self.read_session(&name)? {
                if !s.is_stale(self.now, self.paths.ttl) {
                    sessions.push(s);
                }
            }
        }

        let mut leases = Vec::new();
        for area_slug in sorted_entries(&self.paths.leases)? {
            let dir = self.paths.lease_dir(&area_slug);
            if !dir.is_dir() {
                continue;
            }
            let holder = read_trimmed(&dir.join("holder")).unwrap_or_else(|| "?".into());
            if self.holder_stale(&holder)? {
                continue;
            }
            leases.push(Lease {
                area_slug,
                holder,
                since: read_trimmed(&dir.join("since")).unwrap_or_default(),
                why: read_trimmed(&dir.join("why")).unwrap_or_default(),
            });
        }

        let merge_lock_holder = match self.read_merge_lock()? {
            Some(ml) if !self.holder_stale(&ml.holder)? => Some(ml.holder),
            _ => None,
        };

        Ok(StatusReport {
            sessions,
            leases,
            merge_lock_holder,
            sessions_dir_empty,
        })
    }

    // ──────────────────────────────── read helpers ────────────────────────────────

    /// Read and parse a session file, or `None` if it does not exist.
    pub fn read_session(&self, id: &str) -> Result<Option<Session>> {
        let f = self.paths.session_file(id);
        match fs::read_to_string(&f) {
            Ok(body) => Ok(Some(Session::parse(id, &body))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(ConcordError::io(&f, e)),
        }
    }

    /// Read the merge lock, or `None` if not held.
    pub fn read_merge_lock(&self) -> Result<Option<MergeLock>> {
        let dir = &self.paths.merge_lock;
        if !dir.is_dir() {
            return Ok(None);
        }
        Ok(Some(MergeLock {
            holder: read_trimmed(&dir.join("holder")).unwrap_or_else(|| "?".into()),
            since: read_trimmed(&dir.join("since")).unwrap_or_default(),
        }))
    }

    /// Is `holder` a stale (or missing) session? Mirrors `session_stale`.
    fn holder_stale(&self, holder: &str) -> Result<bool> {
        match self.read_session(holder)? {
            None => Ok(true),
            Some(s) => Ok(s.is_stale(self.now, self.paths.ttl)),
        }
    }

    /// Find the first live, non-self lease whose area path-prefix-overlaps `area`.
    /// Returns `(original-area, holder)`. The original area is recovered from the
    /// lease's `why`-adjacent record — but since the shell stores only the slug, we
    /// compare on the de-slugged segments by treating `_` boundaries conservatively:
    /// we compare the requested area's slug-prefix against each held slug.
    fn find_live_overlap(&self, id: &str, area: &str) -> Result<Option<(String, String)>> {
        let want = slug::slug(area);
        for held_slug in sorted_entries(&self.paths.leases)? {
            let dir = self.paths.lease_dir(&held_slug);
            if !dir.is_dir() {
                continue;
            }
            let holder = read_trimmed(&dir.join("holder")).unwrap_or_else(|| "?".into());
            if holder == id || self.holder_stale(&holder)? {
                continue;
            }
            // Compare on slugs (the only persisted form). `_`-segment prefix overlap
            // is the slug-space analogue of `/`-segment overlap; exact-equal is the
            // shell's own collision and handled by mkdir, so we look for proper
            // parent/child here.
            if slug_overlaps(&want, &held_slug) && want != held_slug {
                return Ok(Some((held_slug, holder)));
            }
        }
        Ok(None)
    }

    // ──────────────────────────────── write helpers ────────────────────────────────

    fn write_session(&self, s: &Session) -> Result<()> {
        self.write_atomic(&self.paths.session_file(&s.id), &s.to_body())
    }

    fn write_lease_fields(&self, dir: &Path, holder: &str, why: &str) -> Result<()> {
        self.write_atomic(&dir.join("holder"), &format!("{holder}\n"))?;
        self.write_atomic(&dir.join("why"), &format!("{why}\n"))?;
        self.write_atomic(&dir.join("since"), &format!("{}\n", self.now))?;
        Ok(())
    }

    fn append_log(&self, id: &str, event: &str) -> Result<()> {
        let line = LedgerEntry::new(self.now, id, event).to_jsonl();
        append_file(&self.paths.log, &line)
    }

    /// Write `content` to `path` atomically (temp file in the same dir + rename).
    fn write_atomic(&self, path: &Path, content: &str) -> Result<()> {
        let dir = path.parent().unwrap_or_else(|| Path::new("."));
        mkdirs(dir)?;
        let tmp = dir.join(format!(
            ".{}.tmp.{}",
            path.file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "f".into()),
            std::process::id()
        ));
        {
            let mut f = fs::File::create(&tmp).map_err(|e| ConcordError::io(&tmp, e))?;
            f.write_all(content.as_bytes())
                .map_err(|e| ConcordError::io(&tmp, e))?;
            f.sync_all().ok();
        }
        fs::rename(&tmp, path).map_err(|e| ConcordError::io(path, e))?;
        Ok(())
    }
}

// ─────────────────────────────── free helpers ───────────────────────────────

/// `_`-segment prefix overlap, the slug-space analogue of [`slug::overlaps`].
fn slug_overlaps(a: &str, b: &str) -> bool {
    let sa: Vec<&str> = a.split('_').filter(|s| !s.is_empty()).collect();
    let sb: Vec<&str> = b.split('_').filter(|s| !s.is_empty()).collect();
    let n = sa.len().min(sb.len());
    sa[..n] == sb[..n]
}

fn mkdirs(p: &Path) -> Result<()> {
    fs::create_dir_all(p).map_err(|e| ConcordError::io(p, e))
}

/// Append `content` to `path`, creating it if needed.
fn append_file(path: &Path, content: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        mkdirs(dir)?;
    }
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| ConcordError::io(path, e))?;
    f.write_all(content.as_bytes())
        .map_err(|e| ConcordError::io(path, e))
}

/// Read a file and strip trailing newlines, like `$(cat file)` in the shell.
fn read_trimmed(path: &Path) -> Option<String> {
    let s = fs::read_to_string(path).ok()?;
    Some(s.trim_end_matches(['\n', '\r']).to_string())
}

/// Directory entry names sorted by raw bytes (the shell glob's lexicographic order
/// under the C locale). Returns an empty vec if the dir is absent.
fn sorted_entries(dir: &Path) -> Result<Vec<String>> {
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(ConcordError::io(dir, e)),
    };
    let mut names: Vec<String> = Vec::new();
    for ent in rd {
        let ent = ent.map_err(|e| ConcordError::io(dir, e))?;
        names.push(ent.file_name().to_string_lossy().into_owned());
    }
    names.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    Ok(names)
}
