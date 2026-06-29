# MCP Agent Mail — Code-Level Verification (against Concord's mechanics)

> Status: 2026-06 · Verifier: session `concord-w` (built Concord's M1/M2 core) · Method:
> the three repos were **cloned and read at source level** (`Dicklesworthstone/mcp_agent_mail`,
> `amaar-mc/wit`, `garniergeorges/claude-presence`); every verdict cites `file:line` in the
> actual code, not the README or the description-based competitive note. Factual, no advocacy —
> the stay-vs-refocus decision is the operator's.
>
> Commissioned by `hub` to verify the competitive-landscape claims (survey archived locally) *at the code*, with the
> decisive question being **enforced vs. advisory** coordination (Concord's vision dividing line).

## TL;DR — verdict table

| Claim (from the competitive note) | Code-level verdict |
|---|---|
| Agent Mail is **Rust** (12-crate workspace, Tokio) | **REFUTED** — it is **Python** (FastMCP + SQLAlchemy + aiosqlite). No `Cargo.toml`, 160 `.py` files. |
| …and is "already the **Rust+SQLite+MCP** endstate Concord targets" | **PARTIAL** — SQLite ✓, MCP ✓, **Rust ✗**. It is *not* the Rust endstate. |
| File **leases** with glob/TTL/stale-reclaim | **CONFIRMED** — globs+paths, TTL=3600s default, two stale-reclaim sweeps, exclusive+shared. |
| **Path-prefix overlap** detection "built-in / standard" | **PARTIAL** — overlap via gitignore PathSpec; symmetric for globs, **one-directional for literal paths**. Not fully symmetric. |
| Leases are **advisory** | **CONFIRMED** — the grant never blocks; "Advisory model: still grant … surface conflicts". |
| **Build-slots ≈ merge-lock** (singleton merge coordination) | **PARTIAL** — advisory per-named-slot mutex *signal*, **no numeric concurrency cap**, writes lease regardless of conflict. Not an enforced singleton merge-lock. |
| **Messaging** richer (threads/ack/search) than our JSONL inbox | **CONFIRMED** — threaded + reply edges, read/ack ts, to/cc/bcc, 4 importance levels, FTS5 BM25 search, SQLite **+ git archive**. Far richer. |
| **Registry + heartbeat** with TTL expiry | **CONFIRMED (nuance)** — project-scoped registry, memorable identities, TTL auto-retire (24h); **no dedicated heartbeat ping** (liveness = activity-derived). |
| **34 MCP tools** | **REFUTED (undercount)** — **~48** `@mcp.tool` decorators. |
| **Fencing token** (reject stale holder after reclaim) | **REFUTED** — no monotonic fencing/generation token anywhere. |
| **Ownership-enforcement** (no foreign release) | **CONFIRMED** — release filters `agent_id`; an agent can only release its own leases. |
| **Human-overseer** centered (no autonomous coordinator) | **CONFIRMED** — explicit `HumanOverseer` HTTP role; **no autonomous-coordinator role in code**; agents are strict peers. |
| Adopt mechanics via MCP, keep own coordinator | **CONFIRMED feasible** — all mechanics are MCP-exposed, no hard human dependency. |

## 1. Stack — Python, not Rust  (REFUTED a headline claim)

The competitive note calls Agent Mail "Rust (12-crate-workspace, Tokio)". The cloned HEAD is
**Python**: `pyproject.toml` declares `fastmcp>=2.10.5`, `sqlalchemy[asyncio]>=2.0.35`,
`aiosqlite>=0.20.0`; there is a `uv.lock`, 160 `.py` files, no `Cargo.toml`, no `.rs`. The core is
`src/mcp_agent_mail/`: `app.py` (14,497 LoC, the MCP tools), `storage.py` (3,763, git mirror),
`guard.py` (778, the commit hook), `db.py`, `models.py`. SQLite ✓ and MCP ✓ are real; **Rust is
not**. So the framing "Agent Mail is already the Rust+SQLite+MCP endstate Concord's roadmap targets"
is wrong on the language axis — adopting it means depending on a **Python FastMCP** service, not a
Rust core.

## 2. Leases — CONFIRMED, overlap PARTIAL

`FileReservation` rows (`models.py:117-137`), created by the `file_reservation_paths` MCP tool
accepting glob patterns or paths (`app.py:11134-11176`). `ttl_seconds: int = 3600` (1h default,
`app.py:11141`). Stale-reclamation in `_expire_stale_file_reservations` (`app.py:4181-4272`): both
TTL-expired rows and rows whose owner is orphaned/inactive are released. Exclusive *and* shared:
`exclusive: bool = True` (`models.py:133`); two non-exclusive reservations don't conflict
(`app.py:4280-4281`).

**Overlap is PARTIAL.** `_patterns_overlap` (`app.py:4326-4338`) uses gitignore-semantics PathSpec.
For globs it cross-matches both directions (symmetric); for two **literal** paths with PathSpec it
matches the candidate against the *existing* pattern only (one-directional, `app.py:4298-4301`), so a
candidate that is a *parent* of an existing literal child is not reliably caught. Parent-directory
containment works (a `src/` reservation matches `src/app.py`). Net: overlap exists and is more
sophisticated than naive equality, but the note's "path-prefix-overlap … already standard" overstates
it slightly — it is not fully symmetric for literal paths. (Concord's own overlap check is symmetric
segment-prefix on both sides — `slug::overlaps` — so on *this narrow point* Concord is actually
cleaner, though Agent Mail's gitignore semantics handle globs we don't.)

## 3. ★ Enforcement — ADVISORY, no fencing  (the decisive, vision-critical finding)

This is the axis the competitive note itself named as Concord's dividing line (enforced vs.
cooperative). At the code:

- **The lease grant never blocks.** `# Advisory model: still grant the file_reservation but surface
  conflicts` → it appends conflicts and returns the lease anyway (`app.py:11349-11351`). The tool
  docstring: "Request **advisory** file reservations" (`app.py:11148`); responses carry
  `enforcement_off_for_code_paths: … server-side exclusivity is advisory only` (`app.py:11433-11442`).
- **Server-side enforcement covers only the mail archive**, not code paths: a conflict makes
  `send_message` fail only for `messages/agents/attachments/` surfaces (`app.py:5473-5518`); code-repo
  paths get no server block.
- **The only hard block on code files is the git pre-commit/pre-push guard** (`guard.py:336-343`,
  `573-580`), and it is **gated off by default** (`if not GATE_ENABLED: sys.exit(0)` unless
  `WORKTREES_ENABLED`/`GIT_IDENTITY_ENABLED`, `guard.py:170-177`) and **bypassable** four ways:
  `AGENT_MAIL_BYPASS=1` (`guard.py:183-186`), `AGENT_MAIL_GUARD_MODE=warn` (`guard.py:179-181`),
  `git commit --no-verify`, or deleting the repo-local hook.
- **Fencing token — REFUTED.** No monotonic fencing/generation token exists. A stale holder who wakes
  after their lease was reclaimed is **not** rejected on write — the guard only re-reads current
  reservation JSON and checks `released_ts`/`expires_ts` (`guard.py:296-304`). The only "token" is
  `registration_token`, an auth credential (`models.py:72`), not write-fencing.
- **Ownership-enforcement — CONFIRMED.** `release_file_reservations` filters
  `FileReservation.agent_id == agent.id` (`app.py:11541,11562`); no agent can release another's lease.

**Contrast with Concord M1/M2 (verified against our own merged code):**

| Enforcement property | Agent Mail | Concord M1+M2 (merged) |
|---|---|---|
| Lease conflict blocks the actor | No (advisory) | No (advisory claim) — **parity** |
| Foreign release/unlock blocked | **Yes** (`agent_id` filter) | **Yes** (`ReleaseOutcome::NotYours`, `MergeUnlock` owner check) — **parity** |
| Fencing token (stale-after-reclaim rejected) | **No** | **Yes** — field in M1, enforced in M2 (Floor self-check + Strong daemon-mediated) |
| Singleton merge-lock with hard serialization | **No** (build-slots advisory, no cap) | **Yes** — M2.3 daemon single-thread check-and-apply (airtight) + Floor fallback |
| Authoritative store | SQLite (+ git mirror the guard reads) | Filesystem (ADR policy 4) |

So on the **enforced-coordination axis the note treats as Concord's differentiator, Concord M2 is
genuinely ahead**: it has the fencing token and an airtight daemon-mediated merge-lock that Agent Mail
has no equivalent of. The note under-weighted this — it judged Concord's *locking* "behind", which is
true for **breadth/maturity**, but on **enforcement** specifically Concord's M1+M2 is stronger.

**Honest caveat (for the decision):** these cooperative coding agents already follow the CLAUDE.md
protocol, so advisory may be "enough" in practice for most cases. Fencing earns its keep specifically
in the split-brain-after-pause case — a dormant session waking after its lease was reclaimed — which
*does* occur here (sessions go dark between turns). Whether that narrow, real advantage justifies a
bespoke tool over a richer advisory one is the operator's judgment.

## 4. Build-slots — advisory signal, not a merge-lock (PARTIAL)

`acquire/renew/release_build_slot` (`app.py:12018-12163`), gated behind `worktrees_enabled`. Slots are
filesystem JSON leases (`build_slots/<slot>/<agent__branch>.json`). `acquire_build_slot` computes
conflicts but **writes the lease regardless** and returns them (`app.py:12057-12079`); docstring:
"Acquire a build slot **(advisory)**". There is **no numeric concurrency cap** ("max N builders") and
**no merge sequencing primitive** anywhere (grep negative). So "build-slots ≈ merge-lock" is only
loosely true: it's a per-named-slot advisory mutual-exclusion *signal*, weaker than Concord's
singleton, enforced, daemon-serialized merge-lock.

## 5. Messaging / registry — CONFIRMED richer (the note is right here)

Messaging is full mail-grade and **genuinely exceeds our JSONL inbox on every axis but two**: threaded
(`thread_id` + `reply_to` parent edges, `models.py:100,104`), read+ack timestamps + `ack_required`
(`models.py:85-86,109`; `acknowledge_message` `app.py:9649`), to/cc/bcc (`models.py:84`), 4 importance
levels (`models.py:108`), FTS5 BM25 full-text search + LIKE fallback (`db.py:836`, `app.py:10475`;
**no** semantic/embeddings), dual-persisted to SQLite **and** a committed git archive
(`storage.py:39,88`). Registry: project-scoped with memorable persistent identities
(`utils.py:182`, `models.py:161`) and TTL auto-retire at 24h (`app.py:4120-4174`, `config.py:286`) —
but **no dedicated heartbeat tool**; liveness is `last_active_ts` activity-derived. **~48 MCP tools**
(not 34). The only structure *we* have that Agent Mail lacks: conservative message-**kind**
classification and **AP-ref** extraction (its `kind` is recipient-class to/cc/bcc, not a semantic
kind). On breadth/maturity of messaging, Agent Mail clearly wins.

## 6. Coordinator fit — human-overseer-centric, NO autonomous coordinator (CONFIRMED Concord's gap)

Integration is an **HTTP (Streamable-HTTP) FastMCP** server with bearer auth (stdio also exists),
wired per-runtime via `*.mcp.json` for Claude Code/Codex/Gemini/Cursor/Windsurf/Cline — multi-runtime
confirmed. The **only authority role is a human**: explicit `# Human Overseer Routes` (`http.py:3561`)
inject a priority-superseding directive as a synthetic `HumanOverseer` agent ("The human's guidance
supersedes all other priorities", `http.py:3623-3638`). There is **no autonomous-coordinator/leader/
orchestrator role in code** — capability enforcement is optional/default-allow (`app.py:371-386`),
"project admin" = any agent with the shared token, `force_release` works only on *stale* leases (no
override of an active peer, `app.py:11695-11704`), broadcast-to-all is rejected (`app.py:2710`), and
there is no work-assignment / sequencing primitive. **This is precisely Concord's distinct asset**:
the autonomous, vision-driven coordinator agent (`hub`) that arbitrates ownership and sequences merges
on the critical path — Agent Mail has a *human* at that seat, not an agent.

## 7. Adoption feasibility — CONFIRMED feasible (mechanics via MCP, governance stays external)

A project keeping its **own** enforced-governance coordinator can adopt Agent Mail purely for the
plumbing: every mechanic is MCP-exposed with no hard human dependency — `ensure_project`,
`register_agent`, `file_reservation_paths`/`renew`/`release`/`force_release` (stale-only),
`acquire/renew/release_build_slot`, `send_message` (with `ack_required` as the directive mechanism),
`fetch_inbox`, `search_messages`, `summarize_thread`, contacts, cross-repo "product" bus, plus
read-only `resource://` views. Caveats: (1) **no authority/enforcement model** — a coordinator's
leases/messages carry no more weight than a peer's; enforced governance must stay external (for ais,
this matches Concord's stated split: enforced coordination vs. an advisory channel — Agent Mail would
slot in as the *advisory/registry/lease/messaging substrate*, not the enforcement). (2) Auth is a
single shared static bearer (or coarse JWT RBAC) — effectively single-team per project. (3) The
human-only override powers (priority directive, delete, retire) live on **HTTP routes, not MCP tools**
— a coordinator agent would call those endpoints directly. (4) Guard enforces only exclusive-lease
conflicts, default-off, and only your *leases*, never your governance rules. (5) git-archive-backed
(assumes a repo per project); no broadcast.

## 8. Secondary "learn-from" candidates (verified)

- **wit** (`amaar-mc/wit`) — TypeScript/Bun (not Rust). **Genuine tree-sitter AST symbol-level
  locks** (`symbols.ts:22-41`, locks keyed `file.ts:symbolName`) **plus a call-graph** (`calls.ts`)
  driving DEP_CHAIN conflict warnings — "you're about to edit a symbol whose callee is locked
  elsewhere" (`handlers.ts:261-486`). Advisory locks (TTL 30m) + a commit-time *contract* hook that
  blocks on signature drift (best-effort, 2s-timeout, bypassable). TS/Python only. **The transferable
  idea for Concord: symbol-range locks + call-graph dependency-conflict warnings — strictly more
  expressive than file-path leases for "two agents, same file, different functions."**
- **claude-presence** (`garniergeorges/claude-presence`) — TypeScript/Node, MCP stdio,
  better-sqlite3, **9 tools**, registry + advisory opaque-resource locks + broadcast inbox.
  **No path-prefix overlap** (exact-match resource strings, `repository.ts:220`). Simpler cousin;
  little Concord doesn't already have.

## 9. Recommendation (factual — operator's call)

The verification **confirms the competitive note's core conclusion and sharpens it**:

1. **Building M3.2 (board) and M3.3 (MCP server) as designed would be reinventing a richer wheel.**
   Agent Mail already exposes ~48 mature MCP tools, full-text-searchable threaded/ack messaging, a
   registry, leases and build-slots, multi-runtime, with a human dashboard. A bespoke WP8 board / WP9
   MCP would be a thinner copy. M3.1 (typed JSONL inbox) is likewise thinner than Agent Mail's mail.

2. **But Concord's merged M1+M2 hold a real, distinct asset the note under-weighted:** *enforced*
   coordination — the **fencing token** (no Agent Mail equivalent) and the **airtight daemon-mediated
   merge-lock** — plus the **autonomous coordinator agent (`hub`)** at the seat where Agent Mail puts
   a human. On the enforcement axis the note itself called decisive, Concord is *ahead*, not behind.

3. **Therefore the evidence points to REFOCUS, not stay-the-course on M3 expansion:**
   - **Keep & finish nothing more on M3 mechanics-breadth** (pause/retire WP8/WP9 build-out; M3.1 is a
     clean checkpoint, not worth extending).
   - **Keep M1+M2** — the enforced core is genuinely differentiated and already merged.
   - **Adopt Agent Mail (or its MCP mechanics) for the breadth** (registry/leases/messaging) *if* the
     team wants rich coordination plumbing, with Concord's `hub` + enforced governance layered on top
     as the external authority Agent Mail deliberately lacks. Feasible per §7.
   - **Optionally graft wit's idea** (symbol/AST locks + call-graph conflict warnings) if file-grain
     leases prove too coarse — that's a capability *neither* Agent Mail nor current Concord has.
   - Caveat the adoption: Agent Mail is **Python/advisory**, not a Rust core, with single-team bearer
     auth — so "adopt" means taking a Python FastMCP dependency, not acquiring the Rust endstate.

   The one honest counter-argument for *stay*: if enforced (fencing) coordination is considered
   essential and cooperative-advisory insufficient for dormant-session split-brain, Concord's enforced
   core is unique enough to justify continuing — but that argues for **deepening M1+M2 enforcement +
   the hub-coordinator**, *not* for building the M3 breadth that Agent Mail already does better.

**Bottom line:** the differentiator is enforcement + the autonomous coordinator, not the mechanics.
Spend Concord's energy there; do not rebuild messaging/board/MCP that a mature tool already does richer.

## Code locations (for re-verification)

`mcp_agent_mail/src/mcp_agent_mail/`: `app.py` (MCP tools/resources, leases, build-slots, auth,
force-release, overseer), `storage.py` (git mirror + commit queue), `guard.py` (pre-commit/pre-push
lease guard + gating/bypass), `http.py` (dashboard + Human Overseer routes + MCP HTTP mount + auth),
`db.py` (SQLite engine + FTS5), `models.py` (tables), `config.py` (gating/auth), `cli.py`
(serve-http/serve-stdio); client configs `*.mcp.json`. `wit/src/{parser,daemon,cli}`;
`claude-presence/src/{tools,db}`.
