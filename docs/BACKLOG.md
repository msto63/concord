# Concord — Backlog

Actionable work, grouped into work packages (WPs). This is the execution layer; it is kept
reconciled with [ROADMAP.md](ROADMAP.md) (the roadmap section each WP serves is noted as `→ §n`).

**Status:** `[x]` done · `[~]` in progress · `[ ]` open  **Priority:** P1 (next) · P2 · P3 (later)

---

## WP0 — Coordination core `[x]` (→ Done)
- [x] File-based registry (`register` / `heartbeat` / `status`), stale TTL
- [x] Area leases (`claim` / `release`) with cooperative conflict + stale reclaim
- [x] Singleton merge lock (`merge-lock` / `merge-unlock`)
- [x] Prose channel + `coord.sh sync` (sandbox-safe posting)
- [x] Structured intent log (`coord.sh log`)

## WP1 — Automation hooks `[x]` (→ Done)
- [x] Statusline window identity (`● <id>` via `CONCORD_ID`)
- [x] SessionStart auto-register + heartbeat + standing-instruction injection
- [x] Status injection of new `### … → <id>` directives (no missed broadcasts)
- [x] Lease / merge PreToolUse guard (default-allow, block on certain collision)
- [x] `install.sh` / `uninstall.sh` (settings backup + merge)

## WP2 — Mission-control CLI `[x]` (→ Done)
- [x] `concord start/stop/pause/resume/dash`
- [x] Identity via `CONCORD_ID`
- [x] `<repo>-<id>` worktree convention (no hardcoded map)
- [x] READY/GO dispatch handshake
- [x] Launch in the current terminal (terminal-agnostic)

## WP3 — Documentation & public release `[~]` P1
- [x] MIT `LICENSE`, `.gitignore` (excludes local management: `CLAUDE.md`, state, channel)
- [x] English `README.md` (overview + quick start)
- [x] English `docs/MANUAL.md` (full reference)
- [x] English `docs/ROADMAP.md`, `docs/BACKLOG.md`
- [x] Local `CLAUDE.md` (git-ignored project instructions)
- [x] `CONTRIBUTING.md`
- [x] Public GitHub repo (MIT) created + pushed — https://github.com/msto63/concord
- [ ] Screenshot / asciinema of `concord dash` for the README  `P3`

## WP4 — Multi-project support `[~]` P1 (→ §8)
- [x] `coord.sh` / `concord` read `CONCORD_DIR`/`CONCORD_SYNC`/`CONCORD_PROJECT`, else derive by convention
- [x] `concord` exports the env at launch
- [x] `hooks/lib.sh` + `user-prompt.sh` read `CONCORD_SYNC`
- [ ] De-hardcode the message path in `hooks/session-start.sh`
- [ ] `concord init <ids…>` — create standard worktrees + state dir for a new project  `P2`
- [ ] Remove `AIS_*` legacy fallbacks once ais is migrated  `P3`

## WP5 — Dogfooding (Concord coordinates itself) `[ ]` P2 (→ Dogfooding)
- [ ] `concord-coord/` + `concord-SESSION-SYNC.md`
- [ ] Worktrees `concord-a … concord-k`
- [ ] Verify two isolated projects (ais + concord) run in parallel without cross-talk

## WP6 — ais migration `[ ]` P2 (→ ais migration)
- [ ] Point global hooks at `~/Projects/concord/hooks/`
- [ ] Launch ais sessions with ais `CONCORD_DIR`/`CONCORD_SYNC`
- [ ] Symlink ais `tools/coord.sh` → `bin/coord.sh` (single source of truth)
- [ ] Verify the running ais team is unaffected through the switch

## WP7 — Cheaper inter-agent communication `[ ]` P2 (→ §9)
- [ ] Structured message format `{from,to,type,ref,body}` + type enum
- [ ] Per-recipient inbox (`inbox/<id>`) — read only your own messages
- [ ] Delta injection (per-id seen-marker)
- [ ] Keep a human-readable prose/audit log alongside
- [ ] Measure token cost before/after on a real session

## WP8 — Structured board `[ ]` P3 (→ §6)
- [ ] `board.jsonl` schema (work package → tasks, status × priority × owner)
- [ ] `concord board` view; coordinator sets priority, owner flips status

## WP9 — Concord MCP server `[ ]` P3 (→ §7)
- [ ] Typed tools for register/claim/merge-lock/status/board
- [ ] Feeds the board/dashboard

## WP10 — Versioning & release discipline `[x]` (→ Done)
- [x] `VERSION` (single source of truth) starting at `0.1.0`; `concord version`
- [x] `CHANGELOG.md` (Keep a Changelog + semver)
- [x] `scripts/check-version.sh` (VERSION ↔ CHANGELOG ↔ `concord version` ↔ tag)
- [x] Local **pre-push hook** enforcing the check (`scripts/install-hooks.sh`) — no CI, no cost
- [x] Release process documented (CONTRIBUTING) + standing rule (CLAUDE.md)
- [x] Tag `v0.1.0`

## WP12 — Rust rewrite (platform-independent binary) `[ ]` P1 — DECISION: ADOPT (operator, M1+M2+M3)
*Proposed direction: replace the shell scripts with a single cross-platform Rust binary. If
adopted, this **supersedes WP11** (no need to paper over BSD-vs-GNU shell differences — Rust's
stdlib is portable) and gives native Windows support without WSL2.*

> **Operator-Entscheid (2026-06-29):** ADOPT, voller Schnitt **M1+M2+M3**, Start nach der laufenden
> ais-Welle, dedizierte Session `concord-w`, research-gestützter ADR zuerst (→ `WP12-RUST-PORT.md`,
> `WP12-RESEARCH.md`). **WP7 + WP8 + WP9 sind in WP12 gefaltet** (= dessen M3 „typisierte Agent-Schicht"):
> sie sind nicht Nachbarn des Ports, sondern sein Ertrag — ein gemeinsamer typisierter Kern (M1),
> exponiert als CLI + MCP-Server (`rmcp`), Push via `notify`+debouncer (M2), Inbox/Board/MCP fallen
> aus dem Kern heraus (M3). M1 (Parität) bleibt eigenständig auslieferbar.
- [ ] **Decide** shell-maintenance vs. Rust rewrite (see ROADMAP §11). Owner: operator.
- [ ] Define the CLI surface (`concord` + the `coord` subcommands) and keep the **file-based state
      layout unchanged**, so the binary is a drop-in replacement that can coexist with the scripts
      during transition.
- [ ] Hooks as binary subcommands (`concord hook session-start` etc.) — Claude Code invokes a
      command, which can be the binary.
- [ ] Version from `Cargo.toml` (`env!("CARGO_PKG_VERSION")`) becomes the source of truth;
      `concord version` and the changelog discipline carry over.
- [ ] Release prebuilt binaries (macOS/Linux/Windows) and/or `cargo install`.
- [ ] Port incrementally with behaviour parity; retire the shell version once at parity.

## WP11 — Cross-platform support via shell (Linux, Windows 11) `[ ]` P3 — likely SUPERSEDED by WP12
*Only pursue if the Rust rewrite (WP12) is declined. macOS works today.*
- [ ] Abstract OS-specific calls behind portable helpers — `date -r` (BSD) vs `date -d @` (GNU),
      `stat -f %m` (BSD) vs `stat -c %Y` (GNU). One change unlocks **Linux + WSL2 + Git Bash**.
- [ ] Replace macOS `/opt/homebrew/bin` examples with a generic PATH dir (`/usr/local/bin`, `~/.local/bin`).
- [ ] `.gitattributes` forcing LF on all scripts (so a Windows checkout doesn't CRLF-break shebangs).
- [ ] **Linux:** verify native run (bash + the helpers above).  `P2`
- [ ] **Windows 11:** document + verify **WSL2** as the recommended path (= the Linux run, near-zero
      extra work); note Git Bash as a fragile alternative (path/CRLF caveats).  `P2`
- [ ] Support matrix in README + MANUAL (macOS ✓ · Linux ✓ · Windows via WSL2 ✓ · native PowerShell = out of scope).
- [ ] (Out of scope) native PowerShell port — large rewrite, not worth it while WSL2 exists.  `P3`
