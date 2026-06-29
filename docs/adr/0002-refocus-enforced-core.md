# ADR-0002: Refocus on the enforced core + autonomous coordinator (M3-lean)

- **Status:** Proposed (draft by session `concord-w`; awaiting `hub` review + operator sign-off)
- **Date:** 2026-06
- **Refs:** supersedes the *scope* of [ADR-0001](0001-rust-rewrite.md) (the "full M1+M2+M3" cut) ·
  evidence in [`AGENT-MAIL-VERIFICATION.md`](../AGENT-MAIL-VERIFICATION.md) ·
  competitive landscape (operator-commissioned, archived locally) · ROADMAP §11

## Context

ADR-0001 adopted a staged Rust port with a release cut of **M1 + M2 + M3-full**, where M3 was a
broad "typed agent layer" folding WP7 (rich typed messaging), WP8 (structured board), and WP9 (MCP
server). M1 (typed core + CLI) and M2 (push daemon + fencing) are built and merged. M3.1 (a typed
JSONL inbox) is built and committed (`ecb718c`).

A competitive research pass (operator-commissioned) and then a **source-level verification** (this
session cloned and read the code, citing `file:line`) changed the picture:

- **MCP Agent Mail** (`Dicklesworthstone/mcp_agent_mail`) is a mature, ~2k★ tool covering the same
  *mechanics* — registry, file leases (glob/TTL/stale-reclaim), build-slots, threaded/ack/searchable
  messaging, a git pre-commit guard, multi-runtime — exposed as **~48 MCP tools**. On
  **breadth and maturity of the mechanics**, it clearly exceeds Concord's M3 plans. Building WP8's
  board and WP7's rich messaging would be **reinventing a richer wheel**.
- **But** the verification refuted two framings and surfaced the real dividing line:
  - Agent Mail is **Python (FastMCP + SQLAlchemy + aiosqlite)**, *not* the "Rust endstate" the
    landscape note claimed; and its leases are **advisory** with **no fencing token** — the only hard
    block is an optional, default-off, four-ways-bypassable git hook (`AGENT-MAIL-VERIFICATION.md` §3).
  - It is **human-overseer-centric**: there is **no autonomous-coordinator role in code** — agents are
    strict peers; the only authority is a human dashboard (`…VERIFICATION.md` §6).
  - On the **enforcement axis the landscape note itself named as Concord's dividing line, Concord's
    merged M1+M2 is *ahead*** — it has the fencing token (design-for-M1, enforced-M2) and an airtight
    daemon-mediated merge-lock that Agent Mail has no equivalent of (`…VERIFICATION.md` §3 table).

So the mechanics are a 2026 commodity; the **enforced vertical and the autonomous coordinator are
the scarce, vision-aligned assets** — and Concord already holds them in M1+M2.

**Why enforcement is the *forward* bet, not just today's edge.** Advisory coordination is adequate
while a few cooperative agents share a repo; it degrades exactly as agent autonomy and count grow —
and as sessions pause, go dormant, or get reclaimed (the split-brain we hit this session with dormant
sessions is not hypothetical). Enforcement (fencing, airtight serialization) is the structurally
harder, structurally *necessary* property as multi-agent work scales. Refocusing here is not
defending a sunk asset; it is betting on where coordination must go.

## Decision

**Refocus.** Keep M1 + M2 as the enforced core; **re-scope M3 from "full" to "M3-lean"** — retire the
commodity breadth (WP8 board, WP7 rich messaging) and spend the remaining M3 effort only on what is
distinctively Concord: **deepening the enforced core and letting the autonomous coordinator (`hub`)
drive it through typed tools.**

Concretely (the M3-lean shape; the design is arbitrated separately in `DESIGN: M3-lean`):

- **RETIRE:** WP8 structured-board breadth; WP7 rich-messaging expansion (threads/ack/search) — Agent
  Mail does these better; rebuilding them is reinventing.
- **KEEP minimal WP9:** expose only the **enforced primitives** (claim/release/verify/merge-lock/
  merge-unlock/status/fence) as typed `rmcp` MCP tools, so `hub` drives the core typed instead of via
  bash strings. This is *not* "reimplement Agent Mail's 48 tools" — it is exposing Concord's *unique
  enforced* operations.
- **DEEPEN enforcement:** extend the airtight daemon-mediated path (M2.3, merge-lock only) to the
  other consequential ops (claim/release), closing the documented Floor TOCTOU residual fully.
- **KEEP M3.1** (the typed JSONL inbox) **as the thin push substrate** (M2-adjacent, the §8 token
  lever), *not* as a messaging product — no threading/ack/search.

This **supersedes the scope of ADR-0001** (the full-M3 cut), not its architecture or its M1/M2
decisions, which stand. The staged port, parity policy, OverlapPolicy default, and fencing
field/enforcement decisions of ADR-0001 remain in force.

**Rejected alternatives:** (a) *Stay the course on full M3* — rebuilds commodity mechanics a mature
tool does better. (b) *Replace Concord with Agent Mail wholesale* — loses the enforced core + the
autonomous coordinator, and would take a Python/advisory dependency in place of the enforced vertical.
(c) *Adopt Agent Mail for mechanics now* — feasible (`…VERIFICATION.md` §7) and a reasonable future
option, but orthogonal to this decision and not required to refocus; left as a backlog option.

## Policies

1. **Vision over mechanics-breadth.** Where a generic tool already does a coordination *mechanic*
   better, Concord does not rebuild it; Concord's effort goes to the **enforced vertical** (fencing,
   ownership-enforcement, airtight serialization) and the **autonomous coordinator**, which the market
   does not provide. (This is `ais`'s "enforced vertical > convenience" applied at tool level.)
2. **Enforced, not advisory, is the differentiator.** Every M3-lean addition must strengthen
   enforcement or the coordinator's typed control of it — never add advisory breadth for its own sake.
3. **Commodity mechanics are an adoption option, not a build target.** Rich messaging, boards, and
   broad MCP surfaces may be *adopted* from a mature tool (Agent Mail) if ever wanted; they are not
   reimplemented here.

## Architecture

Unchanged from ADR-0001's "one typed core → many surfaces", but the M3 surface shrinks: the typed
`concord-core` (M1) + push daemon with fencing (M2) gain a **lean MCP surface** (M3-lean WP9) that
exposes only the enforced primitives, optionally routing consequential ops through the daemon's
single-thread serialization point for the airtight guarantee. No board subsystem, no rich-message
store; the prose channel stays the human log and the typed JSONL inbox stays the thin push substrate.

## Consequences

- **Positive:** effort concentrates on the scarce, defensible asset; no thin copy of a richer tool;
  the enforced core becomes typed-tool-drivable by the coordinator; smaller surface to maintain.
- **Negative / cost:** Concord will be *narrower* than Agent Mail on mechanics (intentionally); teams
  wanting rich messaging/boards must adopt a separate tool. Some M3.1 generality (kind-classification
  beyond the coordinator's needs) may go unused.
- **Backlog (not now):** **wit's AST symbol-level locks + call-graph conflict warnings** — a capability
  *neither* Agent Mail nor Concord has, the most interesting future differentiator (`…VERIFICATION.md`
  §8). Adoption of Agent Mail's mechanics via MCP as the advisory substrate under Concord's enforced
  coordinator (`…VERIFICATION.md` §7).

## Sources

- `docs/AGENT-MAIL-VERIFICATION.md` — source-level verification (advisory/no-fencing/Python/no-autonomous-coordinator), this repo.
- Competitive landscape survey — operator-commissioned, kept in the local archive (not published).
- MCP Agent Mail: https://github.com/Dicklesworthstone/mcp_agent_mail
- wit (AST symbol locks): https://github.com/amaar-mc/wit
- M. Kleppmann, fencing tokens: https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html
