# Frozen shell tools (legacy)

These are the **original shell implementations**, frozen here as a parity fallback after
the Rust migration completed (WP12 S1):

- `coord.sh` — the coordination CLI (the 9 verbs). Superseded by the Rust `concord`
  binary (same verbs, same on-disk format, parity-proven; see `tests/parity-harness.sh`,
  which still runs the Rust CLI against this frozen `coord.sh`).
- `concord` — the launcher (`start/dash/pause/resume/stop`). Superseded by the same
  Rust binary's subcommands (`concord start …`).

They are kept (not deleted) because the on-disk format is identical, so the shell remains
an instant fallback for any coordination state (`scripts/wp6-ais-cutover.sh --rollback`
restores a project to these). New work uses the Rust binary.
