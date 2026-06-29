//! The Concord launcher — `start / stop / pause / resume / dash`, ported from the shell
//! `bin/concord` (S1, the last shell piece → complete Rust migration).
//!
//! This lives in the `concord` *binary* crate (CLI layer): it uses `std::process` to
//! spawn sessions, which must NOT leak into `concord-core` (the core stays std-only,
//! zero-dep, pure typed state). It drives the same typed core the verbs do.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use concord_core::config::Config;
use concord_core::{Paths, Store};

/// Default permission flags for a spawned worker session (full access — the same level
/// the coordinator runs at, on the operator's own machine). From `[launcher] claude_flags`.
fn claude_flags(cfg: &Config) -> Vec<String> {
    cfg.launcher
        .claude_flags
        .split_whitespace()
        .map(String::from)
        .collect()
}

/// The coordinator/steward id (gets the coordinator kickoff, not the worker one).
/// Case-insensitive (`hub == HUB`); from `[escalation] coordinator`.
fn coordinator_id(cfg: &Config) -> String {
    cfg.escalation.coordinator.clone()
}
fn is_coordinator(id: &str, cfg: &Config) -> bool {
    id.eq_ignore_ascii_case(&coordinator_id(cfg))
}

/// Load the effective config for a launcher command (keyed off the coord dir).
fn launcher_config(paths: &Paths) -> Config {
    concord_config::load(&paths.coord)
}

/// Resolve a session's worktree. Standard convention: `<repo-parent>/<repo>-<id-lower>`.
/// An optional override map (`<coord>/hooks/worktree-map`, "<path> <id>" lines) wins.
fn worktree_for(paths: &Paths, id: &str) -> PathBuf {
    let map = paths.coord.join("hooks").join("worktree-map");
    if let Ok(contents) = std::fs::read_to_string(&map) {
        for line in contents.lines() {
            let mut it = line.split_whitespace();
            if let (Some(p), Some(i)) = (it.next(), it.next()) {
                if i == id {
                    return PathBuf::from(p);
                }
            }
        }
    }
    let parent = paths.project.parent().unwrap_or_else(|| Path::new("."));
    let base = paths
        .project
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    parent.join(format!("{base}-{}", id.to_lowercase()))
}

// ─────────────────────────── kickoff / loop prompts ───────────────────────────
// Ported verbatim from bin/concord (the established protocol text), parameterised by
// the coordinator id and the prose channel.

fn worker_loop_prompt(coord: &str, sync: &str) -> String {
    format!("12m Concord worker tick: 1) tools/coord.sh heartbeat <your-id> (your id is in the injected [Concord] context). 2) STARTLINE GATE: if you have NOT yet received a '### {coord} -> <your-id> (GO: <task>)' -> hold READY: only heartbeat, grab NO work, keep checking. Start work only AFTER the coordinator's GO. Pause flag (concord pause) -> also only heartbeat. 3) (after GO) read new ### …-> <id>/ALL directives in {sync} (Read tool or bash) and continue the GO task; for real design forks first post ### <your-id> -> {coord} (DESIGN: …) to the coordinator. 4) Lease before shared edits (tools/coord.sh claim); merge ONLY via the coordinator. 5) Post: append to {sync} (cat >>) OR (if sandbox-blocked) tools/coord.sh sync <id> <target> \"<topic>\" \"<body>\". 6) Done/blocked -> report visibly to the coordinator. No silent idling, no interactive decision prompts.")
}

fn worker_kickoff_prompt(id: &str, coord: &str, sync: &str) -> String {
    let loop_p = worker_loop_prompt(coord, sync);
    format!("You are Concord worker session {id}. Read CLAUDE.md (Concord block). Set up your self-tick NOW by running exactly this command:\n/loop {loop_p}\nThen, EVERY time right after running this, announce yourself to the coordinator (session {coord}) and WAIT for the start signal: post  ### {id} -> {coord} (READY: <your-terrain>, waiting for GO) . Always do this even if you have nothing to report yet, so the coordinator knows you are present. Grab NO work until the coordinator posts  ### {coord} -> {id} (GO: <task>)  — until then only heartbeat + hold READY.")
}

fn coordinator_loop_prompt(coord: &str, sync: &str) -> String {
    format!("25m Concord coordinator tick: 1) tools/coord.sh heartbeat <your-id> (id in the injected [Concord] context; usually kept by the hook). 2) You are the COORDINATOR/steward, NOT a worker — you wait for NO GO and grab NO code terrain. 3) Read new ### …-> <id>/ALL directives in {sync} + tools/coord.sh status; acknowledge open READY/ACK/STATUS/DESIGN from workers so each one knows you are present. 4) MUSTER MODE: hold GOs until the operator says \"GO free\"; then roll out the dispatch plan (### {coord} -> <id> (GO: <task>)). 5) Arbitrate merges neutrally via merge-lock (standing operator delegation for commits/PRs/merges); actively drive utilization and progress of all sessions. 6) Escalate real direction questions to the operator via the coordinator session; interactive decision prompts are coordinator-only. Channel hygiene: terse ### <id> -> <target> entries; record consequential decisions via tools/coord.sh log.")
}

fn coordinator_kickoff_prompt(id: &str, sync: &str) -> String {
    let loop_p = coordinator_loop_prompt(id, sync);
    format!("You are Concord coordinator session {id} (neutral steward, NOT a worker). Read CLAUDE.md (Concord block) + the HANDOFF and any open ### … -> {id} directives in {sync}. You wait for NO GO and take NO code terrain. Set up your coordinator self-tick NOW by running exactly this command:\n/loop {loop_p}\nThen take up coordination: assess the situation (tools/coord.sh status + prose channel), acknowledge open READY/STATUS so every worker knows you are present, in MUSTER MODE hold the GOs until the operator says \"GO free\" and then roll out the dispatch plan (### {id} -> <id> (GO: <task>)). You are the single voice operator->{id}->sessions: assign, sequence on the vision critical path, arbitrate ownership/merges (merge-lock). Escalate real direction questions to the operator; interactive prompts are yours only.")
}

fn kickoff_for(id: &str, sync: &str, cfg: &Config) -> String {
    if is_coordinator(id, cfg) {
        coordinator_kickoff_prompt(id, sync)
    } else {
        worker_kickoff_prompt(id, &coordinator_id(cfg), sync)
    }
}

// ─────────────────────────────── subcommands ───────────────────────────────

/// `concord start <id> [--print]` — launch a session in the CURRENT terminal with the
/// right id, env, permissions, and kickoff prompt. With `--print` (or `--dry-run`) it
/// shows the resolved worktree + env + command + prompt WITHOUT spawning claude.
pub fn cmd_start(paths: &Paths, id: &str, print: bool) -> ExitCode {
    let wt = worktree_for(paths, id);
    let worktree = if wt.is_dir() {
        wt
    } else {
        eprintln!(
            "WARN: worktree '{}' for {id} does not exist — using {}",
            wt.display(),
            paths.project.display()
        );
        paths.project.clone()
    };
    let cfg = launcher_config(paths);
    let sync = paths.sync.to_string_lossy().into_owned();
    let prompt = kickoff_for(id, &sync, &cfg);
    let flags = claude_flags(&cfg);
    let role = if is_coordinator(id, &cfg) {
        "coordinator · takes up coordination directly (no GO wait)"
    } else {
        "worker · announces READY, waits for the coordinator's GO"
    };

    // Persist the kickoff prompt (parity with the shell + debuggability).
    let pf = paths.coord.join(format!(".start-prompt-{id}.txt"));
    let _ = std::fs::write(&pf, &prompt);
    if let Ok(store) = Store::open(paths.clone()) {
        let _ = store.log(id, "concord-start (current terminal)");
    }

    // F-config: env is retired. Bind the session id to its worktree via an idbind marker
    // (the hooks read `<coord>/idbind/<slug(worktree)>`), so no CONCORD_ID env is needed.
    write_idbind(paths, &worktree, id);

    // F4: when telemetry is on, point this session's native OTel at Concord's local
    // receiver + tag every metric with `concord.id`. These are Claude Code's OWN OTEL_*
    // vars (set by the launcher) — NOT Concord env, so no conflict with the F-config
    // env-retirement. Metrics-only (no OTEL_LOG_USER_PROMPTS) — privacy by construction.
    let env_refs = otel_env(&cfg, id);

    if print {
        println!("▶ would start session {id} · worktree: {} · {role}", worktree.display());
        println!("  idbind: {} = {id}", idbind_path(paths, &worktree).display());
        if !env_refs.is_empty() {
            println!("  telemetry env: {}", env_refs.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" "));
        }
        println!("  exec: claude {} <kickoff-prompt>", flags.join(" "));
        println!("  prompt-file: {}", pf.display());
        println!("  ── kickoff prompt ──\n{prompt}");
        return ExitCode::SUCCESS;
    }

    println!("▶ Session {id} in DIESEM Terminal · Worktree: {} · volle Rechte · {role}.", worktree.display());
    spawn_claude(&worktree, &flags, &prompt, &env_refs)
}

/// The Claude Code OTel environment for a launched session (F4), or empty when telemetry
/// is disabled. Points the native OTLP/HTTP-JSON exporter at Concord's local receiver and
/// tags every metric with `concord.id` via a resource attribute.
fn otel_env(cfg: &Config, id: &str) -> Vec<(&'static str, String)> {
    if !cfg.telemetry.enabled {
        return Vec::new();
    }
    vec![
        ("CLAUDE_CODE_ENABLE_TELEMETRY", "1".to_string()),
        ("OTEL_METRICS_EXPORTER", "otlp".to_string()),
        ("OTEL_EXPORTER_OTLP_PROTOCOL", "http/json".to_string()),
        ("OTEL_EXPORTER_OTLP_ENDPOINT", format!("http://127.0.0.1:{}", cfg.telemetry.port)),
        ("OTEL_RESOURCE_ATTRIBUTES", format!("concord.id={id}")),
        ("OTEL_METRIC_EXPORT_INTERVAL", "10000".to_string()),
    ]
}

/// The idbind marker path for a worktree (F-config): `<coord>/idbind/<slug(worktree)>`.
/// The hooks derive the session id from cwd → this file, so the spawned session needs no
/// `CONCORD_ID` environment variable.
fn idbind_path(paths: &Paths, worktree: &Path) -> PathBuf {
    let key = worktree.to_string_lossy().replace(['/', ' '], "_");
    paths.coord.join("idbind").join(key)
}
fn write_idbind(paths: &Paths, worktree: &Path, id: &str) {
    let p = idbind_path(paths, worktree);
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&p, format!("{id}\n"));
}

/// Build and run `claude <flags> <prompt>` in `worktree` with the Concord env. On Unix
/// this REPLACES the current process (like the shell's `exec` — the launcher becomes the
/// session, no new window); elsewhere it spawns and waits.
fn spawn_claude(worktree: &Path, flags: &[String], prompt: &str, envs: &[(&str, String)]) -> ExitCode {
    let mut cmd = std::process::Command::new("claude");
    cmd.args(flags).arg(prompt).current_dir(worktree);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec(); // returns only on failure
        eprintln!("FATAL: could not exec claude: {err}");
        ExitCode::FAILURE
    }
    #[cfg(not(unix))]
    {
        match cmd.status() {
            Ok(s) => ExitCode::from(s.code().unwrap_or(1) as u8),
            Err(e) => {
                eprintln!("FATAL: could not spawn claude: {e}");
                ExitCode::FAILURE
            }
        }
    }
}

/// `concord dash` — the live overview: typed status + each session's last prose post +
/// a [PAUSED] marker. Reuses the typed core (no board subsystem — M3-lean).
pub fn cmd_dash(store: &Store) -> ExitCode {
    let paths = store.paths();
    let report = match store.status() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("concord: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "── Concord — multi-session coordination ({}) ──",
        paths.coord.display()
    );
    println!("ACTIVE SESSIONS:");
    if report.sessions_dir_empty {
        println!("  (none)");
    } else {
        for s in &report.sessions {
            println!("  {:<10} focus: {}", s.id, s.focus);
        }
    }
    println!("HELD LEASES:");
    if report.leases.is_empty() {
        println!("  (none)");
    } else {
        for l in &report.leases {
            println!("  {:<28} by {} — {}", l.area_slug, l.holder, l.why);
        }
    }
    if let Some(h) = &report.merge_lock_holder {
        println!("MERGE LOCK: held by {h}");
    }

    println!("\n── Last prose post per session ──");
    let sync = std::fs::read_to_string(&paths.sync).unwrap_or_default();
    for s in &report.sessions {
        if let Some(line) = last_prose_post(&sync, &s.id) {
            let paused = if paths.coord.join("paused").join(&s.id).exists() {
                " [PAUSED]"
            } else {
                ""
            };
            println!("  {line}{paused}");
        }
    }
    ExitCode::SUCCESS
}

/// The last `### <id> ` header line for a session in the prose channel.
fn last_prose_post(sync: &str, id: &str) -> Option<String> {
    let needle = format!("### {id} ");
    sync.lines()
        .rfind(|l| l.to_lowercase().starts_with(&needle.to_lowercase()))
        .map(String::from)
}

/// `concord pause <id>` — set the pause flag (the session's tick should then only heartbeat).
pub fn cmd_pause(paths: &Paths, id: &str) -> ExitCode {
    let dir = paths.coord.join("paused");
    let _ = std::fs::create_dir_all(&dir);
    match std::fs::write(dir.join(id), "") {
        Ok(()) => {
            println!("pausiert: {id} (Self-Tick soll das Flag prüfen → nur heartbeaten)");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("concord: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `concord resume <id>` — clear the pause flag.
pub fn cmd_resume(paths: &Paths, id: &str) -> ExitCode {
    let _ = std::fs::remove_file(paths.coord.join("paused").join(id));
    println!("fortgesetzt: {id}");
    ExitCode::SUCCESS
}

/// `concord stop <id>` — ask a session (via the prose channel) to stop cleanly.
pub fn cmd_stop(store: &Store, id: &str) -> ExitCode {
    let coord = coordinator_id(&launcher_config(store.paths()));
    let _ = store.sync(
        &coord,
        id,
        "STOP (concord)",
        "Bitte sauber stoppen: Leases freigeben, IDLE an den Koordinator posten, dann Fenster schließen.",
    );
    println!("Stop-Signal an {id} in den Prosa-Kanal gepostet (die Session stoppt beim nächsten Tick).");
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths_at(project: &str) -> Paths {
        let p = PathBuf::from(project);
        Paths {
            coord: p.with_file_name(format!(
                "{}-coord",
                p.file_name().unwrap().to_string_lossy()
            )),
            sessions: PathBuf::new(),
            leases: PathBuf::new(),
            resources: PathBuf::new(),
            acks: PathBuf::new(),
            escalations: PathBuf::new(),
            telemetry: PathBuf::new(),
            contracts: PathBuf::new(),
            log: PathBuf::new(),
            merge_lock: PathBuf::new(),
            sync: PathBuf::new(),
            project: p,
            ttl: 1800,
        }
    }

    #[test]
    fn worktree_follows_convention() {
        let paths = paths_at("/home/u/Projects/myrepo");
        // <repo-parent>/<repo>-<id-lower>
        assert_eq!(
            worktree_for(&paths, "B"),
            Path::new("/home/u/Projects/myrepo-b")
        );
        assert_eq!(
            worktree_for(&paths, "concord-w"),
            Path::new("/home/u/Projects/myrepo-concord-w")
        );
    }

    #[test]
    fn coordinator_is_case_insensitive() {
        // default coordinator id is "hub"
        let cfg = Config::default();
        assert!(is_coordinator("hub", &cfg));
        assert!(is_coordinator("HUB", &cfg));
        assert!(!is_coordinator("a", &cfg));
    }

    #[test]
    fn last_prose_post_picks_the_latest() {
        let sync = "### a → hub  (FIRST)\nbody\n### b → hub (x)\n### a → hub  (LATEST)\nmore";
        assert_eq!(
            last_prose_post(sync, "a").as_deref(),
            Some("### a → hub  (LATEST)")
        );
        assert_eq!(last_prose_post(sync, "zzz"), None);
    }

    #[test]
    fn kickoff_picks_role_and_substitutes() {
        let cfg = Config::default();
        let wk = kickoff_for("a", "/x/sync.md", &cfg);
        assert!(wk.contains("worker session a"));
        assert!(wk.contains("/x/sync.md"));
        assert!(wk.contains("### a -> hub (READY"));
        let co = kickoff_for("hub", "/x/sync.md", &cfg);
        assert!(co.contains("coordinator session hub"));
    }
}
