#!/usr/bin/env bash
# Concord SessionStart hook — auto-register + heartbeat + tell the model its id.
# Output (stdout) is added to the session context. Fail-open (never break startup).
# F1/A4: on `source=compact`, re-inject the pre-compaction snapshot (belt-and-suspenders
# with the PreCompact additionalContext) so protocol memory survives compaction.
. "$(dirname "$0")/lib.sh" 2>/dev/null || exit 0
id=$(concord_id); [ -z "$id" ] && exit 0
[ -n "$COORD_SH" ] || exit 0

input=$(cat 2>/dev/null)
source=$(printf '%s' "$input" | python3 -c 'import sys,json
try: print(json.load(sys.stdin).get("source",""))
except Exception: print("")' 2>/dev/null)

if [ -f "$COORD/sessions/$id" ]; then
  "$COORD_SH" heartbeat "$id" >/dev/null 2>&1            # keep existing focus
else
  "$COORD_SH" register "$id" "(session started)" >/dev/null 2>&1
fi

# A4: after a compaction, replay the snapshot the PreCompact hook saved, then clear it.
if [ "$source" = "compact" ] && [ -f "$COORD/state/$id.precompact" ]; then
  cat "$COORD/state/$id.precompact" 2>/dev/null
  rm -f "$COORD/state/$id.precompact" 2>/dev/null
fi

# Add a short context note so the model knows which Concord session it is.
focus=$(concord_field "$id" focus)
printf '[Concord] Du bist Session **%s**. Lies CLAUDE.md (Concord-Block) + den Prosa-Kanal ' "$id"
printf '%s auf `### … → %s`-Direktiven. ' "$SYNC" "$id"
[ -n "$focus" ] && [ "$focus" != "(session started)" ] && printf 'Aktueller Fokus: %s. ' "$focus"
printf 'Heartbeat/Registry werden jetzt automatisch per Hook gepflegt.\n'
exit 0
