//! `concord` — the CLI binary. A drop-in for `bin/coord.sh`: same verbs, same
//! argument order, same on-disk format, byte-compatible stdout.
//!
//! Verb dispatch mirrors `coord.sh` exactly (command-first):
//! ```text
//!   concord register <id> <focus>
//!   concord heartbeat <id>
//!   concord status
//!   concord claim <id> <area> [why]
//!   concord release <id> <area>
//!   concord merge-lock <id> [why]
//!   concord merge-unlock <id>
//!   concord log <id> <event...>
//!   concord sync <id> <target> <topic> <body>
//! ```
//! With no command it defaults to `status` (shell parity). `--version`/`version`
//! is an additive convenience (not in coord.sh; printed from Cargo.toml).
//!
//! Exit codes match the shell: claim CONFLICT and merge-lock-held ⇒ 2; unknown
//! command ⇒ 1; missing required arg ⇒ 1.

use std::process::ExitCode;

mod hooks_embed;
mod launcher;

use concord_core::ipc::{Request, Response, SOCKET_NAME};
use concord_core::message::{Message, MessageKind};
use concord_core::store::{
    ClaimOutcome, HoldStatus, LeaseCheck, MergeLockOutcome, MergeUnlockOutcome, OverlapPolicy,
    ReleaseOutcome, ResourceOutcome, ResourceReleaseOutcome, StatusReport, Store,
};
use concord_core::{Paths, Result};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("concord: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Dispatch one invocation. Returns the process exit code.
fn run(args: &[String]) -> Result<ExitCode> {
    let cmd = args.first().map(String::as_str).unwrap_or("status");
    let rest = &args[args.len().min(1)..];

    // Additive: version flags (coord.sh has none; harmless for parity).
    if matches!(cmd, "version" | "--version" | "-v") {
        println!("concord {VERSION}");
        return Ok(ExitCode::SUCCESS);
    }

    let store = Store::open(Paths::from_cwd())?;

    match cmd {
        "register" => {
            let id = require(rest, 0, "session id")?;
            let focus = opt(rest, 1).unwrap_or("");
            store.register(id, focus)?;
            println!("registered session '{id}' (focus: {focus})");
            print_status(&store)?;
            Ok(ExitCode::SUCCESS)
        }

        "heartbeat" => {
            let id = require(rest, 0, "session id")?;
            store.heartbeat(id)?;
            Ok(ExitCode::SUCCESS)
        }

        "session-end" => {
            // F1/A2: clean-exit teardown — release all of id's leases, drop the merge-lock
            // if held, and deregister. Idempotent; driven by the SessionEnd hook.
            let id = require(rest, 0, "session id")?;
            let r = store.session_end(id)?;
            if r.released.is_empty()
                && r.resources_released.is_empty()
                && !r.merge_unlocked
                && !r.deregistered
            {
                println!("session-end {id}: nothing to release (already clean)");
            } else {
                let mut parts = Vec::new();
                if !r.released.is_empty() {
                    parts.push(format!("released {}: {}", r.released.len(), r.released.join(", ")));
                }
                if !r.resources_released.is_empty() {
                    parts.push(format!(
                        "resources {}: {}",
                        r.resources_released.len(),
                        r.resources_released.join(", ")
                    ));
                }
                if r.merge_unlocked {
                    parts.push("merge-unlock".to_string());
                }
                if r.deregistered {
                    parts.push("deregistered".to_string());
                }
                println!("session-end {id}: {}", parts.join("; "));
            }
            Ok(ExitCode::SUCCESS)
        }

        "status" => {
            print_status(&store)?;
            Ok(ExitCode::SUCCESS)
        }

        // ── launcher (S1) — the former bin/concord, now folded into the one tool ──
        "start" => {
            let id = require(rest, 0, "session id")?;
            let print = rest.iter().any(|a| a == "--print" || a == "--dry-run");
            Ok(launcher::cmd_start(store.paths(), id, print))
        }
        "dash" => Ok(launcher::cmd_dash(&store)),
        "pause" => {
            let id = require(rest, 0, "session id")?;
            Ok(launcher::cmd_pause(store.paths(), id))
        }
        "resume" => {
            let id = require(rest, 0, "session id")?;
            Ok(launcher::cmd_resume(store.paths(), id))
        }
        "stop" => {
            let id = require(rest, 0, "session id")?;
            Ok(launcher::cmd_stop(&store, id))
        }

        "symbols" => {
            // List the top-level symbols a (Rust) file defines — the claimable
            // symbol-leases under that path (S2).
            let file = require(rest, 0, "file")?;
            let path = store.paths().project.join(file);
            let Some(lang) = concord_ast::Lang::from_path(file) else {
                println!("unsupported file type: {file} (rust/typescript/python)");
                return Ok(ExitCode::from(2));
            };
            match std::fs::read_to_string(&path) {
                Ok(src) => {
                    let syms = concord_ast::extract_symbols(lang, &src);
                    if syms.is_empty() {
                        println!("(no symbols found in {file})");
                    }
                    for s in &syms {
                        println!(
                            "{file}:{}  [{}]  lines {}-{}",
                            s.name,
                            s.kind,
                            s.start_row + 1,
                            s.end_row + 1
                        );
                    }
                    Ok(ExitCode::SUCCESS)
                }
                Err(e) => {
                    println!("cannot read {}: {e}", path.display());
                    Ok(ExitCode::from(2))
                }
            }
        }

        "paths" => {
            // Emit the resolved coordination paths as eval-able shell assignments —
            // `eval "$(concord paths)"` gives a script/hook the right env for THIS
            // project (multi-project single-source-of-truth, M5).
            let p = store.paths();
            println!("CONCORD_DIR={}", p.coord.display());
            println!("CONCORD_SYNC={}", p.sync.display());
            println!("CONCORD_PROJECT={}", p.project.display());
            Ok(ExitCode::SUCCESS)
        }

        "init" => {
            // Bootstrap a project's coordination state (idempotent). Resolve paths from
            // --project when given, else the cwd convention; scaffold dirs+sync+ledger;
            // optionally register a comma-separated --ids list.
            let init_store = match flag_value(rest, "--project") {
                Some(p) => Store::open(Paths::resolve(std::path::Path::new(p)))?,
                None => store,
            };
            init_store.init()?;
            let ids: Vec<&str> = flag_value(rest, "--ids")
                .map(|s| s.split(',').map(str::trim).filter(|s| !s.is_empty()).collect())
                .unwrap_or_default();
            for id in &ids {
                init_store.register(id, "(init)")?;
            }
            let p = init_store.paths();
            println!("initialized coordination state:");
            println!("  coord:   {}", p.coord.display());
            println!("  sync:    {}", p.sync.display());
            println!("  project: {}", p.project.display());
            if ids.is_empty() {
                println!("  sessions: (none registered — use `concord register <id> <focus>`)");
            } else {
                println!("  sessions: {}", ids.join(", "));
            }
            // --with-hooks: also lay down + wire the Claude Code automation hooks (so a
            // fresh project is one command from coordinated). --no-wire skips settings.
            if has_flag(rest, "--with-hooks") {
                let wire = !has_flag(rest, "--no-wire");
                return Ok(hooks_embed::cmd_install_hooks(p, wire));
            }
            Ok(ExitCode::SUCCESS)
        }

        "install-hooks" => {
            // Materialize the embedded Claude Code automation hooks into <coord>/hooks/
            // and (on Unix, unless --no-wire) wire ~/.claude/settings.json. Lets a shipped
            // binary set up session automation with no repo checkout.
            let wire = !has_flag(rest, "--no-wire");
            Ok(hooks_embed::cmd_install_hooks(store.paths(), wire))
        }

        "claim" if is_resource(rest) => {
            // F2: claim a slot of a named resource semaphore (orthogonal namespace).
            //   concord claim <id> <name> --kind resource [--slots N] [why]
            let pos = positional_args(rest, &["--kind", "--slots"]);
            let id = pos.first().map(String::as_str).ok_or(missing("session id"))?;
            let name = pos.get(1).map(String::as_str).ok_or(missing("resource name"))?;
            let why = pos.get(2).map(String::as_str).unwrap_or("");
            let slots: u32 = flag_value(rest, "--slots").and_then(|s| s.parse().ok()).unwrap_or(1);
            match store.acquire_resource(id, name, slots, why)? {
                ResourceOutcome::Acquired { slot, capacity, fence } => {
                    println!("RESOURCE-ACQUIRED {name} slot {slot}/{capacity} (fence {fence})");
                    Ok(ExitCode::SUCCESS)
                }
                ResourceOutcome::Reclaimed { slot, capacity, previous } => {
                    println!("RESOURCE-RECLAIMED {name} slot {slot}/{capacity} (stale holder {previous})");
                    Ok(ExitCode::SUCCESS)
                }
                ResourceOutcome::AlreadyHeld { slot } => {
                    println!("already holding {name} slot {slot}");
                    Ok(ExitCode::SUCCESS)
                }
                ResourceOutcome::Busy { capacity } => {
                    println!("RESOURCE-BUSY {name} ({capacity}/{capacity} slots held) — coordinate first (status / SESSION-SYNC)");
                    Ok(ExitCode::from(2))
                }
                ResourceOutcome::CapacityMismatch { declared } => {
                    println!("RESOURCE-CAPACITY-MISMATCH {name}: declared capacity is {declared} — retry with --slots {declared}");
                    Ok(ExitCode::from(2))
                }
            }
        }

        "release" if is_resource(rest) => {
            // F2: release the caller's slot of a named resource.
            //   concord release <id> <name> --kind resource
            let pos = positional_args(rest, &["--kind", "--slots"]);
            let id = pos.first().map(String::as_str).ok_or(missing("session id"))?;
            let name = pos.get(1).map(String::as_str).ok_or(missing("resource name"))?;
            match store.release_resource(id, name)? {
                ResourceReleaseOutcome::Released { slot } => {
                    println!("RESOURCE-RELEASED {name} slot {slot}");
                    Ok(ExitCode::SUCCESS)
                }
                ResourceReleaseOutcome::NotHeld => {
                    println!("not holding {name}");
                    Ok(ExitCode::SUCCESS)
                }
            }
        }

        "claim" => {
            let id = require(rest, 0, "session id")?;
            let area = require(rest, 1, "area")?;
            let why = opt(rest, 2).unwrap_or("");
            // S2: if this is a symbol-lease (`<file>:<symbol>`), emit advisory notes —
            // the symbol's existence (S2.1) and a call-graph DEP_CHAIN warning (S2.2).
            // Both are advisory (stderr); the claim itself is enforced and proceeds.
            symbol_claim_advisories(&store, area, id);
            // M3L.2 Strong tier: route through the daemon (airtight check-and-apply)
            // when it is up; the Floor (direct, RejectOverlap default) otherwise.
            if let Some(resp) = mediate(
                &store,
                Request::Claim {
                    id: id.to_string(),
                    area: area.to_string(),
                    why: why.to_string(),
                },
            ) {
                match resp {
                    Response::Claimed => {
                        println!("CLAIMED {area}");
                        return Ok(ExitCode::SUCCESS);
                    }
                    Response::AlreadyYours => {
                        println!("already yours: {area}");
                        return Ok(ExitCode::SUCCESS);
                    }
                    Response::Reclaimed { previous } => {
                        println!("RECLAIMED {area} (stale holder {previous})");
                        return Ok(ExitCode::SUCCESS);
                    }
                    Response::ClaimConflict { holder } => {
                        println!("CONFLICT: '{area}' is leased by '{holder}' — coordinate first (status / SESSION-SYNC)");
                        return Ok(ExitCode::from(2));
                    }
                    Response::Overlap { area: other, holder } => {
                        println!("OVERLAP: '{area}' path-overlaps '{other}' leased by '{holder}' — coordinate first (status / SESSION-SYNC)");
                        return Ok(ExitCode::from(2));
                    }
                    _ => {} // unexpected → fall through to the Floor
                }
            }
            match store.claim(id, area, why, overlap_policy())? {
                ClaimOutcome::Claimed => {
                    println!("CLAIMED {area}");
                    Ok(ExitCode::SUCCESS)
                }
                ClaimOutcome::AlreadyYours => {
                    println!("already yours: {area}");
                    Ok(ExitCode::SUCCESS)
                }
                ClaimOutcome::Reclaimed { previous } => {
                    println!("RECLAIMED {area} (stale holder {previous})");
                    Ok(ExitCode::SUCCESS)
                }
                ClaimOutcome::Conflict { holder } => {
                    println!(
                        "CONFLICT: '{area}' is leased by '{holder}' — coordinate first (status / SESSION-SYNC)"
                    );
                    Ok(ExitCode::from(2))
                }
                ClaimOutcome::OverlapConflict {
                    area: other,
                    holder,
                } => {
                    // New (RejectOverlap): name the overlapping held area so the
                    // caller can coordinate, mirroring the CONFLICT phrasing.
                    println!(
                        "OVERLAP: '{area}' path-overlaps '{other}' leased by '{holder}' — coordinate first (status / SESSION-SYNC)"
                    );
                    Ok(ExitCode::from(2))
                }
            }
        }

        "release" => {
            let id = require(rest, 0, "session id")?;
            let area = require(rest, 1, "area")?;
            // Optional fencing Floor: `release <id> <area> --fence <N>` refuses if the
            // lease's fence has advanced (a reclaim happened) since the caller acquired.
            let expected_fence = flag_value(rest, "--fence").and_then(|v| v.parse::<u64>().ok());
            // M3L.2 Strong tier: mediate through the daemon when up, else the Floor.
            if let Some(resp) = mediate(
                &store,
                Request::Release {
                    id: id.to_string(),
                    area: area.to_string(),
                    fence: expected_fence,
                },
            ) {
                match resp {
                    Response::Released => {
                        println!("released {area}");
                        return Ok(ExitCode::SUCCESS);
                    }
                    Response::NoLease => {
                        println!("no lease on {area}");
                        return Ok(ExitCode::SUCCESS);
                    }
                    Response::NotYours { holder } => {
                        println!("REFUSED: '{area}' is held by '{holder}', not '{id}' — not releasing");
                        return Ok(ExitCode::from(2));
                    }
                    Response::FenceStale { current } => {
                        println!("REFUSED: '{area}' fence advanced to {current} (your authority is stale) — not releasing");
                        return Ok(ExitCode::from(2));
                    }
                    _ => {} // unexpected → fall through to the Floor
                }
            }
            match store.release(id, area, expected_fence)? {
                ReleaseOutcome::Released => {
                    println!("released {area}");
                    Ok(ExitCode::SUCCESS)
                }
                ReleaseOutcome::NoLease => {
                    println!("no lease on {area}");
                    Ok(ExitCode::SUCCESS)
                }
                ReleaseOutcome::NotYours { holder } => {
                    println!("REFUSED: '{area}' is held by '{holder}', not '{id}' — not releasing");
                    Ok(ExitCode::from(2))
                }
                ReleaseOutcome::FenceStale { current } => {
                    println!(
                        "REFUSED: '{area}' fence advanced to {current} (your authority is stale) — not releasing"
                    );
                    Ok(ExitCode::from(2))
                }
            }
        }

        "merge-lock" => {
            let id = require(rest, 0, "session id")?;
            let why = opt(rest, 1).unwrap_or("");
            // Strong tier: route through the daemon when it is up (atomic check-and-
            // apply at the single serialization point); otherwise the Floor (direct FS).
            if let Some(resp) = mediate(
                &store,
                Request::MergeLock {
                    id: id.to_string(),
                    why: why.to_string(),
                },
            ) {
                match resp {
                    Response::Acquired { .. } => {
                        println!("MERGE LOCK acquired");
                        return Ok(ExitCode::SUCCESS);
                    }
                    Response::Reacquired { .. } => {
                        println!("MERGE LOCK (re)acquired");
                        return Ok(ExitCode::SUCCESS);
                    }
                    Response::Held { holder } => {
                        println!("MERGE LOCK held by '{holder}' — wait until released");
                        return Ok(ExitCode::from(2));
                    }
                    _ => {} // unexpected → fall through to the Floor
                }
            }
            match store.merge_lock(id, why)? {
                MergeLockOutcome::Acquired => {
                    println!("MERGE LOCK acquired");
                    Ok(ExitCode::SUCCESS)
                }
                MergeLockOutcome::Reacquired => {
                    println!("MERGE LOCK (re)acquired");
                    Ok(ExitCode::SUCCESS)
                }
                MergeLockOutcome::Held { holder } => {
                    println!("MERGE LOCK held by '{holder}' — wait until released");
                    Ok(ExitCode::from(2))
                }
            }
        }

        "merge-unlock" => {
            let id = require(rest, 0, "session id")?;
            if let Some(resp) = mediate(&store, Request::MergeUnlock { id: id.to_string() }) {
                match resp {
                    Response::Released => {
                        println!("merge lock released");
                        return Ok(ExitCode::SUCCESS);
                    }
                    Response::NotHeld => {
                        println!("merge lock not held");
                        return Ok(ExitCode::SUCCESS);
                    }
                    Response::NotYours { holder } => {
                        println!(
                            "REFUSED: merge lock held by '{holder}', not '{id}' — not unlocking"
                        );
                        return Ok(ExitCode::from(2));
                    }
                    _ => {} // unexpected → fall through to the Floor
                }
            }
            match store.merge_unlock(id)? {
                MergeUnlockOutcome::Released => {
                    println!("merge lock released");
                    Ok(ExitCode::SUCCESS)
                }
                MergeUnlockOutcome::NotHeld => {
                    println!("merge lock not held");
                    Ok(ExitCode::SUCCESS)
                }
                MergeUnlockOutcome::NotYours { holder } => {
                    println!("REFUSED: merge lock held by '{holder}', not '{id}' — not unlocking");
                    Ok(ExitCode::from(2))
                }
            }
        }

        "check-lease" => {
            // F1/A1+A6: may <id> edit <area>? Exit 0 = ALLOW, exit 2 = DENY (with a
            // one-line reason on stdout the hook reuses as permissionDecisionReason).
            // P2 default (block-on-conflict); --strict = P1 (capability-strict).
            let id = require(rest, 0, "session id")?;
            let area = require(rest, 1, "area")?;
            let strict = has_flag(rest, "--strict");
            match store.check_lease(id, area, strict)? {
                LeaseCheck::Allow => {
                    println!("ALLOW {area}");
                    Ok(ExitCode::SUCCESS)
                }
                LeaseCheck::Deny { area: a, holder } => {
                    if holder.is_empty() {
                        println!("DENY {a} (no lease held by {id}; --strict)");
                    } else {
                        println!("DENY {a} (held by {holder})");
                    }
                    Ok(ExitCode::from(2))
                }
            }
        }

        "verify" => {
            // Fence-aware self-check: does <id> still legitimately hold <area>?
            let id = require(rest, 0, "session id")?;
            let area = require(rest, 1, "area")?;
            match store.verify_hold(id, area)? {
                HoldStatus::Held { fence } => {
                    println!("HELD by {id} (fence {fence})");
                    Ok(ExitCode::SUCCESS)
                }
                HoldStatus::HeldByOther { holder } => {
                    println!("HELD-BY-OTHER {holder}");
                    Ok(ExitCode::from(2))
                }
                HoldStatus::Stale { holder } => {
                    println!("STALE (was {holder}, reclaimable)");
                    Ok(ExitCode::from(2))
                }
                HoldStatus::Vacant => {
                    println!("VACANT");
                    Ok(ExitCode::from(2))
                }
            }
        }

        "log" => {
            let id = require(rest, 0, "session id")?;
            // `log <id> <event...>`: the rest of argv joined by single spaces, the
            // shell's `$*`. The trailing-space quirk is added by the ledger writer.
            let event = rest[rest.len().min(1)..].join(" ");
            store.log(id, &event)?;
            println!("logged");
            Ok(ExitCode::SUCCESS)
        }

        "sync" => {
            let id = require(rest, 0, "session id")?;
            let target = require(rest, 1, "target (e.g. K, ALLE, \"C + B\")")?;
            let topic = opt(rest, 2).unwrap_or("");
            let body = opt(rest, 3).unwrap_or("");
            store.sync(id, target, topic, body)?;
            println!(
                "posted to SESSION-SYNC ({})",
                store.paths().sync.display()
            );
            Ok(ExitCode::SUCCESS)
        }

        "send" => {
            // First-class typed message (WP7): concord send <from> <to> <kind> [--ref R] <body...>
            // Delivers a typed message straight to inbox/<to>.jsonl (no prose mirror,
            // so it never double-delivers via the daemon's prose demux).
            let reference = flag_value(rest, "--ref").map(str::to_string);
            let pos = positional_args(rest, &["--ref"]);
            let from = pos.first().ok_or(concord_core::ConcordError::MissingArg("from"))?;
            let to = pos.get(1).ok_or(concord_core::ConcordError::MissingArg("to"))?;
            let kind_tok = pos
                .get(2)
                .ok_or(concord_core::ConcordError::MissingArg("kind"))?;
            let kind = match MessageKind::parse(kind_tok) {
                Some(k) => k,
                None => {
                    println!(
                        "unknown kind '{kind_tok}' (go|ack|design|arbitration|status|decision|blocked|done|ready|idle|merge-ready|stand-down|note)"
                    );
                    return Ok(ExitCode::from(2));
                }
            };
            let body = pos.get(3..).map(|s| s.join(" ")).unwrap_or_default();
            let msg = Message::new(store.now(), from, to, kind, reference, &body);
            store.deliver_message(&msg)?;
            println!("sent {from} → {to} ({})", kind.as_str());
            Ok(ExitCode::SUCCESS)
        }

        other => {
            println!("unknown command: {other}");
            print_usage();
            Ok(ExitCode::FAILURE)
        }
    }
}

/// Read the overlap policy from the environment. Default = `RejectOverlap`: the
/// path-prefix overlap check is the core WP12 §6 fix and the coordinator's STEER is
/// "fix the bug in M1, don't replicate it". Set `CONCORD_STRICT_OVERLAP=0` to fall
/// back to shell behaviour (no overlap detection) if ever needed for a pure drop-in.
fn overlap_policy() -> OverlapPolicy {
    match std::env::var("CONCORD_STRICT_OVERLAP").ok().as_deref() {
        Some("0") | Some("false") | Some("no") => OverlapPolicy::ParityShell,
        _ => OverlapPolicy::RejectOverlap,
    }
}

/// Print the `status` block, byte-for-byte with `coord.sh status`.
fn print_status(store: &Store) -> Result<()> {
    let r: StatusReport = store.status()?;
    // NOTE: the literal "ais" is reproduced from coord.sh:73 for drop-in parity even
    // in the generalized concord repo; project-name abstraction is an M5 item.
    println!(
        "── Concord — ais multi-session coordination ({}) ──",
        store.paths().coord.display()
    );
    println!("ACTIVE SESSIONS:");
    if r.sessions_dir_empty {
        // "(none)" only when the sessions dir is genuinely empty (shell glob miss).
        println!("  (none)");
    } else {
        for s in &r.sessions {
            println!("  {:<10} focus: {}", s.id, s.focus);
        }
    }
    println!("HELD LEASES:");
    if r.leases.is_empty() {
        println!("  (none)");
    } else {
        for l in &r.leases {
            println!("  {:<28} by {} — {}", l.area_slug, l.holder, l.why);
        }
    }
    if let Some(holder) = &r.merge_lock_holder {
        println!("MERGE LOCK: held by {holder}");
    }
    // F2: named resource locks / build-slots (only when any exist, to keep the common
    // status output unchanged).
    let resources = store.resource_locks()?;
    if !resources.is_empty() {
        println!("RESOURCE LOCKS:");
        for r in &resources {
            let holders = if r.held.is_empty() {
                "free".to_string()
            } else {
                r.held
                    .iter()
                    .map(|(s, h)| format!("#{s}={h}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            println!(
                "  {:<28} {}/{} slots — {}",
                r.name,
                r.held.len(),
                r.capacity,
                holders
            );
        }
    }
    Ok(())
}

/// The help text printed on an unknown command (the shell dumps its leading comment
/// block; we print the equivalent usage summary).
fn print_usage() {
    let usage = "\
Concord — multi-session coordination (Rust port of bin/coord.sh).

  concord init [--project <path>] [--ids a,b,c] # bootstrap a project's coordination state
  concord paths                                 # print resolved CONCORD_DIR/SYNC/PROJECT (eval-able)
  concord start <id> [--print]                  # launch a session in this terminal (--print = dry-run)
  concord dash                                  # live overview: status + last prose post per session
  concord pause <id> | resume <id>              # set/clear a session's pause flag
  concord stop <id>                             # ask a session to stop cleanly (via the prose channel)
  concord register <id> <focus>                 # once, at session start
  concord heartbeat <id>                         # periodically (keeps you \"alive\")
  concord status                                 # who is active + what is leased
  concord claim <id> <area> [why]                # BEFORE editing a shared area (area may be <file>:<symbol>)
  concord symbols <file>                          # list a Rust file's symbols (claimable symbol-leases)
  concord release <id> <area> [--fence N]        # when done (refuses foreign/stale)
  concord verify <id> <area>                     # do I still hold it? (fencing self-check)
  concord merge-lock <id> [why]                  # BEFORE merging (singleton)
  concord merge-unlock <id>                      # after the merge (refuses foreign)
  concord log <id> <event...>                    # record a structured intent
  concord sync <id> <target> <topic> <body>      # post to the prose channel (human log)
  concord send <from> <to> <kind> [--ref R] <body>  # typed message → inbox/<to>.jsonl (WP7)
  concord version                                # print the Concord version";
    println!("{usage}");
}

// ─────────────────────────── tiny arg helpers ───────────────────────────

/// Require the positional arg at `idx` (0-based within the post-command args),
/// erroring like the shell's `${n:?label}` if absent.
fn require<'a>(rest: &'a [String], idx: usize, label: &'static str) -> Result<&'a str> {
    opt(rest, idx).ok_or(concord_core::ConcordError::MissingArg(label))
}

/// The positional arg at `idx`, or `None`.
fn opt(rest: &[String], idx: usize) -> Option<&str> {
    rest.get(idx).map(String::as_str)
}

/// S2: advisory notes (stderr) for a symbol-lease claim — never blocks (the lease itself
/// is enforced). (1) Existence: warn if the symbol isn't in the file (it may be new).
/// (2) DEP_CHAIN: warn if the claimed Rust symbol CALLS a symbol another session holds —
/// a call edge is a hint, not mutual exclusion (the genuinely-advisory layer, like wit).
fn symbol_claim_advisories(store: &Store, area: &str, claimer: &str) {
    let (file, sym) = concord_core::slug::split_symbol(area);
    let Some(symbol) = sym else { return };
    let Some(lang) = concord_ast::Lang::from_path(file) else { return };
    let Ok(src) = std::fs::read_to_string(store.paths().project.join(file)) else { return };

    if concord_ast::resolve_symbol(lang, &src, symbol).is_none() {
        eprintln!(
            "note: symbol '{symbol}' not found in {file} (claiming anyway — it may be new or about to be created)"
        );
    }

    // DEP_CHAIN (Rust call graph): which symbols does `symbol` call?
    if lang != concord_ast::Lang::Rust {
        return;
    }
    let callees: std::collections::HashSet<String> = concord_ast::extract_rust_calls(&src)
        .into_iter()
        .filter(|d| d.caller == symbol)
        .map(|d| d.callee)
        .collect();
    if callees.is_empty() {
        return;
    }
    if let Ok(report) = store.status() {
        for lease in &report.leases {
            if lease.holder == claimer {
                continue;
            }
            let (_, held_sym) = concord_core::slug::split_symbol(&lease.area);
            if let Some(hs) = held_sym {
                if callees.contains(hs) {
                    eprintln!(
                        "DEP_CHAIN note: '{symbol}' calls '{hs}', which is leased by '{}' ({}) — coordinate if you change its contract",
                        lease.holder, lease.area
                    );
                }
            }
        }
    }
}

/// Try to route a consequential request through the daemon (Strong tier). Returns
/// `Some(response)` when the daemon is reachable and answered with a usable verdict;
/// `None` when there is no daemon, the connection failed, or it returned an error — in
/// all of which the caller falls back to the Floor (direct FS). `CONCORD_NO_DAEMON=1`
/// forces the Floor unconditionally.
fn mediate(store: &Store, req: Request) -> Option<Response> {
    if matches!(
        std::env::var("CONCORD_NO_DAEMON").ok().as_deref(),
        Some("1") | Some("true") | Some("yes")
    ) {
        return None;
    }
    let sock = store.paths().coord.join(SOCKET_NAME);
    concord_core::ipc::mediate(&sock, &req)
}

/// The value following `flag` (e.g. `--fence 7` ⇒ `Some("7")`), or `None`.
fn flag_value<'a>(rest: &'a [String], flag: &str) -> Option<&'a str> {
    rest.iter()
        .position(|a| a == flag)
        .and_then(|i| rest.get(i + 1))
        .map(String::as_str)
}

/// True if a bare `--flag` is present anywhere in `rest`.
fn has_flag(rest: &[String], flag: &str) -> bool {
    rest.iter().any(|a| a == flag)
}

/// True if `--kind resource` selects the F2 resource-semaphore namespace.
fn is_resource(rest: &[String]) -> bool {
    flag_value(rest, "--kind") == Some("resource")
}

/// The same missing-argument error `require` produces, for hand-parsed verbs.
fn missing(label: &'static str) -> concord_core::ConcordError {
    concord_core::ConcordError::MissingArg(label)
}

/// Positional args, skipping `--flag value` pairs named in `value_flags` and any other
/// bare `--flag`. Used by `send` to separate from/to/kind/body from `--ref R`.
fn positional_args(rest: &[String], value_flags: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip = false;
    for a in rest {
        if skip {
            skip = false;
            continue;
        }
        if value_flags.contains(&a.as_str()) {
            skip = true;
            continue;
        }
        if a.starts_with("--") {
            continue;
        }
        out.push(a.clone());
    }
    out
}
