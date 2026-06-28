#!/usr/bin/env bash
# Concord WP12 — differential parity harness.
#
# Drives the SAME command sequence through both the shell coordinator (bin/coord.sh)
# and the Rust binary (target/release/concord), each against its own coordination
# dir, then asserts they produced byte-identical results — both on stdout and in the
# on-disk state — after normalizing the unavoidable wall-clock differences
# (timestamps, absolute coord/sync paths).
#
# This is the M1 acceptance gate (WP12 §4 "bit-gleichen Zustand für dieselbe
# Befehlsfolge") and is meant to run in CI / the pre-push hook.
#
# Usage:  tests/parity-harness.sh           # builds release, runs, diffs
#         CONCORD_BIN=/path/to/concord tests/parity-harness.sh   # use a prebuilt binary
set -euo pipefail

HERE="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SHELL_COORD_SH="$HERE/bin/coord.sh"
RUST_BIN="${CONCORD_BIN:-$HERE/target/release/concord}"

if [ ! -x "$RUST_BIN" ]; then
  echo "building release binary…"
  ( cd "$HERE" && cargo build --release -q )
fi
[ -x "$RUST_BIN" ] || { echo "FATAL: rust binary not found at $RUST_BIN"; exit 1; }
[ -x "$SHELL_COORD_SH" ] || { echo "FATAL: shell coord.sh not found at $SHELL_COORD_SH"; exit 1; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/concord-parity.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
S_DIR="$WORK/shell-coord"; S_SYNC="$WORK/shell-SYNC.md"
R_DIR="$WORK/rust-coord";  R_SYNC="$WORK/rust-SYNC.md"

# Normalize the two runs into a comparable canonical form: collapse 10-digit Unix
# timestamps, and replace each run's own coord-dir and sync-file paths with tokens.
norm() {  # <coord-dir> <sync-file>
  sed -e "s#$1#<COORD>#g" -e "s#$2#<SYNC>#g" -e 's/[0-9]\{10\}/<TS>/g'
}

run_shell() { CONCORD_DIR="$S_DIR" CONCORD_SYNC="$S_SYNC" bash "$SHELL_COORD_SH" "$@"; }
run_rust()  { CONCORD_DIR="$R_DIR" CONCORD_SYNC="$R_SYNC" "$RUST_BIN" "$@"; }

fail=0
check_cmd() {  # "<label>" <args...>
  local label="$1"; shift
  local so ro sc rc
  # `&& sc=0 || sc=$?` keeps a non-zero exit (e.g. CONFLICT=2) from tripping `set -e`.
  so="$(run_shell "$@" 2>&1)" && sc=0 || sc=$?
  ro="$(run_rust  "$@" 2>&1)" && rc=0 || rc=$?
  local son ron
  son="$(printf '%s\n' "$so" | norm "$S_DIR" "$S_SYNC")"
  ron="$(printf '%s\n' "$ro" | norm "$R_DIR" "$R_SYNC")"
  if [ "$son" != "$ron" ]; then
    echo "✗ STDOUT MISMATCH: $label"
    diff <(printf '%s\n' "$son") <(printf '%s\n' "$ron") | sed 's/^/    /'
    fail=1
  elif [ "$sc" != "$rc" ]; then
    echo "✗ EXIT MISMATCH: $label  (shell=$sc rust=$rc)"
    fail=1
  else
    echo "✓ $label  (exit $sc)"
  fi
}

# Dump a coord tree to a normalized "path:<newline>content" stream for comparison.
dump_tree() {  # <coord-dir> <sync-file>
  local d="$1" sync="$2"
  ( cd "$d" && find . -type f | LC_ALL=C sort | while read -r f; do
      printf '=== %s ===\n' "$f"
      cat "$f"
      printf '\n'
    done ) | norm "$d" "$sync"
  if [ -f "$sync" ]; then
    printf '=== SESSION-SYNC ===\n'
    norm "$d" "$sync" < "$sync"
  fi
}

echo "── command-sequence parity ──"
check_cmd "register a"        register a "B15.3 baseline"
check_cmd "register hub"      register hub "coordinator"
check_cmd "heartbeat a"       heartbeat a
check_cmd "claim a area1"     claim a kernel/src/main.rs "edit main"
check_cmd "claim hub area1 (conflict)"  claim hub kernel/src/main.rs "want it"
check_cmd "claim a area1 (already yours)" claim a kernel/src/main.rs "again"
check_cmd "claim hub area2"   claim hub user/usbd "usb work"
check_cmd "status"            status
check_cmd "log a"             log a "decided to do X with care"
check_cmd "merge-lock hub"    merge-lock hub "merge #1"
check_cmd "merge-lock a (held)" merge-lock a "want merge"
check_cmd "merge-unlock hub"  merge-unlock hub
check_cmd "release a area1"   release a kernel/src/main.rs
check_cmd "release a area1 (no lease)" release a kernel/src/main.rs
check_cmd "sync a"            sync a hub "STATUS" "made progress on the thing"
check_cmd "status (final)"    status

echo ""
echo "── on-disk state parity ──"
if diff <(dump_tree "$S_DIR" "$S_SYNC") <(dump_tree "$R_DIR" "$R_SYNC") > "$WORK/tree.diff"; then
  echo "✓ on-disk trees identical (normalized)"
else
  echo "✗ ON-DISK MISMATCH:"
  sed 's/^/    /' "$WORK/tree.diff"
  fail=1
fi

echo ""
if [ "$fail" = 0 ]; then echo "PARITY: ALL PASS"; else echo "PARITY: FAILURES (see above)"; fi
exit $fail
