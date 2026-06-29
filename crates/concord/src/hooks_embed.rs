//! Embedded Concord hook scripts + the `install-hooks` command (M4.1).
//!
//! A `cargo install`'d `concord` binary ships with no repo checkout, so the Claude
//! Code automation layer (the `hooks/` scripts) must travel *inside* the binary. Each
//! script is embedded verbatim via [`include_str!`] at build time; `install-hooks`
//! materializes them into `<coord>/hooks/` and (on Unix) wires `~/.claude/settings.json`
//! by running the just-written `install.sh` — the same proven, parity-tested path the
//! repo uses, so no logic is duplicated and the core binary stays dependency-light.
//!
//! Cross-platform note: the file laydown works everywhere; the settings-wiring step is
//! Unix session-automation (bash + python3) and is skipped off Unix. That matches the
//! support matrix — Windows runs the enforced *Floor* (FS-authoritative leases), not the
//! session-automation hooks.

use std::fs;
use std::io;
use std::path::Path;
use std::process::ExitCode;

use concord_core::Paths;

/// One embedded hook file: its name under `hooks/`, its contents, and whether it should
/// be executable (the `.sh` scripts) or a plain data file (`shared-regions`).
struct HookFile {
    name: &'static str,
    body: &'static str,
    exec: bool,
}

/// The full set of embedded hook files. Paths are relative to THIS source file
/// (`crates/concord/src/`), reaching the repo-root `hooks/` directory.
const HOOK_FILES: &[HookFile] = &[
    HookFile { name: "lib.sh", body: include_str!("../../../hooks/lib.sh"), exec: true },
    HookFile { name: "session-start.sh", body: include_str!("../../../hooks/session-start.sh"), exec: true },
    HookFile { name: "session-end.sh", body: include_str!("../../../hooks/session-end.sh"), exec: true },
    HookFile { name: "user-prompt.sh", body: include_str!("../../../hooks/user-prompt.sh"), exec: true },
    HookFile { name: "post-tool.sh", body: include_str!("../../../hooks/post-tool.sh"), exec: true },
    HookFile { name: "pre-tool.sh", body: include_str!("../../../hooks/pre-tool.sh"), exec: true },
    HookFile { name: "stop.sh", body: include_str!("../../../hooks/stop.sh"), exec: true },
    HookFile { name: "pre-compact.sh", body: include_str!("../../../hooks/pre-compact.sh"), exec: true },
    HookFile { name: "statusline.sh", body: include_str!("../../../hooks/statusline.sh"), exec: true },
    HookFile { name: "install.sh", body: include_str!("../../../hooks/install.sh"), exec: true },
    HookFile { name: "uninstall.sh", body: include_str!("../../../hooks/uninstall.sh"), exec: true },
    HookFile { name: "shared-regions", body: include_str!("../../../hooks/shared-regions"), exec: false },
];

/// `concord install-hooks [--no-wire]` — write the embedded hook scripts into
/// `<coord>/hooks/` and (on Unix, unless `--no-wire`) wire `~/.claude/settings.json`.
pub fn cmd_install_hooks(paths: &Paths, wire_settings: bool) -> ExitCode {
    match write_hooks(paths) {
        Ok(dir) => {
            println!("✓ wrote {} hook files → {}", HOOK_FILES.len(), dir.display());
            if wire_settings {
                wire(&dir);
            } else {
                println!("  (--no-wire: skipped ~/.claude/settings.json — run `bash {}/install.sh` to wire it)", dir.display());
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("concord: install-hooks failed: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Materialize the embedded files into `<coord>/hooks/`, returning that directory.
fn write_hooks(paths: &Paths) -> io::Result<std::path::PathBuf> {
    let dir = paths.coord.join("hooks");
    fs::create_dir_all(&dir)?;
    for f in HOOK_FILES {
        let path = dir.join(f.name);
        fs::write(&path, f.body)?;
        if f.exec {
            set_executable(&path)?;
        }
    }
    Ok(dir)
}

/// Set the user/group/other execute bits on Unix; a no-op elsewhere.
#[cfg(unix)]
fn set_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perm = fs::metadata(path)?.permissions();
    perm.set_mode(perm.mode() | 0o755);
    fs::set_permissions(path, perm)
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// Wire `~/.claude/settings.json` by running the just-written `install.sh` (Unix only —
/// the wiring + the hooks themselves are bash + python3 session-automation).
#[cfg(unix)]
fn wire(dir: &Path) {
    let install = dir.join("install.sh");
    match std::process::Command::new("bash").arg(&install).status() {
        Ok(s) if s.success() => {}
        Ok(s) => eprintln!(
            "concord: settings wiring exited with {} — run `bash {}` manually",
            s.code().unwrap_or(-1),
            install.display()
        ),
        Err(e) => eprintln!(
            "concord: could not run {} ({e}) — wire it manually",
            install.display()
        ),
    }
}

/// Off Unix the session-automation hooks (bash + python3) don't apply: only the enforced
/// Floor is available. The files are still written (for reference / WSL); settings stay
/// untouched.
#[cfg(not(unix))]
fn wire(_dir: &Path) {
    println!("  (non-Unix: session-automation hooks are Unix-only; settings.json left untouched.");
    println!("   Windows runs the enforced Floor — FS-authoritative leases — without the daemon/hooks.)");
}
