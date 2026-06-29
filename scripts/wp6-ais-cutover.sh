#!/usr/bin/env bash
# WP6 — cut the LIVE ais coordination CLI over from the shell coord.sh to the Rust
# `concord` binary, reversibly. Both tools share the same on-disk format (parity-
# proven), so the cutover is a single repoint of the CLI and the rollback is a single
# restore — no state migration, fully reversible.
#
# ⚠️  This MUTATES the live ais coordination substrate. It is OPERATOR-RUN, jointly with
#     the coordinator, NOT executed unilaterally. Default mode is --dry-run (no changes).
#
# Modes:
#   --dry-run   (default) print exactly what apply/rollback would do; change nothing.
#   --apply     build+stage the Rust binary, back up the shell CLI, repoint tools/coord.sh
#               to the Rust binary, then run --verify.
#   --rollback  restore the shell tools/coord.sh from the backup (one step back).
#   --verify    non-destructive-ish health check on the live coord via the CURRENT CLI:
#               status reads the registry, and a throwaway session round-trips
#               (register→status→release→deregister) WITHOUT touching real sessions.
#
# Overrides (env or flags): --project <ais repo> --coord-cli <path> --bin <rust binary>
#   --install <stable path for the rust binary>
set -euo pipefail

PROJECT="${CONCORD_PROJECT:-$HOME/Projects/ais}"
CONCORD_SRC="${CONCORD_SRC:-$HOME/Projects/concord}"
COORD=""               # coordination dir; default <repo>-coord sibling of --project
COORD_CLI=""           # the CLI the hooks/sessions call; default <project>/tools/coord.sh
RUST_BIN=""            # built release binary; default <src>/target/release/concord
INSTALL=""             # stable location to stage the rust binary; default <coord>/concord-rs
MODE="--dry-run"

while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run|--apply|--rollback|--verify) MODE="$1" ;;
    --project)  PROJECT="$2"; shift ;;
    --coord)    COORD="$2"; shift ;;
    --coord-cli) COORD_CLI="$2"; shift ;;
    --bin)      RUST_BIN="$2"; shift ;;
    --install)  INSTALL="$2"; shift ;;
    *) echo "unknown arg: $1"; exit 2 ;;
  esac
  shift
done

# Derive the coord dir STRICTLY from --project (the <repo>-coord sibling) — NOT from an
# ambient $CONCORD_DIR. (Honoring the ambient env is exactly the leak that made an
# earlier run touch the live ais-coord; this script is project-targeted, so it ignores
# the env. Use --coord to override explicitly.)
: "${COORD:=$(dirname "$PROJECT")/$(basename "$PROJECT")-coord}"
: "${COORD_CLI:=$PROJECT/tools/coord.sh}"
: "${RUST_BIN:=$CONCORD_SRC/target/release/concord}"
: "${INSTALL:=$COORD/concord-rs}"
BAK="$COORD_CLI.shell-bak"

say() { printf '  %s\n' "$*"; }
hr() { printf -- '── %s ──\n' "$*"; }

plan() {
  hr "WP6 ais-cutover plan ($MODE)"
  say "project:        $PROJECT"
  say "coord dir:      $COORD"
  say "coord CLI:      $COORD_CLI   (the file hooks + sessions invoke)"
  say "rust binary:    $RUST_BIN"
  say "stable install: $INSTALL"
  say "backup:         $BAK"
}

build_and_stage() {
  [ -x "$RUST_BIN" ] || { say "building release binary…"; ( cd "$CONCORD_SRC" && cargo build --release -q ); }
  [ -x "$RUST_BIN" ] || { echo "FATAL: rust binary not found/built at $RUST_BIN"; exit 1; }
  mkdir -p "$(dirname "$INSTALL")"
  install -m 0755 "$RUST_BIN" "$INSTALL"
  say "staged rust binary → $INSTALL ($("$INSTALL" version 2>/dev/null || echo '?'))"
}

# A thin shim so `tools/coord.sh <verb>` execs the Rust binary (drop-in: identical verbs
# + on-disk format). A shim (not a bare symlink) keeps the path explicit + greppable.
write_shim() {
  cat > "$COORD_CLI" <<SHIM
#!/bin/sh
# WP6: ais coordination CLI cut over to the Rust concord binary (reversible).
# Rollback: restore this file from $BAK  (or run wp6-ais-cutover.sh --rollback).
exec "$INSTALL" "\$@"
SHIM
  chmod 0755 "$COORD_CLI"
}

verify() {
  hr "verify (via the CURRENT $COORD_CLI)"
  local cli="$COORD_CLI" t="wp6-verify-$$"
  CONCORD_DIR="$COORD" "$cli" status >/dev/null && say "✓ status: registry readable"
  # Throwaway round-trip — never touches real sessions (unique id/area).
  CONCORD_DIR="$COORD" "$cli" register "$t" "wp6 cutover verify" >/dev/null && say "✓ register: write works"
  CONCORD_DIR="$COORD" "$cli" claim "$t" "wp6/verify/area" "verify" >/dev/null && say "✓ claim: lease works"
  CONCORD_DIR="$COORD" "$cli" status 2>/dev/null | grep -q "$t" && say "✓ status reflects the throwaway lease"
  CONCORD_DIR="$COORD" "$cli" release "$t" "wp6/verify/area" >/dev/null && say "✓ release: works"
  rm -f "$COORD/sessions/$t"; say "✓ cleaned up throwaway session"
  # Confirm the real coordinator/worker sessions are intact + readable.
  for s in hub a; do
    [ -f "$COORD/sessions/$s" ] && say "✓ session '$s' intact: focus=$(sed -n 's/^focus=//p' "$COORD/sessions/$s")"
  done
  say "↳ also eyeball: a fresh prose-channel post still appends, hub/A heartbeats update."
}

case "$MODE" in
  --dry-run)
    plan
    hr "would do (apply)"
    say "1. build/stage rust binary → $INSTALL"
    say "2. cp $COORD_CLI → $BAK   (backup the shell CLI, if not already)"
    say "3. replace $COORD_CLI with a shim → exec $INSTALL"
    say "4. run --verify"
    hr "rollback is"
    say "cp $BAK → $COORD_CLI   (one step back; on-disk format identical, no data migration)"
    echo; echo "DRY-RUN only — nothing changed. Re-run with --apply to execute (operator + coordinator together)."
    ;;
  --apply)
    plan
    build_and_stage
    [ -f "$BAK" ] || { cp -p "$COORD_CLI" "$BAK"; say "backed up shell CLI → $BAK"; }
    write_shim; say "repointed $COORD_CLI → rust binary (shim)"
    verify
    echo; echo "APPLIED. If anything looks off, roll back NOW: $0 --rollback --project '$PROJECT'"
    ;;
  --rollback)
    plan
    [ -f "$BAK" ] || { echo "FATAL: no backup at $BAK — cannot roll back automatically"; exit 1; }
    cp -p "$BAK" "$COORD_CLI"; chmod 0755 "$COORD_CLI"
    say "restored shell CLI from $BAK"
    verify
    echo; echo "ROLLED BACK to the shell coord.sh."
    ;;
  --verify)
    plan; verify ;;
esac
