# ADR-0003: Wave 2 — Enforcement-Härtung

- **Status:** Proposed (draft by `concord-w`, reviewed + vision-framed by `hub`; awaiting operator sign-off, then implementation F1-first)
- **Date:** 2026-06
- **Refs:** grounded in [`FEATURE-RESEARCH.md`](../FEATURE-RESEARCH.md) (feature-mining, 2026-06-29) ·
  landscape in [`COMPETITIVE-LANDSCAPE.md`](../COMPETITIVE-LANDSCAPE.md) ·
  continues the enforced-core thesis of [ADR-0002](0002-refocus-enforced-core.md) ·
  CLAUDE.md (Concord protocol + vision guard-rail) · ROADMAP

## Context

Concord is released (**v0.5.0**, self-hosting, shippable). The enforced core is in place:
multi-session registry + heartbeat/TTL-stale-reclaim; **enforced area-leases on path *and*
symbol/AST level** (tree-sitter); singleton merge-lock; **fencing tokens** (Floor = FS self-check,
Strong = daemon-mediated); the push-daemon (notify, per-session inbox-demux); a typed MCP surface;
the launcher; multi-project; cross-platform distribution; and the **autonomous coordinator `hub`**.

An operator-commissioned feature-mining pass ([`FEATURE-RESEARCH.md`](../FEATURE-RESEARCH.md))
surveyed agentic-coding peers (Agent Mail, Gastown, wit, agent-kanban, Power Loom, …) and the
Claude Code harness itself. Three findings drive this wave:

1. **Concord's differentiator is *enforcement*, not mechanics.** Almost every peer implements leases
   as **advisory**; Concord is the only one with fencing + AST-leases. The highest-value features are
   therefore the ones that make Concord's *enforcement* **harder** — not more convenient Kanban breadth.
2. **The harness (Claude Code hooks) now offers real enforcement primitives** Concord does not yet
   fully use — `PreToolUse`+`mcp_tool`-deny (lease-block at the keystroke), `Stop`/`PreCompact`
   (against "dark" sessions), `SessionEnd`/`WorktreeRemove` (clean release). This is Concord's
   **cheapest, most vision-true lever**: the infrastructure already exists; only the harness wiring is
   missing. (Plan-mode is verified *prompt-only, not enforced* — which validates `PreToolUse`-deny +
   fencing as the only real guarantee.)
3. **Anthropic is building in Concord's space** ("Agent Teams") but **without enforced leases, without
   a merge-lock, lead-fixed, one-team-per-session** — confirming the moat (enforced vertical + a
   cross-worktree autonomous coordinator) while offering patterns to mirror.

The vision guard-rail (CLAUDE.md / VISION): Concord's value is the **enforced vertical** (capability,
classification, provenance, accountability) plus an **intelligent autonomous coordinator**. Maxim:
**enforced coordination > convenience**, **no reinventing** where mature prior-art fits. Wave 2
selects strictly along that axis.

> **Why this wave, why now — the dogfood is the requirements source.** Wave 2 is not a wish-list; it
> hardens the exact failure modes Concord's *own* development surfaced. Building Concord through Concord
> (the M5 dogfood) repeatedly hit: sessions going dark, the coordinator's heartbeat lapsing,
> build-env/QEMU contention with no resource primitive, and ACK/escalation rules honored only by
> convention. F1/F2/F3 fix precisely those — the dogfood turned the protocol's soft spots into a
> verified requirements list. Wave 2 completes *both* halves of the moat: the **enforced vertical** (F1
> makes leases hard at the keystroke, F2/F5 extend enforcement, F3 mechanizes policy) and the
> **intelligent autonomous coordinator** (F4 lets `hub` *measure* fleet health instead of reading
> prose). Enforced where the market is advisory; measured where it self-reports.

## Decision

Adopt **Wave 2 — Enforcement-Härtung**: five features, sequenced, each a 🟢 vision-strengthening pick
from the research's ADOPT tier. F1 first (top lever; cures "going dark"). Each ships as its own
PLAN→build→verify slice under `hub`; one PR per feature (or per coherent sub-slice), no speculative
batching.

### F1 — Harness-native enforcement wiring *(research A1–A6; ADOPT ranks 1 + 6)*

- **Scope:** wire Concord's *existing* lease-store + MCP server into the Claude Code hook surface:
  - **A1 `PreToolUse`+`mcp_tool`-deny** on `Edit|Write|MultiEdit` → call Concord's MCP tool;
    `permissionDecision:"deny"` blocks any edit to a *non-leased* file/symbol **at the keystroke**,
    before the tool runs. (Must live on `PreToolUse`, not `SessionStart` — MCP connects *after* session
    start.)
  - **A2 `SessionEnd` → auto-release** leases/merge-lock + deregister on clean exit (idempotent;
    complements TTL-reclaim by shrinking the window a finished session holds leases).
  - **A3 `Stop`-hook (block-to-continue)** → on turn-end, if the session holds a lease with open work or
    an un-ACK'd `hub` directive, inject `additionalContext` and refuse the stop. Harness-native cure for
    "going dark" (needs a clean termination predicate to avoid endless turns).
  - **A4 `PreCompact` + `SessionStart(source=compact)`** → dump lease/merge-lock/directive state before
    compaction, re-inject as `additionalContext` after reset. Protects protocol memory across compaction.
  - **A5 `FileChanged`+`watchPaths`** → replace the brittle hand-rolled `stat -f %m … sleep 30` monitor
    (named in CLAUDE.md as the #1 cause of dark sessions) with a harness-native wake on
    `SESSION-SYNC`/registry.
  - **A6 `PostToolUse`(Edit|Write) → out-of-scope-write detection** *(final sub-slice)* → post-hoc detect
    a write *outside* the session's leases → log/flag as a policy violation. An audit tooth **behind** the
    A1 deny (defense-in-depth: catches a violation even if the deny path is bypassed).
- **Sub-sequence:** A5 + A2 first (low-risk quick wins), then A1 + A3 (the harder enforcement), A6 last.
- **Value:** 🟢 highest — turns leases from *advisory* to **hard** and cures "going dark" at the harness
  boundary. Lifts Concord's core invariant. **Effort:** S–M per hook (A2/A5 small; A1/A3 medium; A4 S–M).
- **Vision rationale:** directly hardens the enforced vertical (Cap-checks at the keystroke) and the
  autonomous-coordinator reliability (no silent dormancy). Cheapest because the store + MCP already exist.

### F2 — Named resource-locks / build-slots *(research E4; ADOPT rank 2)*

- **Scope:** extend the lease engine with a `kind=resource` namespace — advisory locks on *non-file*
  resources (CI, deploys, **ports**) with shared/exclusive **N-slot semaphore** semantics; reuses the
  existing fencing/TTL/stale-reclaim machinery.
- **Value:** 🟢 (concrete for `ais`) — solves the *documented* `ais` contention (QEMU ports, build-env,
  mutual QEMU-killing) cleanly instead of forcing it into path-leases or convention. **Effort:** S–M.
- **Vision rationale:** generalizes the enforced lease primitive to the real resource-contention class
  the fleet hits, without a new subsystem.

### F3 — Ack-tracking + tracked escalation *(research E3 + E2; ADOPT rank 3)*

- **Scope:** mechanize two CLAUDE.md policies that are today toothless prose:
  - **E3 enforced message-ack** — per-recipient `ack_ts`/`read_ts`; the push-daemon auto-re-delivers /
    escalates un-ACK'd `hub` directives on a TTL (CLAUDE.md already *mandates* "no ACK within a tick →
    hub re-delivers/escalates").
  - **E2 tracked escalation primitive** — a blocker escalates with severity, routed up the chain, and
    **creates a tracked object that persists until resolved** → blockers cannot silently vanish; gives
    `hub` a real forwarding queue to the operator.
- **Value:** 🟢 — gives the prose protocols teeth via the existing inbox-demux. **Effort:** M.
- **Vision rationale:** makes the coordinator's authority and the "no silent idling" rule *enforced*
  state rather than self-reported discipline.

### F4 — hub telemetry on native OTel (+ ccusage) *(research B1 + B2; ADOPT rank 4)*

- **Scope:** consume Claude Code's native OpenTelemetry stream (`CLAUDE_CODE_ENABLE_TELEMETRY=1`):
  token-burn, cost, tool-spans, permission-reject events, subagent-spans, `session.id` on every span.
  Map `session.id`→Concord-id at launch; `hub` computes per session: **burn-rate, idle (no spans for
  N min), looping (repetitive spans / no commit progress), reject-storms**. Add **ccusage** (local
  JSONL→token/cost, no upload) for the cost view.
- **Final sub-slice — B3 dark-session watchdog:** a multi-stage health watcher consumes this telemetry
  (+ the existing heartbeat data) and **actively alerts `hub`** on a miss / lease-but-silent, instead of
  only passively reclaiming on TTL — active alerting directly against the #1 failure mode.
- **Value:** 🟢/🟡 — makes `hub` *telemetry-driven*, turning "no silent idling" from self-report into a
  **measured** signal. The emitting side is built-in/free. **Effort:** M (S for ccusage).
- **Vision rationale:** the intelligent-coordinator half of the moat. Build the heuristic **natively on
  the OTel stream** — *no* SaaS observability dependency (off-vision, infra-heavy); Langfuse stays an
  OTLP fallback store only if ever needed.

### F5 — Enforced signature contracts *(research D1; ADOPT rank 5)*

- **Scope:** two agents agree on a function signature / wire-format; the existing tree-sitter snapshots
  it, and a commit/merge gate **blocks** a commit that changes the agreed contract without renegotiation.
- **Value:** 🟢 — gives the *only* Peer-collaboration CLAUDE.md permits ("negotiate interfaces") teeth;
  reuses tree-sitter. **Effort:** M.
- **Vision rationale:** turns the one sanctioned peer interaction from prose into an enforced contract,
  pairing with the merge-lock.

> **Folded in (resolved in `hub` review):** research **A6 (out-of-scope-write detection,
> `PostToolUse`)** + **B3 (dark-session watchdog with active alerting)** — ADOPT rank 6 — are the audit
> teeth *behind* the leases (catch a write outside lease even if A1 is bypassed) and active alerting vs.
> passive reclaim. **A6 ships as F1's final sub-slice** (an audit tooth behind the deny = defense-in-depth);
> **B3 ships as F4's final sub-slice** (it consumes the telemetry). No separate Wave-2.5.

### Adopt order + rationale

**F1 → F2 → F3 → F4 → F5**, ordered by *value × readiness*:

1. **F1 first** — cheapest, hardens the core invariant, and cures the #1 failure mode ("going dark")
   that degrades *every* other feature's reliability. Unblocks confident autonomy for the rest of the wave.
2. **F2** — independent, small, solves a concrete already-felt `ais` pain; no dependency on F1.
3. **F3** — builds on the existing push-daemon inbox-demux; mechanizes coordination policy.
4. **F4** — emitting side is free/built-in; gives `hub` measured health to *drive* the fleet (and feeds
   a possible B3 watchdog).
5. **F5** — smallest blast-radius, reuses tree-sitter; valuable but least urgent.

### Backlog (explicitly NOT this wave)

Valuable but larger / dependent / later — sequenced after Wave 2, each its own future ADR/PLAN:

- **D2 Pre-Merge-Enforcement-Gate** (M) — promote the merge-lock from *serialization* to a *quality gate*
  (no test-removal/skip, no coverage manipulation, no permission-escalation). Maps 1:1 to CLAUDE.md
  "load-bearing invariants are off-limits for shortcuts".
- **C1 cryptographic agent identity + signed commits/PRs** (M–L) — strongest provenance play; makes
  "who merged/edited" non-repudiable; fencing token can become *signed*.
- **E1 Task-DAG + auto-unblock** (L) **+ E5 context-brief** (S–M companion) — the largest structural
  addition; gives `hub` a machine-readable critical path (today only in prose), leases *derived* from
  claimed tasks.
- **D3 speculative cross-branch conflict-probe** (M) — background dry-merge of in-flight branch pairs;
  natural extension of the existing advisory call-graph warning into the cross-branch dimension.

**Adopt the pattern, not the product:** workgraph/saltbo dependency-frontier work-stealing for `hub`
dispatch; Task Master's dependency schema; Backlog.md git-native cards; Power Loom's transactional
envelope (C3) as a far goal. **Reject / note only:** Kanban/worktree-isolation (sidesteps the problem
Concord solves), merge-queue products (merge-lock suffices), cross-device/federation (single-host),
heavyweight observability as a dependency (build the insight natively on F4). SAM/TIA (D4/D5) and
FTS/vector-memory (F2/F3 in the research's §F) only as a spike if a concrete problem pulls them.

**Rejected alternatives:** (a) *Adopt a broad peer tool (Agent Mail) for breadth* — advisory, Python,
no autonomous coordinator; orthogonal to hardening enforcement (already weighed in ADR-0002). (b) *Skip
the harness hooks, keep the hand-rolled monitor* — leaves the #1 dark-session failure mode and keeps
leases effectively advisory at the edit boundary. (c) *Take a SaaS observability platform* — off-vision
dependency; the native OTel stream gives the same signal local-first.

## Policies

1. **Enforced, not advisory, is the differentiator.** Every Wave-2 addition must strengthen enforcement
   or the coordinator's measured control of it — never advisory breadth for its own sake.
2. **Harness-native over hand-rolled.** Prefer official hook primitives to brittle shell loops where one
   exists (the monitor → `FileChanged` swap is the archetype).
3. **No SaaS dependency for observability/provenance.** Build the heuristic natively on the built-in OTel
   stream and local JSONL; external platforms are at most an optional OTLP fallback store.
4. **Patterns, not products.** Where prior-art is a richer wheel, adopt the *pattern* into Concord's
   governed, local-first model; do not take the dependency.
5. **One slice, one gate.** Each feature is PLAN→build→verify under `hub`, one PR per coherent slice; no
   speculative batching, no VERSION churn until a release-worthy cut.

## Architecture

Unchanged from ADR-0001/0002's "one typed core → many surfaces". Wave 2 adds *surfaces and consumers*,
not a new core:

- **F1** → `hooks/` scripts (new `PreToolUse`/`Stop`/`PreCompact`/`SessionEnd` handlers) + the existing
  MCP server (a `check-lease` decision tool for the deny path); embedded + installed via the existing
  `concord install-hooks`. No new crate.
- **F2** → `concord-core` lease engine gains a `kind=resource` namespace + slot-count; reuses
  fencing/TTL/reclaim. New CLI/MCP verbs (`claim --kind resource --slots N`).
- **F3** → `concordd` inbox-demux gains per-recipient ack-state + TTL re-deliver; a typed escalation
  record (open/closed) surfaced in `hub` status.
- **F4** → a new telemetry consumer (OTel receiver or JSONL reader) feeding `hub` via an MCP tool;
  `session.id`→Concord-id map established at launch by the launcher.
- **F5** → tree-sitter signature snapshot stored under the coordination dir; a commit/merge gate
  (pre-commit hook + merge-lock precondition) verifies the staged diff against it.

## Consequences

- **Positive:** leases become hard at the edit boundary; "going dark" is cured harness-natively; `hub`
  gains measured fleet health; the one sanctioned peer interaction (interfaces) becomes enforced; the
  concrete `ais` resource contention is solved — all by extending existing machinery, no new core.
- **Negative / cost:** more hook scripts to maintain and a coupling to the Claude Code hook API
  (mitigate: keep hooks fail-open and version-noted); OTel requires an opt-in env var per session; the
  `Stop`/watchdog predicates need careful design to avoid endless turns / false dark-alerts; F5's
  contract store adds a small state surface.
- **Risks + mitigations:** (a) hook-API drift → fail-open hooks + a CI smoke that asserts the deny path;
  (b) `Stop`-hook endless-turn → conservative termination predicate (only block on *open lease + un-ACK'd
  directive*, with a hard turn cap); (c) telemetry false idle/loop signals → tune N-min windows on real
  fleet data before acting on them.

## Open questions (resolved in `hub` review)

1. **F1 sub-sequencing — RESOLVED: yes.** Ship A5 (`FileChanged` wake) + A2 (`SessionEnd` release) first
   as the quick, low-risk wins, then A1 (deny) + A3 (`Stop`) as the harder enforcement pieces.
2. **A6 + B3 placement — RESOLVED: fold.** A6 (out-of-scope-write / `PostToolUse`) becomes F1's final
   sub-slice (an audit tooth *behind* the deny = defense-in-depth); B3 (dark-session watchdog) becomes
   F4's final sub-slice (it consumes the telemetry). No separate Wave-2.5.
3. **F4 storage — RESOLVED: native-only now.** Per Policy 3 (no SaaS dependency); the Langfuse OTLP
   fallback store is deferred unless a concrete need pulls it.
4. **F5 contract store format — RESOLVED.** Versioned snapshot under `<coord>/contracts/` keyed by
   `<file>:<symbol>`, reusing the S2 AST extraction + symbol-lease key scheme (consistent with the
   existing symbol-lease keys).

(Operator sign-off on the wave as a whole remains; `hub` presents the final ADR to the operator.)

## Sources

- [`FEATURE-RESEARCH.md`](../FEATURE-RESEARCH.md) — feature-mining research (catalog A–F, prioritized
  recommendation), this repo.
- [`COMPETITIVE-LANDSCAPE.md`](../COMPETITIVE-LANDSCAPE.md) — landscape survey, this repo.
- Claude Code Hooks (all events): https://code.claude.com/docs/en/hooks
- Plan-mode is not enforced: https://blog.sondera.ai/p/claude-codes-plan-mode-isnt-read
- OTel / observability: https://code.claude.com/docs/en/agent-sdk/observability · ccusage: https://github.com/ryoppippi/ccusage
- Peers: Agent Mail https://github.com/Dicklesworthstone/mcp_agent_mail · Gastown https://github.com/gastownhall/gastown · wit https://github.com/amaar-mc/wit · Power Loom https://github.com/shashankcm95/claude-power-loom
