# Changelog

All notable changes to Concord are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Concord adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

`VERSION` is the single source of truth; `concord version` prints it. While Concord is
`0.y.z`, the CLI, protocol, and state layout may change between **MINOR** versions; **PATCH**
releases are backward-compatible fixes. See [CONTRIBUTING](CONTRIBUTING.md#release-discipline)
for the enforced release process.

## [Unreleased]
### Changed
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

[Unreleased]: https://github.com/msto63/concord/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/msto63/concord/releases/tag/v0.1.0
