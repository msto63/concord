#!/usr/bin/env bash
# Concord hook common library. Sourced by the hook scripts. EVERYTHING here is
# fail-open: any error must leave the session working normally.
# Paths are DERIVED by convention, never from the environment (F-config — there is no
# ambient location authority). This library lives in <coord>/hooks/, so the coordination
# dir is its parent; the project repo + prose channel follow the project-agnostic naming
# convention (<repo>-coord, <repo>-SESSION-SYNC.md, both siblings of <repo>).
_libdir="$(cd -P "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd)"
COORD="$(dirname "$_libdir")"
HOOKS="$COORD/hooks"
PROJECT="${COORD%-coord}"
SYNC="${COORD%-coord}-SESSION-SYNC.md"
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

# Is COORD_SH the Rust binary or the legacy shell `coord.sh`? Only the Rust binary takes the
# explicit `--coord` bootstrap flag; the shell rejects it (and resolves the coord itself). The
# shell candidates end in `coord.sh`; anything else is a concord binary. (Edge case: a
# $CONCORD_BIN pointing at a `…coord.sh` is classified as the shell — acceptable, since
# CONCORD_BIN is meant to name a concord binary.)
case "$COORD_SH" in
  ""|*coord.sh) COORD_SH_RUST="" ;;
  *)           COORD_SH_RUST=1 ;;
esac

# Drive the resolved coordinator with a verb. The hooks already computed COORD from their own
# location (<coord>/hooks), but the Rust binary otherwise re-resolves location by cwd
# convention — which is WRONG from a `<repo>-<id>` suffix worktree (it would derive
# `<repo>-<id>-coord` instead of `<repo>-coord`). So pass the already-correct COORD explicitly,
# but ONLY to the Rust binary; the shell can't take `--coord` and resolves the coord on its own.
coord() {
  if [ -n "$COORD_SH_RUST" ]; then "$COORD_SH" --coord "$COORD" "$@"; else "$COORD_SH" "$@"; fi
}

# Print the Concord session-id. Identity is an EXPLICIT declaration — never inherited
# ambiently from the location environment. Resolution order (all three explicit/structural):
#   1. $CONCORD_ID — the explicit launch override (`CONCORD_ID=hub claude …`), the peer of
#      $CONCORD_BIN. This is how a session that shares ONE checkout with others (the dogfood
#      fleet, all running from the main repo) names itself: convention/marker key off the
#      worktree, so they cannot tell two sessions in one worktree apart — only this can.
#   2. idbind marker keyed by the worktree (git toplevel), written by `concord start` — the
#      normal one-session-per-worktree topology needs no env at all.
#   3. convention: the worktree basename `<repo>-<id>` → the `<id>` suffix.
# Empty if none resolve → hooks no-op (never guess a wrong id).
concord_id() {
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
