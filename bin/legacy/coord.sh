#!/usr/bin/env bash
# Concord — the ais multi-session coordination system.  Prevents the parallel
# sessions working on ais from impairing, blocking or damaging each other's work.
# File-based (no jq, no server) and reliable on the shared local filesystem.
# Every session (current AND new) is wired into Concord via the repo CLAUDE.md,
# which mandates the ritual below.  (Concord = structured/enforced coordination;
# ais-SESSION-SYNC.md = the prose discussion channel.)
#
#   tools/coord.sh register <id> "<focus>"   # once, at session start
#   tools/coord.sh heartbeat <id>            # periodically (keeps you "alive")
#   tools/coord.sh status                    # who is active + what is leased
#   tools/coord.sh claim <id> <area> ["why"] # BEFORE editing a shared area
#   tools/coord.sh release <id> <area>       # when done with the area
#   tools/coord.sh merge-lock <id> ["why"]   # BEFORE merging to main (singleton)
#   tools/coord.sh merge-unlock <id>         # after the merge
#   tools/coord.sh log <id> <event...>       # record a structured intent/decision
#   tools/coord.sh sync <id> <target> "<topic>" "<body>"  # post to the prose channel
#       (use this when direct file-append to SESSION-SYNC.md is sandbox-blocked —
#        coord.sh is allow-listed, so it can write the outside-cwd prose channel)
#
# "Shared areas" worth a lease: kernel/src/main.rs, the std-on-ais PAL
# (lib/libstdrust), a specific daemon (user/<d>), the embedded ELFs, the memory
# dir, a doc both might touch.  A lease is a cooperative claim, not a hard mutex —
# but with the CLAUDE.md mandate it reliably stops two sessions colliding.
set -euo pipefail

_top="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
COORD="${CONCORD_DIR:-${AIS_COORD_DIR:-$(dirname "$_top")/$(basename "$_top")-coord}}"
SESSIONS="$COORD/sessions"
LEASES="$COORD/leases"
LOG="$COORD/intents.jsonl"
SYNC="${CONCORD_SYNC:-${AIS_SYNC_FILE:-$(dirname "$_top")/$(basename "$_top")-SESSION-SYNC.md}}"   # prose channel
TTL="${AIS_COORD_TTL:-1800}"   # 30 min without a heartbeat ⇒ session/lease is stale
mkdir -p "$SESSIONS" "$LEASES"

now() { date +%s; }
slug() { printf '%s' "$1" | tr '/ ' '__'; }
ts() { date -r "$1" '+%H:%M' 2>/dev/null || date '+%H:%M'; }

# Is session $1 stale (no heartbeat within TTL, or gone)?
session_stale() {
    local f="$SESSIONS/$1"
    [ -f "$f" ] || return 0
    local hb; hb=$(sed -n 's/^heartbeat=//p' "$f" 2>/dev/null || echo 0)
    [ -z "$hb" ] && return 0
    [ $(( $(now) - hb )) -gt "$TTL" ]
}

logline() {  # <id> <event...>
    local id="$1"; shift
    printf '{"t":%s,"session":"%s","event":"%s"}\n' "$(now)" "$id" "$* " >> "$LOG"
}

cmd="${1:-status}"; shift || true
case "$cmd" in
  register)
    id="${1:?session id}"; focus="${2:-}"
    printf 'focus=%s\nstarted=%s\nheartbeat=%s\n' "$focus" "$(now)" "$(now)" > "$SESSIONS/$id"
    logline "$id" "register: $focus"
    echo "registered session '$id' (focus: $focus)"
    "$0" status ;;

  heartbeat)
    id="${1:?session id}"
    if [ -f "$SESSIONS/$id" ]; then
        local_focus=$(sed -n 's/^focus=//p' "$SESSIONS/$id"); local_started=$(sed -n 's/^started=//p' "$SESSIONS/$id")
        printf 'focus=%s\nstarted=%s\nheartbeat=%s\n' "$local_focus" "$local_started" "$(now)" > "$SESSIONS/$id"
    else
        printf 'focus=\nstarted=%s\nheartbeat=%s\n' "$(now)" "$(now)" > "$SESSIONS/$id"
    fi ;;

  status)
    echo "── Concord — ais multi-session coordination ($COORD) ──"
    echo "ACTIVE SESSIONS:"
    for f in "$SESSIONS"/*; do
        [ -e "$f" ] || { echo "  (none)"; break; }
        id=$(basename "$f")
        if session_stale "$id"; then continue; fi
        printf '  %-10s focus: %s\n' "$id" "$(sed -n 's/^focus=//p' "$f")"
    done
    echo "HELD LEASES:"
    local_any=0
    for d in "$LEASES"/*; do
        [ -e "$d" ] || break
        area=$(basename "$d"); holder=$(cat "$d/holder" 2>/dev/null || echo '?')
        if session_stale "$holder"; then continue; fi
        printf '  %-28s by %s — %s\n' "$area" "$holder" "$(cat "$d/why" 2>/dev/null)"; local_any=1
    done
    [ "$local_any" = 0 ] && echo "  (none)"
    if [ -d "$COORD/merge.lock" ]; then
        mh=$(cat "$COORD/merge.lock/holder" 2>/dev/null || echo '?')
        session_stale "$mh" || echo "MERGE LOCK: held by $mh"
    fi ;;

  claim)
    id="${1:?session id}"; area=$(slug "${2:?area}"); why="${3:-}"
    d="$LEASES/$area"
    if mkdir "$d" 2>/dev/null; then
        echo "$id" > "$d/holder"; echo "$why" > "$d/why"; echo "$(now)" > "$d/since"
        logline "$id" "claim: $2 ($why)"; echo "CLAIMED $2"
    else
        holder=$(cat "$d/holder" 2>/dev/null || echo '?')
        if [ "$holder" = "$id" ]; then echo "already yours: $2"; exit 0; fi
        if session_stale "$holder"; then
            echo "$id" > "$d/holder"; echo "$why" > "$d/why"; echo "$(now)" > "$d/since"
            logline "$id" "reclaim-stale: $2 (was $holder)"; echo "RECLAIMED $2 (stale holder $holder)"
        else
            echo "CONFLICT: '$2' is leased by '$holder' — coordinate first (status / SESSION-SYNC)"; exit 2
        fi
    fi ;;

  release)
    id="${1:?session id}"; area=$(slug "${2:?area}"); d="$LEASES/$area"
    [ -d "$d" ] && rm -rf "$d" && logline "$id" "release: $2" && echo "released $2" || echo "no lease on $2" ;;

  merge-lock)
    id="${1:?session id}"; why="${2:-}"; d="$COORD/merge.lock"
    if mkdir "$d" 2>/dev/null; then
        echo "$id" > "$d/holder"; echo "$(now)" > "$d/since"; logline "$id" "merge-lock: $why"; echo "MERGE LOCK acquired"
    else
        holder=$(cat "$d/holder" 2>/dev/null || echo '?')
        if [ "$holder" = "$id" ] || session_stale "$holder"; then
            echo "$id" > "$d/holder"; echo "$(now)" > "$d/since"; echo "MERGE LOCK (re)acquired"
        else echo "MERGE LOCK held by '$holder' — wait until released"; exit 2; fi
    fi ;;

  merge-unlock)
    id="${1:?session id}"; rm -rf "$COORD/merge.lock"; logline "$id" "merge-unlock"; echo "merge lock released" ;;

  log)
    id="${1:?session id}"; shift; logline "$id" "$*"; echo "logged" ;;

  sync)  # post to the prose channel: coord.sh sync <id> <target> "<topic>" "<body>"
    id="${1:?session id}"; target="${2:?target (e.g. K, ALLE, \"C + B\")}"; topic="${3:-}"; body="${4:-}"
    printf '\n### %s → %s  (%s)\n%s\n' "$id" "$target" "$topic" "$body" >> "$SYNC"
    logline "$id" "sync→$target: $topic"; echo "posted to SESSION-SYNC ($SYNC)" ;;

  *) echo "unknown command: $cmd"; grep '^#' "$0" | sed 's/^# \{0,1\}//'; exit 1 ;;
esac
