#!/usr/bin/env bash
# Concord PostToolUse hook — bump the heartbeat on every tool use, so "alive"
# couples automatically to real activity (a dormant session stops heart-beating
# and goes stale on its own). Fail-open, silent, fast.
. "$(dirname "$0")/lib.sh" 2>/dev/null || exit 0
id=$(concord_id); [ -z "$id" ] && exit 0
[ -n "$COORD_SH" ] && "$COORD_SH" heartbeat "$id" >/dev/null 2>&1
exit 0
