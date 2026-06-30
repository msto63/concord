#!/usr/bin/env bash
# Concord PreToolUse guard (F1/A1) — HARD lease enforcement at the keystroke.
#
# On Edit|Write|MultiEdit|NotebookEdit, ask the typed core `concord check-lease <id> <file>`
# and, if it says DENY, return a `permissionDecision:"deny"` that blocks the tool BEFORE it
# runs. Policy is the typed core's (P2 block-on-conflict by default: deny only when a
# *different active* session holds an overlapping lease; symbol-aware via the S2 AST). A
# `<coord>/strict-leases` marker switches the core to P1 (capability-strict). On Bash, keep
# the merge-singleton guard (no parallel merge to main while another holds the merge-lock).
#
# DEFAULT-ALLOW / FAIL-OPEN: any uncertainty (no id, no binary, parse failure, allow verdict)
# leaves the tool to run. Only an explicit DENY blocks.
. "$(dirname "$0")/lib.sh" 2>/dev/null || exit 0
id=$(concord_id); [ -z "$id" ] && exit 0
[ -n "$COORD_SH" ] || exit 0
input=$(cat 2>/dev/null)

parsed=$(printf '%s' "$input" | python3 -c '
import sys,json
try: d=json.load(sys.stdin)
except Exception: sys.exit(0)
ti=d.get("tool_input",{}) or {}
print(d.get("tool_name",""))
print(ti.get("file_path") or ti.get("notebook_path") or ti.get("command") or "")
' 2>/dev/null)
tool=$(printf '%s' "$parsed" | sed -n 1p)
field=$(printf '%s' "$parsed" | sed -n '2,$p')
[ -z "$tool" ] && exit 0

# Emit a PreToolUse deny verdict (JSON on stdout, exit 0) and stop.
deny() {  # <reason>
  printf '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":%s}}\n' \
    "$(printf '%s' "$1" | python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()))')"
  exit 0
}

holder_active() {  # <holder> -> true if heartbeat within TTL (30 min)
  local hb now
  hb=$(sed -n 's/^heartbeat=//p' "$COORD/sessions/$1" 2>/dev/null)
  now=$(date +%s 2>/dev/null)
  [ -n "$hb" ] && [ -n "$now" ] && [ $(( now - hb )) -le 1800 ]
}

case "$tool" in
  Edit|Write|MultiEdit|NotebookEdit)
    f="$field"; [ -z "$f" ] && exit 0
    # Relativize to the repo root. Normalize BOTH to physical paths (pwd -P) so a
    # symlinked prefix (e.g. macOS /var → /private/var) doesn't defeat the strip; the
    # file may not exist yet (a Write), so resolve its directory, not the file.
    top=$(git rev-parse --show-toplevel 2>/dev/null); top=$(cd "$top" 2>/dev/null && pwd -P)
    rel="$f"
    fdir=$(cd "$(dirname "$f")" 2>/dev/null && pwd -P)
    [ -n "$fdir" ] && f="$fdir/$(basename "$f")"
    case "$f" in "$top"/*) rel="${f#"$top"/}";; esac
    strict=""; [ -f "$COORD/strict-leases" ] && strict="--strict"
    # Typed-core decision; fail-open on any error (out=empty / nonzero-from-missing-binary).
    out=$(coord check-lease "$id" "$rel" $strict 2>/dev/null) || true
    case "$out" in
      DENY*)
        deny "Concord: '$rel' is leased — $out. Coordinate first (status / SESSION-SYNC; claim after the holder releases) instead of editing in parallel. Override: release/reassign the lease, or remove $COORD/strict-leases." ;;
    esac
    ;;
  Bash)
    c="$field"
    case "$c" in
      *"gh pr merge"*|*"git merge "*|*"git push"*origin*main*)
        if [ -d "$COORD/merge.lock" ]; then
          h=$(cat "$COORD/merge.lock/holder" 2>/dev/null)
          if [ -n "$h" ] && [ "$h" != "$id" ] && holder_active "$h"; then
            deny "Concord: the merge-lock is held by session '$h'. Do not merge to main in parallel — wait for merge-unlock or coordinate via the coordinator."
          fi
        fi ;;
    esac
    ;;
esac
exit 0
