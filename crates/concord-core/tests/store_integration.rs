//! Integration tests for the filesystem-backed [`Store`]: overlap rejection, fence
//! monotonicity, ownership-enforced release, and reading shell-shaped leases (no
//! `area` file) — the WP12 M1 hardening the coordinator asked for.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use concord_core::store::{
    ClaimOutcome, HoldStatus, MergeUnlockOutcome, OverlapPolicy, ReleaseOutcome,
};
use concord_core::{Paths, Store};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A throwaway coord dir under the system temp dir, unique per test without RNG.
fn temp_paths() -> (PathBuf, Paths) {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("concord-it-{}-{}", std::process::id(), n));
    let coord = root.join("coord");
    let paths = Paths {
        sessions: coord.join("sessions"),
        leases: coord.join("leases"),
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
