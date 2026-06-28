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
- **Human-director role is now name-abstract (`the operator`).** The coordinator kickoff and
  self-tick prompts (and README/guide/backlog) refer to the human who directs the fleet as
  "the operator" instead of a hardcoded personal name — keeping Concord's prompts identity-neutral
  and portable across projects. The `operator → coordinator → workers` delegation chain is unchanged;
  only the label is. The MIT copyright holder in `LICENSE`/`README` is intentionally left as-is
  (legal attribution, not an operational role).

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
