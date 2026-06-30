#!/usr/bin/env bash
# Concord — fencing token (no split-brain) at the CLI level.
#
# The core fencing Floor is covered by store_integration.rs (release with a stale fence is
# refused). This smoke exercises the full split-brain scenario end-to-end through the CLI:
# a holds a lease (fence F1) and goes stale → b reclaims the SAME area (fence advances to
# F2) → a, waking with stale authority, tries to release with its old fence F1 and is
# REFUSED, so it cannot clobber b's lease. Stale is forced deterministically (rewrite a's
# heartbeat to the past) — no sleep.
set -uo pipefail

HERE="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${CONCORD_BIN:-$HERE/target/debug/concord}"
[ -x "$BIN" ] || ( cd "$HERE" && cargo build -p concord -q )
[ -x "$BIN" ] || { echo "FATAL: concord not built at $BIN"; exit 1; }

W=$(mktemp -d "${TMPDIR:-/tmp}/concord-fence.XXXXXX"); trap 'rm -rf "$W"' EXIT
PROJ="$W/proj"; mkdir -p "$PROJ"; ( cd "$PROJ" && git init -q )
COORD="$W/proj-coord"
fail=0
R() { ( cd "$PROJ" && "$BIN" "$@" ); }
chk() { if printf '%s' "$2" | grep -qF "$3"; then echo "✓ $1"; else echo "✗ $1 — want '$3' in: $2"; fail=1; fi; }
chkx() { if [ "$2" = "$3" ]; then echo "✓ $1 (exit $2)"; else echo "✗ $1 — exit $2 != $3"; fail=1; fi; }

R init --ids a,b >/dev/null

# ── Part A: the fence token itself (a holds; a stale fence is refused) ──
R claim a "lib/x.rs" "edit" >/dev/null
v=$(R verify a "lib/x.rs"); chk "a holds lib/x.rs" "$v" "HELD by a"
Fx=$(printf '%s' "$v" | sed -n 's/.*fence \([0-9][0-9]*\).*/\1/p')
[ -n "$Fx" ] && echo "✓ read a's fence token (Fx=$Fx)" || { echo "✗ no fence"; fail=1; }
# Releasing with a WRONG (advanced) fence is refused — the holder's authority is stale.
o=$(R release a "lib/x.rs" --fence "$((Fx + 99))") && rc=0 || rc=$?
chk "stale (too-high) fence → REFUSED" "$o" "fence advanced to $Fx"; chkx "refused exits 2" "$rc" 2
# Releasing with the CORRECT fence works.
o=$(R release a "lib/x.rs" --fence "$Fx") && rc=0 || rc=$?
chk "correct fence → released" "$o" "released"; chkx "correct release exits 0" "$rc" 0

# ── Part B: the split-brain scenario (stale → reclaim → old holder can't clobber) ──
AREA="kernel/src/main.rs"
R claim a "$AREA" "edit" >/dev/null
F1=$(R verify a "$AREA" | sed -n 's/.*fence \([0-9][0-9]*\).*/\1/p')
# a goes stale: rewrite its heartbeat far into the past (deterministic; default TTL 1800s).
old=$(( $(date +%s) - 5000 ))
sed -i.bak "s/^heartbeat=.*/heartbeat=$old/" "$COORD/sessions/a"
# b reclaims the SAME area (a is stale) — the fence advances past F1.
o=$(R claim b "$AREA" "take over") && rc=0 || rc=$?; chk "b reclaims a's stale lease" "$o" "RECLAIMED"
F2=$(R verify b "$AREA" | sed -n 's/.*fence \([0-9][0-9]*\).*/\1/p')
[ -n "$F2" ] && [ "$F2" -gt "$F1" ] && echo "✓ fence advanced on reclaim (F2=$F2 > F1=$F1)" || { echo "✗ fence did not advance ($F1 → ${F2:-?})"; fail=1; }
# a wakes with stale authority and tries to release with its old fence → REFUSED (b owns it
# now), so a cannot clobber b's reclaimed lease — no split-brain.
o=$(R release a "$AREA" --fence "$F1") && rc=0 || rc=$?
chk "stale holder a cannot release b's reclaimed lease" "$o" "REFUSED"; chkx "refused exits 2" "$rc" 2
v3=$(R verify b "$AREA"); chk "b still holds the lease (no split-brain)" "$v3" "HELD by b"

echo ""
if [ "$fail" = 0 ]; then echo "FENCING SMOKE: ALL PASS — stale holder's fence is refused after reclaim, no split-brain"; else echo "FENCING SMOKE: FAILURES"; fi
exit $fail
