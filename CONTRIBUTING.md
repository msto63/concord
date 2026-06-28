# Contributing to Concord

Thanks for your interest! Concord is intentionally small: plain POSIX shell over the local
filesystem, plus Claude Code hooks. Keep it that way.

## Principles

- **No server, no database, no `jq`.** State is files. Reliability comes from simplicity.
- **Project-agnostic.** Never hardcode a path. Read `CONCORD_DIR` / `CONCORD_SYNC` /
  `CONCORD_PROJECT` from the environment, else derive `<repo>-coord` / `<repo>-SESSION-SYNC.md`
  by convention.
- **Docs in English, kept in sync.** A behavioural change updates `README.md`, `docs/MANUAL.md`,
  and — if it shifts direction — `docs/ROADMAP.md` + `docs/BACKLOG.md`.
- **Dogfood.** Where practical, coordinate Concord's own development with Concord.

## Workflow

1. Pick (or file) a task in [docs/BACKLOG.md](docs/BACKLOG.md).
2. Branch, make the change, run `bash -n` on any script you touch.
3. Update the relevant docs in the same change.
4. Open a PR describing what changed and which backlog item it advances.

## Code style

- `#!/usr/bin/env bash`, `set -euo pipefail` in executables.
- Keep functions small; prefer plain loops over clever pipelines where it aids portability (macOS
  `bash` is old; avoid GNU-only flags).
- Comment the *why*, not the *what*.

## Scope

Bug fixes, portability improvements, documentation, and roadmap items (see
[docs/ROADMAP.md](docs/ROADMAP.md)) are all welcome. For larger changes, open an issue first so we
can agree on the approach.
