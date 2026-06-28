#!/usr/bin/env bash
# Concord PreToolUse guard — mechanically enforces leases + the merge singleton.
# DEFAULT-ALLOW: only blocks (exit 2) when a file is in a shared region AND a
# *different, currently-active* session holds the matching lease — or a Bash
# merge while another active session holds the merge-lock. Any uncertainty → allow.
. "$(dirname "$0")/lib.sh" 2>/dev/null || exit 0
id=$(concord_id); [ -z "$id" ] && exit 0
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

holder_active() {  # <holder> -> true if heartbeat within 30 min
  local hb now
  hb=$(sed -n 's/^heartbeat=//p' "$COORD/sessions/$1" 2>/dev/null)
  now=$(date +%s 2>/dev/null)
  [ -n "$hb" ] && [ -n "$now" ] && [ $(( now - hb )) -le 1800 ]
}

case "$tool" in
  Edit|Write|MultiEdit|NotebookEdit)
    f="$field"; [ -z "$f" ] && exit 0
    top=$(git rev-parse --show-toplevel 2>/dev/null)
    rel="$f"; case "$f" in "$top"/*) rel="${f#"$top"/}";; esac
    [ -f "$HOOKS/shared-regions" ] || exit 0
    while IFS= read -r pat; do
      case "$pat" in ''|\#*) continue;; esac
      case "$rel" in
        "$pat"|"$pat"/*)
          rslug=$(concord_slug "$pat")
          for d in "$COORD"/leases/*; do
            [ -e "$d" ] || break
            b=$(basename "$d")
            case "$b" in
              "$rslug"*)
                h=$(cat "$d/holder" 2>/dev/null)
                if [ -n "$h" ] && [ "$h" != "$id" ] && holder_active "$h"; then
                  echo "Concord: '$rel' liegt in einer geteilten Region, die Session '$h' geleast hat (Lease '$b'). Erst koordinieren (status / SESSION-SYNC, ggf. claim nach release) statt parallel editieren. Override falls nötig: Lease freigeben/neu zuweisen oder die Region aus $HOOKS/shared-regions nehmen." >&2
                  exit 2
                fi ;;
            esac
          done ;;
      esac
    done < "$HOOKS/shared-regions"
    ;;
  Bash)
    c="$field"
    case "$c" in
      *"gh pr merge"*|*"git merge "*|*"git push"*origin*main*)
        if [ -d "$COORD/merge.lock" ]; then
          h=$(cat "$COORD/merge.lock/holder" 2>/dev/null)
          if [ -n "$h" ] && [ "$h" != "$id" ] && holder_active "$h"; then
            echo "Concord: Die Merge-Sperre wird gerade von Session '$h' gehalten. Nicht parallel nach main mergen — warte auf merge-unlock oder koordiniere über K." >&2
            exit 2
          fi
        fi ;;
    esac
    ;;
esac
exit 0
