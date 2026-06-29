# Concord

**Let several AI coding assistants work in the same codebase at once — without stepping on each other.**

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.11.0-blue.svg)](CHANGELOG.md)

Concord is a small, self-contained tool that coordinates a team of AI coding sessions (for
example several [Claude Code](https://claude.com/claude-code) windows) working on one
repository. It hands out **locks** on the parts of the code each session is changing, lets a
single **AI coordinator** assign and sequence the work, and keeps every session from
clobbering another's edits or merges. No server and no database — just files on disk.

---

## The problem

Run five or six AI sessions on one codebase and, without coordination, they collide: two
edit the same file at the same moment, their merges race, one kills another's test process.
And an AI session only does something when it's given a turn — between turns it goes quiet,
so sessions drift apart, repeat each other's work, or simply stall unnoticed.

Concord fixes this by making the rules **enforced**, not merely suggested.

## The headline: an autonomous coordinator

Most tools in this space assume a **human** sits at a dashboard handing out tasks. Concord
is built around an **AI coordinator** instead — one session whose whole job is to assign work
to the others, decide the order, and approve merges, with no human babysitting required. You
talk to the coordinator; the coordinator runs the team. (You can still watch and step in any
time.) This self-driving coordinator is the thing Concord has that comparable tools don't.

## What Concord does

Every feature below is **enforced through the shared filesystem**, so it works the same
whether sessions cooperate or not.

- **Locks on the code you're touching (leases).** A session claims a file or a folder; while
  it holds that claim, no other active session may edit it. When the session finishes (or
  crashes and goes silent), the claim is released automatically.

- **Locks on a single function, not just a whole file.** Concord understands the *structure*
  of your code (via tree-sitter), so two sessions can safely edit the **same file at the same
  time as long as they touch different functions or types**. This finer-grained locking is
  something file-level tools cannot offer.

- **Interface agreements (signature contracts).** Two sessions can agree on the shape of a
  function or type — its signature — and Concord will **block any commit or merge that changes
  that agreed shape** until both sides renegotiate. Changing the *body* is fine; changing the
  *interface* others depend on is caught. Built on the same code-structure understanding as
  symbol locks.

- **A safety token so a paused session can't cause damage (fencing).** If a session stalls
  past its lock's expiry and another takes over, the stalled one cannot later act on its
  now-stale authority — no "both think they hold the lock" split-brain.

- **One merge at a time (the merge lock).** Only one session merges to the main branch at
  once, so merges never race. The coordinator hands out this turn.

- **Instant notifications instead of constant polling (the push daemon).** A small background
  helper watches the shared state and delivers a message to a session the moment something
  relevant changes, instead of every session waking up repeatedly to check.

- **Hard enforcement right at the keystroke (editor hooks).** Optional Claude Code hooks turn
  the locks into a real stop: an edit to a file another session has locked is **blocked before
  it happens**, not flagged afterwards. The same hooks keep a session from quietly ending its
  turn while it still has an unanswered instruction — curing the "went silent" failure.

- **Self-healing locks for shared resources and build slots.** Beyond files, Concord can lock
  named resources — ports, a build environment, deploy slots — with **N slots** (a pool). If
  a session holding a slot crashes, its slot frees itself automatically, so a stuck session
  never blocks the pool forever. (Designed for exactly the kind of "two sessions fight over
  the same test port" problem.)

- **Instructions that can't be silently ignored (acknowledgements + escalation).** When the
  coordinator sends a session an instruction, Concord tracks whether it was acknowledged. An
  unanswered instruction is re-delivered and, if still ignored, **escalated** as a tracked
  item that stays open until someone resolves it — blockers can't vanish.

- **A coordinator that measures the team, not just reads messages (telemetry).** Concord can
  consume each session's built-in usage signals (tokens, cost, tool activity) locally — no
  external service — so the coordinator can *see* which session is idle, burning through its
  budget, looping, or hitting a wall, and a **watchdog automatically escalates a session that
  has gone dark** while still holding work.

- **One configuration file.** All the knobs — lock timeouts, the coordinator's name, ports,
  thresholds — live in one readable `config.toml`. No environment variables to remember.

- **Typed tools for the coordinator (MCP).** Concord exposes its operations as typed tools
  over the Model Context Protocol, so an AI coordinator can drive them directly and safely.

- **A launcher and a live overview.** Start a session in a terminal, see who is doing what at
  a glance, and pause / resume / stop sessions — all from one command.

- **Many projects, and itself.** One Concord install coordinates several repositories at
  once, each with its own state. It even coordinates its own development.

- **Runs everywhere, ships as one binary.** A single cross-platform executable with prebuilt
  releases and a one-line installer (see Platform support for what's available on Windows).

## Install

```bash
# macOS / Linux — one line:
curl --proto '=https' --tlsv1.2 -LsSf \
  https://raw.githubusercontent.com/msto63/concord/main/scripts/install.sh | sh

# …or with Cargo (any platform):
cargo install --git https://github.com/msto63/concord concord

# …or download a prebuilt binary (incl. a Windows .zip) from the GitHub Releases page.
```

## 60-second quick start

```bash
# 1. In your repository, set up coordination + the editor hooks:
concord init --with-hooks

# 2. Give each session its own copy of the repo (a git worktree), named <repo>-<id>:
git worktree add ../your-repo-a -b a/work     # a worker
git worktree add ../your-repo-k -b k/work     # the coordinator

# 3. Launch a session per terminal:
concord start a      # a worker — announces itself, waits for the coordinator's go-ahead
concord start k      # the coordinator — assigns and sequences the work

# 4. See who's working on what:
concord dash
```

Each session boots into its own worktree, reports that it's ready, and waits for the
coordinator's go-ahead before taking work. Coordination state lives in `<repo>-coord/`
next to your repo; the human-readable discussion log is `<repo>-SESSION-SYNC.md`.

## Platform support

| Platform | What you get |
|---|---|
| **macOS / Linux** | Everything: the full locking stack, the airtight background daemon, telemetry, and the editor-hook automation. |
| **Windows** | The core guarantees still hold — leases (file and symbol level), the merge lock, fencing, and signature contracts all enforce through the shared filesystem. The background daemon's tightest path, the telemetry receiver, and the bash-based editor hooks are Unix-only for now (native-Windows equivalents are on the roadmap). |

## Documentation

| Document | Purpose |
|---|---|
| **[docs/MANUAL.md](docs/MANUAL.md)** | Full reference: the model, every command, the hooks, configuration, and ready-made prompts. |
| **[docs/QUICKSTART.md](docs/QUICKSTART.md)** | A short, worked setup. |
| **[docs/ROADMAP.md](docs/ROADMAP.md)** | Direction and planned capabilities. |
| **[docs/BACKLOG.md](docs/BACKLOG.md)** | Actionable work and its status. |
| **[docs/adr/](docs/adr/)** | Architecture Decision Records — the durable rationale behind the design. |

## Status

Concord was built and battle-tested inside the [`ais`](https://github.com/msto63/ais)
project (a Rust operating system) over many weeks with five to six concurrent sessions, then
spun out as its own tool. It now coordinates any repository — and its own development.

## License

[MIT](LICENSE) © 2026 Mike Stoffels
