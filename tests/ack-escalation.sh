#!/usr/bin/env bash
# Concord WP12 F3 — ack-tracking + tracked escalation (CLI surface).
#
# Proves the verbs (escalate/resolve/ack/escalations) + the status ESCALATIONS/PENDING-ACKS
# surface. The daemon's TTL re-deliver/auto-escalate timing is covered by the integration
# tests (it needs a controllable clock); this asserts the user-facing Floor behavior.
set -uo pipefail

HERE="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${CONCORD_BIN:-$HERE/target/debug/concord}"
[ -x "$BIN" ] || ( cd "$HERE" && cargo build -p concord -q )
[ -x "$BIN" ] || { echo "FATAL: concord not built at $BIN"; exit 1; }

W=$(mktemp -d "${TMPDIR:-/tmp}/concord-ae.XXXXXX"); trap 'rm -rf "$W"' EXIT
PROJ="$W/proj"; mkdir -p "$PROJ"; ( cd "$PROJ" && git init -q )
fail=0
R() { ( cd "$PROJ" && "$BIN" "$@" ); }
chk() { if printf '%s' "$2" | grep -qF "$3"; then echo "✓ $1"; else echo "✗ $1 — want '$3' in: $2"; fail=1; fi; }
chkx() { if [ "$2" = "$3" ]; then echo "✓ $1 (exit $2)"; else echo "✗ $1 — exit $2 != $3"; fail=1; fi; }

R init --ids a,hub >/dev/null

# Escalate: tracked record, default --to = coordinator (hub).
o=$(R escalate a high "build-env deadlock"); chk "escalate → ESCALATED #1 [high]" "$o" "ESCALATED #1 [high] → hub"
o=$(R escalate a critical "vision blocker" --ref B7.9); chk "escalate critical with --ref" "$o" "ESCALATED #2 [critical]"
o=$(R escalate a bogus "x") && rc=0 || rc=$?; chkx "invalid severity rejected" "$rc" 1

# List: open first, ref shown.
o=$(R escalations); chk "escalations lists #2 critical (open first)" "$o" "#2 [critical] OPEN"; chk "escalations shows ref" "$o" "ref=B7.9"

# Status surface.
o=$(R status); chk "status shows ESCALATIONS (open)" "$o" "ESCALATIONS (open)"; chk "status lists #2" "$o" "#2"

# Resolve closes it; persists as resolved (does not vanish).
o=$(R resolve hub 1 "freed the build-env"); chk "resolve #1" "$o" "RESOLVED escalation #1"
o=$(R resolve hub 1 "again"); chk "re-resolve is idempotent" "$o" "already resolved"
o=$(R resolve hub 99 "x") && rc=0 || rc=$?; chk "resolve unknown → message" "$o" "no escalation #99"; chkx "unknown resolve exit 2" "$rc" 2
o=$(R escalations); chk "resolved #1 still tracked (not vanished)" "$o" "#1 [high] resolved"

# Status no longer lists the resolved one as open (only #2 open remains).
o=$(R status | sed -n '/ESCALATIONS/,/PENDING/p'); chk "status open list drops resolved #1" "$o" "#2"
if printf '%s' "$o" | grep -q "#1 "; then echo "✗ resolved #1 should not be in open list"; fail=1; else echo "✓ resolved #1 absent from open list"; fi

# ack with no pending is a clean no-op.
o=$(R ack a "nothing pending"); chk "ack clears 0 pending cleanly" "$o" "0 pending"

echo ""
if [ "$fail" = 0 ]; then echo "ACK + ESCALATION: ALL PASS — escalate/list/resolve/ack, status surface, persist-until-resolved"; else echo "ACK + ESCALATION: FAILURES"; fi
exit $fail
