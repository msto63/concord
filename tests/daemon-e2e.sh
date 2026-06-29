#!/usr/bin/env bash
# Concord WP12 M2.1 — concordd inbox-demux end-to-end test.
#
# Proves the push substrate: the daemon parses `### from → to` directives appended to
# the prose channel and demultiplexes each block into per-recipient inboxes
# (`inbox/<id>`), fanning broadcasts (`→ ALLE`) out to every registered session except
# the sender. Two checks:
#   1. --once demux mechanics (offset forced to 0 ⇒ full processing, deterministic).
#   2. live watch push (daemon running; a fresh append is delivered within seconds).
#
# Usage: tests/daemon-e2e.sh   (builds debug concordd if needed)
set -euo pipefail

HERE="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DD="${CONCORDD_BIN:-$HERE/target/debug/concordd}"
[ -x "$DD" ] || ( cd "$HERE" && cargo build -p concordd -q )
[ -x "$DD" ] || { echo "FATAL: concordd not built at $DD"; exit 1; }

fail=0
have() { grep -qF "$1" "$2" 2>/dev/null; }

# ── 1. --once demux mechanics ──
W=$(mktemp -d "${TMPDIR:-/tmp}/concordd-e2e.XXXXXX")
CD="$W/coord"; SY="$W/SYNC.md"
mkdir -p "$CD/sessions"; : > "$CD/sessions/a"; : > "$CD/sessions/hub"; : > "$CD/sessions/concord-w"
printf '### hub → concord-w  (go)\nbody-go\n### x → ALLE  (bcast)\nhi-all\n### hub → a  (direct)\nyo-a\n' > "$SY"
echo 0 > "$CD/.inbox-offset"
CONCORD_DIR="$CD" CONCORD_SYNC="$SY" "$DD" --once >/dev/null 2>&1

# concord-w: directed 'go' + broadcast 'bcast'; a: broadcast + directed 'direct'; hub: broadcast only.
if have "(go)" "$CD/inbox/concord-w" && have "(bcast)" "$CD/inbox/concord-w" && ! have "(direct)" "$CD/inbox/concord-w"; then
  echo "✓ 1a: inbox/concord-w = directed + broadcast (no foreign directed)"
else echo "✗ 1a: inbox/concord-w wrong"; fail=1; fi
if have "(direct)" "$CD/inbox/a" && have "(bcast)" "$CD/inbox/a"; then
  echo "✓ 1b: inbox/a = directed + broadcast"
else echo "✗ 1b: inbox/a wrong"; fail=1; fi
if have "(bcast)" "$CD/inbox/hub" && ! have "(go)" "$CD/inbox/hub"; then
  echo "✓ 1c: inbox/hub = broadcast only"
else echo "✗ 1c: inbox/hub wrong"; fail=1; fi
# Broadcast sender exclusion: a directive '### concord-w → ALLE' must NOT echo to concord-w.
printf '### concord-w → ALLE  (selfcast)\nself\n' >> "$SY"
CONCORD_DIR="$CD" CONCORD_SYNC="$SY" "$DD" --once >/dev/null 2>&1
if ! have "(selfcast)" "$CD/inbox/concord-w" && have "(selfcast)" "$CD/inbox/hub"; then
  echo "✓ 1d: broadcast excludes its own sender"
else echo "✗ 1d: sender exclusion failed"; fail=1; fi
rm -rf "$W"

# ── 2. live watch push ──
W2=$(mktemp -d "${TMPDIR:-/tmp}/concordd-e2e.XXXXXX")
CD2="$W2/coord"; SY2="$W2/SYNC.md"
mkdir -p "$CD2/sessions"; : > "$CD2/sessions/concord-w"
printf '# prose channel\npreamble\n' > "$SY2"
CONCORD_DIR="$CD2" CONCORD_SYNC="$SY2" "$DD" >"$W2/daemon.log" 2>&1 &
DPID=$!
sleep 1.5
printf '\n### hub → concord-w  (live go M2)\nlive-body\n' >> "$SY2"
# Poll the inbox up to ~5s for the delivered block (event-driven ~300ms; safety tick 5s).
ok=0
for _ in 1 2 3 4 5 6 7 8 9 10; do
  if have "(live go M2)" "$CD2/inbox/concord-w"; then ok=1; break; fi
  sleep 0.5
done
kill "$DPID" 2>/dev/null || true; wait "$DPID" 2>/dev/null || true
if [ "$ok" = 1 ]; then echo "✓ 2: live append delivered to inbox/concord-w"; else echo "✗ 2: live push not delivered"; cat "$W2/daemon.log" | sed 's/^/    /'; fail=1; fi
rm -rf "$W2"

echo ""
if [ "$fail" = 0 ]; then echo "DAEMON E2E: ALL PASS"; else echo "DAEMON E2E: FAILURES"; fi
exit $fail
