# Concord Dogfood Runbook (WP12 M5)

> How to make Concord coordinate **its own** development with the **Rust** tool —
> isolated from any other project's coordination — and what is automated vs. what the
> operator does. Status: M5 delivers the machinery; the live cutover is operator-driven.

## What M5 delivered (automated, in this repo)

- **Multi-project path derivation.** `concord` derives a per-project coordination state
  (`<repo>-coord/`, `<repo>-SESSION-SYNC.md`) from `CONCORD_DIR`/`CONCORD_SYNC`/
  `CONCORD_PROJECT` env, else the git-toplevel sibling convention. Two projects never
  cross-talk (`tests/multiproject.sh`).
- **`concord init [--project <path>] [--ids a,b,c]`** — idempotently scaffolds a
  project's coord dir + prose channel (+ optional session registrations).
- **`concord paths`** — prints eval-able `CONCORD_DIR/SYNC/PROJECT`
  (`eval "$(concord paths)"`), the single source of truth for scripts/hooks.
- **Hooks prefer the Rust binary (self-scoping).** `hooks/lib.sh` resolves the
  coordination CLI to the project's own `target/{release,debug}/concord` (or an explicit
  `$CONCORD_BIN`), falling back to the shell `coord.sh`. A project without a local
  concord build stays on shell — so this does **not** switch other projects to Rust.
- **Dogfood acceptance** (`tests/dogfood-smoke.sh`): the Rust tool drives a full
  multi-session flow (register / claim / conflict / path-overlap / merge-lock singleton /
  ownership-enforced release / sync / status) in an isolated coord dir — green.

## Operator runbook — turn the dogfood ON

The dogfood is "future concord-dev sessions coordinate via `concord-coord/` using the
Rust binary." The machinery is self-scoping, so turning it on is two steps:

1. **Build the binary once** (so the hooks find it):
   `cd ~/Projects/concord && cargo build --release`
   (or set `CONCORD_BIN=/path/to/concord` for the sessions.)

2. **Bootstrap the dogfood coord state** (additive; does not touch any other project):
   `cd ~/Projects/concord && env -u CONCORD_DIR -u CONCORD_SYNC -u CONCORD_PROJECT \
      ./target/release/concord init --ids hub,a,b`
   → creates `~/Projects/concord-coord/` + `~/Projects/concord-SESSION-SYNC.md`.

3. **Launch concord-dev sessions in the concord repo.** A session started **in
   `~/Projects/concord`** derives `concord-coord` by convention and uses the Rust binary
   via the updated `lib.sh`. Use the launcher (`bin/concord start <id>`, which exports
   `CONCORD_DIR=concord-coord`) or set the env yourself. Those sessions now coordinate
   over the dedicated `concord-SESSION-SYNC.md`, isolated from `ais-SESSION-SYNC.md`.

That's it — "Concord coordinates Concord with Concord."

## Crucial: the env-override lesson (isolation is via coord-dir + sync, and **env wins**)

`CONCORD_DIR`/`CONCORD_SYNC` in the environment **override** the per-project convention.
This is by design — it is exactly how a session inherits its project's coord dir at
launch. But it has a sharp edge:

> If you run a coordination command in a shell that already has `CONCORD_DIR=…ais-coord`
> exported (e.g. a session launched for the *ais* project), the command writes to
> **ais-coord**, no matter what directory you are in.

During M5 development a prep test did exactly this and wrote a stray session into the live
`ais-coord` (cosmetic, self-healing). **Rule:** to operate on (or test) a *different*
project's state, either set `CONCORD_DIR`/`CONCORD_SYNC` explicitly for that project, or
clear them (`env -u CONCORD_DIR -u CONCORD_SYNC -u CONCORD_PROJECT …`). The test suites do
the latter on purpose.

## Safety: instant shell fallback (parity)

The Rust binary and the shell `coord.sh` read/write the **same on-disk format** (proven
by `tests/parity-harness.sh`). If the Rust tool ever misbehaves while dogfooding, fall
back instantly without losing state:

- set `CONCORD_BIN=~/Projects/concord/bin/coord.sh` for the sessions, or
- remove/rename `target/*/concord` so `lib.sh` resolves the shell `bin/coord.sh`.

Both operate on the same `concord-coord/`.

## Moving Concord's own dev onto `concord-coord` (the channel handover)

The dedicated dogfood state has been scaffolded (additive; `ais-coord` untouched):

- `~/Projects/concord-coord/` — sessions, leases, ledger (sessions `hub`, `w` registered)
- `~/Projects/concord-SESSION-SYNC.md` — the dedicated prose channel for Concord's own dev
- `~/Projects/concord-coord/hooks/` — the deployed hooks for these sessions

To actually move Concord's development off the shared `ais-SESSION-SYNC.md` onto its own
channel, the **operator** does two things (the session move is operator-run so live work
is never cut mid-flight):

1. **Launch the concord-dev sessions in the concord repo.** A session started **in
   `~/Projects/concord`** derives `concord-coord` + `concord-SESSION-SYNC.md` by the
   sibling convention and uses the Rust binary via `lib.sh`. Use `bin/concord start <id>`
   (it exports `CONCORD_DIR=concord-coord`), or set the env yourself. Restart the
   concord-dev session(s) — e.g. `concord-w` → a fresh session against `concord-coord`.
2. **Point the coordinator at the new channel.** `hub` must now **watch
   `concord-SESSION-SYNC.md`** (the Monitor target) to keep coordinating Concord's work —
   the channel handover from `ais-SESSION-SYNC.md`. Past directives stay in the ais channel
   as history; new Concord-dev coordination flows on `concord-SESSION-SYNC.md`.

**Why this matters (the structural fix).** While a concord-dev session runs against the
shared `ais-coord` (its ambient `CONCORD_DIR=…ais-coord`), any stray tool call writes to
ais-coord — the env-override hazard behind the two prep incidents. Once the session runs
against `concord-coord`, its ambient coord dir *is* `concord-coord`, so the hazard is gone
**by construction**, not by discipline.

## Out of scope (future operator decisions)

- **WP6 done — ais now runs on the Rust tool** (deployed `ais-coord/hooks/lib.sh`
  `COORD_SH`→Rust binary, reversible via `scripts/wp6-ais-cutover.sh --rollback`). The
  session-launcher (`concord`) staying shell vs. Rust is a separate, smaller follow-up.
- M4 (cross-platform/distribution) and M6 (Concord-on-ais) are separate (ADR-0002 scope).
