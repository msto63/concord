#!/usr/bin/env bash
# Concord Stop hook (F1/A3) — harness-native cure for "going dark". When the agent tries
# to end its turn, refuse the stop IF there is an un-ACK'd coordinator directive addressed
# to this session — and inject it, so the session handles it instead of falling dormant
# with an unread `### … → <id>`.
#
# PRECISE PREDICATE (kept narrow to avoid endless turns): block ONLY when a
# `### <sender> → … <id>|ALL …` directive appears in the prose channel AFTER this
# session's own last `### <id> → …` post (its implicit ACK watermark). Merely *holding a
# lease* does NOT block — holding a lease across turns is normal. The `stop_hook_active`
# loop-guard (and Claude Code's 8-block ceiling) bound re-blocking.
#
# Fail-open: any error / no id / no channel → allow the stop.
. "$(dirname "$0")/lib.sh" 2>/dev/null || exit 0
id=$(concord_id); [ -z "$id" ] && exit 0
input=$(cat 2>/dev/null)

# Loop-guard: if we already blocked once this turn, allow the stop now.
active=$(printf '%s' "$input" | python3 -c 'import sys,json
try: print(json.load(sys.stdin).get("stop_hook_active"))
except Exception: print("")' 2>/dev/null)
[ "$active" = "True" ] && exit 0

[ -f "$SYNC" ] || exit 0
# Find an un-ACK'd directive to this session (after its own last post). Prints the
# directive line if one exists, else nothing.
pending=$(python3 - "$SYNC" "$id" <<'PY' 2>/dev/null
import sys
path, mid = sys.argv[1], sys.argv[2].lower()
try:
    lines = open(path, encoding="utf-8", errors="replace").read().splitlines()
except Exception:
    sys.exit(0)
# Watermark: index of this session's own last "### <id> → …" post.
mylast = -1
for i, ln in enumerate(lines):
    s = ln.strip()
    if s.startswith("###") and s[3:].strip().lower().startswith(mid + " "):
        mylast = i
# Scan after the watermark for a directive addressed to <id> or ALL, from someone else.
for ln in lines[mylast + 1:]:
    s = ln.strip()
    if not s.startswith("###") or "→" not in s:
        continue
    head, tgt = s[3:].split("→", 1)
    sender = head.strip().lower()
    targets = [t.lower() for t in tgt.split("(", 1)[0].replace("+", " ").replace(",", " ").split()]
    if sender.startswith(mid):
        continue
    if mid in targets or "all" in targets or "alle" in targets:
        print(s)
        break
PY
)
[ -z "$pending" ] && exit 0

# Block the stop and inject the un-ACK'd directive.
reason="Concord: you have an un-ACK'd coordinator directive: \"$pending\". Handle it (or post a '### $id → <sender> (ACK: …)') before ending your turn — do not go dark with an unread directive."
printf '%s' "$reason" | python3 -c 'import json,sys; print(json.dumps({"decision":"block","reason":sys.stdin.read()}))' 2>/dev/null \
  || printf '{"decision":"block","reason":"Concord: un-ACK''d coordinator directive pending; handle or ACK before stopping."}\n'
exit 0
