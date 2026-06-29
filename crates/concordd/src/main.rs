//! concordd — the Concord push daemon (M2).
//!
//! M2.1 (this file): **inbox demux.** Watch the coordination dir + the prose channel
//! and, whenever the prose channel grows, parse the new `### from → to` directives
//! and append each block to a per-recipient inbox (`inbox/<id>`), bumping its mtime.
//! A session then points its harness watcher at `inbox/<id>` (one small file) instead
//! of re-reading the whole growing `*-SESSION-SYNC.md` — the §9/WP7 token lever. The
//! daemon is a *derived accelerator*: posters keep writing `### …` unchanged; the
//! filesystem state stays authoritative and works without the daemon (ADR policy 4).
//!
//! Scope guard: this is the inbox *substrate*, not the full typed inbox protocol
//! (that is M3). Fencing enforcement is M2.2/M2.3, separate.
//!
//! Modes:
//!   concordd            run the watch loop (event-driven + a periodic safety catch-up)
//!   concordd --once     catch up the demux once from the saved offset, then exit
//!                       (suitable for a hook/cron; offset persists between runs)
//!
//! Watching uses `notify` + `notify-debouncer-full` (probe-validated on macOS: atomic
//! renames surface on the target path, one logical write debounces to one batch).

use std::fs;
use std::io::Write as _;
#[cfg(unix)]
use std::io::{BufRead as _, BufReader};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::time::Duration;

use concord_core::clock;
use concord_core::directive::{demux, route};
#[cfg(unix)]
use concord_core::ipc::{Request, Response, SOCKET_NAME};
use concord_core::message::{Message, MessageKind};
#[cfg(unix)]
use concord_core::store::{
    ClaimOutcome, MergeLockOutcome, MergeUnlockOutcome, OverlapPolicy, ReleaseOutcome,
};
use concord_core::Paths;
#[cfg(unix)]
use concord_core::Store;
use notify_debouncer_full::notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebounceEventResult};

fn main() {
    let once = std::env::args().skip(1).any(|a| a == "--once");
    let paths = Paths::from_cwd();
    fs::create_dir_all(inbox_dir(&paths)).ok();

    if once {
        let n = catch_up(&paths);
        eprintln!("[concordd] --once: routed {n} block(s)");
        return;
    }

    // On first start with no saved offset, skip history: begin from the current end
    // of the prose channel so only NEW directives are delivered.
    if read_offset(&paths).is_none() {
        write_offset(&paths, sync_len(&paths));
    }
    // Catch up anything appended between offset-init and the watcher arming.
    catch_up(&paths);

    // M2.3 Strong: the mediation socket — the single serialization point for
    // consequential writes (merge-lock/unlock). Runs in its own thread, serving
    // requests SERIALLY so check-and-apply is atomic (closes the Floor's TOCTOU
    // window). The watch loop owns the main thread. Unix only — off Unix the daemon is
    // watch/inbox-only (no mediation; the CLI uses the enforced Floor).
    #[cfg(unix)]
    {
        let p = paths.clone();
        std::thread::spawn(move || run_socket_server(&p));
    }

    run_watch_loop(&paths);
}

// ─────────────────────────── mediation socket (M2.3) ───────────────────────────

#[cfg(unix)]
fn socket_path(paths: &Paths) -> PathBuf {
    paths.coord.join(SOCKET_NAME)
}

/// Serve mediated requests on the Unix socket, one connection at a time (serial — the
/// single serialization point). Each request is dispatched to a fresh [`Store`] (so
/// the timestamp is current) and answered with one response line.
#[cfg(unix)]
fn run_socket_server(paths: &Paths) {
    let path = socket_path(paths);
    let _ = fs::create_dir_all(&paths.coord);
    // Remove a stale socket from a previous run (bind fails if the path exists).
    let _ = fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[concordd] warn: cannot bind socket {}: {e}", path.display());
            return;
        }
    };
    eprintln!("[concordd] mediation socket at {}", path.display());

    for conn in listener.incoming() {
        match conn {
            Ok(stream) => handle_conn(paths, stream),
            Err(e) => eprintln!("[concordd] socket accept error: {e}"),
        }
    }
}

/// Handle one connection: read a request line, apply it, write a response line.
#[cfg(unix)]
fn handle_conn(paths: &Paths, stream: UnixStream) {
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
        return;
    }
    let resp = match Request::parse_line(&line) {
        Some(req) => apply(paths, req),
        None => Response::Err("malformed request".to_string()),
    };
    let mut w = stream;
    let _ = writeln!(w, "{}", resp.to_line());
}

/// Apply a mediated request against the store — the atomic check-and-apply.
#[cfg(unix)]
fn apply(paths: &Paths, req: Request) -> Response {
    match req {
        Request::Ping => Response::Pong,
        Request::MergeLock { id, why } => {
            let store = match Store::open(paths.clone()) {
                Ok(s) => s,
                Err(e) => return Response::Err(e.to_string()),
            };
            match store.merge_lock(&id, &why) {
                Ok(MergeLockOutcome::Acquired) => Response::Acquired {
                    fence: merge_fence(&store),
                },
                Ok(MergeLockOutcome::Reacquired) => Response::Reacquired {
                    fence: merge_fence(&store),
                },
                Ok(MergeLockOutcome::Held { holder }) => Response::Held { holder },
                Err(e) => Response::Err(e.to_string()),
            }
        }
        Request::MergeUnlock { id } => {
            let store = match Store::open(paths.clone()) {
                Ok(s) => s,
                Err(e) => return Response::Err(e.to_string()),
            };
            match store.merge_unlock(&id) {
                Ok(MergeUnlockOutcome::Released) => Response::Released,
                Ok(MergeUnlockOutcome::NotHeld) => Response::NotHeld,
                Ok(MergeUnlockOutcome::NotYours { holder }) => Response::NotYours { holder },
                Err(e) => Response::Err(e.to_string()),
            }
        }
        // M3L.2: claim/release mediated too — the check-and-apply runs in this single
        // handler thread, so the Floor's check-then-commit TOCTOU window is closed for
        // these ops when the daemon is up. The enforced overlap policy always applies.
        Request::Claim { id, area, why } => {
            let store = match Store::open(paths.clone()) {
                Ok(s) => s,
                Err(e) => return Response::Err(e.to_string()),
            };
            match store.claim(&id, &area, &why, OverlapPolicy::RejectOverlap) {
                Ok(ClaimOutcome::Claimed) => Response::Claimed,
                Ok(ClaimOutcome::AlreadyYours) => Response::AlreadyYours,
                Ok(ClaimOutcome::Reclaimed { previous }) => Response::Reclaimed { previous },
                Ok(ClaimOutcome::Conflict { holder }) => Response::ClaimConflict { holder },
                Ok(ClaimOutcome::OverlapConflict { area, holder }) => {
                    Response::Overlap { area, holder }
                }
                Err(e) => Response::Err(e.to_string()),
            }
        }
        Request::Release { id, area, fence } => {
            let store = match Store::open(paths.clone()) {
                Ok(s) => s,
                Err(e) => return Response::Err(e.to_string()),
            };
            match store.release(&id, &area, fence) {
                Ok(ReleaseOutcome::Released) => Response::Released,
                Ok(ReleaseOutcome::NoLease) => Response::NoLease,
                Ok(ReleaseOutcome::NotYours { holder }) => Response::NotYours { holder },
                Ok(ReleaseOutcome::FenceStale { current }) => Response::FenceStale { current },
                Err(e) => Response::Err(e.to_string()),
            }
        }
    }
}

/// The fence currently stamped on the merge lock (0 if unreadable).
#[cfg(unix)]
fn merge_fence(store: &Store) -> u64 {
    store
        .read_merge_lock()
        .ok()
        .flatten()
        .map(|ml| ml.fence)
        .unwrap_or(0)
}

// ───────────────────────────── watch loop ─────────────────────────────

fn run_watch_loop(paths: &Paths) {
    let (tx, rx) = channel();
    let mut debouncer = match new_debouncer(Duration::from_millis(300), None, tx) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[concordd] FATAL: cannot create watcher: {e}");
            std::process::exit(1);
        }
    };
    // Root 1: the coordination dir (recursive) — leases, sessions, merge lock, log.
    if let Err(e) = debouncer.watch(&paths.coord, RecursiveMode::Recursive) {
        eprintln!("[concordd] FATAL: cannot watch {}: {e}", paths.coord.display());
        std::process::exit(1);
    }
    // Root 2: the prose channel, which lives OUTSIDE the coord dir (probe finding #5).
    // Watch the file directly; it is appended in place (cat >> / coord.sh sync), so the
    // inode is stable. If it does not exist yet, the periodic catch-up will pick it up.
    if paths.sync.exists() {
        if let Err(e) = debouncer.watch(&paths.sync, RecursiveMode::NonRecursive) {
            eprintln!("[concordd] warn: cannot watch {}: {e}", paths.sync.display());
        }
    }

    eprintln!(
        "[concordd] watching {} + {} — demuxing directives → {}",
        paths.coord.display(),
        paths.sync.display(),
        inbox_dir(paths).display()
    );

    // Event-driven, with a periodic safety catch-up (covers a missed file event or a
    // recreated prose file). The catch-up is cheap: a length check, read only on growth.
    loop {
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(_batch) => {
                consume(&rx); // drain any piled-up batches
                catch_up(paths);
            }
            Err(RecvTimeoutError::Timeout) => {
                catch_up(paths);
                tick_acks(paths); // F3 active layer: re-deliver / auto-escalate overdue acks
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// F3 daemon active layer: each periodic cycle, re-deliver directives un-ACK'd past the
/// TTL (bumping the recipient's inbox mtime to wake it) and auto-escalate after K misses.
/// The spacing/threshold logic lives in [`Store::tick_acks`]; the daemon performs the
/// inbox append (the wake) for each returned re-delivery.
fn tick_acks(paths: &Paths) {
    const TTL_ACK: u64 = 15 * 60; // ≈ one worker tick
    const K_REDELIVER: u32 = 2; // re-deliver twice, then escalate (severity High)
    let store = match concord_core::Store::open_at(paths.clone(), clock::now()) {
        Ok(s) => s,
        Err(_) => return,
    };
    let report = match store.tick_acks(TTL_ACK, K_REDELIVER) {
        Ok(r) => r,
        Err(_) => return,
    };
    for r in &report.redelivered {
        let msg = Message {
            ts: clock::now(),
            from: "concordd".to_string(),
            to: r.to.clone(),
            kind: MessageKind::Note,
            reference: None,
            body: format!(
                "[redelivery] un-ACK'd directive from {} (seq {}) — ACK (`concord ack {}`) or handle it",
                r.from, r.seq, r.to
            ),
        };
        let _ = append_inbox(paths, &r.to, &msg.to_jsonl());
    }
    if !report.redelivered.is_empty() || !report.escalated.is_empty() {
        eprintln!(
            "[concordd] ack-tick: {} re-delivered, {} auto-escalated",
            report.redelivered.len(),
            report.escalated.len()
        );
    }
}

/// Drain queued debounced batches without blocking (we re-derive from the offset, so
/// the batch contents themselves are irrelevant — only that *something* changed).
fn consume(rx: &std::sync::mpsc::Receiver<DebounceEventResult>) {
    while rx.try_recv().is_ok() {}
}

// ───────────────────────────── demux core ─────────────────────────────

/// Process everything appended to the prose channel since the saved offset: demux the
/// new directives, route them to recipient inboxes, append, advance the offset.
/// Returns the number of routed blocks. Idempotent across runs via the offset.
fn catch_up(paths: &Paths) -> usize {
    let content = match fs::read(&paths.sync) {
        Ok(b) => b,
        Err(_) => return 0, // no prose channel yet
    };
    let len = content.len() as u64;
    let offset = read_offset(paths).unwrap_or(len).min(len);
    if offset >= len {
        // Nothing new (or the file shrank/rotated — reset to current end).
        if offset > len {
            write_offset(paths, len);
        }
        return 0;
    }

    let new_text = String::from_utf8_lossy(&content[offset as usize..]);
    let blocks = demux(&new_text);
    let registered = registered_sessions(paths);
    let routed = route(&blocks, &registered);

    // WP7: each routed block becomes a typed Message, appended as one JSONL line to
    // the recipient's typed inbox (`inbox/<id>.jsonl`). The kind is classified
    // conservatively from the topic; an AP-ref is extracted when present.
    let ts = clock::now();
    for (recipient, block) in &routed {
        let msg = Message::from_block(block, recipient, ts);
        if let Err(e) = append_inbox(paths, recipient, &msg.to_jsonl()) {
            eprintln!("[concordd] warn: inbox append for {recipient} failed: {e}");
        }
    }

    // F3 ack-tracking: a poster is "caught up" (derived ack ⇒ clear its pending — the A3
    // watermark mechanized); each routed directive becomes pending an ack for its recipient.
    if let Ok(store) = concord_core::Store::open_at(paths.clone(), ts) {
        for b in &blocks {
            let _ = store.clear_pending(&b.directive.from);
        }
        for (recipient, block) in &routed {
            let _ = store.add_pending(recipient, &block.directive.from);
        }
    }

    write_offset(paths, len);
    if !routed.is_empty() {
        eprintln!("[concordd] routed {} typed message(s) to inboxes", routed.len());
    }
    routed.len()
}

/// Append a pre-formatted JSONL line (already `\n`-terminated) to the recipient's
/// typed inbox `inbox/<recipient>.jsonl`, creating it if needed. The append bumps the
/// file's mtime — the signal a consumer's watcher wakes on.
fn append_inbox(paths: &Paths, recipient: &str, jsonl_line: &str) -> std::io::Result<()> {
    let dir = inbox_dir(paths);
    fs::create_dir_all(&dir)?;
    let file = dir.join(format!("{recipient}.jsonl"));
    let mut f = fs::OpenOptions::new().create(true).append(true).open(file)?;
    f.write_all(jsonl_line.as_bytes())
}

// ───────────────────────────── helpers ─────────────────────────────

fn inbox_dir(paths: &Paths) -> PathBuf {
    paths.coord.join("inbox")
}

fn offset_file(paths: &Paths) -> PathBuf {
    paths.coord.join(".inbox-offset")
}

fn read_offset(paths: &Paths) -> Option<u64> {
    fs::read_to_string(offset_file(paths))
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn write_offset(paths: &Paths, offset: u64) {
    let _ = fs::create_dir_all(&paths.coord);
    let _ = fs::write(offset_file(paths), format!("{offset}\n"));
}

fn sync_len(paths: &Paths) -> u64 {
    fs::metadata(&paths.sync).map(|m| m.len()).unwrap_or(0)
}



/// The registered session ids (names of files under `sessions/`). Used to fan out a
/// broadcast (`→ ALLE`). Order is unspecified; routing order follows this list.
fn registered_sessions(paths: &Paths) -> Vec<String> {
    let mut ids = Vec::new();
    if let Ok(rd) = fs::read_dir(&paths.sessions) {
        for ent in rd.flatten() {
            if ent.path().is_file() {
                ids.push(ent.file_name().to_string_lossy().into_owned());
            }
        }
    }
    ids.sort();
    ids
}
