#!/usr/bin/env bash
# Concord hook common library. Sourced by the hook scripts. EVERYTHING here is
# fail-open: any error must leave the session working normally.
# Paths are DERIVED, not wired. This library lives in <coord>/hooks/, so the
# coordination dir is its parent; the project repo + prose channel follow the
# project-agnostic naming convention (<repo>-coord, <repo>-SESSION-SYNC.md, both
# siblings of <repo>). Env (exported by `concord` at launch) wins; the location
# derivation is the fallback for sessions not launched via concord.
_libdir="$(cd -P "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd)"
COORD="${CONCORD_DIR:-${AIS_COORD_DIR:-$(dirname "$_libdir")}}"
HOOKS="$COORD/hooks"
PROJECT="${CONCORD_PROJECT:-${AIS_PROJECT_DIR:-${COORD%-coord}}}"
SYNC="${CONCORD_SYNC:-${AIS_SYNC_FILE:-${COORD%-coord}-SESSION-SYNC.md}}"
COORD_SH=""
for c in "$PROJECT/tools/coord.sh" "$PROJECT"-*/tools/coord.sh; do
  [ -x "$c" ] && { COORD_SH="$c"; break; }
done

# Print the Concord session-id. Source of truth = the CONCORD_ID env var, set when
# the session is launched (`CONCORD_ID=E claude …`). This is robust: it does NOT
# depend on the working directory (all sessions here run from the main repo, so the
# cwd does NOT identify the logical session). Empty if unset → hooks no-op (never
# guess a wrong id). Optional fallback file lets a running session self-declare.
concord_id() {
  if [ -n "${CONCORD_ID:-}" ]; then printf '%s' "$CONCORD_ID"; return 0; fi
  # Fallback: a marker a session may write to claim its id for this tty/pane.
  local tty mk
  tty=$(tty 2>/dev/null); mk="$COORD/idbind/$(printf '%s' "${tty:-none}" | tr '/ ' '__')"
  [ -f "$mk" ] && printf '%s' "$(cat "$mk" 2>/dev/null)"
}

# ANSI colour for a session id (for the statusline). Case-insensitive (K == k).
concord_colour() {
  case "$(printf '%s' "$1" | tr 'a-z' 'A-Z')" in
    A) printf '36' ;;  B) printf '34' ;;  C) printf '32' ;;
    D) printf '35' ;;  E) printf '33' ;;  K) printf '90' ;;
    *) printf '37' ;;
  esac
}

# Slugify a path the way coord.sh does (/, space -> _).
concord_slug() { printf '%s' "$1" | tr '/ ' '__'; }

# Read a session field (focus|heartbeat|started) from the registry.
concord_field() {  # <id> <field>
  sed -n "s/^$2=//p" "$COORD/sessions/$1" 2>/dev/null | head -1
}
