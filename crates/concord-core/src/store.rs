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
use crate::config::TelemetryConfig;
use crate::error::{ConcordError, Result};
use crate::escalation::{
    AckTickReport, EscStatus, Escalation, Pending, Redeliver, ResolveOutcome, Severity,
};
use crate::message::Message;
use crate::telemetry::{health, HealthFlag, SessionHealth, TelemetryPoint};
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

/// A registered signature contract (F5): the agreed interface for `<file>:<symbol>`, the
/// one Peer-collaboration CLAUDE.md permits, mechanized. The `signature` is the normalized
/// declaration (computed by the binary via `concord-ast`); the store just persists + reads
/// it, so `concord-core` stays free of the tree-sitter dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contract {
    /// `<file>:<symbol>` (same key scheme as a symbol-lease).
    pub key: String,
    /// The agreed normalized signature.
    pub signature: String,
    /// Who registered it.
    pub by: String,
    /// The counter-party (`--with`), for provenance; empty if none.
    pub with: String,
    pub since: String,
}

/// The decision from [`Store::check_lease`] — the `PreToolUse` deny verdict (F1/A1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseCheck {
    /// The edit is permitted.
    Allow,
    /// The edit is refused: `holder` actively holds a lease on the overlapping `area`
    /// (P2), or — under strict P1 with no conflicting holder — `holder` is empty meaning
    /// "you hold no covering lease".
    Deny { area: String, holder: String },
}

/// What a clean session-end teardown did (F1/A2). All steps are idempotent, so a
/// repeated call returns empty/false fields rather than erroring.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionEndReport {
    /// Areas whose leases were released (held by the ending session).
    pub released: Vec<String>,
    /// Resource slots (`<name>#<slot>`) released (F2; the A2 composition AUFLAGE).
    pub resources_released: Vec<String>,
    /// Whether the ending session held — and thus released — the merge-lock.
    pub merge_unlocked: bool,
    /// Whether a registry entry existed and was removed.
    pub deregistered: bool,
}

/// The outcome of acquiring a slot of a named resource (F2 semaphore).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceOutcome {
    /// Acquired free slot `slot` of `capacity`.
    Acquired { slot: u32, capacity: u32, fence: u64 },
    /// Reclaimed slot `slot` from a stale holder (the pool self-heals after a crash).
    Reclaimed { slot: u32, capacity: u32, previous: String },
    /// The caller already holds slot `slot` (idempotent).
    AlreadyHeld { slot: u32 },
    /// Every slot is held by a live session — the pool is exhausted.
    Busy { capacity: u32 },
    /// The requested capacity disagrees with the persisted one for this resource.
    CapacityMismatch { declared: u32 },
}

/// The outcome of releasing a caller's slot of a named resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceReleaseOutcome {
    Released { slot: u32 },
    NotHeld,
}

/// A snapshot of one named resource for `status`: its capacity and the held slots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceLock {
    pub name: String,
    pub capacity: u32,
    /// `(slot, holder)` for each currently-held (live) slot, sorted by slot.
    pub held: Vec<(u32, String)>,
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
    /// airtight version is the daemon-mediated path (M3L.2 for claim/release, M2.3 for
    /// merge-lock), where check-and-apply runs in the daemon's single thread. See
    /// ADR-0001 §Consequences.
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

    /// Decide whether `id` may edit `area`, for the `PreToolUse` deny hook (F1/A1) and
    /// the `PostToolUse` audit (F1/A6). Two policies:
    ///
    /// - **P2 (default, `strict=false`) — block-on-conflict:** deny only if a *different,
    ///   currently-active* session holds a lease overlapping `area`. Un-leased edits are
    ///   allowed (no claim-everything friction); only a real collision is blocked. This is
    ///   the enforced version of today's `pre-tool.sh` default-allow guard.
    /// - **P1 (`strict=true`) — capability-strict:** deny unless `id` itself holds a lease
    ///   covering `area`. Opt-in for high-assurance work.
    ///
    /// Symbol-aware throughout (a path-lease subsumes its symbols; disjoint symbols are
    /// compatible), via [`slug::area_overlaps`]. Fail-open is the caller's job: the hook
    /// treats any error/missing binary as Allow.
    pub fn check_lease(&self, id: &str, area: &str, strict: bool) -> Result<LeaseCheck> {
        // A live, non-self lease overlapping `area` is the conflict in both policies.
        // NOTE: unlike `find_live_overlap` (claim-path, which excludes the exact-same area
        // because the mkdir collision handles it), the edit-guard MUST catch the exact-same
        // file/symbol too — that is the most common A1 collision.
        let conflict = self.find_blocking_lease(id, area)?;
        if strict {
            // P1: allow only if `id` holds an overlapping (covering) lease of its own.
            if self.holds_overlapping(id, area)? {
                Ok(LeaseCheck::Allow)
            } else {
                let (holder, blocking_area) = match conflict {
                    Some((a, h)) => (h, a),
                    None => (String::new(), area.to_string()),
                };
                Ok(LeaseCheck::Deny { area: blocking_area, holder })
            }
        } else {
            // P2: allow unless someone else actively holds an overlapping lease.
            match conflict {
                Some((a, holder)) => Ok(LeaseCheck::Deny { area: a, holder }),
                None => Ok(LeaseCheck::Allow),
            }
        }
    }

    /// First live, non-self lease whose area overlaps `area` — INCLUDING an exact match
    /// (the edit-guard conflict scan for [`Store::check_lease`]). Returns `(area, holder)`.
    fn find_blocking_lease(&self, id: &str, area: &str) -> Result<Option<(String, String)>> {
        for held_slug in sorted_entries(&self.paths.leases)? {
            let dir = self.paths.lease_dir(&held_slug);
            if !dir.is_dir() {
                continue;
            }
            let holder = read_trimmed(&dir.join("holder")).unwrap_or_else(|| "?".into());
            if holder == id || self.holder_stale(&holder)? {
                continue;
            }
            let held_area = read_trimmed(&dir.join("area")).unwrap_or_else(|| held_slug.clone());
            if slug::area_overlaps(area, &held_area) {
                return Ok(Some((held_area, holder)));
            }
        }
        Ok(None)
    }

    /// Does `id` hold a live lease whose area overlaps `area`? (P1 allow-condition.)
    fn holds_overlapping(&self, id: &str, area: &str) -> Result<bool> {
        for held_slug in sorted_entries(&self.paths.leases)? {
            let dir = self.paths.lease_dir(&held_slug);
            if !dir.is_dir() {
                continue;
            }
            if read_trimmed(&dir.join("holder")).as_deref() != Some(id) {
                continue;
            }
            let held_area = read_trimmed(&dir.join("area")).unwrap_or_else(|| held_slug.clone());
            if slug::area_overlaps(area, &held_area) {
                return Ok(true);
            }
        }
        Ok(false)
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
    /// TOCTOU note as [`Store::release`]; the airtight path is daemon-mediated.
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

    // ─────────────────────────── session-end teardown (F1/A2) ───────────────────────────

    /// Release every lease currently held by `id`, returning the released areas (sorted).
    /// Used by the clean-exit teardown; ownership is enforced per-lease by [`Store::release`],
    /// so only the caller's own leases are removed. Idempotent.
    pub fn release_all(&self, id: &str) -> Result<Vec<String>> {
        let mut released = Vec::new();
        for area_slug in sorted_entries(&self.paths.leases)? {
            let dir = self.paths.lease_dir(&area_slug);
            if !dir.is_dir() {
                continue;
            }
            if read_trimmed(&dir.join("holder")).as_deref() != Some(id) {
                continue;
            }
            let area = read_trimmed(&dir.join("area")).unwrap_or_else(|| area_slug.clone());
            if let ReleaseOutcome::Released = self.release(id, &area, None)? {
                released.push(area);
            }
        }
        Ok(released)
    }

    /// Remove `id`'s registry entry (idempotent). Returns whether an entry existed.
    /// Unlike letting the heartbeat go stale, this deregisters immediately on clean exit.
    pub fn deregister(&self, id: &str) -> Result<bool> {
        let f = self.paths.session_file(id);
        match fs::remove_file(&f) {
            Ok(()) => {
                self.append_log(id, "deregister")?;
                Ok(true)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(ConcordError::io(&f, e)),
        }
    }

    /// Clean-exit teardown (F1/A2): release all of `id`'s leases, release the merge-lock
    /// if `id` holds it, and deregister. Each step is idempotent, so re-running on an
    /// already-ended session is a harmless no-op. Shrinks the window in which a finished
    /// session still appears to hold authority (complements TTL-stale-reclaim).
    pub fn session_end(&self, id: &str) -> Result<SessionEndReport> {
        let released = self.release_all(id)?;
        let resources_released = self.release_all_resources(id)?;
        let merge_unlocked = matches!(self.merge_unlock(id)?, MergeUnlockOutcome::Released);
        let deregistered = self.deregister(id)?;
        Ok(SessionEndReport {
            released,
            resources_released,
            merge_unlocked,
            deregistered,
        })
    }

    // ───────────────────── named resource locks / build-slots (F2) ─────────────────────

    /// Acquire one slot of a named, N-slot resource semaphore (F2): a lock on a
    /// *non-file* resource (a port, the build-env, a deploy) in a namespace that is
    /// structurally orthogonal to the path/symbol leases (`<coord>/resources/`, never
    /// touched by `area_overlaps`). `slots` is the capacity (1 = exclusive). The first
    /// free slot `0..slots` is taken by an atomic `mkdir` (race-safe — a colliding mkdir
    /// just falls through to the next slot); a slot held by a *stale* session is reclaimed,
    /// so the pool self-heals after a crash (the documented `ais` QEMU-port / build-env
    /// contention). The capacity is persisted on first acquire and validated thereafter.
    pub fn acquire_resource(
        &self,
        id: &str,
        name: &str,
        slots: u32,
        why: &str,
    ) -> Result<ResourceOutcome> {
        let slots = slots.max(1);
        let dir = self.paths.resource_dir(name);
        mkdirs(&dir)?;
        // Persist capacity on first acquire; validate on later ones.
        let cap_file = self.paths.resource_capacity_file(name);
        let capacity = match read_trimmed(&cap_file).and_then(|s| s.parse::<u32>().ok()) {
            Some(existing) => {
                if existing != slots {
                    return Ok(ResourceOutcome::CapacityMismatch { declared: existing });
                }
                existing
            }
            None => {
                self.write_atomic(&cap_file, &format!("{slots}\n"))?;
                slots
            }
        };

        for i in 0..capacity {
            let slot = self.paths.resource_slot_dir(name, i);
            match fs::create_dir_all(slot.parent().unwrap()).and_then(|_| fs::create_dir(&slot)) {
                Ok(()) => {
                    let fence = self.append_log(id, &format!("resource-acquire: {name}#{i} ({why})"))?;
                    self.write_lease_fields(&slot, id, &format!("{name}#{i}"), why, fence)?;
                    return Ok(ResourceOutcome::Acquired { slot: i, capacity, fence });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let holder = read_trimmed(&slot.join("holder")).unwrap_or_default();
                    if holder == id {
                        return Ok(ResourceOutcome::AlreadyHeld { slot: i });
                    }
                    if self.holder_stale(&holder)? {
                        let fence = self
                            .append_log(id, &format!("resource-reclaim: {name}#{i} (was {holder})"))?;
                        self.write_lease_fields(&slot, id, &format!("{name}#{i}"), why, fence)?;
                        return Ok(ResourceOutcome::Reclaimed { slot: i, capacity, previous: holder });
                    }
                    // Live holder — try the next slot.
                }
                Err(e) => return Err(ConcordError::io(&slot, e)),
            }
        }
        Ok(ResourceOutcome::Busy { capacity })
    }

    /// Release the caller's slot of a named resource. Ownership-enforced: only a slot
    /// whose holder is `id` is removed.
    pub fn release_resource(&self, id: &str, name: &str) -> Result<ResourceReleaseOutcome> {
        let slots = self.paths.resource_dir(name).join("slots");
        for entry in sorted_entries(&slots)? {
            let dir = slots.join(&entry);
            if read_trimmed(&dir.join("holder")).as_deref() == Some(id) {
                fs::remove_dir_all(&dir).map_err(|e| ConcordError::io(&dir, e))?;
                let slot = entry.parse::<u32>().unwrap_or(0);
                self.append_log(id, &format!("resource-release: {name}#{slot}"))?;
                return Ok(ResourceReleaseOutcome::Released { slot });
            }
        }
        Ok(ResourceReleaseOutcome::NotHeld)
    }

    /// Release every resource slot held by `id` (the F1/A2 SessionEnd composition AUFLAGE:
    /// a crashed/finished session auto-frees its ports / build-env). Returns the freed
    /// `<name>#<slot>` keys.
    pub fn release_all_resources(&self, id: &str) -> Result<Vec<String>> {
        let mut freed = Vec::new();
        for name_slug in sorted_entries(&self.paths.resources)? {
            let slots = self.paths.resources.join(&name_slug).join("slots");
            for entry in sorted_entries(&slots)? {
                let dir = slots.join(&entry);
                if read_trimmed(&dir.join("holder")).as_deref() == Some(id) {
                    fs::remove_dir_all(&dir).map_err(|e| ConcordError::io(&dir, e))?;
                    freed.push(format!("{name_slug}#{entry}"));
                    self.append_log(id, &format!("resource-release: {name_slug}#{entry}"))?;
                }
            }
        }
        Ok(freed)
    }

    /// Snapshot all named resources for `status`: capacity + the live held slots.
    pub fn resource_locks(&self) -> Result<Vec<ResourceLock>> {
        let mut out = Vec::new();
        for name_slug in sorted_entries(&self.paths.resources)? {
            let rdir = self.paths.resources.join(&name_slug);
            let capacity = read_trimmed(&rdir.join("capacity"))
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);
            let mut held = Vec::new();
            for entry in sorted_entries(&rdir.join("slots"))? {
                let dir = rdir.join("slots").join(&entry);
                let holder = read_trimmed(&dir.join("holder")).unwrap_or_default();
                if holder.is_empty() || self.holder_stale(&holder)? {
                    continue;
                }
                held.push((entry.parse::<u32>().unwrap_or(0), holder));
            }
            if capacity > 0 || !held.is_empty() {
                held.sort_by_key(|(s, _)| *s);
                out.push(ResourceLock { name: name_slug, capacity, held });
            }
        }
        Ok(out)
    }

    // ───────────────────────── ack-tracking + escalation (F3) ─────────────────────────

    /// The default escalation target: the coordinator. Read from `<coord>/coordinator`
    /// if present (project-configurable), else `"hub"`. Workers cannot reach the operator
    /// directly (CLAUDE.md), so an escalation routes to the coordinator, who forwards.
    pub fn coordinator(&self) -> String {
        read_trimmed(&self.paths.coord.join("coordinator")).unwrap_or_else(|| "hub".to_string())
    }

    /// Raise a tracked escalation (E2). Returns its seq. Race-safe: the seq is allocated
    /// by an atomic `mkdir` (a collision just tries the next number), so concurrent
    /// escalators never clobber each other.
    pub fn escalate(
        &self,
        from: &str,
        to: &str,
        severity: Severity,
        about: &str,
        reference: Option<&str>,
    ) -> Result<u64> {
        mkdirs(&self.paths.escalations)?;
        let mut seq = self.max_escalation_seq()? + 1;
        loop {
            let dir = self.paths.escalation_dir(seq);
            match fs::create_dir(&dir) {
                Ok(()) => {
                    self.write_atomic(&dir.join("from"), &format!("{from}\n"))?;
                    self.write_atomic(&dir.join("to"), &format!("{to}\n"))?;
                    self.write_atomic(&dir.join("severity"), &format!("{}\n", severity.as_str()))?;
                    self.write_atomic(&dir.join("about"), &format!("{about}\n"))?;
                    self.write_atomic(&dir.join("created"), &format!("{}\n", self.now))?;
                    self.write_atomic(&dir.join("status"), "open\n")?;
                    if let Some(r) = reference {
                        self.write_atomic(&dir.join("ref"), &format!("{r}\n"))?;
                    }
                    self.append_log(from, &format!("escalate #{seq} [{}] → {to}: {about}", severity.as_str()))?;
                    return Ok(seq);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => seq += 1,
                Err(e) => return Err(ConcordError::io(&dir, e)),
            }
        }
    }

    fn max_escalation_seq(&self) -> Result<u64> {
        Ok(sorted_entries(&self.paths.escalations)?
            .iter()
            .filter_map(|n| n.parse::<u64>().ok())
            .max()
            .unwrap_or(0))
    }

    /// Resolve (close) an escalation. Ownership is not enforced — anyone (typically the
    /// coordinator) may resolve — but the resolver/note is recorded for provenance.
    pub fn resolve_escalation(
        &self,
        resolver: &str,
        seq: u64,
        note: &str,
    ) -> Result<ResolveOutcome> {
        let dir = self.paths.escalation_dir(seq);
        if !dir.is_dir() {
            return Ok(ResolveOutcome::NotFound);
        }
        if read_trimmed(&dir.join("status")).as_deref() == Some("resolved") {
            return Ok(ResolveOutcome::AlreadyResolved);
        }
        self.write_atomic(&dir.join("status"), "resolved\n")?;
        self.write_atomic(&dir.join("resolved"), &format!("{}\n", self.now))?;
        self.write_atomic(&dir.join("resolver"), &format!("{resolver}: {note}\n"))?;
        self.append_log(resolver, &format!("resolve escalation #{seq}: {note}"))?;
        Ok(ResolveOutcome::Resolved)
    }

    /// All escalations (open first, newest within each), for `status`/`escalations`.
    pub fn escalations(&self) -> Result<Vec<Escalation>> {
        let mut out = Vec::new();
        for name in sorted_entries(&self.paths.escalations)? {
            let Ok(seq) = name.parse::<u64>() else { continue };
            let dir = self.paths.escalation_dir(seq);
            if !dir.is_dir() {
                continue;
            }
            let status = EscStatus::parse(&read_trimmed(&dir.join("status")).unwrap_or_default());
            let resolved = match (
                read_trimmed(&dir.join("resolved")).and_then(|s| s.parse::<u64>().ok()),
                read_trimmed(&dir.join("resolver")),
            ) {
                (Some(ts), Some(by)) => Some((ts, by)),
                _ => None,
            };
            out.push(Escalation {
                seq,
                from: read_trimmed(&dir.join("from")).unwrap_or_default(),
                to: read_trimmed(&dir.join("to")).unwrap_or_default(),
                severity: Severity::parse(&read_trimmed(&dir.join("severity")).unwrap_or_default())
                    .unwrap_or(Severity::Medium),
                about: read_trimmed(&dir.join("about")).unwrap_or_default(),
                created: read_trimmed(&dir.join("created")).and_then(|s| s.parse().ok()).unwrap_or(0),
                status,
                resolved,
                reference: read_trimmed(&dir.join("ref")),
            });
        }
        // Open escalations first, then by recency (highest seq first within a status).
        out.sort_by(|a, b| {
            let oa = (a.status != EscStatus::Resolved) as u8;
            let ob = (b.status != EscStatus::Resolved) as u8;
            ob.cmp(&oa).then(b.seq.cmp(&a.seq))
        });
        Ok(out)
    }

    /// Record a directive addressed to `to` (from `from`) as pending an ack (E3),
    /// assigning the next per-recipient seq. Called by the daemon as it routes directives
    /// (each directive is routed exactly once via the persisted offset, so no cross-run
    /// dedup is needed). Returns the assigned seq.
    pub fn add_pending(&self, to: &str, from: &str) -> Result<u64> {
        let mut items = self.read_pending(to)?;
        let seq = items.iter().map(|p| p.seq).max().unwrap_or(0) + 1;
        items.push(Pending { seq, from: from.to_string(), first_seen: self.now, redelivers: 0, escalated: false });
        self.write_pending(to, &items)?;
        Ok(seq)
    }

    /// Clear all of `id`'s pending directives — it posted (it is alive and caught up),
    /// the derived-ack watermark (reuse of the A3 predicate). Also the effect of an
    /// explicit [`Store::ack`].
    pub fn clear_pending(&self, id: &str) -> Result<()> {
        let f = self.paths.pending_file(id);
        match fs::remove_file(&f) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(ConcordError::io(&f, e)),
        }
    }

    /// Explicit ack (E3): clear `id`'s pending directives and log it. Complements the
    /// derived watermark — a clean machine/MCP action.
    pub fn ack(&self, id: &str, note: &str) -> Result<u32> {
        let n = self.read_pending(id)?.len() as u32;
        self.clear_pending(id)?;
        self.append_log(id, &format!("ack ({n} pending cleared): {note}"))?;
        Ok(n)
    }

    /// Per-session pending summary for `status`: `(id, count, oldest_first_seen)`.
    pub fn pending_summary(&self) -> Result<Vec<(String, u32, u64)>> {
        let mut out = Vec::new();
        for name in sorted_entries(&self.paths.acks)? {
            let id = name.strip_suffix(".pending").unwrap_or(&name).to_string();
            let items = self.read_pending(&id)?;
            if items.is_empty() {
                continue;
            }
            let oldest = items.iter().map(|p| p.first_seen).min().unwrap_or(self.now);
            out.push((id, items.len() as u32, oldest));
        }
        Ok(out)
    }

    /// One ack-timeout tick (F3 active layer; the daemon calls it periodically). For each
    /// pending directive overdue by `ttl` (spaced per redeliver), either re-deliver it
    /// (under K misses) or auto-escalate (at K misses, severity `High`). Returns what to
    /// re-deliver (the daemon does the inbox append) and which escalations were raised.
    pub fn tick_acks(&self, ttl: u64, k: u32, coordinator: &str) -> Result<AckTickReport> {
        let mut report = AckTickReport::default();
        let coord = coordinator.to_string();
        for name in sorted_entries(&self.paths.acks)? {
            let id = name.strip_suffix(".pending").unwrap_or(&name).to_string();
            let mut items = self.read_pending(&id)?;
            let mut changed = false;
            for p in items.iter_mut() {
                if p.escalated {
                    continue;
                }
                // Overdue for the next action once `ttl*(redelivers+1)` has elapsed.
                let due_at = p.first_seen + ttl * (p.redelivers as u64 + 1);
                if self.now < due_at {
                    continue;
                }
                if p.redelivers >= k {
                    // K misses → auto-escalate (High) and stop re-delivering.
                    let about = format!(
                        "{id} has not ACK'd a directive from {} (seq {}) after {k} redelivers — possible going-dark/stuck",
                        p.from, p.seq
                    );
                    let seq = self.escalate("concordd", &coord, Severity::High, &about, Some(&id))?;
                    p.escalated = true;
                    report.escalated.push(seq);
                } else {
                    p.redelivers += 1;
                    report.redelivered.push(Redeliver { to: id.clone(), from: p.from.clone(), seq: p.seq });
                }
                changed = true;
            }
            if changed {
                self.write_pending(&id, &items)?;
            }
        }
        Ok(report)
    }

    // ─────────────────────────── signature contracts (F5) ───────────────────────────

    /// Register (or, idempotently, update) the agreed signature contract for `key`
    /// (`<file>:<symbol>`). Returns `true` if newly created, `false` if it updated an
    /// existing contract (an explicit renegotiation). The `signature` is the normalized
    /// declaration the binary extracted via `concord-ast`.
    pub fn register_contract(
        &self,
        key: &str,
        signature: &str,
        by: &str,
        with: &str,
    ) -> Result<bool> {
        let dir = self.paths.contract_dir(key);
        let is_new = !dir.is_dir();
        mkdirs(&dir)?;
        self.write_atomic(&dir.join("key"), &format!("{key}\n"))?;
        self.write_atomic(&dir.join("signature"), &format!("{signature}\n"))?;
        self.write_atomic(&dir.join("by"), &format!("{by}\n"))?;
        self.write_atomic(&dir.join("with"), &format!("{with}\n"))?;
        self.write_atomic(&dir.join("since"), &format!("{}\n", self.now))?;
        let verb = if is_new { "contract" } else { "contract-update" };
        self.append_log(by, &format!("{verb}: {key} = {signature}"))?;
        Ok(is_new)
    }

    /// Read the contract for `key`, or `None` if none is registered.
    pub fn contract(&self, key: &str) -> Result<Option<Contract>> {
        let dir = self.paths.contract_dir(key);
        if !dir.is_dir() {
            return Ok(None);
        }
        Ok(Some(Contract {
            key: read_trimmed(&dir.join("key")).unwrap_or_else(|| key.to_string()),
            signature: read_trimmed(&dir.join("signature")).unwrap_or_default(),
            by: read_trimmed(&dir.join("by")).unwrap_or_default(),
            with: read_trimmed(&dir.join("with")).unwrap_or_default(),
            since: read_trimmed(&dir.join("since")).unwrap_or_default(),
        }))
    }

    /// All registered contracts (for `contracts` / `contract-check` / status).
    pub fn contracts(&self) -> Result<Vec<Contract>> {
        let mut out = Vec::new();
        for slug in sorted_entries(&self.paths.contracts)? {
            let key = read_trimmed(&self.paths.contracts.join(&slug).join("key"));
            if let Some(k) = key {
                if let Some(c) = self.contract(&k)? {
                    out.push(c);
                }
            }
        }
        Ok(out)
    }

    /// Drop a contract (renegotiation away / no longer enforced). Returns whether one existed.
    pub fn release_contract(&self, key: &str, by: &str) -> Result<bool> {
        let dir = self.paths.contract_dir(key);
        if !dir.is_dir() {
            return Ok(false);
        }
        fs::remove_dir_all(&dir).map_err(|e| ConcordError::io(&dir, e))?;
        self.append_log(by, &format!("contract-release: {key}"))?;
        Ok(true)
    }

    // ──────────────────────────── telemetry / health (F4) ────────────────────────────

    /// Append a normalized telemetry datapoint for `id` (the daemon's OTLP receiver calls
    /// this after parsing a Claude Code metric). Privacy: only metric attributes, never
    /// prompt content.
    pub fn record_telemetry(&self, id: &str, point: &TelemetryPoint) -> Result<()> {
        let f = self.paths.telemetry_file(id);
        if let Some(dir) = f.parent() {
            mkdirs(dir)?;
        }
        append_file(&f, &format!("{}\n", point.to_line()))
    }

    fn read_telemetry(&self, id: &str) -> Result<Vec<TelemetryPoint>> {
        let f = self.paths.telemetry_file(id);
        match fs::read_to_string(&f) {
            Ok(body) => Ok(body.lines().filter_map(TelemetryPoint::parse_line).collect()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(ConcordError::io(&f, e)),
        }
    }

    /// The health verdict for one session (F4), or `None` if it has no telemetry yet.
    pub fn session_health(&self, id: &str, cfg: &TelemetryConfig) -> Result<Option<SessionHealth>> {
        let points = self.read_telemetry(id)?;
        if points.is_empty() {
            return Ok(None);
        }
        Ok(Some(health(id, &points, cfg, self.now)))
    }

    /// Health for every session with telemetry — the `hub` TELEMETRY/HEALTH surface.
    pub fn all_health(&self, cfg: &TelemetryConfig) -> Result<Vec<SessionHealth>> {
        let mut out = Vec::new();
        for name in sorted_entries(&self.paths.telemetry)? {
            let id = name.strip_suffix(".jsonl").unwrap_or(&name).to_string();
            if let Some(h) = self.session_health(&id, cfg)? {
                out.push(h);
            }
        }
        Ok(out)
    }

    /// Does `id` hold any live lease? (B3 watchdog predicate.)
    fn holds_any_lease(&self, id: &str) -> Result<bool> {
        for slug in sorted_entries(&self.paths.leases)? {
            let dir = self.paths.lease_dir(&slug);
            if dir.is_dir() && read_trimmed(&dir.join("holder")).as_deref() == Some(id) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// B3 watchdog (F4 → F3): a session that has gone telemetry-**idle** while it still
    /// holds a lease or has un-ACK'd directives is auto-escalated to the coordinator
    /// (severity High), reusing the F3 escalation machine. Dedup: a `<id>.watchdog` marker
    /// prevents re-escalation until the session recovers (telemetry resumes), when the
    /// marker is cleared. Returns the escalation seqs raised this tick.
    pub fn telemetry_watchdog(&self, cfg: &TelemetryConfig, coordinator: &str) -> Result<Vec<u64>> {
        let mut raised = Vec::new();
        for h in self.all_health(cfg)? {
            let marker = self.paths.telemetry.join(format!("{}.watchdog", slug::slug(&h.id)));
            let dark = h.flag == HealthFlag::Idle
                && (self.holds_any_lease(&h.id)? || !self.read_pending(&h.id)?.is_empty());
            if dark {
                if !marker.exists() {
                    let mins = h.idle_secs / 60;
                    let about = format!(
                        "{} is telemetry-idle for {mins} min while holding work (lease/pending) — possible going-dark/stuck",
                        h.id
                    );
                    let seq = self.escalate("concordd", coordinator, Severity::High, &about, Some(&h.id))?;
                    let _ = self.write_atomic(&marker, "1\n");
                    raised.push(seq);
                }
            } else {
                // Recovered → allow a future re-escalation.
                let _ = fs::remove_file(&marker);
            }
        }
        Ok(raised)
    }

    fn read_pending(&self, id: &str) -> Result<Vec<Pending>> {
        let f = self.paths.pending_file(id);
        match fs::read_to_string(&f) {
            Ok(body) => Ok(body.lines().filter_map(Pending::parse_line).collect()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(ConcordError::io(&f, e)),
        }
    }

    fn write_pending(&self, id: &str, items: &[Pending]) -> Result<()> {
        let f = self.paths.pending_file(id);
        if items.is_empty() {
            let _ = fs::remove_file(&f);
            return Ok(());
        }
        let body: String = items.iter().map(|p| format!("{}\n", p.to_line())).collect();
        self.write_atomic(&f, &body)
    }

    // ──────────────────────────────── log / sync ────────────────────────────────

    /// `log <id> <event...>`: append a structured ledger record.
    pub fn log(&self, id: &str, event: &str) -> Result<()> {
        self.append_log(id, event)?;
        Ok(())
    }

    /// `init`: scaffold a project's coordination state idempotently. The `sessions/`
    /// and `leases/` dirs already exist (created on `open`); this additionally creates
    /// the prose channel (with a header) and the ledger so the project is fully
    /// bootstrapped and inspectable. Safe to re-run — existing files are left intact.
    pub fn init(&self) -> Result<()> {
        if !self.paths.sync.exists() {
            if let Some(dir) = self.paths.sync.parent() {
                mkdirs(dir)?;
            }
            let header = format!(
                "# Concord prose channel — {}\n\n> The human audit/discussion log. Structured state lives in {}.\n",
                self.paths.project.display(),
                self.paths.coord.display()
            );
            fs::write(&self.paths.sync, header)
                .map_err(|e| ConcordError::io(&self.paths.sync, e))?;
        }
        if !self.paths.log.exists() {
            fs::write(&self.paths.log, "").map_err(|e| ConcordError::io(&self.paths.log, e))?;
        }
        Ok(())
    }

    /// `send`: deliver a typed message to the recipient's inbox (`inbox/<to>.jsonl`),
    /// the first-class WP7 path. Same on-disk shape the daemon's demux writes, so a
    /// consumer reads `send`-delivered and derived messages uniformly.
    pub fn deliver_message(&self, msg: &Message) -> Result<()> {
        let dir = self.paths.coord.join("inbox");
        mkdirs(&dir)?;
        append_file(&dir.join(format!("{}.jsonl", msg.to)), &msg.to_jsonl())
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
                // S2: symbol-aware composition (path ⊃ symbol, disjoint symbols compatible).
                Some(ha) => (slug::area_overlaps(area, ha), ha == area),
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
