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
use std::path::PathBuf;
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::time::Duration;

use concord_core::directive::{demux, route};
use concord_core::Paths;
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

    run_watch_loop(&paths);
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
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
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

    for (recipient, text) in &routed {
        if let Err(e) = append_inbox(paths, recipient, text) {
            eprintln!("[concordd] warn: inbox append for {recipient} failed: {e}");
        }
    }

    write_offset(paths, len);
    if !routed.is_empty() {
        eprintln!("[concordd] routed {} block(s) to inboxes", routed.len());
    }
    routed.len()
}

/// Append a directive block to `inbox/<recipient>`, creating it if needed. The append
/// bumps the file's mtime — the signal a consumer's watcher wakes on.
fn append_inbox(paths: &Paths, recipient: &str, text: &str) -> std::io::Result<()> {
    let dir = inbox_dir(paths);
    fs::create_dir_all(&dir)?;
    let file = dir.join(recipient);
    let mut f = fs::OpenOptions::new().create(true).append(true).open(file)?;
    // Blank-line separated blocks so a consumer reads clean, whole directives.
    writeln!(f, "{text}\n")
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
