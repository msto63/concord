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

## Release discipline

Concord follows [Semantic Versioning](https://semver.org). `VERSION` is the **single source of
truth**; `concord version` prints it; `CHANGELOG.md` documents every release. CI runs
`scripts/check-version.sh` on every push and PR and **fails the build** if `VERSION`, the latest
`CHANGELOG.md` entry, and `concord version` disagree — so the version can never silently drift.

**Every change that ships:**
1. Add a bullet under `## [Unreleased]` in `CHANGELOG.md`.

**Every release:**
1. Decide the bump (while `0.y.z`: MINOR may break; PATCH is fixes only).
2. Move the `[Unreleased]` bullets under a new `## [X.Y.Z] - YYYY-MM-DD` heading.
3. Write the new number into `VERSION`.
4. `bash scripts/check-version.sh` must pass.
5. Commit, then tag: `git tag vX.Y.Z && git push --tags`. CI re-checks that the tag equals
   `v$VERSION`.

Recommended local guard — wire the check into a pre-push hook:
```bash
printf '#!/usr/bin/env bash\nexec bash scripts/check-version.sh\n' > .git/hooks/pre-push
chmod +x .git/hooks/pre-push
```

## Scope

Bug fixes, portability improvements, documentation, and roadmap items (see
[docs/ROADMAP.md](docs/ROADMAP.md)) are all welcome. For larger changes, open an issue first so we
can agree on the approach.
