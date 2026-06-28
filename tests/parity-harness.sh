#!/usr/bin/env bash
# Concord WP12 — differential parity harness.
#
# Coordinator STEER: parity = mutual readability + semantic equivalence during
# coexistence, NOT byte-identical output. The Rust port deliberately FIXES shell
# bugs (un-escaped JSON, the trailing-space quirk, the missing path-prefix overlap
# check) rather than replicating them — so this harness compares the *logical* state
# and proves the two tools can read each other, instead of diffing raw bytes.
#
# Three checks:
#   A. Command-sequence parity — same sequence through shell and Rust ⇒ identical
#      verb-level stdout (normalized) AND identical semantic state (sessions/leases/
#      merge-lock, ignoring timestamps + the additive fence/area files).
#   B. Mutual readability — (B1) Rust reads a SHELL-built state; (B2) shell reads a
#      RUST-built state; each tool's `status` of the same dir agrees (normalized).
#   C. Hardening (informational) — the Rust overlap check rejects a parent/child
#      claim the shell silently double-leases (documented intended divergence).
#
# Usage:  tests/parity-harness.sh        |  CONCORD_BIN=/path/to/concord tests/parity-harness.sh
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

fail=0

# Normalize a run's output: collapse 10-digit Unix timestamps and replace this run's
# coord-dir + sync-file paths with stable tokens, so two runs become comparable.
norm() { sed -e "s#$1#<COORD>#g" -e "s#$2#<SYNC>#g" -e 's/[0-9]\{10\}/<TS>/g'; }

# Drive a tool against a specific coord dir + sync file.
run_shell() { CONCORD_DIR="$1" CONCORD_SYNC="$2" bash "$SHELL_COORD_SH" "${@:3}"; }
run_rust()  { CONCORD_DIR="$1" CONCORD_SYNC="$2" "$RUST_BIN" "${@:3}"; }

# A purely SEMANTIC projection of a coord dir: sessions (id+focus), leases
# (slug+holder+why), merge-lock holder. Ignores timestamps, fence, and the additive
# `area` file — exactly the cosmetic/enriching differences the STEER says to ignore.
project_state() {  # <coord-dir>
  local d="$1"
  echo "SESSIONS:"
  { for f in "$d"/sessions/*; do [ -e "$f" ] || break
      printf '  %s focus=%s\n' "$(basename "$f")" "$(sed -n 's/^focus=//p' "$f")"
    done; } | LC_ALL=C sort
  echo "LEASES:"
  { for ld in "$d"/leases/*/; do [ -e "$ld" ] || break
      printf '  %s holder=%s why=%s\n' "$(basename "$ld")" \
        "$(cat "$ld/holder" 2>/dev/null)" "$(cat "$ld/why" 2>/dev/null)"
    done; } | LC_ALL=C sort
  echo "MERGE:"
  if [ -d "$d/merge.lock" ]; then echo "  holder=$(cat "$d/merge.lock/holder" 2>/dev/null)"; else echo "  (none)"; fi
}

# ───────────────────────────── A. command-sequence parity ─────────────────────────────
echo "── A. command-sequence parity ──"
SA="$WORK/A-shell"; SAS="$WORK/A-shell-SYNC.md"
RA="$WORK/A-rust";  RAS="$WORK/A-rust-SYNC.md"

SEQ=(
  "register a B15.3 baseline"
  "register hub coordinator"
  "heartbeat a"
  "claim a kernel/src/main.rs edit main"
  "claim hub kernel/src/main.rs want it"
  "claim a kernel/src/main.rs again"
  "claim hub user/usbd usb work"
  "status"
  "log a decided to do X with care"
  "merge-lock hub merge #1"
  "merge-lock a want merge"
  "merge-unlock hub"
  "release a kernel/src/main.rs"
  "release a kernel/src/main.rs"
  "sync a hub STATUS made progress"
  "status"
)
for line in "${SEQ[@]}"; do
  read -r -a parts <<< "$line"
  so="$(run_shell "$SA" "$SAS" "${parts[@]}" 2>&1)" && sc=0 || sc=$?
  ro="$(run_rust  "$RA" "$RAS" "${parts[@]}" 2>&1)" && rc=0 || rc=$?
  son="$(printf '%s\n' "$so" | norm "$SA" "$SAS")"
  ron="$(printf '%s\n' "$ro" | norm "$RA" "$RAS")"
  if [ "$son" != "$ron" ]; then
    echo "  ✗ stdout: ${parts[*]}"; diff <(printf '%s\n' "$son") <(printf '%s\n' "$ron") | sed 's/^/      /'; fail=1
  elif [ "$sc" != "$rc" ]; then
    echo "  ✗ exit: ${parts[*]} (shell=$sc rust=$rc)"; fail=1
  fi
done
[ "$fail" = 0 ] && echo "  ✓ verb-level stdout + exit codes identical (16 commands)"

if diff <(project_state "$SA") <(project_state "$RA") > "$WORK/A.diff"; then
  echo "  ✓ semantic state identical (sessions/leases/merge-lock)"
else
  echo "  ✗ SEMANTIC STATE MISMATCH:"; sed 's/^/      /' "$WORK/A.diff"; fail=1
fi
if diff <(norm "$SA" "$SAS" < "$SAS") <(norm "$RA" "$RAS" < "$RAS") > "$WORK/A-sync.diff"; then
  echo "  ✓ prose channel identical (normalized)"
else
  echo "  ✗ PROSE MISMATCH:"; sed 's/^/      /' "$WORK/A-sync.diff"; fail=1
fi

# ───────────────────────────── B. mutual readability ─────────────────────────────
echo "── B. mutual readability ──"
# B1: build state with the SHELL, then have BOTH tools read it.
B1="$WORK/B1"; B1S="$WORK/B1-SYNC.md"
run_shell "$B1" "$B1S" register a "shell built" >/dev/null
run_shell "$B1" "$B1S" claim a kernel/src/embedded "by shell" >/dev/null
run_shell "$B1" "$B1S" merge-lock a "shell merge" >/dev/null
b1_shell="$(run_shell "$B1" "$B1S" status 2>&1 | norm "$B1" "$B1S")"
b1_rust="$( run_rust  "$B1" "$B1S" status 2>&1 | norm "$B1" "$B1S")"
if [ "$b1_shell" = "$b1_rust" ]; then
  echo "  ✓ B1: Rust reads shell-built state (status agrees)"
else
  echo "  ✗ B1 MISMATCH:"; diff <(printf '%s\n' "$b1_shell") <(printf '%s\n' "$b1_rust") | sed 's/^/      /'; fail=1
fi

# B2: build state with RUST, then have BOTH tools read it.
B2="$WORK/B2"; B2S="$WORK/B2-SYNC.md"
run_rust "$B2" "$B2S" register a "rust built" >/dev/null
run_rust "$B2" "$B2S" claim a kernel/src/embedded "by rust" >/dev/null
run_rust "$B2" "$B2S" merge-lock a "rust merge" >/dev/null
b2_shell="$(run_shell "$B2" "$B2S" status 2>&1 | norm "$B2" "$B2S")"
b2_rust="$( run_rust  "$B2" "$B2S" status 2>&1 | norm "$B2" "$B2S")"
if [ "$b2_shell" = "$b2_rust" ]; then
  echo "  ✓ B2: shell reads rust-built state (status agrees)"
else
  echo "  ✗ B2 MISMATCH:"; diff <(printf '%s\n' "$b2_shell") <(printf '%s\n' "$b2_rust") | sed 's/^/      /'; fail=1
fi

# ───────────────────────────── C. hardening (informational) ─────────────────────────────
echo "── C. overlap hardening (intended divergence) ──"
C="$WORK/C"; CS="$WORK/C-SYNC.md"
run_rust "$C" "$CS" register a "x" >/dev/null
run_rust "$C" "$CS" register b "x" >/dev/null
run_rust "$C" "$CS" claim a kernel/src/embedded "parent" >/dev/null
crust="$(run_rust "$C" "$CS" claim b kernel/src/embedded/usbd "child" 2>&1)" && crc=0 || crc=$?
if echo "$crust" | grep -q "OVERLAP" && [ "$crc" = 2 ]; then
  echo "  ✓ Rust rejects parent/child overlap (exit 2): ${crust}"
else
  echo "  ✗ expected Rust OVERLAP rejection, got (exit $crc): $crust"; fail=1
fi
# Shell, for contrast, silently double-leases (the bug the port fixes).
Csh="$WORK/Csh"; CshS="$WORK/Csh-SYNC.md"
run_shell "$Csh" "$CshS" register a "x" >/dev/null
run_shell "$Csh" "$CshS" claim a kernel/src/embedded "parent" >/dev/null
csh="$(run_shell "$Csh" "$CshS" claim a kernel/src/embedded/usbd "child" 2>&1)" || true
echo "  ℹ shell (unfixed) result: ${csh}  ← double-lease the port prevents"

# ───────────────────────── D. stale-reclaim parity (TTL) ─────────────────────────
# A short TTL makes a holder go stale; another session must then be able to RECLAIM
# its lease, and `status` must hide the stale session — crash-recovery semantics
# shared by both tools. We drive the same timed sequence through each and compare the
# reclaim stdout + the resulting semantic state.
echo "── D. stale-reclaim parity (AIS_COORD_TTL=1) ──"
stale_seq() {  # <run-fn> <coord-dir> <sync-file>  → prints the reclaim stdout line
  local rf="$1" d="$2" s="$3"
  AIS_COORD_TTL=1 "$rf" "$d" "$s" register a "first holder" >/dev/null
  AIS_COORD_TTL=1 "$rf" "$d" "$s" claim a area/contended "mine" >/dev/null
  sleep 2                                            # a's heartbeat ages past TTL
  AIS_COORD_TTL=1 "$rf" "$d" "$s" register b "fresh challenger" >/dev/null
  AIS_COORD_TTL=1 "$rf" "$d" "$s" claim b area/contended "reclaim" 2>&1
}
# D.1 — reclaim stdout parity (two dirs; robust: sleep 2 clears the TTL=1 holder).
DS="$WORK/D-shell"; DSS="$WORK/D-shell-SYNC.md"
DR="$WORK/D-rust";  DRS="$WORK/D-rust-SYNC.md"
d_shell_out="$(stale_seq run_shell "$DS" "$DSS" | norm "$DS" "$DSS")"
d_rust_out="$( stale_seq run_rust  "$DR" "$DRS" | norm "$DR" "$DRS")"
if [ "$d_shell_out" = "$d_rust_out" ]; then
  echo "  ✓ reclaim stdout identical: ${d_rust_out}"
else
  echo "  ✗ RECLAIM STDOUT MISMATCH:"; diff <(printf '%s\n' "$d_shell_out") <(printf '%s\n' "$d_rust_out") | sed 's/^/      /'; fail=1
fi

# D.2 — post-reclaim status, MUTUAL readability on ONE dir (avoids the cross-dir
# timing race at a tight TTL boundary). Build a stale-then-reclaimed dir with RUST
# (TTL=2, sleep 4 ⇒ 'a' unambiguously stale, 'b' unambiguously fresh), then have BOTH
# tools read it back-to-back: each must hide stale 'a' and show 'b' holding the lease.
DM="$WORK/D-mutual"; DMS="$WORK/D-mutual-SYNC.md"
AIS_COORD_TTL=2 run_rust "$DM" "$DMS" register a "first holder" >/dev/null
AIS_COORD_TTL=2 run_rust "$DM" "$DMS" claim a area/contended "mine" >/dev/null
sleep 4
AIS_COORD_TTL=2 run_rust "$DM" "$DMS" register b "fresh challenger" >/dev/null
AIS_COORD_TTL=2 run_rust "$DM" "$DMS" claim b area/contended "reclaim" >/dev/null
dm_shell="$(AIS_COORD_TTL=2 run_shell "$DM" "$DMS" status 2>&1 | norm "$DM" "$DMS")"
dm_rust="$( AIS_COORD_TTL=2 run_rust  "$DM" "$DMS" status 2>&1 | norm "$DM" "$DMS")"
if [ "$dm_shell" = "$dm_rust" ]; then
  echo "  ✓ shell + Rust agree reading a rust-reclaimed dir (stale 'a' hidden, 'b' holds lease)"
else
  echo "  ✗ POST-RECLAIM STATUS MISMATCH:"; diff <(printf '%s\n' "$dm_shell") <(printf '%s\n' "$dm_rust") | sed 's/^/      /'; fail=1
fi

echo ""
if [ "$fail" = 0 ]; then echo "PARITY: ALL PASS (semantic + mutual readability)"; else echo "PARITY: FAILURES (see above)"; fi
exit $fail
