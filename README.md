# Concord

**File-based coordination for multi-session AI development teams.**

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.1.0-blue.svg)](CHANGELOG.md)

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

A human ("mike") talks to the coordinator; the coordinator dispatches the workers.

## Quick start

```bash
# 1. Put the CLI on your PATH
ln -s ~/Projects/concord/bin/concord /opt/homebrew/bin/concord

# 2. Install the Claude Code hooks (statusline, registry, status injection, guards)
~/Projects/concord/hooks/install.sh

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
