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
/Projects/your-repo-a      ← session A
/Projects/your-repo-b      ← session B
/Projects/your-repo-k      ← session K (coordinator)
```

The session id is a short, stable letter (A, B, C, …, K). Worktrees keep edits isolated; a session
can read and modify its own terrain without touching another's checkout.

### The coordinator ("K")

One session is the **coordinator/steward**. It owns no code terrain. It:

- sequences work along the critical path,
- assigns tasks and arbitrates ownership disputes,
- holds the (delegated) authority to commit/merge, and runs all merges through the merge lock,
- keeps the work aligned with the project's direction.

### The command chain

Tasks, priorities, and ordering flow **human → K → sessions**. This is deliberate:

- **No peer-to-peer task assignment.** No worker tells another what to do or reorders its
  priorities. If you need something from another session, it goes *through K*.
- **Allowed peer collaboration:** negotiating interfaces/contracts (a shared API, a wire format),
  sharing information, asking questions, handing off findings. That is collaboration, not command.
- **Ownership disputes → K** decides.

## 4. Identity

A session's identity comes from the `CONCORD_ID` environment variable, set when the session is
launched (`concord start <id>` exports it). This is robust: deriving identity from the working
directory fails when every session is launched from the same repo root. The hooks and CLIs all
read `CONCORD_ID` to know which session they are acting for.

## 5. The lifecycle

### Register (once, at start)

```bash
coord.sh register <id> "<focus>"
```

Writes the session's focus and timestamps into the registry. With the hooks installed, this happens
automatically on session start.

### Heartbeat (while you hold a lease)

```bash
coord.sh heartbeat <id>
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
coord.sh claim <id> <path-or-area> "why"     # acquire
coord.sh release <id> <path-or-area>          # release when done
```

Shared regions worth a lease: a build-critical file many touch, a shared library, a specific daemon,
a doc several might edit. On `CONFLICT`, do **not** edit anyway — coordinate first (check `status`,
post to the prose channel, ask K). A lease is a cooperative claim, not a hard mutex, but with the
mandate in `CLAUDE.md` it reliably stops two sessions colliding.

### Merge lock (before merging to main)

```bash
coord.sh merge-lock <id> "merge #NN"  &&  <do the merge>  &&  coord.sh merge-unlock <id>
```

Only one session merges at a time. Merges run through K.

### Stale reclaim

A session with no heartbeat past the TTL is stale; its leases and merge lock can be reclaimed. This
is automatic and is what keeps a crashed session from wedging the team.

## 6. The dispatch handshake (READY / GO)

To keep startup orderly, a freshly launched session does **not** grab work immediately. Instead:

1. The session registers, sets up its self-tick, and posts `### <id> → K (READY: <terrain>, waiting for GO)`.
2. It **holds** — heartbeating only — until the coordinator posts `### K → <id> (GO: <task>)`.
3. On `GO`, it takes the assigned work.

This gives the coordinator deliberate control over *when* each session starts and *what* it picks,
instead of every session racing to grab work at launch.

## 7. Communication rules

- **The prose channel** is `<repo>-SESSION-SYNC.md` (one shared file, absolute path). Each post is
  one `### <id> → <target> (<topic>)` block, kept short and concrete.
- **`coord.sh sync`** posts to the channel from a sandboxed session that cannot append to a file
  outside its working directory (the CLI is allow-listed):
  ```bash
  coord.sh sync <id> <target> "<topic>" "<body>"
  ```
- **No blocking decision forms in worker sessions.** A worker must never use an interactive
  decision dialog — nobody is watching that window, so it blocks invisibly. Instead, post the
  decision *with options + your recommendation* as `### <id> → K (DECISION: <topic>)` and continue
  other unblocked work. Only the coordinator uses interactive forms.
- **No silent idling.** A worker either holds a lease and works, or has an open status/decision post
  to K. Surface "done / blocked / idle" visibly: `### <id> → K (DONE | BLOCKED | IDLE: <what/why>)`.
- **Acknowledge assignments (ACK).** Reply to a coordinator directive with `### <id> → K (ACK: …)`.

## 8. Strategic guard-rails

Concord is content-agnostic, but the projects it coordinates usually have load-bearing invariants.
Two principles travel with the coordinator role:

- **Load-bearing invariants are off-limits to shortcuts.** Don't undermine the guarantees the
  project exists to enforce. When in doubt, take the hard, faithful path.
- **Stop-gaps are allowed — but only visibly and with a clean-up path.** Mark a quick hack in code
  (`// HACK(<id> <date>): <why provisional> — CLEANUP: <condition>`) and record it findably (a
  backlog entry or `coord.sh log`), so it never quietly becomes permanent.

## 9. Staying self-driven (avoiding dormancy)

Two mechanisms, both needed:

**(a) A persistent watcher** over the *one* shared channel, using the harness's monitor primitive
(not a bare `while true`, whose stdout vanishes). It turns each change of the channel into a wake
event. On macOS, watch the file's mtime and emit a line on change.

**(b) A self-tick** (cron or a `/loop`, ~10–15 min) as a reliability net: a timer cannot be
"missed," an event can. Each tick: heartbeat (if holding a lease), pull new `### … → <id>`
directives from the channel, and continue assigned work *or* post status.

## 10. CLI reference

### `concord` — mission control (humans)

| Command | Effect |
|---|---|
| `concord start <id>` | Launch session `<id>` **in the current terminal**: cd to its worktree, export `CONCORD_ID`, exec the AI CLI with full permissions and a kickoff prompt. The session reports READY and waits for GO. |
| `concord dash` | Live overview: active sessions (focus · lease · heartbeat age · last prose post). |
| `concord status` | Raw registry status. |
| `concord pause <id>` / `resume <id>` | Set/clear a pause flag the session's tick respects. |
| `concord stop <id>` | Signal a session to stop. |

`concord start` runs the session in the terminal you invoke it from — no new window, terminal-agnostic.
Open one tab per session and run `concord start <id>` in each.

### `coord.sh` — coordination state (sessions)

`register` · `heartbeat` · `status` · `claim` · `release` · `merge-lock` · `merge-unlock` ·
`log <id> <event…>` · `sync <id> <target> "<topic>" "<body>"`. See §5–§7 for usage.

## 11. Configuration

All tunables live in **one `config.toml`**. A fully-commented template ships with every
release (and is in the repo root) as **`config.toml.example`** — every key is shown at its
built-in default. `concord init` also drops this file into the coordination dir for you.

### Where to put it

| Location | Scope | Precedence |
|---|---|---|
| `<repo>-coord/config.toml` | this project only | highest |
| `~/.config/concord/config.toml` (or `$XDG_CONFIG_HOME/concord/config.toml`) | all projects (user-global) | middle |
| *(built-in defaults)* | — | lowest |

Effective config = **built-in defaults ← user-global ← project** (the more specific layer
wins, per key). To start from the template:

```bash
cp config.toml.example <repo>-coord/config.toml   # from an unpacked release, then edit
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
| **statusline** | Shows `● <id>` (window identity) using `CONCORD_ID`. |
| **SessionStart** | Auto-`register` + initial heartbeat; injects the session's standing instructions. |
| **PostToolUse / UserPromptSubmit** | Injects new `### … → <id>` directives from the channel so broadcasts are never missed. |
| **PreToolUse** | Lease / merge guard — warns or blocks on a certain collision with another active session (default-allow otherwise). |

`hooks/install.sh` backs up and merges the hook configuration into the Claude Code settings;
`hooks/uninstall.sh` reverts it.

## 13. Setup

### A new project

1. Symlink `bin/concord` onto your `PATH`.
2. Run `hooks/install.sh`.
3. Create worktrees `<repo>-a … <repo>-k` (see README quick start).
4. Add the **standing instructions block** (see §14) to the repo's `CLAUDE.md` so every session is
   wired into Concord automatically.
5. From the repo root, `concord start <id>` per terminal tab.

### An existing project already using a copy

Repoint the global hooks at this repo's `hooks/`, launch the project's sessions with that project's
`CONCORD_DIR` / `CONCORD_SYNC` (or let `concord` derive them from the repo root), and symlink the
project's `coord.sh` to `bin/coord.sh` so there is a single source of truth.

## 14. Ready-made prompts

### Standing instructions (add to the coordinated repo's `CLAUDE.md`)

> Multiple sessions work this repo in parallel via **Concord**. At session start: pick a stable id
> and `coord.sh register <id> "<focus>"`. Become self-driven (a persistent channel watcher **and** a
> ~10–15 min self-tick) so you don't go dormant. Before editing a shared region, `coord.sh claim`.
> Before merging to `main`, take the singleton `coord.sh merge-lock`. Heartbeat in your turn rhythm
> while you hold a lease; `release` when done. Tasks flow human → K → sessions: no peer task
> assignment, no interactive decision forms in worker sessions (post `### <id> → K (DECISION: …)`
> instead). Never idle silently — hold a lease and work, or post status to K.

### Self-tick (worker)

> Concord worker tick: 1) `coord.sh heartbeat <id>`. 2) If you have no `### K → <id> (GO: …)` yet,
> hold READY (heartbeat only). 3) After GO, read new `### … → <id>` directives and continue the
> task; raise real design forks as `### <id> → K (DESIGN: …)`. 4) Lease before shared edits; merge
> only via K. 5) Post via the channel (or `coord.sh sync` if sandbox-blocked). 6) Surface
> done/blocked to K. Never idle silently; no interactive decision forms.

## 15. Lessons learned

- **Identity must come from `CONCORD_ID`, not the working directory** — all sessions launch from the
  same root, so cwd-based identity makes every session look like the same one.
- **Run sessions in the current terminal**, not a spawned window — simpler, terminal-agnostic, and
  it puts each session in the tab you chose.
- **The prose channel grows monotonically and every session reads it** — this is the main token
  cost, and the motivation for per-recipient inboxes (a planned enhancement).
- **A bare background `while true` does not wake an AI session.** Use the harness monitor + a timer.
- **Worktrees must follow the `<repo>-<id>` convention** so the CLI can derive paths without a map.
