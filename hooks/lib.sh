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
# The coordination CLI the hooks drive. Resolution order:
#   1. $CONCORD_BIN              — an explicit override.
#   2. the PROJECT's own target/ build — a checked-out concord repo uses its fresh build.
#   3. a global `concord` on PATH — an installed binary (curl|sh / cargo install / release);
#      this is what lets a project with NO local build (e.g. ais) drive the Rust tool.
#   4. the shell coord.sh        — the instant fallback if no concord is present.
# The Rust binary and the shell read/write the SAME on-disk state, so falling back to the
# shell is safe; and with no concord anywhere the hooks stay on the shell — fail-safe.
COORD_SH=""
for c in "${CONCORD_BIN:-}" \
         "$PROJECT/target/release/concord" "$PROJECT/target/debug/concord" \
         "$(command -v concord 2>/dev/null)" \
         "$PROJECT/tools/coord.sh" "$PROJECT/bin/coord.sh" "$PROJECT"-*/tools/coord.sh; do
  [ -n "$c" ] && [ -x "$c" ] && { COORD_SH="$c"; break; }
done

# Print the Concord session-id. Source of truth = the CONCORD_ID env var, set when
# the session is launched (`CONCORD_ID=E claude …`). This is robust: it does NOT
# depend on the working directory (all sessions here run from the main repo, so the
# cwd does NOT identify the logical session). Empty if unset → hooks no-op (never
# guess a wrong id). Optional fallback file lets a running session self-declare.
concord_id() {
  # F-config: env is retired (convention-first). Resolution order:
  #  1. $CONCORD_ID — legacy, honored for continuity (deprecated; removed next release).
  #  2. idbind marker keyed by the worktree (git toplevel), written by `concord start`.
  #  3. convention: the worktree basename `<repo>-<id>` → the `<id>` suffix.
  if [ -n "${CONCORD_ID:-}" ]; then printf '%s' "$CONCORD_ID"; return 0; fi
  local top mk
  top=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
  mk="$COORD/idbind/$(printf '%s' "$top" | tr '/ ' '__')"
  [ -f "$mk" ] && { printf '%s' "$(cat "$mk" 2>/dev/null)"; return 0; }
  # Convention fallback: <repo>-<id> worktree name.
  local proj repo base
  proj="${PROJECT:-${COORD%-coord}}"; repo=$(basename "$proj")
  base=$(basename "$top")
  case "$base" in "$repo"-?*) printf '%s' "${base#"$repo"-}";; esac
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
