# Concord — Manual

Complete reference for **Concord**, a file-based system that lets several AI coding sessions work
the same repository in parallel without damaging each other's work. This manual covers the model,
the mechanics, the protocols, the CLI, the automation hooks, setup, and ready-made prompts.

> New here? Read the [README](../README.md) first for the one-paragraph pitch and quick start.

---

## 1. What Concord is

When multiple AI sessions (e.g. Claude Code) work one project at the same time — each in its own
git worktree — they will, without coordination, impair each other: concurrent edits to the same
file, colliding merges to `main`, one session killing another's test process. Concord prevents
that with three things:

1. **A structured registry** — who is active, what they are focused on, when they last checked in.
2. **Area leases** — a cooperative claim on a shared region before editing it.
3. **A singleton merge lock** — only one session merges to `main` at a time.

All of it is plain files on the shared local filesystem — no server, no database, no `jq`. It is
reliable precisely because it is simple.

Two complementary channels run side by side:

- **Concord** — the structured, *enforced* coordination state (registry, leases, merge lock).
- **The prose channel** (`<repo>-SESSION-SYNC.md`) — free-text discussion, decisions, rationale.

## 2. The problem it solves

**Collisions.** Five or six sessions editing one tree will overwrite each other's changes, produce
conflicting merges, and step on shared resources.

**Dormancy.** An AI session only acts when its harness gives it a turn. After a turn it goes
dormant until something wakes it. A naive background loop (`& while true …`) does **not** wake a
Claude session — its output goes nowhere. This is the single biggest reason sessions "go dark."
Concord addresses it with two mechanisms (see §9):

- a **persistent watcher** on the shared channel that turns each change into a wake event, and
- a **self-tick** (a timer) as a reliability net, because a timer cannot be "missed" while an event
  can.

## 3. Roles and topology

### Sessions and worktrees

Every session runs in **its own git worktree**, named by the convention `<repo>-<id>`:

```
/Projects/your-repo        ← canonical main checkout + launch root (no session lives here)
/Projects/your-repo-a      ← session a
/Projects/your-repo-b      ← session b
/Projects/your-repo-hub    ← the coordinator (hub)
```

The session id is a short, stable name (`a`, `b`, `c`, …); one of them is the coordinator (by
default `hub`). Worktrees keep edits isolated; a session can read and modify its own terrain
without touching another's checkout.

### The coordinator (`hub`)

One session is the **coordinator/steward**. It owns no code terrain. It:

- sequences work along the critical path,
- assigns tasks and arbitrates ownership disputes,
- holds the (delegated) authority to commit/merge, and runs all merges through the merge lock,
- keeps the work aligned with the project's direction.

### The command chain

Tasks, priorities, and ordering flow **human → hub → sessions**. This is deliberate:

- **No peer-to-peer task assignment.** No worker tells another what to do or reorders its
  priorities. If you need something from another session, it goes *through hub*.
- **Allowed peer collaboration:** negotiating interfaces/contracts (a shared API, a wire format),
  sharing information, asking questions, handing off findings. That is collaboration, not command.
- **Ownership disputes → hub** decides.

## 4. Identity

A session's **id** (e.g. `a`, `hub`) comes from the **worktree** it runs in. `concord start <id>`
runs the session in the `<repo>-<id>` worktree and writes an id-bind marker so the hooks know which
session they are; with no marker the hooks fall back to the worktree-name convention (`<repo>-<id>`
→ the `<id>` suffix). No environment variable is needed.

> *Deprecated:* a legacy `CONCORD_ID` environment variable is still honored with a deprecation
> warning for one release, then removed.

## 5. The lifecycle

### Register (once, at start)

```bash
concord register <id> "<focus>"
```

Writes the session's focus and timestamps into the registry. With the hooks installed, this happens
automatically on session start.

### Heartbeat (while you hold a lease)

```bash
concord heartbeat <id>
```

Only `register` and `heartbeat` refresh your timestamp (`claim`/`log`/`status` do **not**). Without
a heartbeat for the TTL (default 30 min) your session is considered **stale** and your leases become
reclaimable by others — deliberate crash recovery, so a dead session never blocks anything forever.
Heartbeat **in your turn rhythm** while holding a lease (this couples "alive" to real activity and
ends automatically when the session ends). Do **not** run a detached `nohup … sleep` loop: it would
survive a crash and keep dead leases fresh forever, defeating stale-reclaim. If you hold no lease,
you need no heartbeat.

### Leases (before editing a shared region)

```bash
concord claim <id> <path-or-area> "why"     # acquire
concord release <id> <path-or-area>          # release when done
```

Shared regions worth a lease: a build-critical file many touch, a shared library, a specific daemon,
a doc several might edit. On `CONFLICT`, do **not** edit anyway — coordinate first (check `status`,
post to the prose channel, ask hub). A lease is a cooperative claim, not a hard mutex, but with the
mandate in `CLAUDE.md` it reliably stops two sessions colliding.

### Merge lock (before merging to main)

```bash
concord merge-lock <id> "merge #NN"  &&  <do the merge>  &&  concord merge-unlock <id>
```

Only one session merges at a time. Merges run through hub.

### Stale reclaim

A session with no heartbeat past the TTL is stale; its leases and merge lock can be reclaimed. This
is automatic and is what keeps a crashed session from wedging the team.

## 6. The dispatch handshake (READY / GO)

To keep startup orderly, a freshly launched session does **not** grab work immediately. Instead:

1. The session registers, sets up its self-tick, and posts `### <id> → hub (READY: <terrain>, waiting for GO)`.
2. It **holds** — heartbeating only — until the coordinator posts `### hub → <id> (GO: <task>)`.
3. On `GO`, it takes the assigned work.

This gives the coordinator deliberate control over *when* each session starts and *what* it picks,
instead of every session racing to grab work at launch.

## 7. Communication rules

- **The prose channel** is `<repo>-SESSION-SYNC.md` (one shared file, absolute path). Each post is
  one `### <id> → <target> (<topic>)` block, kept short and concrete.
- **`concord sync`** posts to the channel from a sandboxed session that cannot append to a file
  outside its working directory (the CLI is allow-listed):
  ```bash
  concord sync <id> <target> "<topic>" "<body>"
  ```
- **No blocking decision forms in worker sessions.** A worker must never use an interactive
  decision dialog — nobody is watching that window, so it blocks invisibly. Instead, post the
  decision *with options + your recommendation* as `### <id> → hub (DECISION: <topic>)` and continue
  other unblocked work. Only the coordinator uses interactive forms.
- **No silent idling.** A worker either holds a lease and works, or has an open status/decision post
  to hub. Surface "done / blocked / idle" visibly: `### <id> → hub (DONE | BLOCKED | IDLE: <what/why>)`.
- **Acknowledge assignments (ACK).** Reply to a coordinator directive with `### <id> → hub (ACK: …)`.

## 8. Strategic guard-rails

Concord is content-agnostic, but the projects it coordinates usually have load-bearing invariants.
Two principles travel with the coordinator role:

- **Load-bearing invariants are off-limits to shortcuts.** Don't undermine the guarantees the
  project exists to enforce. When in doubt, take the hard, faithful path.
- **Stop-gaps are allowed — but only visibly and with a clean-up path.** Mark a quick hack in code
  (`// HACK(<id> <date>): <why provisional> — CLEANUP: <condition>`) and record it findably (a
  backlog entry or `concord log`), so it never quietly becomes permanent.

## 9. Staying self-driven (avoiding dormancy)

Two mechanisms, both needed:

**(a) A persistent watcher** over the *one* shared channel, using the harness's monitor primitive
(not a bare `while true`, whose stdout vanishes). It turns each change of the channel into a wake
event. On macOS, watch the file's mtime and emit a line on change.

**(b) A self-tick** (cron or a `/loop`, ~10–15 min) as a reliability net: a timer cannot be
"missed," an event can. Each tick: heartbeat (if holding a lease), pull new `### … → <id>`
directives from the channel, and continue assigned work *or* post status.

## 10. CLI reference

Everything is the one `concord` binary (plus the `concordd` daemon and `concord-mcp` server,
which run in the background). There is no separate shell tool.

**Launching + overview (you, the human):**

| Command | Effect |
|---|---|
| `concord init [--ids a,b,…] [--with-hooks]` | Scaffold a project's coordination state (and, with `--with-hooks`, wire the editor hooks). |
| `concord start <id>` | Launch session `<id>` **in the current terminal**: cd to its worktree, write an id-bind marker, exec the AI CLI with full permissions and a kickoff prompt. The session reports READY and waits for GO. |
| `concord dash` | Live overview: active sessions (focus · lease · heartbeat age · last prose post). |
| `concord status` | Registry + leases + resources + escalations + telemetry health. |
| `concord pause <id>` / `resume <id>` | Set/clear a pause flag the session's tick respects. |
| `concord stop <id>` | Signal a session to stop. |
| `concord install-hooks` | Install the editor hooks into Claude Code's settings (also done by `init --with-hooks`). |

`concord start` runs the session in the terminal you invoke it from — no new window, terminal-agnostic.

**Coordination verbs (the sessions):**

`register` · `heartbeat` · `status` · `claim` / `release` (path **or** `<file>:<symbol>`; resources
via `--kind resource [--slots N]`) · `verify` · `check-lease` · `merge-lock` / `merge-unlock` ·
`ack` · `escalate` / `resolve` / `escalations` · `contract` / `contract-check` / `contracts` /
`contract-release` · `log <id> <event…>` · `sync <id> <target> "<topic>" "<body>"`. See §5–§7 for usage.

## 11. Configuration

All tunables live in **one `config.toml`**. A fully-commented template ships with every release
(under `config/`) as **`config/config.toml.example`** — every key is shown at its built-in default.
`concord init` also drops this file into the coordination dir for you.

### Where to put it

| Location | Scope | Precedence |
|---|---|---|
| `<repo>-coord/config.toml` | this project only | highest |
| `~/.config/concord/config.toml` (or `$XDG_CONFIG_HOME/concord/config.toml`) | all projects (user-global) | middle |
| *(built-in defaults)* | — | lowest |

Effective config = **built-in defaults ← user-global ← project** (the more specific layer
wins, per key). To start from the template:

```bash
cp config/config.toml.example <repo>-coord/config.toml   # from an unpacked release, then edit
```

### Settings

| Section | Key | Meaning | Default |
|---|---|---|---|
| `[leases]` | `stale_ttl` | Seconds with no heartbeat before a session is stale | `1800` |
| | `overlap_policy` | `"reject"` (block overlapping claims) or `"shell"` (exact-slug only) | `"reject"` |
| | `strict` | Deny edits to files you have not leased (capability-strict) | `false` |
| `[daemon]` | `enabled` | Route consequential ops through the daemon when it is up | `true` |
| `[launcher]` | `claude_flags` | Flags passed to `claude` at launch | `"--dangerously-skip-permissions"` |
| | `worktree_pattern` | Per-session worktree naming | `"{repo}-{id}"` |
| `[escalation]` | `coordinator` | Session that receives escalations + the coordinator role | `"hub"` |
| | `ack_ttl` | Seconds before an un-ACK'd directive is re-delivered | `900` |
| | `redeliver_max` | Re-deliveries before an escalation is auto-raised | `2` |
| | `auto_severity` | Severity of an auto-raised escalation | `"high"` |
| `[resources]` | `port_base` | A `qemu-port` pool hands out `port_base + slot` | `5900` |
| | `default_slots` | Default capacity when `--slots` is omitted | `1` |
| `[telemetry]` | `enabled` | Consume Claude Code's local OTel stream + run the receiver | `false` |
| | `port` | Local OTLP/HTTP-JSON receiver port | `4319` |
| | `idle_min` | Minutes with no telemetry before a session is IDLE | `15` |
| | `burn_warn` | Tokens/minute above which a session is BURN | `20000` |
| | `reject_storm` | Edit-tool reject/deny decisions in `loop_window` that flag REJECT | `5` |
| | `loop_window` | Look-back window (seconds) for burn/reject/loop | `600` |

The user-global file may also carry a `[projects]` map (project root → coord dir) for
projects whose coord dir is not the conventional `<repo>-coord` sibling.

### Bootstrap (the two values config cannot define)

A config file lives *inside* a coordination dir, so two values are resolved before it is
read — by **convention**, with flags to override:

- **Coordination dir / channel** — the `<repo>-coord/` and `<repo>-SESSION-SYNC.md` siblings
  of the git toplevel; override with `--coord <dir>` or the user-global `[projects]` map.
- **Session id** — the worktree name `<repo>-<id>`; override with `--id <id>`. `concord start`
  also writes an id-bind marker so the hooks need no environment variable.

> **Deprecated:** the old `CONCORD_*` / `AIS_*` environment variables are retired. A still-set
> one is honored for one release **with a deprecation warning**, then removed; use `config.toml`,
> the convention, or `--coord`/`--id`/`--project` instead.

## 12. The automation layer (Claude Code hooks)

Installing the hooks removes the discipline burden from the sessions:

| Hook | Effect |
|---|---|
| **statusline** | Shows `● <id>` (window identity), resolved from the id-bind marker / worktree convention. |
| **SessionStart** | Auto-`register` + initial heartbeat; injects the session's standing instructions. |
| **PostToolUse / UserPromptSubmit** | Injects new `### … → <id>` directives from the channel so broadcasts are never missed. |
| **PreToolUse** | Lease / merge guard — blocks an edit to a file another active session has leased (default-allow otherwise). |
| **Stop / PreCompact / SessionEnd** | Keep a session from going silent with an un-ACK'd directive, preserve protocol state across compaction, and auto-release leases on a clean exit. |

`concord install-hooks` backs up and merges the hook configuration into Claude Code's settings
(it is also run by `concord init --with-hooks`); the scripts ship inside the binary, so no repo
checkout is needed.

## 13. Setup

1. **Install the binary** — the `curl … | sh` installer, `cargo install`, or a prebuilt release
   (see the README "Install" section).
2. **Initialise the project** — from the repo, `concord init --with-hooks` (scaffolds the
   coordination state and wires the editor hooks into Claude Code's settings).
3. **Create one worktree per session**, named `<repo>-<id>` (e.g. `<repo>-a`, `<repo>-hub`) — see
   the README quick start.
4. **Add the standing-instructions block** (see §14) to the repo's `CLAUDE.md`, so every session is
   wired into Concord automatically.
5. **Launch** — `concord start <id>` per terminal tab.

To change any setting, drop a `config.toml` in `<repo>-coord/` (or the user-global location) — see
§11. Concord resolves the coordination dir for each project by convention (`<repo>-coord` next to
the repo); no per-project environment is needed.

## 14. Ready-made prompts

### Standing instructions (add to the coordinated repo's `CLAUDE.md`)

> Multiple sessions work this repo in parallel via **Concord**. At session start: pick a stable id
> and `concord register <id> "<focus>"`. Become self-driven (a persistent channel watcher **and** a
> ~10–15 min self-tick) so you don't go dormant. Before editing a shared region, `concord claim`.
> Before merging to `main`, take the singleton `concord merge-lock`. Heartbeat in your turn rhythm
> while you hold a lease; `release` when done. Tasks flow human → hub → sessions: no peer task
> assignment, no interactive decision forms in worker sessions (post `### <id> → hub (DECISION: …)`
> instead). Never idle silently — hold a lease and work, or post status to hub.

### Self-tick (worker)

> Concord worker tick: 1) `concord heartbeat <id>`. 2) If you have no `### hub → <id> (GO: …)` yet,
> hold READY (heartbeat only). 3) After GO, read new `### … → <id>` directives and continue the
> task; raise real design forks as `### <id> → hub (DESIGN: …)`. 4) Lease before shared edits; merge
> only via hub. 5) Post via the channel (or `concord sync` if sandbox-blocked). 6) Surface
> done/blocked to hub. Never idle silently; no interactive decision forms.

## 15. Lessons learned

- **Identity comes from the worktree** — `concord start` writes an id-bind marker and the
  `<repo>-<id>` worktree name carries the id, so no ambient environment variable is needed (and no
  two sessions look alike).
- **Run sessions in the current terminal**, not a spawned window — simpler, terminal-agnostic, and
  it puts each session in the tab you chose.
- **The prose channel grows monotonically and every session reads it** — this is the main token
  cost, and the motivation for per-recipient inboxes (a planned enhancement).
- **A bare background `while true` does not wake an AI session.** Use the harness monitor + a timer.
- **Worktrees must follow the `<repo>-<id>` convention** so the CLI can derive paths without a map.
