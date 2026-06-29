#!/usr/bin/env bash
# Concord SessionEnd hook (F1/A2) — clean-exit teardown. On a session ending, release
# all of its leases, drop the merge-lock if held, and deregister — so a finished session
# stops appearing to hold authority immediately, instead of waiting out the TTL-stale
# window. Idempotent and fail-open (never block the session's exit).
. "$(dirname "$0")/lib.sh" 2>/dev/null || exit 0
id=$(concord_id); [ -z "$id" ] && exit 0
[ -n "$COORD_SH" ] || exit 0
"$COORD_SH" session-end "$id" >/dev/null 2>&1
exit 0
