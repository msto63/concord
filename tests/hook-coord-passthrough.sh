#!/usr/bin/env bash
# Concord v0.12.1 — hooks pass --coord to the Rust binary (multi-worktree fix).
#
# The hooks compute COORD from their own location (<coord>/hooks), but the Rust binary
# otherwise re-resolves location by cwd convention. From a `<repo>-<id>` SUFFIX worktree that
# would derive `<repo>-<id>-coord` instead of `<repo>-coord` — so a session in `ais-a` would
# write to `ais-a-coord`, not `ais-coord`. The fix: the `coord()` wrapper passes the already
# correct `--coord "$COORD"` — but ONLY to the Rust binary, since the legacy shell `coord.sh`
# rejects `--coord` (and resolves the coord itself).
#
# Part A (functional): a hook call from a suffix worktree writes to the RIGHT coord.
# Part B (shell-compat): the wrapper adds --coord for the Rust binary, never for coord.sh.
set -uo pipefail

HERE="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${CONCORD_BIN:-$HERE/target/debug/concord}"
[ -x "$BIN" ] || ( cd "$HERE" && cargo build -p concord -q )
[ -x "$BIN" ] || { echo "FATAL: concord not built at $BIN"; exit 1; }

W=$(mktemp -d "${TMPDIR:-/tmp}/concord-hcp.XXXXXX"); trap 'rm -rf "$W"' EXIT
ok() { echo "✓ $1"; }; no() { echo "✗ $1"; fail=1; }
fail=0

# ── Part A: functional — suffix worktree writes to <coord>, not <repo>-<id>-coord ──
# Deployed layout: hooks live in <coord>/hooks (as install-hooks materializes them); the
# session runs in the `repo-a` suffix worktree. COORD derives to $W/repo-coord from the hook
# location; without the --coord passthrough the binary would mis-resolve to $W/repo-a-coord.
mkdir -p "$W/repo-coord/hooks"; cp "$HERE"/hooks/*.sh "$W/repo-coord/hooks/"
mkdir -p "$W/repo-a"; ( cd "$W/repo-a" && git init -q )

info=$( cd "$W/repo-a" && CONCORD_ID=a CONCORD_BIN="$BIN" bash -c '
  . "'"$W"'/repo-coord/hooks/lib.sh" 2>/dev/null
  coord register a "suffix worktree test" >/dev/null 2>&1
  printf "COORD=%s RUST=%s" "$COORD" "$COORD_SH_RUST"' )

# (suffix-match COORD: macOS canonicalizes /var → /private/var via `cd -P` in lib.sh).
case "$info" in *"/repo-coord "*) ok "COORD derived from hook location (repo-coord)";;
  *) no "COORD wrong: $info";; esac
case "$info" in *"RUST=1"*) ok "binary classified as Rust (gets --coord)";; *) no "RUST flag unset: $info";; esac
[ -f "$W/repo-coord/sessions/a" ] && ok "register from repo-a wrote to the RIGHT coord (repo-coord)" \
  || no "register did not write repo-coord/sessions/a"
[ ! -e "$W/repo-a-coord" ] && ok "did NOT create the bogus suffix coord (repo-a-coord)" \
  || no "BUG: a suffix coord repo-a-coord was created"

# ── Part B: shell-compat — coord() adds --coord only for the Rust binary ──
mkdir -p "$W/b"
printf '#!/bin/sh\necho "ARGS:$*"\n' > "$W/b/concord";  chmod +x "$W/b/concord"   # rust-like (name concord)
printf '#!/bin/sh\necho "ARGS:$*"\n' > "$W/b/coord.sh"; chmod +x "$W/b/coord.sh"  # legacy shell
. "$W/repo-coord/hooks/lib.sh" 2>/dev/null   # bring the coord() function into scope
wrap() {  # <coord_sh> -> what coord() actually invokes
  COORD_SH="$1"; COORD="/X"
  case "$COORD_SH" in ""|*coord.sh) COORD_SH_RUST="" ;; *) COORD_SH_RUST=1 ;; esac
  coord register a "x"
}
ro=$(wrap "$W/b/concord")
case "$ro" in *"--coord /X"*) ok "Rust binary is invoked WITH --coord";; *) no "rust missing --coord: $ro";; esac
so=$(wrap "$W/b/coord.sh")
case "$so" in *"--coord"*) no "shell coord.sh wrongly got --coord: $so";; *) ok "shell coord.sh invoked WITHOUT --coord (compat intact)";; esac

echo ""
if [ "$fail" = 0 ]; then echo "HOOK-COORD-PASSTHROUGH: ALL PASS — suffix worktree writes the right coord; shell path unchanged"; else echo "HOOK-COORD-PASSTHROUGH: FAILURES"; fi
exit $fail
