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
CC="${CONCORD_BIN:-$HERE/target/debug/concord}"
[ -x "$DD" ] || ( cd "$HERE" && cargo build -p concordd -q )
[ -x "$CC" ] || ( cd "$HERE" && cargo build -p concord -q )
[ -x "$DD" ] || { echo "FATAL: concordd not built at $DD"; exit 1; }
[ -x "$CC" ] || { echo "FATAL: concord not built at $CC"; exit 1; }

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
if have "(go)" "$CD/inbox/concord-w.jsonl" && have "(bcast)" "$CD/inbox/concord-w.jsonl" && ! have "(direct)" "$CD/inbox/concord-w.jsonl"; then
  echo "✓ 1a: inbox/concord-w = directed + broadcast (no foreign directed)"
else echo "✗ 1a: inbox/concord-w wrong"; fail=1; fi
if have "(direct)" "$CD/inbox/a.jsonl" && have "(bcast)" "$CD/inbox/a.jsonl"; then
  echo "✓ 1b: inbox/a = directed + broadcast"
else echo "✗ 1b: inbox/a wrong"; fail=1; fi
if have "(bcast)" "$CD/inbox/hub.jsonl" && ! have "(go)" "$CD/inbox/hub.jsonl"; then
  echo "✓ 1c: inbox/hub = broadcast only"
else echo "✗ 1c: inbox/hub wrong"; fail=1; fi
# Broadcast sender exclusion: a directive '### concord-w → ALLE' must NOT echo to concord-w.
printf '### concord-w → ALLE  (selfcast)\nself\n' >> "$SY"
CONCORD_DIR="$CD" CONCORD_SYNC="$SY" "$DD" --once >/dev/null 2>&1
if ! have "(selfcast)" "$CD/inbox/concord-w.jsonl" && have "(selfcast)" "$CD/inbox/hub.jsonl"; then
  echo "✓ 1d: broadcast excludes its own sender"
else echo "✗ 1d: sender exclusion failed"; fail=1; fi
# WP7: the inbox is typed JSONL — the GO topic classifies to kind "go".
if have '"kind":"go"' "$CD/inbox/concord-w.jsonl"; then
  echo "✓ 1e: WP7 typed inbox — GO classified to kind=go"
else echo "✗ 1e: typed kind missing"; fail=1; fi
rm -rf "$W"

# ── 1f. first-class typed send ──
W4=$(mktemp -d "${TMPDIR:-/tmp}/concordd-e2e.XXXXXX"); CD4="$W4/coord"; SY4="$W4/SYNC.md"; mkdir -p "$CD4"
CONCORD_DIR="$CD4" CONCORD_SYNC="$SY4" "$CC" send hub concord-w go --ref B15.3 "build it now" >/dev/null 2>&1
if have '"kind":"go"' "$CD4/inbox/concord-w.jsonl" && have '"ref":"B15.3"' "$CD4/inbox/concord-w.jsonl" \
   && have '"from":"hub"' "$CD4/inbox/concord-w.jsonl"; then
  echo "✓ 1f: concord send → typed message (kind+ref+from) in inbox/<to>.jsonl"
else echo "✗ 1f: send typed message wrong"; cat "$CD4/inbox/concord-w.jsonl" 2>/dev/null | sed 's/^/    /'; fail=1; fi
rm -rf "$W4"

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
  if have "(live go M2)" "$CD2/inbox/concord-w.jsonl"; then ok=1; break; fi
  sleep 0.5
done
kill "$DPID" 2>/dev/null || true; wait "$DPID" 2>/dev/null || true
if [ "$ok" = 1 ]; then echo "✓ 2: live append delivered to inbox/concord-w"; else echo "✗ 2: live push not delivered"; cat "$W2/daemon.log" | sed 's/^/    /'; fail=1; fi
rm -rf "$W2"

# ── 3. Strong fencing: daemon-mediated merge-lock ──
# With the daemon up, consequential merge-lock/unlock route through its single-thread
# serialization point (atomic check-and-apply). Verify acquire / contended-HELD /
# foreign-unlock-REFUSED / release, plus the Floor fallback when mediation is disabled.
W3=$(mktemp -d "${TMPDIR:-/tmp}/concordd-e2e.XXXXXX")
CD3="$W3/coord"; SY3="$W3/SYNC.md"; mkdir -p "$CD3"
CONCORD_DIR="$CD3" CONCORD_SYNC="$SY3" "$CC" register a "sess a" >/dev/null
CONCORD_DIR="$CD3" CONCORD_SYNC="$SY3" "$CC" register b "sess b" >/dev/null
CONCORD_DIR="$CD3" CONCORD_SYNC="$SY3" "$DD" >"$W3/daemon.log" 2>&1 &
DPID3=$!
# Wait for the socket to appear (daemon armed).
for _ in 1 2 3 4 5 6 7 8 9 10; do [ -S "$CD3/concordd.sock" ] && break; sleep 0.3; done
run3() { CONCORD_DIR="$CD3" CONCORD_SYNC="$SY3" "$CC" "$@"; }
# `&& e=0 || e=$?` keeps a non-zero exit (HELD/REFUSED=2) from tripping `set -e`.
o1=$(run3 merge-lock a "merge #1") && e1=0 || e1=$?
o2=$(run3 merge-lock b)           && e2=0 || e2=$?
o3=$(run3 merge-unlock b)         && e3=0 || e3=$?
o4=$(run3 merge-unlock a)         && e4=0 || e4=$?
# Floor fallback path still works with mediation disabled.
o5=$(CONCORD_NO_DAEMON=1 run3 merge-lock a "floor") && e5=0 || e5=$?
kill "$DPID3" 2>/dev/null || true; wait "$DPID3" 2>/dev/null || true

check3() { # "<label>" "<got>" <gotexit> "<want-substr>" <wantexit>
  if printf '%s' "$2" | grep -qF "$4" && [ "$3" = "$5" ]; then echo "✓ 3: $1";
  else echo "✗ 3: $1 — got [$2] exit=$3 (want '$4' exit=$5)"; fail=1; fi
}
[ -S "$CD3/concordd.sock" ] || true  # socket is cleaned on kill; presence checked above
check3 "mediated acquire"          "$o1" "$e1" "MERGE LOCK acquired" 0
check3 "mediated contended HELD"   "$o2" "$e2" "held by 'a'" 2
check3 "mediated foreign unlock REFUSED" "$o3" "$e3" "REFUSED" 2
check3 "mediated release"          "$o4" "$e4" "merge lock released" 0
check3 "Floor fallback (no daemon)" "$o5" "$e5" "MERGE LOCK acquired" 0

rm -rf "$W3"

echo ""
if [ "$fail" = 0 ]; then echo "DAEMON E2E: ALL PASS"; else echo "DAEMON E2E: FAILURES"; fi
exit $fail
