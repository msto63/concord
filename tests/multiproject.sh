#!/usr/bin/env bash
# Concord WP12 M5.1 — multi-project init/paths isolation test.
#
# Proves that `concord init` + `concord paths` derive a per-project coordination state
# and that two projects are ISOLATED (no cross-talk) — the multi-project foundation
# the dogfood (M5.2) builds on.
#
# F-config: there is no location env at all — each project's coord derives purely from its
# git toplevel by convention, so a "temp" test can no longer be redirected at the real
# ais-coord by a leaked variable (the prep-incident vector is gone by construction).
set -euo pipefail

HERE="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${CONCORD_BIN:-$HERE/target/debug/concord}"
[ -x "$BIN" ] || ( cd "$HERE" && cargo build -p concord -q )
[ -x "$BIN" ] || { echo "FATAL: concord not built at $BIN"; exit 1; }

W=$(mktemp -d "${TMPDIR:-/tmp}/concord-mp.XXXXXX"); trap 'rm -rf "$W"' EXIT
fail=0

# concord resolves each project by convention from cwd / --project; no env involved.
cc() { "$BIN" "$@"; }

mkdir -p "$W/projA" "$W/projB"
( cd "$W/projA" && git init -q )
( cd "$W/projB" && git init -q )

# init two isolated projects.
cc init --project "$W/projA" --ids a,hub >/dev/null
cc init --project "$W/projB" --ids x >/dev/null

# Resolve each project's coord dir via `paths` (robust to realpath, e.g. /private/tmp).
cdA=$( ( cd "$W/projA" && cc paths ) | sed -n 's/^COORD=//p')
cdB=$( ( cd "$W/projB" && cc paths ) | sed -n 's/^COORD=//p')
syA=$( ( cd "$W/projA" && cc paths ) | sed -n 's/^SYNC=//p')

# 1) per-project derivation: distinct coord dirs ending in <repo>-coord.
if [ -n "$cdA" ] && [ -n "$cdB" ] && [ "$cdA" != "$cdB" ] \
   && [ "$(basename "$cdA")" = "projA-coord" ] && [ "$(basename "$cdB")" = "projB-coord" ]; then
  echo "✓ paths derives a distinct <repo>-coord per project (projA-coord ≠ projB-coord)"
else echo "✗ per-project derivation wrong: A=$cdA B=$cdB"; fail=1; fi

# 2) init scaffolded the registered sessions in the RIGHT project.
if [ -f "$cdA/sessions/a" ] && [ -f "$cdA/sessions/hub" ] && [ -f "$cdB/sessions/x" ]; then
  echo "✓ init registered ids into each project's coord"
else echo "✗ init sessions missing"; fail=1; fi

# 3) ISOLATION: projA's ids are not in projB and vice versa (no cross-talk).
if [ ! -e "$cdA/sessions/x" ] && [ ! -e "$cdB/sessions/a" ] && [ ! -e "$cdB/sessions/hub" ]; then
  echo "✓ isolation: no session cross-talk between projects"
else echo "✗ cross-talk detected"; fail=1; fi

# 4) init scaffolded the prose channel with a header.
if [ -f "$syA" ] && grep -q "Concord prose channel" "$syA"; then
  echo "✓ init scaffolded the prose channel (<repo>-SESSION-SYNC.md) with a header"
else echo "✗ sync channel missing/empty: $syA"; fail=1; fi

# 5) `eval "$(concord paths)"` yields usable local path vars for scripts.
ev=$( cd "$W/projA" && eval "$(cc paths)" && printf '%s' "$COORD" )
if [ "$ev" = "$cdA" ]; then echo "✓ eval \"\$(concord paths)\" sets a usable \$COORD"; else echo "✗ eval paths broken"; fail=1; fi

# 6) idempotent: re-init does not error or clobber.
cc init --project "$W/projA" --ids a,hub >/dev/null && echo "✓ init is idempotent (re-run ok)" || { echo "✗ re-init failed"; fail=1; }

echo ""
if [ "$fail" = 0 ]; then echo "MULTIPROJECT: ALL PASS"; else echo "MULTIPROJECT: FAILURES"; fi
exit $fail
