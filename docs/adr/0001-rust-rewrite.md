# ADR-0001: Port Concord from shell to a typed Rust core (CLI + daemon + MCP)

- **Status:** Proposed (draft by session `concord-w`; awaiting `hub` review + operator sign-off)
- **Date:** 2026-06
- **Refs:** [ROADMAP §11](../ROADMAP.md) · BACKLOG WP12 (adopted: M1+M2+M3), supersedes WP11;
  folds WP7 (§9), WP8 (§6), WP9 (§7) into M3 · design [`WP12-RUST-PORT.md`](../WP12-RUST-PORT.md) ·
  research [`WP12-RESEARCH.md`](../WP12-RESEARCH.md)

> This ADR establishes the ADR practice for the Concord repository (see
> [`0000-template.md`](0000-template.md)). It is written in English to match the
> repository's public documentation (README/MANUAL/ROADMAP/BACKLOG); the WP12 design
> and research notes it cites are in German.

## Context

Concord is a multi-session coordination system: it stops the several Claude sessions
working a repository in parallel from impairing each other (concurrent edits, colliding
merges, mutual process-killing) via a structured registry, area **leases**, and a
singleton **merge lock**. State lives in the filesystem (`sessions/<id>`,
`leases/<area>/`, `intents.jsonl`, a prose `*-SESSION-SYNC.md` channel) and sessions are
wired in through Claude Code hooks. Today it is ~320 lines of shell (`bin/coord.sh`,
`bin/concord`) plus those hooks.

The shell prototype was the **right way to prove the model** — it was born inside `ais`
and validated the idea quickly. A literal line-for-line Rust rewrite would add little.
The port is worth doing because it fixes three *structural* limits that shell cannot
cleanly address (ROADMAP §11):

1. **The coordinator is not itself race-free.** The irony of an anti-race tool: its own
   read-modify-write cycles on state files are non-atomic — only the lease `mkdir` is
   atomic. A concurrent reader can observe a half-written `sessions/<id>` or lease field.
2. **Polling, not push.** Sessions self-tick (~12 min) to discover new directives. Every
   tick that finds nothing still spends a turn of model context — the dominant avoidable
   token cost (ROADMAP §9 / WP7). A daemon can deliver events the moment they occur.
3. **Platform fragility.** BSD-vs-GNU divergence (`date -r`, `stat -f`) makes the shell
   non-portable; Rust's stdlib is portable, which **supersedes WP11** (native Linux +
   Windows without WSL2) and yields one installable artifact.

Bonus: Concord is philosophically the same as `ais` — enforced coordination, leases as
capabilities, accountability/provenance. Built properly in Rust it **dogfoods the `ais`
vision at tool level** (north star: Concord as an `ais`-native service, M6).

**Vision alignment (the deeper why).** Concord's primitives are a small-scale instance of
the `ais` enforced vertical: a **lease is a capability** (scoped authority over a region),
the **merge lock is an authority singleton**, the **ledger is the provenance/accountability
trail**, and the **fencing token is what keeps a capability un-forgeable after reclaim** — a
stalled holder cannot act on stale authority. So the port is more than portability: it is a
faithful, *enforceable* rendering of the same guarantees `ais` makes at Ring 0 — which is
what makes M6 (Concord-on-`ais`) a coherent north star, not a slogan. Where parity with the
shell and the vision conflict, the vision wins; M1 already chose vision-mode defaults
(RejectOverlap, correct escaping) rather than deferring them.

**Prior art consulted** (full notes + sources in `WP12-RESEARCH.md`):
- *Leases + TTL + stale-reclaim* are modelled by etcd (Grant/KeepAlive, attached keys
  auto-deleted when the keepalive stops), ZooKeeper (ephemeral znodes), and Consul
  (session-bound locks) — all the same shape as Concord's "no heartbeat > TTL ⇒ lease
  reclaimable" [1][2].
- *TTL alone is unsafe under pause.* Kleppmann's fencing-token argument: a lease holder
  can stall (GC/scheduling/swap) past expiry while another acquires; both briefly believe
  they hold the lock → split-brain. The fix is a **monotonic fencing token** carried on
  every protected write, with the resource rejecting any lower token [3]. etcd's
  per-write revision is exactly such a token [2].
- *fs-watch in Rust* → `notify` is the de-facto standard (rust-analyzer, watchexec,
  deno…); a single save emits 3–5 inotify events / batched FSEvents, so **debouncing is
  mandatory** (`notify-debouncer-full`), and watching the **enclosing directory** is more
  robust than a single file under atomic-rename editors [4][5].
- *Single-binary distribution* → `cargo-dist` (build+distribute, generates the CI release
  matrix + shell installer + SBOM), optionally with `cargo-release`; possibly oversized
  for a one-platform tool (open trade-off) [6][7].
- *MCP server in Rust* → `rmcp`, the official SDK (≥1.x, tokio, `#[tool]` macros generate
  typed schemas) — the direct path from "shell strings" to typed, schema-validated tool
  endpoints [8][9].

## Decision

**Port Concord to a typed Rust core, staged M1–M6, with the adopted release cut being
M1 + M2 + M3.** The richer roadmap items WP7/WP8/WP9 are folded into **M3** as one typed
agent layer (board + MCP tools + inbox). WP11 (cross-platform via shell) is **superseded**.

The core is a single typed state model (`concord-core`) exposed three ways: a **CLI**
(drop-in for `coord.sh`), an optional **daemon** (`concordd`, push notifications over the
unchanged FS state), and an **MCP server** (typed coordination tools). One model,
validated transitions, three surfaces.

**Stages** (each independently shippable; deliberate stop-points after M1 and after M2):

- **M1 — typed CLI, drop-in.** Workspace `concord` (bin) + `concord-core` (lib). All nine
  verbs (`register/heartbeat/status/claim/release/merge-lock/merge-unlock/log/sync`) plus
  hooks as subcommands. Same on-disk layout, but transitions atomic (temp-file +
  atomic-rename; lease acquisition stays the atomic `mkdir`) and validated by a typed
  state machine: no release/merge-unlock of a foreign lease, claim conflict refused
  structurally, path-prefix overlap rejected.
- **M2 — `concordd` daemon: push over poll.** Watches the FS state + prose channel
  (`notify` + `notify-debouncer-full`) and exposes a notify stream; self-tick degrades to
  a long reliability fallback. Honest scope: the daemon cannot *force* a session awake
  (that stays with the harness) but delivers the event immediately instead of up to a
  tick late. **This is where fencing enforcement lands.**
- **M3 — typed agent layer (WP7+8+9).** Structured board (merge queue, lease/dependency
  graph, deadlock detection), typed MCP tools (`rmcp`), and a compact directed inbox
  protocol; the prose channel stays the human audit/discussion log.
- **M4 — cross-platform + distribution** (supersedes WP11): native macOS/Linux/Windows,
  `cargo install` + prebuilt binaries.
- **M5 — multi-project + dogfood:** derive coord-dir/sync-path per project root; Concord
  coordinates its own development.
- **M6 — Concord-on-ais** (north star, post-1.0): leases as real capabilities, ledger via
  `dbd`, audit via `auditd`.

**Rejected:** (a) a literal byte-for-byte shell rewrite — it would port the bugs and add
no structural value; (b) stopping at M1 only — leaves the dominant token cost (polling)
unaddressed; (c) keeping shell + papering over BSD/GNU (WP11) — entrenches the fragility
the port removes.

## Policies

Durable rules this decision establishes:

1. **Parity = mutual readability + semantic equivalence, NOT byte-identical output.**
   During coexistence the criterion is: (a) Rust reads existing shell-written state, (b)
   shell reads Rust-written state, (c) the same command sequence yields the same *logical*
   state. The on-disk *layout* stays compatible; cosmetic differences are normalized. The
   corollary is **fix the shell's bugs in the port, do not replicate them** — forcing
   byte-identity would mean porting the bugs. Concretely fixed in M1: un-escaped JSON in
   `intents.jsonl` (now RFC-8259-escaped); the cosmetic trailing-space quirk (dropped);
   the dead `ts()` helper (omitted). Enforced by `tests/parity-harness.sh` (semantic-state
   diff + both mutual-readability directions + an overlap-hardening contrast).

2. **Path-prefix overlap rejection is the default** (`OverlapPolicy::RejectOverlap`;
   `CONCORD_STRICT_OVERLAP=0` opts back to shell behaviour). A claim that path-prefix-
   overlaps a live lease (e.g. `kernel/src/embedded` ⊃ `kernel/src/embedded/usbd`) is
   refused — the §6 collision the shell's pure string match lets through. Safe-by-default
   and coexistence-safe: Rust still *reads* shell-created overlapping leases; it only
   refuses to *create* a new overlap. The lossy-slug conflation (`a/b` vs `a b`) is closed
   by persisting the original area in an additive `area` file the shell ignores.

3. **Fencing token: field in M1, enforcement in M2.** A monotonic counter (`$COORD/fence`,
   bumped atomically under a short-lived `mkdir` mutex) is *recorded* now on every ledger
   entry and stamped on each lease and the merge lock — forward-compatible, because a
   retrofit later is expensive. The **enforcement** (reject a write carrying a stale fence
   after a stale-reclaim, defeating the pause/split-brain race [3]) arrives with the M2
   daemon, which owns the serialization point. M1 is not blocked on enforcement.

4. **Filesystem-as-truth stays inspectable.** State remains readable with `ls`/`cat`; no
   opaque binary format as the *sole* truth. The daemon (M2) is an accelerator *over* the
   FS state, never its replacement — it stays crash-survivable and debuggable.

5. **New capabilities are opt-in and gated, not bolted onto the parity path.** (e.g. the
   overlap policy is a parameter on the core, flipped by one env var, not hardcoded.)

## Architecture

```
                      ┌───────────────────────────────┐
   CLI  (coord.sh ◄── │        concord-core           │ ──► concordd (M2)
   drop-in, M1)       │  typed state + atomic txns    │     notify + debouncer-full,
                      │  Session / Lease{area,fence}  │     push stream over FS state,
   MCP server (M3) ◄──│  MergeLock / LedgerEntry      │     FENCING ENFORCEMENT
   rmcp, typed tools  │  {t,fence,session,event}      │
                      │  OverlapPolicy · fence counter│
                      └───────────────────────────────┘
                          one model, three surfaces
```

- **`concord-core`** (std-only in M1, zero deps — keeps byte-level on-disk control out of
  serde/clap which format and escape differently). Owns the model and every transition:
  - `Session{ id, focus, started, heartbeat }` with TTL staleness;
  - `Lease{ area_slug, area, holder, since, why, fence }`;
  - `MergeLock{ holder, since }` (singleton) + a `fence` file;
  - `LedgerEntry{ t, fence, session, event }` → `intents.jsonl` (append-only, escaped).
  Transitions are atomic (temp + rename; `mkdir` for the lock) and ownership-reporting
  (release/merge-unlock of a foreign lease is structurally distinguishable).
- **CLI** (`concord`): command-first dispatch identical to `coord.sh`, so hooks can call
  shell *or* binary transparently; byte-equal stdout + exit codes for the verbs.
- **`concordd`** (M2): `notify`-watches the coord dir + the SESSION-SYNC file (debounced,
  directory-level for atomic-rename robustness [4][5]); serves a notify stream; enforces
  fencing at the single serialization point.
- **MCP server** (M3): the same core exposed as typed `rmcp` tools (`#[tool]` schemas) so
  agents call `claim`/`status`/`merge-lock` as schema-validated tools, not shell strings
  [8][9]. Push (M2) + typed tools (M3) compose: the server pushes lease/SYNC changes as
  MCP notifications.

## Consequences

**Positive.** Race-free typed transitions; the polling token cost is attacked head-on by
M2; one portable artifact (supersedes WP11); a clean base for the rich roadmap items
(board/MCP/inbox); fencing makes split-brain impossible once enforced; dogfoods the `ais`
vision.

**Negative / cost.** A daemon adds lifecycle surface (start/stop/crash/upgrade) — mitigated
by keeping it **optional** with the FS state authoritative and functional without it.
Coexistence risks Shell↔Rust drift — mitigated by the parity harness as a CI/pre-push
gate. `rmcp`'s API is still moving across majors, and `cargo-dist`'s maintenance status
should be re-checked before adoption (both flagged open in `WP12-RESEARCH.md`).

**Fencing has a residual TOCTOU window in the Floor tier (accepted).** The daemon-free
Floor (M2.2) enforces fencing by a check-then-commit on the filesystem
(`holder == me ∧ fence == expected`, then remove/mutate). On a plain filesystem these
are not one atomic step, so a reclaim landing in the gap between the check and the commit
is theoretically possible. This is **accepted**: the Floor closes the common
reclaim-after-pause case (a woken stale holder is rejected, and a foreign holder can never
release/unlock another's lease), and the **airtight** guarantee is the daemon-mediated
Strong tier (M2.3), where check-and-apply runs in the daemon's single thread at the one
serialization point. So fencing is "as strong as FS-authority allows when the daemon is
down, airtight on the mediated path when it is up" — consistent with policy 4 rather than
pretending the optional daemon gives a guarantee to direct-FS writers.

**Cutover checklist** (ROADMAP §11 step 3, when the binary becomes default and the shell
is frozen to `bin/legacy/`):
- OverlapPolicy is already vision-mode (RejectOverlap default) — no flip needed.
- JSON escaping is already correct by default — no flip needed.
- Cosmetic parity quirks already removed (trailing space).
  (The list shrank because M1 chose vision-mode defaults up front rather than deferring.)

**Migration.** M1 coexists with shell (same layout); hooks flag shell *or* binary, switch
incrementally, revert any time; at parity, freeze shell to `bin/legacy/`, binary becomes
default; M2+ builds additively over the unchanged FS state.

**Open questions (resolved in review).**
- (1) ADR language — **RESOLVED: English**, matching the English public-doc layer
  (README/MANUAL/ROADMAP/BACKLOG); a flip to German is cheap if the operator prefers.
- (2) `cargo-dist` vs a plain `cargo build` CI matrix — **RESOLVED: deferred to M4**, outside
  the M1+M2+M3 cut; re-evaluate there with a fresh check of cargo-dist's maintenance status.
- (3) whether the fence counter should span shell-issued actions during coexistence —
  **RESOLVED: Rust-only**. The M2 daemon becomes the sole fence issuer at the point
  enforcement begins, so coexistence shell actions need not bump the counter.

## Sources

1. ZooKeeper ephemeral nodes / Consul sessions / lease overview —
   https://singhajit.com/distributed-systems/lease/ ·
   https://www.youngju.dev/blog/architecture/2026-03-12-distributed-lock-redis-redlock-zookeeper-etcd-comparison.en
2. etcd — Lease + KeepAlive + revision-as-fencing — https://etcd.io/docs/v3.5/learning/why/
3. M. Kleppmann, "How to do distributed locking" (fencing tokens), 2016 —
   https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html ·
   https://surfingcomplexity.blog/2025/03/03/locks-leases-fencing-tokens-fizzbee/
4. `notify` crate (backends, reliability caveats) — https://github.com/notify-rs/notify ·
   https://docs.rs/notify
5. File-watcher debouncing in Rust — https://oneuptime.com/blog/post/2026-01-25-file-watcher-debouncing-rust/view
6. `cargo-dist` — https://github.com/axodotdev/cargo-dist ·
   https://blog.orhun.dev/automated-rust-releases/
7. `cargo-release` — https://github.com/crate-ci/cargo-release
8. MCP Rust SDK (`rmcp`) — https://github.com/modelcontextprotocol/rust-sdk ·
   https://docs.rs/rmcp/latest/rmcp/
9. Building a stdio MCP server in Rust — https://www.shuttle.dev/blog/2025/07/18/how-to-build-a-stdio-mcp-server-in-rust
