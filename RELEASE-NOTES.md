# Concord — Release Notes

Narrative highlights of each Concord release: what changed and why it matters. For the
complete, per-release change record — every *Added / Changed / Fixed* line — see the
**[CHANGELOG](CHANGELOG.md)**. This file is the story; the changelog is the ledger.

> The newest release is at the top. Add a new `## vX.Y.Z` section above the previous one.

## The foundation

**Concord** lets several AI coding sessions work the same repository in parallel without
damaging each other's work. It is **file-based** (state is plain files on the local disk —
no server, no database) and its coordination is **enforced**, not advisory: a session is
*stopped* at the editor hook, not merely asked to cooperate. An autonomous coordinator
(`hub`) assigns and sequences the work.

The **enforced core**:

- **Leases** at file *and* symbol level — two sessions can edit the same file on disjoint
  functions, but not the same symbol. Symbol boundaries come from a tree-sitter AST.
- **Fencing tokens** — a stale session that reclaims after losing its lease cannot clobber
  the new holder, so there is no split-brain.
- **A singleton merge lock** — only one session merges at a time, and a *broken signature
  contract even blocks the merge*, so a changed public API can't slip through the gate.
- **A push daemon** (`concordd`) that demuxes `### from → to` directives from the prose
  channel into per-recipient inboxes.
- **An MCP surface** (`concord-mcp`) exposing the coordination primitives to tools.

**Wave-2 hardening** (shipped across the v0.11.x line) layered on: harness-native Claude
Code hooks (deny / anti-dark / pre-compact / audit), self-healing resource semaphores,
ack-tracking with tracked escalation, one-file TOML configuration, a telemetry-driven
watchdog (BURN / REJECT / IDLE), and enforced signature contracts.

---

## v0.12.0 — F-config complete (no ambient location authority)

This is a **breaking** release (pre-1.0, so a minor bump): it finishes **F-config** by
removing the last piece of *ambient authority* from Concord.

The legacy `CONCORD_DIR` / `CONCORD_SYNC` / `CONCORD_PROJECT` environment variables (and
their `AIS_*` aliases) are **gone** — the binary, the daemon, and the hooks no longer read
them. Coordination location now resolves **purely by convention** (the `<repo>-coord` and
`<repo>-SESSION-SYNC.md` siblings of the git toplevel), overridable only by the explicit
`--coord` / `--project` flags or the user-global `[projects]` map. Nothing inherited from
the surrounding shell can silently redirect where Concord reads and writes — which
eliminates a whole class of "a leaked variable wrote into the live coordination dir"
incidents **by construction**.

Identity is kept, deliberately, as an **explicit** declaration: `CONCORD_ID` and
`CONCORD_BIN` remain as the **only two** environment variables — explicit launch knobs, not
ambient authority. `CONCORD_ID` is the one mechanism that distinguishes several logical
sessions sharing a single checkout (convention and the id-bind marker both key off the
worktree). It is transitional — capability-bound cryptographic identity (roadmap **C1**)
will supersede it. The full rationale is recorded as a design decision in
**[MANUAL §17](docs/MANUAL.md#17-design-decision-no-ambient-location-authority)**.

### Quality & delivery

- **Tests: all green** — `make test` runs **71 cargo unit/integration tests** plus **15
  shell smoke tests**, all passing. The smokes exercise the enforced paths end-to-end
  (symbol locks, fencing, contracts, the merge lock, the daemon, telemetry, the hooks).
- **Releases** ship prebuilt binaries for **four targets** — `aarch64-apple-darwin`,
  `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-msvc` — as **eight
  assets** (an archive per target plus a `.sha256` checksum each). The fully-commented
  `config/config.toml.example` is bundled in every release.
- **Install** three ways: the `curl … | sh` one-liner, `cargo install --git`, or a prebuilt
  binary from the GitHub Releases page (see the [README](README.md#install)).

### Platform support

macOS and Linux are **fully** supported. On Windows the **core guarantees** build and run
(the `concord` CLI: leases, fencing, contracts, the merge lock — the Floor); the background
daemon, the telemetry receiver, and the bash hooks are **Unix-only**.

### Deferred backlog

Not in this cut, on the roadmap: **D2** a pre-merge gate, **C1** cryptographic session
identity (which retires `CONCORD_ID`), **E1** a task DAG with **E5** session briefs, and
**D3** a conflict probe.

For the complete v0.12.0 change record, see the **[CHANGELOG](CHANGELOG.md)**.
