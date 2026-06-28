#!/usr/bin/env bash
# Concord UserPromptSubmit hook — inject a compact, current Concord status into
# the context each turn (who's active, my lease, merge-lock) PLUS any NEW prose-
# channel lines addressed to this session since the last prompt. So a session
# never misses a directive and always has fresh awareness. Fail-open, compact.
. "$(dirname "$0")/lib.sh" 2>/dev/null || exit 0
id=$(concord_id); [ -z "$id" ] && exit 0
SYNC="${CONCORD_SYNC:-${AIS_SYNC_FILE:-/Users/mikes/Projects/ais-SESSION-SYNC.md}}"
state="$HOOKS/state"; mkdir -p "$state" 2>/dev/null
seenf="$state/$id.seen"
now=$(date +%s 2>/dev/null)

active=""
for f in "$COORD"/sessions/*; do
  [ -e "$f" ] || break
  sid=$(basename "$f"); hb=$(sed -n 's/^heartbeat=//p' "$f" 2>/dev/null)
  [ -n "$hb" ] && [ -n "$now" ] && [ $((now-hb)) -le 1800 ] && active="$active $sid"
done
mylease=""
for d in "$COORD"/leases/*; do
  [ -e "$d" ] || break
  [ "$(cat "$d/holder" 2>/dev/null)" = "$id" ] && mylease="$mylease $(basename "$d")"
done

newdir=""
if [ -f "$SYNC" ]; then
  total=$(wc -l < "$SYNC" 2>/dev/null | tr -d ' ')
  last=$(cat "$seenf" 2>/dev/null)
  [ -z "$last" ] && { [ -n "$total" ] && last=$(( total>30 ? total-30 : 0 )) || last=0; }
  if [ -n "$total" ] && [ "$total" -gt "${last:-0}" ]; then
    newdir=$(sed -n "$((last+1)),${total}p" "$SYNC" 2>/dev/null | python3 -c '
import sys
mid=sys.argv[1]
for ln in sys.stdin:
    s=ln.rstrip("\n")
    if not s.startswith("###") or "→" not in s: continue
    tgt=s.split("→",1)[1].split("(",1)[0]
    toks=tgt.replace("+"," ").replace(","," ").split()
    if mid in toks or "ALLE" in toks: print(s)
' "$id" 2>/dev/null)
    printf '%s' "$total" > "$seenf" 2>/dev/null
  fi
fi

printf '[Concord] Du bist **%s**. Aktiv:%s.' "$id" "${active:- –}"
[ -n "$mylease" ] && printf ' Dein Lease:%s.' "$mylease"
if [ -d "$COORD/merge.lock" ]; then mh=$(cat "$COORD/merge.lock/holder" 2>/dev/null); [ -n "$mh" ] && printf ' Merge-Sperre: %s.' "$mh"; fi
printf '\n'
[ -n "$newdir" ] && printf 'NEU an dich/ALLE gerichtet im Prosa-Kanal seit letztem Turn:\n%s\n' "$newdir"
exit 0
