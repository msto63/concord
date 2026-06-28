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
- [ ] `CONTRIBUTING.md`
- [ ] Public GitHub repo (MIT) created + pushed
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
