#!/usr/bin/env bash
# Concord PostToolUse hook — (1) bump the heartbeat on every tool use, so "alive"
# couples automatically to real activity; (2) F1/A6 out-of-scope-write AUDIT: after an
# Edit/Write, if the typed core would have DENIED the edit (a *different active* session
# holds the overlapping lease), record a provenance violation in the ledger. This is the
# accountability backstop BEHIND the A1 PreToolUse deny — it catches a write that slipped
# past A1 (hook bypassed/disabled), defense-in-depth. AUDIT-ONLY: it never blocks (the
# write already happened; enforcement is A1). Fail-open, silent, fast.
. "$(dirname "$0")/lib.sh" 2>/dev/null || exit 0
id=$(concord_id); [ -z "$id" ] && exit 0
[ -n "$COORD_SH" ] || exit 0
"$COORD_SH" heartbeat "$id" >/dev/null 2>&1

input=$(cat 2>/dev/null)
[ -n "$input" ] || exit 0
parsed=$(printf '%s' "$input" | python3 -c '
import sys,json
try: d=json.load(sys.stdin)
except Exception: sys.exit(0)
ti=d.get("tool_input",{}) or {}
print(d.get("tool_name",""))
print(ti.get("file_path") or ti.get("notebook_path") or "")
' 2>/dev/null)
tool=$(printf '%s' "$parsed" | sed -n 1p)
f=$(printf '%s' "$parsed" | sed -n '2,$p')
case "$tool" in
  Edit|Write|MultiEdit|NotebookEdit) [ -n "$f" ] || exit 0 ;;
  *) exit 0 ;;
esac
top=$(git rev-parse --show-toplevel 2>/dev/null); top=$(cd "$top" 2>/dev/null && pwd -P)
rel="$f"; fdir=$(cd "$(dirname "$f")" 2>/dev/null && pwd -P)
[ -n "$fdir" ] && f="$fdir/$(basename "$f")"
case "$f" in "$top"/*) rel="${f#"$top"/}";; esac
# Note: A6 always uses the P2 conflict test (would another active holder be stepped on?),
# independent of the strict-leases marker — strict-P1 is an A1 ergonomics knob, not an
# accountability one.
out=$("$COORD_SH" check-lease "$id" "$rel" 2>/dev/null) || true
case "$out" in
  DENY*) "$COORD_SH" log "$id" "out-of-scope-write: $rel ($out)" >/dev/null 2>&1 ;;
esac
exit 0
