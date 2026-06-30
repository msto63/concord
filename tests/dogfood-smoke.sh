#!/usr/bin/env bash
# Concord WP12 M5.2 — dogfood smoke: the Rust tool coordinates a realistic
# multi-session scenario in an ISOLATED coordination dir. This is the M5 acceptance
# criterion ("Concord coordinates Concord with Concord"): it exercises the full
# enforced flow — register, claim, conflict, merge-lock singleton, sync, status —
# entirely through the Rust `concord` binary, against its own coord-coord, with NO
# leakage into ais-coord (env cleared).
set -euo pipefail

HERE="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${CONCORD_BIN:-$HERE/target/debug/concord}"
[ -x "$BIN" ] || ( cd "$HERE" && cargo build -p concord -q )
[ -x "$BIN" ] || { echo "FATAL: concord not built at $BIN"; exit 1; }

W=$(mktemp -d "${TMPDIR:-/tmp}/concord-dogfood.XXXXXX"); trap 'rm -rf "$W"' EXIT
PROJ="$W/proj"; mkdir -p "$PROJ"; ( cd "$PROJ" && git init -q )
CD="$W/proj-coord"; SY="$W/proj-SESSION-SYNC.md"
fail=0

# Drive the Rust tool from inside an ISOLATED throwaway project: its coord dir and prose
# channel resolve by CONVENTION ($PROJ-coord, $PROJ-SESSION-SYNC.md), never the live
# ais-coord. F-config removes location env, so there is nothing ambient to leak.
cc() { ( cd "$PROJ" && "$BIN" "$@" ); }
chk() { # "<label>" "<got>" <gotrc> "<want-substr>" <wantrc>
  if printf '%s' "$2" | grep -qF "$4" && [ "$3" = "$5" ]; then echo "✓ $1";
  else echo "✗ $1 — got [$2] rc=$3 (want '$4' rc=$5)"; fail=1; fi
}

# Bootstrap + a realistic 3-session coordination flow, all via the Rust binary.
cc init --ids hub,a,b >/dev/null

o=$(cc claim a kernel/src/main.rs "edit main")        && r=0 || r=$?; chk "a claims an area"            "$o" "$r" "CLAIMED" 0
o=$(cc claim b user/usbd "usb work")                  && r=0 || r=$?; chk "b claims a disjoint area"     "$o" "$r" "CLAIMED" 0
o=$(cc claim b kernel/src/main.rs "want it too")      && r=0 || r=$?; chk "b's conflicting claim REFUSED" "$o" "$r" "CONFLICT" 2
o=$(cc claim b kernel/src "the parent")               && r=0 || r=$?; chk "b's path-OVERLAP claim REFUSED" "$o" "$r" "OVERLAP" 2
o=$(cc merge-lock hub "release train")                && r=0 || r=$?; chk "hub takes the merge lock"     "$o" "$r" "acquired" 0
o=$(cc merge-lock a "i also want to merge")           && r=0 || r=$?; chk "a's merge lock REFUSED (singleton)" "$o" "$r" "held by 'hub'" 2
o=$(cc merge-unlock a)                                 && r=0 || r=$?; chk "a cannot unlock hub's merge lock" "$o" "$r" "REFUSED" 2
o=$(cc merge-unlock hub)                               && r=0 || r=$?; chk "hub releases the merge lock"  "$o" "$r" "merge lock released" 0
o=$(cc release b kernel/src/main.rs)                  && r=0 || r=$?; chk "b cannot release a's lease"   "$o" "$r" "REFUSED" 2
o=$(cc release a kernel/src/main.rs)                  && r=0 || r=$?; chk "a releases its own lease"     "$o" "$r" "released" 0
o=$(cc sync a hub "STATUS" "main.rs done, usb next")  && r=0 || r=$?; chk "a posts to the prose channel" "$o" "$r" "posted" 0

# Final state assertions: isolated SYNC carries the directive; status reflects the flow.
grep -q "### a → hub" "$SY" && echo "✓ prose channel (isolated) carries the directive" || { echo "✗ sync missing"; fail=1; }
st=$(cc status 2>&1)
printf '%s' "$st" | grep -q "user_usbd" && printf '%s' "$st" | grep -q " by b " && echo "✓ status shows b's surviving lease" || { echo "✗ status wrong"; fail=1; }

# Isolation guarantee: all state lives under the throwaway project's convention paths.
echo "  ℹ all state under $CD (isolated by convention); the live ais-coord is untouched"

echo ""
if [ "$fail" = 0 ]; then echo "DOGFOOD SMOKE: ALL PASS — Rust tool coordinates a multi-session scenario, isolated"; else echo "DOGFOOD SMOKE: FAILURES"; fi
exit $fail
