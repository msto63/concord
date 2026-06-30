#!/usr/bin/env bash
# Concord statusline — shows WHICH session this window is, its focus, lease, heartbeat.
# Input: Claude Code statusline JSON on stdin. Output: one colourised line. Fail-open.
. "$(dirname "$0")/lib.sh" 2>/dev/null || { printf 'concord'; exit 0; }
cat >/dev/null 2>&1   # drain stdin
id=$(concord_id)
[ -z "$id" ] && { printf '○ concord (no session id — set $CONCORD_ID, an idbind marker, or use a <repo>-<id> worktree)'; exit 0; }

col=$(concord_colour "$id")
focus=$(concord_field "$id" focus); [ -z "$focus" ] && focus="(idle)"
# truncate focus to keep the line short
[ "${#focus}" -gt 44 ] && focus="${focus:0:43}…"
lease=""
for d in "$COORD"/leases/*; do
  [ -e "$d" ] || break
  [ "$(cat "$d/holder" 2>/dev/null)" = "$id" ] && { lease=$(basename "$d"); break; }
done
hb=$(concord_field "$id" heartbeat); age="?"
if [ -n "$hb" ]; then now=$(date +%s 2>/dev/null); [ -n "$now" ] && age="$(( (now-hb)/60 ))m"; fi
out="● ${id} · ${focus}"
[ -n "$lease" ] && out="${out} · ⚷${lease}"
out="${out} · ♥${age}"
if [ -d "$COORD/merge.lock" ]; then
  mh=$(cat "$COORD/merge.lock/holder" 2>/dev/null)
  [ -n "$mh" ] && out="${out} · 🔒merge:${mh}"
fi
printf '\033[%sm%s\033[0m' "$col" "$out"
