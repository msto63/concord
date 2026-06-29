#!/usr/bin/env bash
# WP6 — cut the LIVE ais coordination over from the shell coord.sh to the Rust
# `concord` binary, reversibly. Both tools share the same on-disk format (parity-
# proven), so the cutover repoints WHICH cli the hooks resolve and the rollback is a
# single restore — no state migration, fully reversible.
#
# ⚠️  MUTATES the live ais coordination substrate. OPERATOR-RUN, jointly with the
#     coordinator, NOT unilaterally. Default mode is --dry-run (no changes).
#
# CUTOVER POINT (coordinator-arbitrated): the shared, worktree-independent deployed
#   hook library  <coord>/hooks/lib.sh , whose COORD_SH resolution all sessions' hooks
#   use. `--via hook-lib` (default) appends a reversible override making COORD_SH the
#   Rust binary for EVERY session at once. `--via coord-cli` is the fallback: shim a
#   single worktree's tools/coord.sh (covers only that worktree).
#
# Modes:   --dry-run (default) | --apply | --rollback | --verify
# Options: --via hook-lib|coord-cli  --project <ais repo>  --coord <dir>
#          --bin <rust binary>  --install <stable path for the rust binary>
set -euo pipefail

PROJECT="${CONCORD_PROJECT:-$HOME/Projects/ais}"
CONCORD_SRC="${CONCORD_SRC:-$HOME/Projects/concord}"
COORD=""; RUST_BIN=""; INSTALL=""; COORD_CLI=""
VIA="hook-lib"
MODE="--dry-run"

while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run|--apply|--rollback|--verify) MODE="$1" ;;
    --via)      VIA="$2"; shift ;;
    --project)  PROJECT="$2"; shift ;;
    --coord)    COORD="$2"; shift ;;
    --coord-cli) COORD_CLI="$2"; shift ;;
    --bin)      RUST_BIN="$2"; shift ;;
    --install)  INSTALL="$2"; shift ;;
    *) echo "unknown arg: $1"; exit 2 ;;
  esac
  shift
done

# Coord dir is derived STRICTLY from --project (NOT the ambient $CONCORD_DIR — that env
# leak twice touched the live ais-coord; this script is project-targeted). --coord overrides.
: "${COORD:=$(dirname "$PROJECT")/$(basename "$PROJECT")-coord}"
: "${RUST_BIN:=$CONCORD_SRC/target/release/concord}"
: "${INSTALL:=$COORD/concord-rs}"
: "${COORD_CLI:=$PROJECT/tools/coord.sh}"
LIB="$COORD/hooks/lib.sh"
MARK="# --- WP6 cutover override (reversible) ---"

# The cutover TARGET file (what gets backed up / restored) depends on --via.
case "$VIA" in
  hook-lib)  TARGET="$LIB" ;;
  coord-cli) TARGET="$COORD_CLI" ;;
  *) echo "FATAL: --via must be hook-lib or coord-cli"; exit 2 ;;
esac
BAK="$TARGET.shell-bak"

say() { printf '  %s\n' "$*"; }
hr()  { printf -- '── %s ──\n' "$*"; }

plan() {
  hr "WP6 ais-cutover plan ($MODE · via $VIA)"
  say "project:        $PROJECT"
  say "coord dir:      $COORD"
  say "cutover target: $TARGET"
  say "rust binary:    $RUST_BIN  →  staged at $INSTALL"
  say "backup:         $BAK"
}

build_and_stage() {
  [ -x "$RUST_BIN" ] || { say "building release binary…"; ( cd "$CONCORD_SRC" && cargo build --release -q ); }
  [ -x "$RUST_BIN" ] || { echo "FATAL: rust binary not found/built at $RUST_BIN"; exit 1; }
  mkdir -p "$(dirname "$INSTALL")"
  install -m 0755 "$RUST_BIN" "$INSTALL"
  say "staged rust binary → $INSTALL ($("$INSTALL" version 2>/dev/null || echo '?'))"
}

apply_hook_lib() {
  [ -f "$LIB" ] || { echo "FATAL: deployed hook lib not found at $LIB"; exit 1; }
  grep -qF "$MARK" "$LIB" && { say "override already present in lib.sh (idempotent)"; return; }
  # Appended at EOF, so it runs after the existing COORD_SH resolution (lib.sh is
  # SOURCED) and wins — making COORD_SH the Rust binary for every session that sources it.
  { echo ""; echo "$MARK"; echo "[ -x \"$INSTALL\" ] && COORD_SH=\"$INSTALL\""; } >> "$LIB"
  say "appended COORD_SH→Rust override to $LIB"
}
apply_coord_cli() {
  cat > "$COORD_CLI" <<SHIM
#!/bin/sh
$MARK  exec the Rust concord binary (drop-in). Rollback: restore from $BAK.
exec "$INSTALL" "\$@"
SHIM
  chmod 0755 "$COORD_CLI"; say "shimmed $COORD_CLI → $INSTALL"
}

# What COORD_SH a hook would resolve to, after sourcing the (possibly cut-over) lib.sh.
resolved_cli() {
  if [ "$VIA" = hook-lib ]; then
    ( set +u; export CONCORD_DIR="$COORD" CONCORD_PROJECT="$PROJECT"; . "$LIB" 2>/dev/null; printf '%s' "${COORD_SH:-}" )
  else printf '%s' "$COORD_CLI"; fi
}

verify() {
  hr "verify (resolved CLI + throwaway round-trip against $COORD)"
  local cli; cli="$(resolved_cli)"; local t="wp6-verify-$$"
  [ -n "$cli" ] && [ -x "$cli" ] || { echo "✗ no executable COORD_SH resolved ($cli)"; return 1; }
  say "resolved COORD_SH = $cli"
  CONCORD_DIR="$COORD" "$cli" status >/dev/null && say "✓ status: registry readable"
  CONCORD_DIR="$COORD" "$cli" register "$t" "wp6 cutover verify" >/dev/null && say "✓ register works"
  CONCORD_DIR="$COORD" "$cli" claim "$t" "wp6/verify/area" "v" >/dev/null && say "✓ claim works"
  CONCORD_DIR="$COORD" "$cli" status 2>/dev/null | grep -q "$t" && say "✓ status reflects throwaway lease"
  CONCORD_DIR="$COORD" "$cli" release "$t" "wp6/verify/area" >/dev/null && say "✓ release works"
  rm -f "$COORD/sessions/$t"; say "✓ cleaned up throwaway session (real sessions untouched)"
  for s in hub a; do
    [ -f "$COORD/sessions/$s" ] && say "✓ session '$s' intact: focus=$(sed -n 's/^focus=//p' "$COORD/sessions/$s")"
  done
}

case "$MODE" in
  --dry-run)
    plan; hr "would do (apply via $VIA)"
    say "1. stage rust binary → $INSTALL"
    say "2. back up $TARGET → $BAK"
    [ "$VIA" = hook-lib ] && say "3. append reversible COORD_SH→Rust override to $LIB" \
                          || say "3. replace $COORD_CLI with a shim → exec $INSTALL"
    say "4. --verify"
    hr "rollback"; say "restore $BAK → $TARGET  (one step; on-disk format identical, no data migration)"
    echo; echo "DRY-RUN only — nothing changed. --apply executes (operator + coordinator together)." ;;
  --apply)
    plan; build_and_stage
    [ -f "$BAK" ] || { cp -p "$TARGET" "$BAK"; say "backed up → $BAK"; }
    [ "$VIA" = hook-lib ] && apply_hook_lib || apply_coord_cli
    verify
    echo; echo "APPLIED via $VIA. If anything looks off, roll back NOW: $0 --rollback --via $VIA --project '$PROJECT'" ;;
  --rollback)
    plan
    [ -f "$BAK" ] || { echo "FATAL: no backup at $BAK — cannot auto-rollback"; exit 1; }
    cp -p "$BAK" "$TARGET"; say "restored $TARGET from $BAK"
    verify; echo; echo "ROLLED BACK to the shell coord.sh (via $VIA)." ;;
  --verify) plan; verify ;;
esac
