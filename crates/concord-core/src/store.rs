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
    /// Refused: the lease is held by someone else — the shell would blindly delete
    /// it; the typed core enforces ownership (ADR: no release of a foreign lease).
    NotYours { holder: String },
    /// Refused: caller holds it by name, but the presented fence is stale — a reclaim
    /// advanced the lease's fence since the caller acquired it (fencing Floor).
    FenceStale { current: u64 },
}

/// Result of a `merge-unlock`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeUnlockOutcome {
    Released,
    NotHeld,
    /// Refused: the merge lock is held by someone else.
    NotYours { holder: String },
}

/// Whether a caller still legitimately holds a lease — the fence-aware self-check a
/// session runs after waking from a pause, before acting on its authority (Floor).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoldStatus {
    /// The caller holds it, at this fence.
    Held { fence: u64 },
    /// Held by another live session.
    HeldByOther { holder: String },
    /// A lease exists but its holder is stale (reclaimable).
    Stale { holder: String },
    /// No lease on this area.
    Vacant,
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
                let fence = self.append_log(id, &format!("claim: {area} ({why})"))?;
                self.write_lease_fields(&dir, id, area, why, fence)?;
                Ok(ClaimOutcome::Claimed)
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let holder = read_trimmed(&dir.join("holder")).unwrap_or_else(|| "?".into());
                if holder == id {
                    return Ok(ClaimOutcome::AlreadyYours);
                }
                if self.holder_stale(&holder)? {
                    let fence =
                        self.append_log(id, &format!("reclaim-stale: {area} (was {holder})"))?;
                    self.write_lease_fields(&dir, id, area, why, fence)?;
                    Ok(ClaimOutcome::Reclaimed { previous: holder })
                } else {
                    Ok(ClaimOutcome::Conflict { holder })
                }
            }
            Err(e) => Err(ConcordError::io(&dir, e)),
        }
    }

    /// `release <id> <area>`: remove the lease dir — but only if `id` actually holds
    /// it (ownership enforcement), and, when `expected_fence` is given, only if the
    /// lease still carries that fence (the fencing Floor: a reclaim that advanced the
    /// fence makes the caller's authority stale, so its release is refused rather than
    /// clobbering the new holder).
    ///
    /// FENCING FLOOR — residual TOCTOU window: this check-then-remove is not a single
    /// atomic step on a plain filesystem, so a reclaim landing in the gap between the
    /// holder/fence read and the `remove_dir_all` is theoretically possible. It closes
    /// the common reclaim-after-pause case (the woken stale holder is rejected); the
    /// airtight version is the daemon-mediated path (M2.3), where check-and-apply runs
    /// in the daemon's single thread. See ADR-0001 §Consequences.
    pub fn release(
        &self,
        id: &str,
        area: &str,
        expected_fence: Option<u64>,
    ) -> Result<ReleaseOutcome> {
        let dir = self.paths.lease_dir(&slug::slug(area));
        if !dir.is_dir() {
            return Ok(ReleaseOutcome::NoLease);
        }
        let holder = read_trimmed(&dir.join("holder")).unwrap_or_default();
        if holder != id {
            return Ok(ReleaseOutcome::NotYours { holder });
        }
        if let Some(want) = expected_fence {
            let cur = read_trimmed(&dir.join("fence"))
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            if cur != want {
                return Ok(ReleaseOutcome::FenceStale { current: cur });
            }
        }
        fs::remove_dir_all(&dir).map_err(|e| ConcordError::io(&dir, e))?;
        self.append_log(id, &format!("release: {area}"))?;
        Ok(ReleaseOutcome::Released)
    }

    /// Fence-aware ownership check: does `id` still legitimately hold `area`? The
    /// self-check a session runs after waking, before acting on its authority (Floor).
    pub fn verify_hold(&self, id: &str, area: &str) -> Result<HoldStatus> {
        let dir = self.paths.lease_dir(&slug::slug(area));
        if !dir.is_dir() {
            return Ok(HoldStatus::Vacant);
        }
        let holder = read_trimmed(&dir.join("holder")).unwrap_or_default();
        if self.holder_stale(&holder)? {
            return Ok(HoldStatus::Stale { holder });
        }
        if holder == id {
            let fence = read_trimmed(&dir.join("fence"))
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            Ok(HoldStatus::Held { fence })
        } else {
            Ok(HoldStatus::HeldByOther { holder })
        }
    }

    // ───────────────────────── merge-lock / merge-unlock ─────────────────────────

    /// `merge-lock <id> [why]`: acquire the singleton merge gate, or (re)acquire it
    /// if the caller already holds it or the holder is stale.
    pub fn merge_lock(&self, id: &str, why: &str) -> Result<MergeLockOutcome> {
        let dir = &self.paths.merge_lock;
        match fs::create_dir(dir) {
            Ok(()) => {
                let fence = self.append_log(id, &format!("merge-lock: {why}"))?;
                self.write_atomic(&dir.join("holder"), &format!("{id}\n"))?;
                self.write_atomic(&dir.join("since"), &format!("{}\n", self.now))?;
                self.write_atomic(&dir.join("fence"), &format!("{fence}\n"))?;
                Ok(MergeLockOutcome::Acquired)
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let holder = read_trimmed(&dir.join("holder")).unwrap_or_else(|| "?".into());
                if holder == id || self.holder_stale(&holder)? {
                    // Reacquire does not log (shell parity), but still stamps a fresh
                    // fence so the token stays monotonic.
                    let fence = self.bump_fence()?;
                    self.write_atomic(&dir.join("holder"), &format!("{id}\n"))?;
                    self.write_atomic(&dir.join("since"), &format!("{}\n", self.now))?;
                    self.write_atomic(&dir.join("fence"), &format!("{fence}\n"))?;
                    Ok(MergeLockOutcome::Reacquired)
                } else {
                    Ok(MergeLockOutcome::Held { holder })
                }
            }
            Err(e) => Err(ConcordError::io(dir, e)),
        }
    }

    /// `merge-unlock <id>`: release the merge gate — but only if `id` holds it
    /// (ownership enforcement; the shell would unlock unconditionally). Same residual
    /// TOCTOU note as [`Store::release`]; the airtight path is daemon-mediated (M2.3).
    pub fn merge_unlock(&self, id: &str) -> Result<MergeUnlockOutcome> {
        let dir = &self.paths.merge_lock;
        if !dir.is_dir() {
            return Ok(MergeUnlockOutcome::NotHeld);
        }
        let holder = read_trimmed(&dir.join("holder")).unwrap_or_default();
        if holder != id {
            return Ok(MergeUnlockOutcome::NotYours { holder });
        }
        fs::remove_dir_all(dir).map_err(|e| ConcordError::io(dir, e))?;
        self.append_log(id, "merge-unlock")?;
        Ok(MergeUnlockOutcome::Released)
    }

    // ──────────────────────────────── log / sync ────────────────────────────────

    /// `log <id> <event...>`: append a structured ledger record.
    pub fn log(&self, id: &str, event: &str) -> Result<()> {
        self.append_log(id, event)?;
        Ok(())
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
            // Original area from the additive `area` file; for a shell-created lease
            // that lacks it, fall back to the slug (best effort).
            let area = read_trimmed(&dir.join("area")).unwrap_or_else(|| area_slug.clone());
            let fence = read_trimmed(&dir.join("fence"))
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            leases.push(Lease {
                area_slug,
                area,
                holder,
                since: read_trimmed(&dir.join("since")).unwrap_or_default(),
                why: read_trimmed(&dir.join("why")).unwrap_or_default(),
                fence,
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
            fence: read_trimmed(&dir.join("fence"))
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0),
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
    /// Returns `(reported-area, holder)`.
    ///
    /// Overlap is computed on the TRUE area strings when available (the Rust-written
    /// `area` file), using `/`-segment prefix logic ([`slug::overlaps`]); for a
    /// shell-created lease lacking an `area` file we fall back to `_`-segment overlap
    /// on the slugs. An exact-same area is the shell's own `mkdir` collision (handled
    /// on the claim path) — here we flag a PROPER parent/child overlap only.
    fn find_live_overlap(&self, id: &str, area: &str) -> Result<Option<(String, String)>> {
        for held_slug in sorted_entries(&self.paths.leases)? {
            let dir = self.paths.lease_dir(&held_slug);
            if !dir.is_dir() {
                continue;
            }
            let holder = read_trimmed(&dir.join("holder")).unwrap_or_else(|| "?".into());
            if holder == id || self.holder_stale(&holder)? {
                continue;
            }
            let held_area = read_trimmed(&dir.join("area"));
            let (overlaps, same) = match &held_area {
                Some(ha) => (slug::overlaps(area, ha), ha == area),
                None => {
                    let want = slug::slug(area);
                    (slug_overlaps(&want, &held_slug), want == held_slug)
                }
            };
            if overlaps && !same {
                return Ok(Some((held_area.unwrap_or(held_slug), holder)));
            }
        }
        Ok(None)
    }

    // ──────────────────────────────── write helpers ────────────────────────────────

    fn write_session(&self, s: &Session) -> Result<()> {
        self.write_atomic(&self.paths.session_file(&s.id), &s.to_body())
    }

    fn write_lease_fields(
        &self,
        dir: &Path,
        holder: &str,
        area: &str,
        why: &str,
        fence: u64,
    ) -> Result<()> {
        self.write_atomic(&dir.join("holder"), &format!("{holder}\n"))?;
        self.write_atomic(&dir.join("why"), &format!("{why}\n"))?;
        self.write_atomic(&dir.join("since"), &format!("{}\n", self.now))?;
        // Additive files the shell ignores: the ORIGINAL area (fixes the lossy-slug
        // conflation) and the fencing token (WP12 §1, design-for-M2).
        self.write_atomic(&dir.join("area"), &format!("{area}\n"))?;
        self.write_atomic(&dir.join("fence"), &format!("{fence}\n"))?;
        Ok(())
    }

    /// Append a ledger record under a freshly-bumped fence; returns that fence.
    fn append_log(&self, id: &str, event: &str) -> Result<u64> {
        let fence = self.bump_fence()?;
        let line = LedgerEntry::new(self.now, fence, id, event).to_jsonl();
        append_file(&self.paths.log, &line)?;
        Ok(fence)
    }

    /// Atomically increment the global monotonic fence counter (`$COORD/fence`) and
    /// return the new value, guarded by a short-lived `mkdir` mutex so concurrent
    /// processes cannot lose an increment. M1 *records* the token; M2 *enforces* it
    /// (rejecting actions that carry a stale fence after a reclaim — WP12 research §1).
    fn bump_fence(&self) -> Result<u64> {
        mkdirs(&self.paths.coord)?;
        let lock = self.paths.coord.join(".fence.lock");
        let fence_file = self.paths.coord.join("fence");
        let mut acquired = false;
        for _ in 0..500 {
            match fs::create_dir(&lock) {
                Ok(()) => {
                    acquired = true;
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                Err(e) => return Err(ConcordError::io(&lock, e)),
            }
        }
        // ~1s of contention ⇒ assume the holder crashed; steal the stale mutex.
        if !acquired {
            let _ = fs::remove_dir_all(&lock);
            let _ = fs::create_dir(&lock);
        }
        let cur = read_trimmed(&fence_file)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let next = cur + 1;
        let res = self.write_atomic(&fence_file, &format!("{next}\n"));
        let _ = fs::remove_dir_all(&lock);
        res?;
        Ok(next)
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
