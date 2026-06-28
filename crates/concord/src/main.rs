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

use concord_core::store::{
    ClaimOutcome, MergeLockOutcome, OverlapPolicy, ReleaseOutcome, StatusReport, Store,
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

        "status" => {
            print_status(&store)?;
            Ok(ExitCode::SUCCESS)
        }

        "claim" => {
            let id = require(rest, 0, "session id")?;
            let area = require(rest, 1, "area")?;
            let why = opt(rest, 2).unwrap_or("");
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
            match store.release(id, area)? {
                ReleaseOutcome::Released => println!("released {area}"),
                ReleaseOutcome::NoLease => println!("no lease on {area}"),
            }
            Ok(ExitCode::SUCCESS)
        }

        "merge-lock" => {
            let id = require(rest, 0, "session id")?;
            let why = opt(rest, 1).unwrap_or("");
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
            store.merge_unlock(id)?;
            println!("merge lock released");
            Ok(ExitCode::SUCCESS)
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
    Ok(())
}

/// The help text printed on an unknown command (the shell dumps its leading comment
/// block; we print the equivalent usage summary).
fn print_usage() {
    let usage = "\
Concord — multi-session coordination (Rust port of bin/coord.sh).

  concord register <id> <focus>                 # once, at session start
  concord heartbeat <id>                         # periodically (keeps you \"alive\")
  concord status                                 # who is active + what is leased
  concord claim <id> <area> [why]                # BEFORE editing a shared area
  concord release <id> <area>                    # when done with the area
  concord merge-lock <id> [why]                  # BEFORE merging (singleton)
  concord merge-unlock <id>                      # after the merge
  concord log <id> <event...>                    # record a structured intent
  concord sync <id> <target> <topic> <body>      # post to the prose channel
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
