#!/usr/bin/env bash
# Concord SessionStart hook — auto-register + heartbeat + tell the model its id.
# Output (stdout) is added to the session context. Fail-open (never break startup).
. "$(dirname "$0")/lib.sh" 2>/dev/null || exit 0
id=$(concord_id); [ -z "$id" ] && exit 0
[ -n "$COORD_SH" ] || exit 0

if [ -f "$COORD/sessions/$id" ]; then
  "$COORD_SH" heartbeat "$id" >/dev/null 2>&1            # keep existing focus
else
  "$COORD_SH" register "$id" "(session started)" >/dev/null 2>&1
fi

# Add a short context note so the model knows which Concord session it is.
focus=$(concord_field "$id" focus)
printf '[Concord] Du bist Session **%s**. Lies CLAUDE.md (Concord-Block) + den Prosa-Kanal ' "$id"
printf '/Users/mikes/Projects/ais-SESSION-SYNC.md auf `### … → %s`-Direktiven. ' "$id"
[ -n "$focus" ] && [ "$focus" != "(session started)" ] && printf 'Aktueller Fokus: %s. ' "$focus"
printf 'Heartbeat/Registry werden jetzt automatisch per Hook gepflegt.\n'
exit 0
