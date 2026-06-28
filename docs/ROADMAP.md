# Concord — Roadmap

Direction and planned capabilities. This is the *vision* layer; concrete, trackable work lives in
[BACKLOG.md](BACKLOG.md), which is kept reconciled with this document.

Concord's north star: **let any team of AI sessions coordinate any repository — cheaply, reliably,
and without a server — and let Concord coordinate its own development (dogfooding).**

## Done

- **Coordination core** — registry, area leases, singleton merge lock, stale reclaim, the prose
  channel, `coord.sh sync`.
- **Automation layer** — Claude Code hooks: window-identity statusline, auto-register/heartbeat,
  status injection (no missed broadcasts), lease/merge enforcement.
- **Mission-control CLI** — `concord start/stop/pause/resume/dash`; identity via `CONCORD_ID`;
  the `<repo>-<id>` worktree convention; the READY/GO dispatch handshake; launch in the current
  terminal (terminal-agnostic, no spawned window).

## In progress

- **§8 — Multi-project support.** Make Concord coordinate more than one project at once, cleanly
  isolated. Scripts now read `CONCORD_DIR` / `CONCORD_SYNC` / `CONCORD_PROJECT` from the
  environment (else derive `<repo>-coord` / `<repo>-SESSION-SYNC.md` by convention); `concord`
  exports them at launch. **Remaining:** de-hardcode the message path in `session-start.sh`; drop
  the legacy fallbacks later; migrate the `ais` consumer onto this repo; add `concord init`.
  *Isolation key: state directory + channel per project — not the session id.*

## Planned

- **§6 — Structured board.** `board.jsonl` + `concord board`: all work packages → tasks with
  status × priority × owner. The coordinator sets priority; the owner flips status.
- **§7 — Concord MCP server.** Expose register/claim/merge-lock/status/board as *typed* tools
  instead of shell calls; feeds the board/dashboard.
- **§9 — Cheaper inter-agent communication.** The prose channel grows monotonically and every
  session reads or injects from it, which is the dominant token cost. Building blocks:
  - **Structured messages** with fixed fields `{from, to, type, ref, body}` and a type enum
    (`READY | GO | ACK | DONE | BLOCKED | DESIGN | DECISION | PR | NUDGE`) plus a *short* natural-
    language body. Small and parseable — but **not** a cryptic code, because LLM reliability needs
    some natural language.
  - **Per-recipient inboxes** — one queue per session (`inbox/<id>`); a session reads **only its**
    messages, not the whole shared channel. This is the largest token-saving lever.
  - **Delta injection** — the hook injects only unseen messages for the session (a marker per id).
  - **Reference, don't repeat** — point at ids (PR#, lease, task id) instead of restating context.
  - **Keep a human log** — a readable prose/audit log remains for the human and coordinator; the
    *agents* talk over the compact structured queue. (`coord.sh sync` is the first step toward this.)

## Dogfooding

Once multi-project support is solid: `concord-coord/` + `concord-SESSION-SYNC.md` + worktrees
`concord-a … concord-k`, so Concord's own development is coordinated with Concord.

## `ais` migration (deliberate — must not break the running ais team)

1. Point the global Claude Code hooks at this repo's `hooks/`.
2. Launch ais sessions with `CONCORD_DIR`/`CONCORD_SYNC` for the ais project (or let `concord`
   derive them from the ais repo root).
3. Symlink ais's `tools/coord.sh` to this repo's `bin/coord.sh` — one source of truth.
