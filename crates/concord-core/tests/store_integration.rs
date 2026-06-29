//! Integration tests for the filesystem-backed [`Store`]: overlap rejection, fence
//! monotonicity, ownership-enforced release, and reading shell-shaped leases (no
//! `area` file) — the WP12 M1 hardening the coordinator asked for.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use concord_core::config::TelemetryConfig;
use concord_core::escalation::{EscStatus, ResolveOutcome, Severity};
use concord_core::store::{
    ClaimOutcome, HoldStatus, MergeUnlockOutcome, OverlapPolicy, ReleaseOutcome, ResourceOutcome,
    ResourceReleaseOutcome,
};
use concord_core::telemetry::{HealthFlag, TelemetryPoint};
use concord_core::{Paths, Store};

/// F5: a signature contract is registered, read back, updated (renegotiation), and released.
#[test]
fn contract_register_update_release() {
    let (root, paths) = temp_paths();
    let s = store_at(&paths, 1000);

    assert!(s.register_contract("src/lib.rs:foo", "fn foo(x: u32) -> bool", "a", "b").unwrap(), "new");
    let c = s.contract("src/lib.rs:foo").unwrap().unwrap();
    assert_eq!(c.signature, "fn foo(x: u32) -> bool");
    assert_eq!(c.by, "a");
    assert_eq!(c.with, "b");
    assert_eq!(s.contracts().unwrap().len(), 1);

    // Re-register = explicit renegotiation (returns false = updated).
    assert!(!s.register_contract("src/lib.rs:foo", "fn foo(x: u32, y: u32) -> bool", "a", "b").unwrap());
    assert_eq!(s.contract("src/lib.rs:foo").unwrap().unwrap().signature, "fn foo(x: u32, y: u32) -> bool");

    assert!(s.release_contract("src/lib.rs:foo", "a").unwrap());
    assert!(s.contract("src/lib.rs:foo").unwrap().is_none());
    assert!(!s.release_contract("src/lib.rs:foo", "a").unwrap(), "already gone");

    let _ = std::fs::remove_dir_all(root);
}

/// F4: recorded telemetry drives the health verdict; the B3 watchdog auto-escalates a
/// telemetry-idle session that still holds a lease (reusing F3), and clears on recovery.
#[test]
fn telemetry_health_and_watchdog() {
    let (root, paths) = temp_paths();
    let cfg = TelemetryConfig { enabled: true, idle_min: 15, ..TelemetryConfig::default() };

    // No telemetry yet → no health, watchdog raises nothing.
    let s0 = store_at(&paths, 100_000);
    assert!(s0.session_health("w", &cfg).unwrap().is_none());
    assert!(s0.telemetry_watchdog(&cfg, "hub").unwrap().is_empty());

    // Record an old token datapoint, and `w` holds a lease → it is dark (idle+work).
    let t_old = 100_000u64;
    store_at(&paths, t_old).register("w", "").unwrap();
    store_at(&paths, t_old).claim("w", "src/x.rs", "", OverlapPolicy::RejectOverlap).unwrap();
    store_at(&paths, t_old)
        .record_telemetry("w", &TelemetryPoint { ts: t_old, metric: "token".into(), value: 50.0, attr: "output".into() })
        .unwrap();

    // 20 min later (> idle_min): health = Idle, watchdog escalates once.
    let now = t_old + 20 * 60;
    let h = store_at(&paths, now).session_health("w", &cfg).unwrap().unwrap();
    assert_eq!(h.flag, HealthFlag::Idle);
    let raised = store_at(&paths, now).telemetry_watchdog(&cfg, "hub").unwrap();
    assert_eq!(raised.len(), 1, "dark session escalated");

    // The escalation is High, from concordd, references the session.
    let escs = store_at(&paths, now).escalations().unwrap();
    assert_eq!(escs[0].severity, Severity::High);
    assert_eq!(escs[0].reference.as_deref(), Some("w"));

    // Dedup: a second tick does NOT re-escalate (marker present).
    assert!(store_at(&paths, now + 60).telemetry_watchdog(&cfg, "hub").unwrap().is_empty());

    // Recovery: fresh telemetry → not idle → marker cleared, and a later relapse re-escalates.
    let t2 = now + 120;
    store_at(&paths, t2)
        .record_telemetry("w", &TelemetryPoint { ts: t2, metric: "token".into(), value: 10.0, attr: "output".into() })
        .unwrap();
    assert!(store_at(&paths, t2).telemetry_watchdog(&cfg, "hub").unwrap().is_empty()); // healthy → clears marker
    let later = t2 + 20 * 60;
    assert_eq!(store_at(&paths, later).telemetry_watchdog(&cfg, "hub").unwrap().len(), 1, "re-escalates after recovery");

    let _ = std::fs::remove_dir_all(root);
}

/// F3/E2: a tracked escalation persists until resolved; `escalations` lists open first.
#[test]
fn escalation_lifecycle() {
    let (root, paths) = temp_paths();
    let s = store_at(&paths, 1000);
    let e1 = s.escalate("a", "hub", Severity::High, "build-env deadlock", None).unwrap();
    let e2 = s.escalate("a", "hub", Severity::Critical, "vision blocker", Some("B7.9")).unwrap();
    assert_eq!((e1, e2), (1, 2));

    let open = s.escalations().unwrap();
    assert_eq!(open.len(), 2);
    assert_eq!(open[0].seq, 2, "open, newest first");
    assert_eq!(open[0].severity, Severity::Critical);
    assert_eq!(open[0].reference.as_deref(), Some("B7.9"));

    assert_eq!(s.resolve_escalation("hub", 1, "freed").unwrap(), ResolveOutcome::Resolved);
    assert_eq!(s.resolve_escalation("hub", 1, "again").unwrap(), ResolveOutcome::AlreadyResolved);
    assert_eq!(s.resolve_escalation("hub", 99, "x").unwrap(), ResolveOutcome::NotFound);

    let after = s.escalations().unwrap();
    assert_eq!(after[0].seq, 2);
    assert_eq!(after[0].status, EscStatus::Open);
    assert_eq!(after[1].status, EscStatus::Resolved);

    let _ = std::fs::remove_dir_all(root);
}

/// F3/E3: a directive becomes pending; an un-ACK'd directive is re-delivered K times then
/// auto-escalated (severity High); a poster's own post clears its pending (derived ack).
#[test]
fn ack_tracking_redeliver_then_auto_escalate() {
    let (root, paths) = temp_paths();
    let t0 = 1000u64;
    let ttl = 900u64; // 15 min
    let k = 2u32;

    store_at(&paths, t0).add_pending("w", "hub").unwrap();
    assert_eq!(store_at(&paths, t0).pending_summary().unwrap(), vec![("w".to_string(), 1u32, t0)]);

    // Not yet due (before t0 + ttl): no action.
    let tick0 = store_at(&paths, t0 + 10).tick_acks(ttl, k, "hub").unwrap();
    assert!(tick0.redelivered.is_empty() && tick0.escalated.is_empty());

    // t0+ttl → first re-delivery; t0+2ttl → second; t0+3ttl → auto-escalate.
    assert_eq!(store_at(&paths, t0 + ttl).tick_acks(ttl, k, "hub").unwrap().redelivered.len(), 1);
    assert_eq!(store_at(&paths, t0 + 2 * ttl).tick_acks(ttl, k, "hub").unwrap().redelivered.len(), 1);
    let r3 = store_at(&paths, t0 + 3 * ttl).tick_acks(ttl, k, "hub").unwrap();
    assert_eq!(r3.escalated.len(), 1, "auto-escalated after K misses");
    assert!(r3.redelivered.is_empty());

    // The auto-escalation is High, from concordd, references the un-acking session.
    let esc = store_at(&paths, t0 + 3 * ttl).escalations().unwrap();
    assert_eq!(esc.len(), 1);
    assert_eq!(esc[0].severity, Severity::High);
    assert_eq!(esc[0].from, "concordd");
    assert_eq!(esc[0].reference.as_deref(), Some("w"));

    // Further ticks do not re-escalate (pending is marked escalated).
    let r4 = store_at(&paths, t0 + 9 * ttl).tick_acks(ttl, k, "hub").unwrap();
    assert!(r4.escalated.is_empty() && r4.redelivered.is_empty());

    // Derived ack: once `w` posts (clear_pending), the debt is gone.
    store_at(&paths, t0 + 9 * ttl).clear_pending("w").unwrap();
    assert!(store_at(&paths, t0 + 9 * ttl).pending_summary().unwrap().is_empty());

    let _ = std::fs::remove_dir_all(root);
}

/// F2: an N-slot resource semaphore hands out distinct slots in parallel, reports BUSY
/// when full, self-heals a stale holder's slot, and is orthogonal to path leases.
#[test]
fn resource_semaphore_slots_busy_and_self_heal() {
    let (root, paths) = temp_paths();
    let s = store_at(&paths, 1000);
    s.register("a", "").unwrap();
    s.register("b", "").unwrap();
    s.register("c", "").unwrap();

    // Capacity 2: a and b each get a distinct slot; c finds the pool full.
    match s.acquire_resource("a", "qemu-port", 2, "vm").unwrap() {
        ResourceOutcome::Acquired { slot, capacity, .. } => { assert_eq!((slot, capacity), (0, 2)); }
        o => panic!("a should get slot 0: {o:?}"),
    }
    match s.acquire_resource("b", "qemu-port", 2, "vm").unwrap() {
        ResourceOutcome::Acquired { slot, .. } => assert_eq!(slot, 1),
        o => panic!("b should get slot 1: {o:?}"),
    }
    assert_eq!(s.acquire_resource("c", "qemu-port", 2, "vm").unwrap(), ResourceOutcome::Busy { capacity: 2 });

    // Idempotent: a re-acquiring returns its existing slot.
    assert_eq!(s.acquire_resource("a", "qemu-port", 2, "vm").unwrap(), ResourceOutcome::AlreadyHeld { slot: 0 });
    // Capacity validation: a mismatched --slots is rejected.
    assert_eq!(s.acquire_resource("c", "qemu-port", 5, "vm").unwrap(), ResourceOutcome::CapacityMismatch { declared: 2 });

    // a goes stale (heartbeat far in the past) → c reclaims a's slot (pool self-heals).
    let later = store_at(&paths, 1000 + 4000); // > TTL (1800)
    match later.acquire_resource("c", "qemu-port", 2, "vm").unwrap() {
        ResourceOutcome::Reclaimed { slot, previous, .. } => { assert_eq!(slot, 0); assert_eq!(previous, "a"); }
        o => panic!("c should reclaim a's stale slot 0: {o:?}"),
    }

    // Orthogonal to path leases: a file lease named like the resource does not conflict.
    assert_eq!(later.claim("b", "qemu-port", "", OverlapPolicy::RejectOverlap).unwrap(), ClaimOutcome::Claimed);

    // Release returns the slot; a second release reports NotHeld.
    assert_eq!(later.release_resource("b", "qemu-port").unwrap(), ResourceReleaseOutcome::Released { slot: 1 });
    assert_eq!(later.release_resource("b", "qemu-port").unwrap(), ResourceReleaseOutcome::NotHeld);

    let _ = std::fs::remove_dir_all(root);
}

/// F2 + F1/A2: SessionEnd teardown also frees the session's resource slots (the AUFLAGE).
#[test]
fn session_end_releases_resource_slots() {
    let (root, paths) = temp_paths();
    let s = store_at(&paths, 1000);
    s.register("a", "").unwrap();
    s.acquire_resource("a", "build-env", 1, "build").unwrap();
    s.acquire_resource("a", "qemu-port", 4, "vm").unwrap();

    let r = s.session_end("a").unwrap();
    assert_eq!(r.resources_released.len(), 2, "both resource slots freed: {:?}", r.resources_released);
    // The pool is now empty — a fresh session takes slot 0 of each.
    s.register("b", "").unwrap();
    assert!(matches!(s.acquire_resource("b", "build-env", 1, "build").unwrap(), ResourceOutcome::Acquired { slot: 0, .. }));

    let _ = std::fs::remove_dir_all(root);
}

/// F1/A2: clean-exit teardown releases the ending session's leases + merge-lock and
/// deregisters it, leaves other sessions untouched, and is idempotent.
#[test]
fn session_end_releases_all_and_is_idempotent() {
    let (root, paths) = temp_paths();
    let s = store_at(&paths, 1000);
    s.register("a", "work").unwrap();
    s.register("b", "work").unwrap();
    assert_eq!(s.claim("a", "src/x.rs", "", OverlapPolicy::RejectOverlap).unwrap(), ClaimOutcome::Claimed);
    assert_eq!(s.claim("a", "src/y.rs", "", OverlapPolicy::RejectOverlap).unwrap(), ClaimOutcome::Claimed);
    assert_eq!(s.claim("b", "docs/z.md", "", OverlapPolicy::RejectOverlap).unwrap(), ClaimOutcome::Claimed);
    s.merge_lock("a", "merge").unwrap();

    let r = s.session_end("a").unwrap();
    assert_eq!(r.released.len(), 2, "both of a's leases released: {:?}", r.released);
    assert!(r.merge_unlocked, "a held the merge-lock");
    assert!(r.deregistered, "a's registry entry removed");

    // b's lease + registration survive; the merge-lock is free.
    let st = s.status().unwrap();
    assert!(st.sessions.iter().any(|x| x.id == "b"), "b still registered");
    assert!(!st.sessions.iter().any(|x| x.id == "a"), "a deregistered");
    assert_eq!(st.leases.len(), 1, "only b's lease remains");
    assert_eq!(st.leases[0].holder, "b");
    assert_eq!(st.merge_lock_holder, None);

    // Idempotent: a second teardown does nothing and does not error.
    let r2 = s.session_end("a").unwrap();
    assert!(r2.released.is_empty() && !r2.merge_unlocked && !r2.deregistered);

    let _ = std::fs::remove_dir_all(root);
}

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A throwaway coord dir under the system temp dir, unique per test without RNG.
fn temp_paths() -> (PathBuf, Paths) {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("concord-it-{}-{}", std::process::id(), n));
    let coord = root.join("coord");
    let paths = Paths {
        sessions: coord.join("sessions"),
        leases: coord.join("leases"),
        resources: coord.join("resources"),
        acks: coord.join("acks"),
        escalations: coord.join("escalations"),
        telemetry: coord.join("telemetry"),
        contracts: coord.join("contracts"),
        log: coord.join("intents.jsonl"),
        merge_lock: coord.join("merge.lock"),
        coord: coord.clone(),
        sync: root.join("SYNC.md"),
        project: root.join("proj"),
        ttl: 1800,
    };
    (root, paths)
}

fn store_at(paths: &Paths, now: u64) -> Store {
    Store::open_at(paths.clone(), now).expect("open store")
}

#[test]
fn overlap_rejected_under_reject_policy_allowed_under_parity() {
    let (root, paths) = temp_paths();
    let s = store_at(&paths, 1000);
    s.register("a", "work").unwrap();
    s.register("b", "work").unwrap();

    // a holds the parent subtree.
    assert_eq!(
        s.claim("a", "kernel/src/embedded", "parent", OverlapPolicy::RejectOverlap)
            .unwrap(),
        ClaimOutcome::Claimed
    );

    // b's child claim is rejected as an overlap under RejectOverlap…
    match s
        .claim(
            "b",
            "kernel/src/embedded/usbd",
            "child",
            OverlapPolicy::RejectOverlap,
        )
        .unwrap()
    {
        ClaimOutcome::OverlapConflict { area, holder } => {
            assert_eq!(area, "kernel/src/embedded");
            assert_eq!(holder, "a");
        }
        other => panic!("expected OverlapConflict, got {other:?}"),
    }

    // …but the shell-parity policy does not detect it (distinct slug ⇒ fresh mkdir).
    assert_eq!(
        s.claim(
            "b",
            "kernel/src/embedded/usbd",
            "child",
            OverlapPolicy::ParityShell
        )
        .unwrap(),
        ClaimOutcome::Claimed
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn fence_is_monotonic_and_recorded_on_lease() {
    let (root, paths) = temp_paths();
    let s = store_at(&paths, 2000);
    s.register("a", "").unwrap(); // fence 1
    s.claim("a", "area/one", "why", OverlapPolicy::RejectOverlap)
        .unwrap(); // fence 2

    let fence_file = paths.coord.join("fence");
    let v: u64 = std::fs::read_to_string(&fence_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(v, 2, "two logged actions ⇒ fence 2");

    // The lease carries the fence token at acquisition.
    let report = s.status().unwrap();
    let lease = report
        .leases
        .iter()
        .find(|l| l.area == "area/one")
        .expect("lease present");
    assert_eq!(lease.fence, 2);

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn release_reports_no_lease_then_released() {
    let (root, paths) = temp_paths();
    let s = store_at(&paths, 3000);
    s.register("a", "").unwrap();
    assert_eq!(
        s.release("a", "x/y", None).unwrap(),
        ReleaseOutcome::NoLease,
        "releasing an unheld area is a no-op"
    );
    s.claim("a", "x/y", "", OverlapPolicy::RejectOverlap).unwrap();
    assert_eq!(s.release("a", "x/y", None).unwrap(), ReleaseOutcome::Released);

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn release_enforces_ownership_and_fence_floor() {
    let (root, paths) = temp_paths();
    let s = store_at(&paths, 5000);
    s.register("a", "").unwrap();
    s.register("b", "").unwrap();
    s.claim("a", "area/x", "mine", OverlapPolicy::RejectOverlap)
        .unwrap();

    // b cannot release a's lease (ownership enforcement; the shell would delete it).
    match s.release("b", "area/x", None).unwrap() {
        ReleaseOutcome::NotYours { holder } => assert_eq!(holder, "a"),
        other => panic!("expected NotYours, got {other:?}"),
    }
    // The lease is still there.
    assert!(matches!(
        s.verify_hold("a", "area/x").unwrap(),
        HoldStatus::Held { .. }
    ));

    // a presenting a stale fence is refused (fencing Floor).
    let fence = match s.verify_hold("a", "area/x").unwrap() {
        HoldStatus::Held { fence } => fence,
        other => panic!("expected Held, got {other:?}"),
    };
    match s.release("a", "area/x", Some(fence + 99)).unwrap() {
        ReleaseOutcome::FenceStale { current } => assert_eq!(current, fence),
        other => panic!("expected FenceStale, got {other:?}"),
    }
    // a with the correct fence succeeds.
    assert_eq!(
        s.release("a", "area/x", Some(fence)).unwrap(),
        ReleaseOutcome::Released
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn merge_unlock_enforces_ownership() {
    let (root, paths) = temp_paths();
    let s = store_at(&paths, 6000);
    s.register("a", "").unwrap();
    s.register("b", "").unwrap();
    assert_eq!(s.merge_unlock("a").unwrap(), MergeUnlockOutcome::NotHeld);
    s.merge_lock("a", "merge").unwrap();
    // b cannot unlock a's merge lock.
    match s.merge_unlock("b").unwrap() {
        MergeUnlockOutcome::NotYours { holder } => assert_eq!(holder, "a"),
        other => panic!("expected NotYours, got {other:?}"),
    }
    assert_eq!(s.merge_unlock("a").unwrap(), MergeUnlockOutcome::Released);

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn verify_hold_reports_all_states() {
    let (root, paths) = temp_paths();
    let s = store_at(&paths, 7000);
    s.register("a", "").unwrap();
    s.register("b", "").unwrap();
    assert_eq!(s.verify_hold("a", "z/z").unwrap(), HoldStatus::Vacant);
    s.claim("a", "z/z", "", OverlapPolicy::RejectOverlap).unwrap();
    assert!(matches!(
        s.verify_hold("a", "z/z").unwrap(),
        HoldStatus::Held { .. }
    ));
    match s.verify_hold("b", "z/z").unwrap() {
        HoldStatus::HeldByOther { holder } => assert_eq!(holder, "a"),
        other => panic!("expected HeldByOther, got {other:?}"),
    }

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn reads_shell_shaped_lease_without_area_file() {
    // Simulate a lease created by the shell: a `leases/<slug>/` dir with only
    // holder/since/why (no `area`/`fence`). The store must still read it and, in
    // overlap checks, fall back to slug-space.
    let (root, paths) = temp_paths();
    let s = store_at(&paths, 4000);
    s.register("a", "").unwrap();
    s.register("b", "").unwrap();

    let slug_dir = paths.leases.join("kernel_src_embedded");
    std::fs::create_dir_all(&slug_dir).unwrap();
    std::fs::write(slug_dir.join("holder"), "a\n").unwrap();
    std::fs::write(slug_dir.join("since"), "4000\n").unwrap();
    std::fs::write(slug_dir.join("why"), "shell-made\n").unwrap();

    // status reads it, de-slugging the area as a fallback.
    let report = s.status().unwrap();
    let lease = report
        .leases
        .iter()
        .find(|l| l.holder == "a")
        .expect("shell lease visible");
    assert_eq!(lease.area_slug, "kernel_src_embedded");
    assert_eq!(lease.fence, 0, "shell lease has no fence ⇒ 0");

    // Overlap detection still fires against the shell lease via slug-space fallback.
    match s
        .claim(
            "b",
            "kernel/src/embedded/usbd",
            "child",
            OverlapPolicy::RejectOverlap,
        )
        .unwrap()
    {
        ClaimOutcome::OverlapConflict { holder, .. } => assert_eq!(holder, "a"),
        other => panic!("expected OverlapConflict against shell lease, got {other:?}"),
    }

    std::fs::remove_dir_all(&root).ok();
}
