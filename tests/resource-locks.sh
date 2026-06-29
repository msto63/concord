#!/usr/bin/env bash
# Concord WP12 F2 — named resource locks / build-slots (semaphore).
#
# Proves the kind=resource namespace: an N-slot pool hands out distinct slots in parallel,
# reports BUSY when full, validates capacity, is orthogonal to path leases, releases by
# name, and is auto-freed by the SessionEnd teardown (the F1/A2 composition).
set -uo pipefail

HERE="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${CONCORD_BIN:-$HERE/target/debug/concord}"
[ -x "$BIN" ] || ( cd "$HERE" && cargo build -p concord -q )
[ -x "$BIN" ] || { echo "FATAL: concord not built at $BIN"; exit 1; }

W=$(mktemp -d "${TMPDIR:-/tmp}/concord-rl.XXXXXX"); trap 'rm -rf "$W"' EXIT
PROJ="$W/proj"; mkdir -p "$PROJ"; ( cd "$PROJ" && git init -q )
fail=0
R() { ( cd "$PROJ" && env -u CONCORD_DIR -u CONCORD_SYNC -u CONCORD_PROJECT -u AIS_COORD_DIR -u AIS_SYNC_FILE -u AIS_PROJECT_DIR "$BIN" "$@" ); }
chk() { if printf '%s' "$2" | grep -qF "$3"; then echo "✓ $1"; else echo "✗ $1 — want '$3' in: $2"; fail=1; fi; }
chkx() { if [ "$2" = "$3" ]; then echo "✓ $1 (exit $2)"; else echo "✗ $1 — exit $2 != $3"; fail=1; fi; }

R init --ids a,b,c >/dev/null

# Pool of 2 QEMU ports: a + b get distinct slots; c finds the pool full.
o=$(R claim a qemu-port --kind resource --slots 2 vm) && rc=0 || rc=$?; chk "a gets slot 0/2" "$o" "slot 0/2"
o=$(R claim b qemu-port --kind resource --slots 2 vm) && rc=0 || rc=$?; chk "b gets slot 1/2 (parallel, same pool)" "$o" "slot 1/2"
o=$(R claim c qemu-port --kind resource --slots 2 vm) && rc=0 || rc=$?; chk "c → BUSY (pool full)" "$o" "RESOURCE-BUSY"; chkx "BUSY exits 2" "$rc" 2

# Idempotent re-acquire; capacity validation.
o=$(R claim a qemu-port --kind resource --slots 2 vm); chk "a re-acquire is idempotent" "$o" "already holding qemu-port slot 0"
o=$(R claim c qemu-port --kind resource --slots 5 vm) && rc=0 || rc=$?; chk "mismatched --slots rejected" "$o" "RESOURCE-CAPACITY-MISMATCH"; chkx "mismatch exits 2" "$rc" 2

# Orthogonal to path leases: a FILE lease named like the resource does not conflict.
o=$(R claim b qemu-port "as a path lease") && rc=0 || rc=$?; chk "path lease 'qemu-port' is orthogonal (CLAIMED)" "$o" "CLAIMED"; chkx "orthogonal claim ok" "$rc" 0

# Exclusive build-env (N=1).
o=$(R claim a build-env --kind resource build); chk "build-env exclusive slot 0/1" "$o" "slot 0/1"

# status shows the RESOURCE LOCKS section.
o=$(R status); chk "status lists RESOURCE LOCKS" "$o" "RESOURCE LOCKS"; chk "status shows qemu-port 2/2" "$o" "qemu-port"

# Release by name frees the slot; c can then take it.
o=$(R release a qemu-port --kind resource); chk "a releases its qemu-port slot" "$o" "RESOURCE-RELEASED qemu-port slot 0"
o=$(R claim c qemu-port --kind resource --slots 2 vm); chk "c now gets the freed slot 0" "$o" "slot 0/2"

# SessionEnd teardown (F1/A2 composition) frees the session's remaining resource slots.
o=$(R session-end a); chk "session-end frees a's build-env slot" "$o" "build-env#0"
o=$(R claim b build-env --kind resource build); chk "build-env is free after a's teardown" "$o" "slot 0/1"

echo ""
if [ "$fail" = 0 ]; then echo "RESOURCE LOCKS: ALL PASS — N-slot pool, BUSY, capacity-validate, orthogonal, release, SessionEnd auto-free"; else echo "RESOURCE LOCKS: FAILURES"; fi
exit $fail
