# Changelog

All notable changes to Concord are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Concord adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

`VERSION` is the single source of truth; `concord version` prints it. While Concord is
`0.y.z`, the CLI, protocol, and state layout may change between **MINOR** versions; **PATCH**
releases are backward-compatible fixes. See [CONTRIBUTING](CONTRIBUTING.md#release-discipline)
for the enforced release process.

## [Unreleased]

## [0.12.0] - 2026-06-30

### Changed (breaking)
- **F-config completed — no ambient location authority.** All legacy `CONCORD_*` / `AIS_*`
  *location* environment variables (`CONCORD_DIR`, `CONCORD_SYNC`, `CONCORD_PROJECT`, and the
  `AIS_COORD_DIR` / `AIS_SYNC_FILE` / `AIS_PROJECT_DIR` aliases) are **removed** — the binary,
  daemon, and hooks no longer read them. The coordination dir and prose channel resolve purely
  by convention (`<repo>-coord` / `<repo>-SESSION-SYNC.md` siblings of the git toplevel),
  overridable only by the explicit `--coord` / `--project` flags or the user-global `[projects]`
  map. This eliminates the leaked-variable incident class by construction. See MANUAL §17.
- **`concord paths`** now prints neutral, eval-able `COORD=` / `SYNC=` / `PROJECT=` assignments
  (previously `CONCORD_DIR=` …), since those names no longer configure anything.

### Identity (explicit overrides retained)
- `CONCORD_ID` and `CONCORD_BIN` survive as the only two environment variables — both **explicit,
  deliberate launch knobs**, not ambient authority. `CONCORD_ID` declares which logical session a
  process is (needed when several sessions share one checkout); `CONCORD_BIN` points the hooks at a
  specific binary. `CONCORD_ID` is transitional — capability-bound cryptographic identity (roadmap
  C1) will supersede it.

### Migration
- Replace any `CONCORD_DIR=… concord …` with `concord --coord … …` (or just run from the project,
  letting convention resolve it). Drop `CONCORD_SYNC` / `CONCORD_PROJECT` / `AIS_*` entirely.
- Hook installs are unaffected — re-run `concord install-hooks` after upgrading; the materialized
  hooks resolve everything by convention.

## [0.11.4] - 2026-06-30

### Added
- **Telemetry end-to-end smoke** (`tests/telemetry-smoke.sh`): starts `concordd` with the OTLP
  receiver on a probed free port (isolated; never clashes with a live daemon), POSTs synthetic
  OTLP/HTTP-JSON metrics, and asserts `concord status` flags BURN / REJECT / IDLE. Closes the
  one feature without a smoke (the HTTP receiver path).
- **Fencing smoke** (`tests/fencing-smoke.sh`): the fencing token at the CLI — a stale fence is
  refused, and after a stale-reclaim the old holder cannot release the reclaimed lease (no
  split-brain). The core fence Floor stays covered by the integration tests (not duplicated).
- **`make test` / `tests/all.sh`**: a one-command runner — `cargo test --workspace` + every
  shell smoke (auto-discovered, so the list never drifts). CI now runs the full suite (15+
  smokes, incl. the previously CI-absent `multiproject` and `dogfood-smoke`).
- **MANUAL §16 "Testing"**: how to run the suite + a live-validation checklist for the
  experienceable enforcement (claim conflict, symbol locks, contract break, merge-lock
  singleton, the keystroke deny, the telemetry watchdog).

## [0.11.3] - 2026-06-30

### Changed
- **Hooks now find a globally-installed `concord`.** `hooks/lib.sh` resolves the coordination
  CLI as `$CONCORD_BIN` → the project's own `target/` build → **a `concord` on `PATH`** → the
  shell `coord.sh` → none. The added global-`PATH` step lets a project with no local build
  (e.g. one driven by an installed release) use the Rust binary; it stays fail-safe (no
  `concord` anywhere → the shell fallback). Baked into the binary via `include_str!`, so
  `concord install-hooks` materializes it everywhere. New `tests/coord-sh-resolution.sh`
  asserts the precedence (in CI); the fail-open hook smoke now isolates from a globally
  installed binary.
- Fixed two stale code comments that said "repo-root `config.toml.example`" (it now lives in
  `config/`).

## [0.11.2] - 2026-06-30

### Changed
- **`config.toml.example` moved to `config/config.toml.example`.** Still a single source
  (embedded into `concord init` via `include_str!`); the release archives now mirror it under
  `config/`, so the copy command is identical everywhere:
  `cp config/config.toml.example <repo>-coord/config.toml`.

### Documentation
- **Public docs synced to the v0.11.x reality (code-verified).** README, QUICKSTART, and the
  MANUAL no longer reference the retired shell tool (`coord.sh`/`bin/concord`), the old `K`
  coordinator (now `hub`), or `CONCORD_ID`-as-identity. Identity is documented as the
  worktree-derived id (`concord start <id>` writes an id-bind marker; the `<repo>-<id>` worktree
  name is the fallback); the `CONCORD_*` env vars are a deprecated bootstrap footnote. The MANUAL
  CLI reference, setup, and automation sections were rewritten to the real `concord` verbs and
  `concord init --with-hooks` / `concord install-hooks`. README version badge → 0.11.2.

## [0.11.1] - 2026-06-29

### Added
- **`config.toml.example`** — a fully-commented configuration template (every key at its
  built-in default) in the repo root, **shipped in every release archive** and embedded into
  `concord init` (one source of truth via `include_str!`, so the example, the init scaffold,
  and the release asset cannot drift). A round-trip test asserts it parses to exactly
  `Config::default()`.
- **Configuration documented** (`docs/MANUAL.md` §11 rewritten; README + QUICKSTART): the
  `config.toml` is the primary mechanism, with the two install locations (`<repo>-coord/` and
  user-global `~/.config/concord/`), the precedence (defaults ← user-global ← project), and a
  copy-from-template step. The legacy `CONCORD_*`/`AIS_*` env vars are documented as
  deprecated bootstrap-only (honored-with-warning for one release).

### CI / housekeeping
- **`tests/mcp-smoke.sh` now runs in CI** (it was unreferenced and could rot). It drives the
  MCP server over a real JSON-RPC handshake and asserts the enforced-only tool surface.
  Hardened to isolate by the git-toplevel convention (concord-mcp resolves its coord dir
  from the toplevel, not env) so it never touches a live coordination dir.
- **Retired the one-off ais cutover script** to the local, git-ignored archive — its
  migration has been stable for weeks; only historical rollback remained.
- **Trimmed the public documentation surface to what a user needs:** README, `docs/MANUAL.md`,
  `docs/QUICKSTART.md`, CHANGELOG, CONTRIBUTING, LICENSE. Internal/superseded artifacts (the
  ADR records, the backlog, the roadmap, the dogfood runbook, and the agent-mail verification
  note) moved to the local, git-ignored archive (off-GitHub). Dangling internal links removed
  or softened; the contributor workflow now points at GitHub issues.

### Documentation
- **Publication polish.** All project documentation is now English. The README is rewritten
  in plain, jargon-light language covering every feature (incl. the autonomous coordinator
  and symbol/AST-level locking) with no internal codenames. Internal German research/design
  drafts (competitive landscape, feature-mining, the original port research/plan) moved to a
  local, git-ignored archive (off-GitHub); the ADRs that cited them keep their decisions and
  external sources, with internal links softened so nothing dangles. `BACKLOG.md` and
  ADR-0003 translated to English. No code or version change.

## [0.11.0] - 2026-06-29

Wave 2 — F5: enforced signature contracts. The one Peer-collaboration CLAUDE.md permits
(negotiating an interface) gets teeth — two agents agree on a `<file>:<symbol>` signature,
and a commit/merge gate blocks a change to it without renegotiation. **This completes
Wave 2 (F1·F2·F3·F-config·F4·F5).** (ADR-0003 F5.)

### Added
- **`concord_ast::signature()`** — the normalized signature an interface contract pins:
  for a function/method, the declaration up to the body delimiter (`{` for Rust/TS, the
  body `:` for Python) so changing the *body* does not break the contract; for a
  type/struct/class/enum/interface, the full declaration (the shape itself is the
  interface). Whitespace-collapsed (reformatting is not a change).
- **Contract store + verbs.** `concord contract <id> <file>:<symbol> [--with <other>] [why]`
  snapshots the current signature as the agreed contract (the act of registering = the
  agreement reached in prose; `--with` records the counter-party). `contract --update`
  renegotiates, `contract-release` drops it, `contracts` lists them. Stored in
  `<coord>/contracts/<file:symbol>/` (same key scheme as symbol-leases) as readable text —
  `concord-core` persists it; the binary extracts via `concord-ast`, so the core stays
  dependency-free.
- **The gate.** `concord contract-check [<file>]` re-extracts each contract's current
  signature and **exits non-zero** if any was changed without renegotiation. Wired as a
  **git pre-commit hook** (`scripts/install-hooks.sh`) *and* a **merge-lock precondition**
  (the coordinator-level gate; override with `--no-contract-check`). **Fail-open:** an
  unreadable file/symbol is never counted as broken. Surfaced in `status`/`dash`
  (CONTRACTS).
- New CI smoke `tests/contracts.sh` + `signature()` unit test + contract CRUD integration.

### Notes
- F5 is a different axis from F1: **F1** gates *edits* by lease at the keystroke (who may
  edit); **F5** gates the agreed *interface* at commit/merge (did you break a contract).

## [0.10.0] - 2026-06-29

Wave 2 — F4: hub telemetry. The coordinator becomes **telemetry-driven** instead of
prose-reading — it consumes Claude Code's native OpenTelemetry stream to *measure* fleet
health (burn / idle / reject / loop), and a dark-session watchdog auto-escalates via F3.
(ADR-0003 F4 + folded B3.) Built natively on the OTel stream — **no SaaS dependency**
(Policy 3).

### Added
- **OTLP/HTTP-JSON telemetry receiver** (`concordd`, Unix-only): a minimal
  `std::net::TcpListener` + `serde_json` sink on a Concord-specific port (default **4319**,
  config `[telemetry] port`) — no protobuf / no otel-collector dependency. Claude Code,
  launched with `OTEL_EXPORTER_OTLP_PROTOCOL=http/json` + `OTEL_RESOURCE_ATTRIBUTES=
  concord.id=<id>` (the launcher sets these — Claude's *own* OTEL_* vars, not Concord env),
  POSTs metrics that are normalized to `<coord>/telemetry/<id>.jsonl`. **Privacy:** only
  metric attributes are ingested — never prompt content.
- **Health heuristic** (`concord-core::telemetry`, testable + dependency-free): per session
  **OK / IDLE / BURN / REJECT / LOOP** from token-burn rate, idle gap, edit-tool reject
  storms, and sustained-activity-without-progress, against `[telemetry]` thresholds.
  Surfaced in `status`/`dash` (TELEMETRY / HEALTH).
- **B3 dark-session watchdog → F3:** a telemetry-idle session still holding a lease or with
  un-ACK'd directives is auto-escalated (severity `high`) to the coordinator, deduped by a
  per-session marker until it recovers.
- New `[telemetry]` config section + sample; integration test (health + watchdog→F3),
  OTLP-parser + health unit tests, launcher OTel-env smoke.

### Notes
- **Windows:** the telemetry receiver is Unix-only (like the daemon's Strong tier),
  consistent with the Floor support matrix. **ccusage** is an optional local cost view
  (no build dependency).

## [0.9.0] - 2026-06-29

F-config (operator-inserted between F3 and F4): ONE `config.toml`, environment variables
retired. Configuration is now explicit and inspectable instead of ambient env state.
(ADR-0003 §F-config.)

### Added
- **`config.toml`.** A project `<coord>/config.toml` (`[leases]/[daemon]/[launcher]/
  [escalation]/[resources]`) layered over a user-global `~/.config/concord/config.toml`
  (which adds a `[projects]` bootstrap map), over built-in defaults — **`config.toml` >
  defaults, no env.** `concord init` drops a commented sample. The `toml`/`serde` parse is
  isolated in a new **`concord-config`** crate, so `concord-core` stays dependency-free and
  takes a plain `Config`. A malformed file warns and falls back (never crashes).
- **Env-free bootstrap.** The two values config can't define resolve by convention + flags:
  **coord dir** = git-toplevel `<repo>-coord` / `--coord` / the user-global `[projects]`
  map; **session id** = worktree `<repo>-<id>` / `--id` / an idbind marker
  (`<coord>/idbind/<worktree>`, written by `concord start`). The launcher no longer exports
  `CONCORD_ID`; the hooks derive id from the marker / convention.
- New CI smoke `tests/config.sh` + `concord-config` unit tests.

### Changed / Deprecated
- **Environment variables are retired.** `CONCORD_DIR`/`AIS_COORD_DIR`, `CONCORD_SYNC`/
  `AIS_SYNC_FILE`, `CONCORD_PROJECT`/`AIS_PROJECT_DIR`, `AIS_COORD_TTL`,
  `CONCORD_STRICT_OVERLAP`, `CONCORD_NO_DAEMON`, `CONCORD_CLAUDE_FLAGS`,
  `CONCORD_COORDINATOR_ID` all move to `config.toml`/flags/convention. A still-set legacy
  var is **honored with a one-time deprecation warning** this release (protecting existing
  setups), and **removed entirely next release**. The `<coord>/strict-leases` marker folds
  into `[leases] strict`.

## [0.8.0] - 2026-06-29

Wave 2 — F3: ack-tracking + tracked escalation. Two CLAUDE.md prose policies that were
toothless become **enforced state**: un-ACK'd coordinator directives are re-delivered and
auto-escalated, and blockers become tracked objects that cannot silently vanish. (ADR-0003 F3.)

### Added
- **Tracked escalations (E2).** `<coord>/escalations/<seq>/` records (one dir each, like a
  lease) that **persist until resolved** — the coordinator's real forwarding queue to the
  operator. `concord escalate <id> <severity> <body> [--to <target>] [--ref R]` (default
  `--to` = the coordinator, read from `<coord>/coordinator`, fallback `hub`),
  `concord resolve <id> <seq> [note]`, `concord escalations` (open first). Severity
  `low|medium|high|critical`.
- **Ack-tracking (E3).** Per-recipient pending directives in `<coord>/acks/<id>.pending`.
  **Derived ack** (zero-migration): the daemon clears a session's pending when it posts
  (the A3 watermark mechanized); plus an explicit `concord ack <id> [note]`. The daemon's
  active layer (`Store::tick_acks`) **re-delivers** an un-ACK'd directive (bumping the
  recipient's inbox mtime to wake it) after `TTL_ack` (15 min), and **auto-escalates**
  (severity `high`) after `K=2` re-deliveries — a session ignoring directives can no longer
  go dark unnoticed.
- **Status surface.** `status`/`dash` gain **ESCALATIONS (open)** and **PENDING ACKS**
  sections.
- New CI smoke `tests/ack-escalation.sh` + 2 integration tests (escalation lifecycle;
  re-deliver→auto-escalate timing) + escalation unit tests.

### Notes
- The TTL re-deliver / auto-escalate is the **daemon's active layer**; the verbs + status
  work on the Floor (daemon-down) too. **A3's `stop.sh` keeps its own fail-open derivation**
  (it must work daemon-down); the daemon pending is additive, never an A3 dependency.

## [0.7.0] - 2026-06-29

Wave 2 — F2: named resource locks / build-slots. The enforced lease primitive now
covers *non-file* resources (ports, the build-env, deploys) with N-slot semaphore
semantics — solving the documented `ais` contention (QEMU ports, build-env singleton,
mutual QEMU-killing). (ADR-0003 F2.)

### Added
- **`kind=resource` semaphore namespace.** A new `<coord>/resources/` store, structurally
  **orthogonal** to the path/symbol leases (it never touches the overlap logic, so a
  resource and a file of the same name never collide). Each resource is an N-slot
  semaphore: `Store::acquire_resource` takes the first free slot `0..N` via an atomic
  `mkdir` (race-safe), reusing the existing fencing / TTL-stale-reclaim / ownership
  machine — so a **slot held by a crashed session is reclaimed and the pool self-heals**.
  Capacity is persisted (`<coord>/resources/<name>/capacity`) on first acquire and
  validated thereafter. `slots=1` = exclusive.
- **CLI:** `concord claim <id> <name> --kind resource [--slots N] [why]`
  (`RESOURCE-ACQUIRED`/`RESOURCE-RECLAIMED`/`RESOURCE-BUSY`/`RESOURCE-CAPACITY-MISMATCH`)
  and `concord release <id> <name> --kind resource`. `status`/`dash` gain a **RESOURCE
  LOCKS** section (held/capacity + holders).
- **SessionEnd composition (F1/A2 + F2):** the clean-exit teardown now also releases the
  session's resource slots — a crashed or finished session auto-frees its ports/build-env.
- New CI smoke `tests/resource-locks.sh` (N-slot pool, BUSY, capacity-validate,
  orthogonality, release, SessionEnd auto-free) + two integration tests.

### Examples (`ais`)
- Build-env singleton: `claim <id> build-env --kind resource`.
- QEMU port pool: `claim <id> qemu-port --kind resource --slots 8` (slot i → port 5900+i).
- Per-VM exclusive port: `claim <id> qemu-port:5901 --kind resource` (check before killing).

## [0.6.0] - 2026-06-29

Wave 2 — F1: harness-native enforcement. Concord's leases stop being advisory at the
edit boundary and start being **hard at the keystroke**, and the "going dark" failure
mode gets a harness-native cure. (ADR-0003, accepted.)

### Added
- **A1 — `PreToolUse` lease deny.** A new typed `concord check-lease <id> <file>` verb
  (P2 *block-on-conflict* by default — deny only when a *different active* session holds
  an overlapping lease; symbol-aware via the S2 AST; `<coord>/strict-leases` switches to
  P1 *capability-strict*). The `pre-tool.sh` hook calls it and returns
  `permissionDecision:"deny"`, blocking a conflicting edit **before** the tool runs.
- **A2 — `SessionEnd` clean-exit teardown.** `concord session-end <id>`
  (`Store::session_end`/`release_all`/`deregister`, idempotent) releases all of a session's
  leases, drops the merge-lock if held, and deregisters on clean exit — complementing the
  TTL-stale-reclaim.
- **A3 — `Stop` anti-going-dark.** `stop.sh` refuses a turn-end while an un-ACK'd
  `### … → <id>` coordinator directive is pending (narrow predicate; `stop_hook_active`
  loop-guard), injecting it so the session handles it instead of going dormant.
- **A4 — `PreCompact` protocol-memory.** `pre-compact.sh` snapshots leases/merge-lock to
  `<coord>/state/<id>.precompact` and emits `additionalContext`; `session-start.sh`
  re-injects it on `source=compact` (belt-and-suspenders).
- **A6 — `PostToolUse` out-of-scope-write audit.** Audit-only accountability backstop
  behind A1: an edit that slipped past the deny (another active holder) is logged as a
  provenance violation in the ledger.
- New CI smoke `tests/hooks-enforce-smoke.sh` asserts the A1 deny / A6 audit / A3 anti-dark /
  A4 snapshot paths and that all hooks fail-open. `install-hooks` now ships 12 files.

### Notes
- **A5 (`FileChanged` monitor replacement) re-scoped.** Schema verification found
  `FileChanged` is observational-only and `watchPaths` lives on `SessionStart`; whether it
  *wakes a dormant session* is unproven. So the `stat`-loop monitor / self-tick **stays the
  wake** pending a verification task; only then is it retired. (ADR-0003 §F1/A5.)

## [0.5.0] - 2026-06-29

Cross-platform + distribution (S3/M4) — Concord is now shippable.

### Added
- **Distribution pipeline (M4.2).** A self-contained, version-gated GitHub Actions release
  workflow (`.github/workflows/release.yml`): on a `vX.Y.Z` tag it checks version discipline,
  cross-builds the support matrix (aarch64/x86_64 macOS, x86_64 Linux, x86_64 Windows-MSVC),
  and attaches archives + SHA-256 checksums of all three binaries (`concord`, `concordd`,
  `concord-mcp`) to a GitHub Release. A `curl … | sh` installer (`scripts/install.sh`) detects
  the platform, verifies the checksum, and installs to `~/.local/bin`. `dist` config lives in
  `[workspace.metadata.dist]` for richer installers once `dist` is adopted. CI workflow
  (`ci.yml`) runs build/clippy/test/version + a Windows `cargo check`.
- **Windows portability (M4.1).** The Unix-domain-socket code in `concord-core::ipc`,
  `concordd`, and `concord-mcp` is now `cfg(unix)`-gated; off Unix, `ipc::mediate` is a
  no-op so every consequential op falls back to the enforced **Floor** (FS-authoritative
  leases, the merge singleton, fencing, symbol-locks — all platform-portable). The typed
  core, CLI, daemon, and MCP server all `cargo check` cleanly for `x86_64-pc-windows-gnu`.
- **Embedded hooks + `concord install-hooks` (M4.1).** The Claude Code automation scripts
  ship *inside* the binary (`include_str!`), so a `cargo install`'d `concord` needs no repo
  checkout to set up. `concord install-hooks [--no-wire]` materializes them into
  `<coord>/hooks/` (with exec bits) and, on Unix, wires `~/.claude/settings.json` via the
  proven `install.sh`. `concord init --with-hooks` does both in one step. Off Unix the files
  are written but `settings.json` is left untouched (session-automation is Unix-only).

### Notes
- **`cargo-dist` maintenance concern (ADR-0001) RESOLVED.** Verified actively maintained —
  released as `dist` (v0.31/0.32 in 2026). The M4.2 distribution layer adopts it.

## [0.4.0] - 2026-06-29

Symbol-level (AST) leases — the differentiator. Concord can now lease a single symbol
(`<file>:<symbol>`), not just a file path, and it enforces that lease (prior-art tools
have symbol granularity but only advisory locks).

### Added
- **Enforced symbol-level leases (S2.1).** A lease area may be `<file>:<symbol>`, a finer
  lease *under* the path-lease. Two sessions can hold leases on **disjoint symbols of the
  same file in parallel** — what a file lease cannot express — while a file path-lease
  still subsumes any symbol in it (bidirectional), all under the same fence / ownership /
  daemon-mediated enforcement. (`concord-core::slug::area_overlaps`.)
- **`concord-ast` crate** — native tree-sitter symbol extraction for **Rust, TypeScript,
  and Python** (functions/methods/types/classes/…), with byte ranges; and a **Rust call
  graph** (caller→callee).
- **`concord symbols <file>`** lists a file's claimable symbols; `claim <file>:<symbol>`
  validates the symbol exists.
- **Advisory DEP_CHAIN warning (S2.2).** Claiming a Rust symbol that *calls* a symbol
  another session holds emits an advisory note (a call edge is a hint, not exclusion — the
  one genuinely-advisory layer; the lease itself stays enforced).

## [0.3.0] - 2026-06-29

The Rust migration: Concord is now a single typed Rust binary (CLI + push daemon + MCP
server + launcher), with the shell originals frozen as a parity fallback. This release
bundles the work that landed since 0.2.1 (the WP12 milestones M2–M5 + WP6 + S1).

### Added
- **Launcher folded into the one binary (S1).** `concord start/dash/pause/resume/stop`
  (ported from the shell `bin/concord`) — `start` launches a session in the current
  terminal (Unix exec-replace) with the right id/env/permissions/kickoff prompt;
  `--print` is a dry-run; `dash` is the typed live overview. Completes the Rust migration.
- **`concordd` push daemon (M2).** Watches the coord dir + prose channel (`notify` +
  debouncer) and demultiplexes directives into per-recipient typed inboxes
  (`inbox/<id>.jsonl`), so a session wakes on its own deltas instead of re-reading the
  whole channel. Optional; the filesystem stays authoritative.
- **Fencing tokens + enforced ownership (M2).** A monotonic fence on leases/merge-lock;
  release/merge-unlock refuse foreign or stale-fenced authority. The daemon mediates
  consequential ops (merge-lock, claim, release) with an airtight single-thread
  check-and-apply (the Floor's residual TOCTOU closed when the daemon is up).
- **MCP server (M3-lean).** `concord-mcp` exposes the enforced primitives
  (claim/release/verify/merge-lock/…) as typed `rmcp` tools over stdio.
- **Typed inbox protocol (WP7-lean).** `concord send` + classified message kinds.
- **Multi-project + dogfood (M5).** `concord init` / `concord paths`; per-project coord
  derivation; Concord coordinates its own development via a dedicated `concord-coord`.
- **Path-prefix overlap rejection** is the default for `claim`.

### Changed
- **ais coordination cut over to the Rust tool (WP6),** reversibly
  (`scripts/wp6-ais-cutover.sh`).
- **Version discipline is Rust-aware.** `scripts/check-version.sh` verifies VERSION ↔
  CHANGELOG ↔ `Cargo.toml` ↔ the built binary (the frozen shell `bin/concord` is no
  longer the version source).
- Decisions recorded in **ADR-0001** (Rust port) and **ADR-0002** (refocus on the
  enforced core), with a source-level competitive verification.

## [0.2.1] - 2026-06-28
### Changed
- **Human-director role is now name-abstract (`the operator`).** The coordinator kickoff and
  self-tick prompts (and README/guide/backlog) refer to the human who directs the fleet as
  "the operator" instead of a hardcoded personal name — keeping Concord's prompts identity-neutral
  and portable across projects. The `operator → coordinator → workers` delegation chain is unchanged;
  only the label is. The MIT copyright holder in `LICENSE`/`README` is intentionally left as-is
  (legal attribution, not an operational role).
- **No wired absolute paths in the hooks.** `hooks/lib.sh` now derives the coordination dir from
  its own location (`<coord>/hooks/` → `<coord>`) and the project repo + prose channel from the
  naming convention (`<repo>-coord`, `<repo>-SESSION-SYNC.md`); `user-prompt.sh`/`session-start.sh`
  consume those derived values instead of re-hardcoding the sync path. Env (`CONCORD_DIR/SYNC/PROJECT`,
  legacy `AIS_*`) still wins for multi-project. Removes the last hardcoded `/Users/...` fallbacks from
  the hook scripts — the `concord` CLI already derived paths this way.

## [0.2.0] - 2026-06-28
### Added
- **Distinct coordinator role.** `concord start` now gives the coordinator its own kickoff and
  self-tick (a neutral steward, not a worker — it never waits for a GO and takes no code terrain)
  instead of the worker prompt. The coordinator id is configurable via `CONCORD_COORDINATOR_ID`
  (default `hub`) and matched case-insensitively (`is_coordinator`).
- Worker kickoff now **always announces presence** to the coordinator — it posts READY right after
  setup, even with nothing to report yet — so the coordinator reliably knows which sessions are up.

### Changed
- **Case-insensitive session ids** everywhere ids are compared: the directive matcher
  (`### … → K` reaches `k` and vice versa, including the coordinator), the statusline colour map,
  and `concord dash` (now derives the session list from the registry instead of a hardcoded list).
- Worker and coordinator prompts rewritten in **English** and parameterized on the coordinator id
  and the prose-channel path, so the wording always matches the actual setup.
- Session names may be any single token (letters/digits/`-`/`_`, case-insensitive); `alle` (the
  broadcast target) and the coordinator name are reserved.
- Enforce version discipline with a **local pre-push hook** (`scripts/install-hooks.sh`) instead of
  a GitHub Actions workflow — no cloud service, no cost. `scripts/check-version.sh` is unchanged.

## [0.1.0] - 2026-06-28
### Added
- File-based coordination core: registry, area leases, singleton merge lock, stale reclaim,
  prose channel, intent log (`bin/coord.sh`).
- Mission-control CLI `bin/concord`: `start` / `stop` / `pause` / `resume` / `dash`; identity via
  `CONCORD_ID`; the `<repo>-<id>` worktree convention; the READY/GO dispatch handshake; launch in
  the current terminal.
- Claude Code hooks: window-identity statusline, auto-register/heartbeat, status injection,
  lease/merge guard, install/uninstall.
- Project-agnostic configuration via `CONCORD_DIR` / `CONCORD_SYNC` / `CONCORD_PROJECT`
  (the multi-project foundation).
- Documentation: README, `docs/MANUAL.md`, `docs/ROADMAP.md`, `docs/BACKLOG.md`, CONTRIBUTING.
- Versioning: `VERSION` (single source of truth), `concord version`, this changelog,
  `scripts/check-version.sh`, and a CI workflow that enforces version discipline on every push/PR.
- MIT license.

[Unreleased]: https://github.com/msto63/concord/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/msto63/concord/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/msto63/concord/releases/tag/v0.1.0
