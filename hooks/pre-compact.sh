#!/usr/bin/env bash
# Concord PreCompact hook (F1/A4) — protect protocol memory across context compaction.
# Before Claude Code compacts, snapshot this session's coordination state (its leases,
# the merge-lock, its id) to <coord>/state/<id>.precompact AND emit it as
# additionalContext (which survives into the compacted session). The SessionStart hook
# re-injects the snapshot on `source=compact` (belt-and-suspenders). Fail-open.
. "$(dirname "$0")/lib.sh" 2>/dev/null || exit 0
id=$(concord_id); [ -z "$id" ] && exit 0

mylease=""
for d in "$COORD"/leases/*; do
  [ -e "$d" ] || break
  [ "$(cat "$d/holder" 2>/dev/null)" = "$id" ] && mylease="$mylease $(basename "$d")"
done
merge=""
[ -d "$COORD/merge.lock" ] && merge=$(cat "$COORD/merge.lock/holder" 2>/dev/null)

snap="[Concord/precompact] You are session **$id**. Your held leases:${mylease:- (none)}."
[ -n "$merge" ] && snap="$snap Merge-lock holder: $merge."
snap="$snap After compaction: re-read CLAUDE.md (Concord block) + $SYNC for '### … → $id' directives; heartbeat only while holding a lease; release leases when done."

state="$COORD/state"; mkdir -p "$state" 2>/dev/null
printf '%s\n' "$snap" > "$state/$id.precompact" 2>/dev/null

printf '%s' "$snap" | python3 -c '
import json,sys
print(json.dumps({"hookSpecificOutput":{"hookEventName":"PreCompact","additionalContext":sys.stdin.read()}}))' 2>/dev/null
exit 0
