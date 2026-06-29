# Concord

**File-based coordination for multi-session AI development teams.**

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.5.0-blue.svg)](CHANGELOG.md)

Concord lets **several AI coding sessions** (e.g. [Claude Code](https://claude.com/claude-code))
work the **same repository in parallel** without clobbering each other — no server, no
database, just shell scripts and editor hooks over the shared filesystem.

---

## The problem

Run 5–6 AI sessions on one codebase and, without coordination, they collide: two sessions
edit the same file at once, merges to `main` race, one session kills another's test VM. Worse,
an AI session only acts when its harness gives it a turn — between turns it goes **dormant**.
So sessions silently drift apart, duplicate work, or go dark.

## The approach

- **One session per git worktree** (`<repo>-a`, `<repo>-b`, …) so edits never share a checkout.
- **A coordinator session ("K")** sequences work, arbitrates merges, and keeps everyone aligned.
- **Cooperative leases** on shared regions + a **singleton merge lock** prevent collisions.
- **A registry + heartbeats** make "who is active, who is stale" observable.
- **Claude Code hooks** automate the discipline: window identity, auto-register/heartbeat,
  status injection, and lease/merge enforcement — so sessions stay self-driven, not dormant.

A human (the operator) talks to the coordinator; the coordinator dispatches the workers.

## Quick start

```bash
# 1. Install the CLI (any one):
curl --proto '=https' --tlsv1.2 -LsSf \
  https://raw.githubusercontent.com/msto63/concord/main/scripts/install.sh | sh   # macOS / Linux
#   cargo install --git https://github.com/msto63/concord concord                  # any platform
#   …or download a prebuilt archive from the GitHub Releases page (incl. Windows .zip)

# 2. Install the Claude Code hooks (statusline, registry, status injection, guards).
#    The hook scripts ship *inside* the binary — no repo checkout needed.
cd /path/to/your-repo
concord init --with-hooks                  # scaffolds <repo>-coord/ and wires ~/.claude/settings.json

# 3. Create one git worktree per session, named <repo>-<id>
cd /path/to/your-repo
git worktree add ../your-repo-a -b a/work
git worktree add ../your-repo-b -b b/work
# … and ../your-repo-k for the coordinator

# 4. From the repo root, launch a session per terminal tab
concord start a      # runs in THIS terminal, in worktree your-repo-a
concord start k      # the coordinator

# 5. See who is working on what
concord dash
```

Each session boots into its worktree with full permissions, reports `READY`, and waits for the
coordinator's `GO` before taking work. State lives in `<repo>-coord/`; the human-readable
discussion channel is `<repo>-SESSION-SYNC.md` (both next to the repo).

## Platform support

| Platform | Enforcement | Notes |
|---|---|---|
| **macOS / Linux** | **Strong** (airtight) | Full stack: the `concordd` daemon mediates consequential writes over a Unix socket (the single serialization point), plus the FS-authoritative Floor and the Claude Code session-automation hooks. |
| **Windows** | **Floor** (FS-authoritative) | Leases, the merge singleton, fencing, and symbol-locks all enforce via the shared filesystem — the core guarantees hold. The daemon's Unix-socket Strong tier, the **F4 telemetry receiver**, and the bash/python session-automation hooks are Unix-only; `concord install-hooks` lays the scripts down but leaves `settings.json` untouched off Unix. A Windows named-pipe Strong tier is a [backlog](docs/BACKLOG.md) item. |

The typed core (`concord-core`), CLI, daemon, and MCP server all compile cleanly for
Windows — the Unix-socket paths are `cfg`-gated to a no-op so every consequential op
falls back to the enforced Floor when no daemon is present.

## Documentation

| Document | Purpose |
|---|---|
| **[docs/MANUAL.md](docs/MANUAL.md)** | Full reference: model, mechanics, protocols, CLI, hooks, setup, ready-made prompts |
| **[docs/ROADMAP.md](docs/ROADMAP.md)** | Direction and planned capabilities |
| **[docs/BACKLOG.md](docs/BACKLOG.md)** | Actionable work packages and their status |

## Status

Concord was built and battle-tested in the [`ais`](https://github.com/msto63/ais) project (a Rust
operating system) over many weeks with 5–6 concurrent sessions. It is now its own project and is
being generalized to coordinate **any** repository — and itself.

## License

[MIT](LICENSE) © 2026 Mike Stoffels
