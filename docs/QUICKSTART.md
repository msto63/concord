# Concord Quickstart

Coordinate several AI coding sessions on one shared repo — area leases, a singleton
merge lock, and a shared channel — with one small Rust binary. Three steps.

## 1. Build

```sh
git clone https://github.com/msto63/concord && cd concord
cargo build --release          # binary at target/release/concord
```

(Optional: put it on your PATH — `cp target/release/concord ~/.local/bin/` — so you
can type `concord` instead of the full path.)

## 2. Initialise a project's coordination state

Run this **in (or pointed at) the repo your sessions will share**:

```sh
concord init --ids hub,a,b
```

This creates, next to your repo, an inspectable coordination state:

- `<repo>-coord/`  — sessions, leases, the merge lock, the append-only ledger
- `<repo>-SESSION-SYNC.md`  — the human prose channel (discussion + audit log)

`hub` is the coordinator; `a`/`b` are workers. Re-running `init` is safe (idempotent).
`concord paths` prints the resolved locations (`eval "$(concord paths)"`).

## 3. Start a session per terminal

Open one terminal per session and tell Concord who it is via `CONCORD_ID`:

```sh
CONCORD_ID=a   concord status      # this terminal is session "a"
```

(If you use the shell launcher `bin/concord start <id>`, it sets the id + env for you.)

## Daily commands

```sh
concord status                              # who's active, what's leased, merge-lock holder
concord claim   a kernel/src/main.rs "why"  # take a lease before editing a shared area
concord verify  a kernel/src/main.rs        # do I still hold it? (after a pause)
concord release a kernel/src/main.rs        # give it back when done
concord merge-lock   hub "release train"    # only one session merges at a time
concord merge-unlock hub
concord sync    a hub "STATUS" "main.rs done"   # post a line to the prose channel
```

A claim refuses if the area path-overlaps a live lease; release/merge-unlock refuse if
you don't hold it — coordination is *enforced*, not merely advisory.

## The protocol in one paragraph

A coordinator session (`hub`) assigns work and sequences merges; workers claim a lease
before touching a shared area, hold the merge lock only while merging, and post status
to the channel. State lives in the filesystem, so `ls`/`cat` always show the truth and a
crash never corrupts coordination.

## Optional power-ups

- **Push instead of poll** — run the daemon (`concordd`) to demultiplex the channel into
  per-session inboxes so a session wakes on *its* directives instead of re-reading the
  whole channel, and to make merge ops airtight. Optional; the filesystem stays
  authoritative without it.
- **Typed MCP tools** — `concord-mcp` exposes the enforced primitives
  (claim/release/merge-lock/verify/…) as MCP tools over stdio, so an agent drives the
  core schema-validated instead of via shell strings.
- **Worktrees** — give each session its own `git worktree` (`<repo>-<id>`) for parallel
  isolation on top of the shared coordination.
- **Multiple projects / dogfood** — one binary coordinates many repos, each isolated by
  its own coord dir.
