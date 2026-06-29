#!/usr/bin/env bash
# Concord — hooks/lib.sh COORD_SH resolution order.
#
# Verifies the resolution precedence: $CONCORD_BIN > project-local target/ build >
# a global `concord` on PATH > shell coord.sh > nothing. The key new case: a project with
# NO local build picks up a globally-installed `concord` (so ais and any installed-only
# project drive the Rust tool), while staying fail-safe when none is present.
set -uo pipefail

HERE="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LIB="$HERE/hooks/lib.sh"
[ -f "$LIB" ] || { echo "FATAL: hooks/lib.sh not found"; exit 1; }
fail=0
W=$(mktemp -d "${TMPDIR:-/tmp}/concord-csr.XXXXXX"); trap 'rm -rf "$W"' EXIT
ok() { echo "✓ $1"; }; no() { echo "✗ $1"; fail=1; }

# Resolve COORD_SH by sourcing lib.sh with a controlled PROJECT/COORD and PATH. Returns the
# resolved COORD_SH. The PROJECT has no target/ build unless we create one.
resolve() {  # <PATH> [extra env assignments…]
  local p="$1"; shift
  env -i PATH="$p" HOME="$W" CONCORD_DIR="$W/proj-coord" CONCORD_PROJECT="$W/proj" "$@" \
    bash -c '. "'"$LIB"'" 2>/dev/null; printf "%s" "$COORD_SH"'
}

mkdir -p "$W/proj"
# A fake global `concord` on PATH.
mkdir -p "$W/gbin"; printf '#!/bin/sh\nexit 0\n' > "$W/gbin/concord"; chmod +x "$W/gbin/concord"

# 1. No build, no global concord → COORD_SH empty (fail-safe).
out=$(resolve "/usr/bin:/bin")
[ -z "$out" ] && ok "no build + no global concord → empty (fail-safe)" || no "expected empty, got: $out"

# 2. No build, global concord on PATH → resolves to the global concord.
out=$(resolve "$W/gbin:/usr/bin:/bin")
[ "$out" = "$W/gbin/concord" ] && ok "no build + global concord on PATH → global concord" || no "expected global, got: $out"

# 3. Project-local build wins over the global concord.
mkdir -p "$W/proj/target/release"; printf '#!/bin/sh\nexit 0\n' > "$W/proj/target/release/concord"; chmod +x "$W/proj/target/release/concord"
out=$(resolve "$W/gbin:/usr/bin:/bin")
[ "$out" = "$W/proj/target/release/concord" ] && ok "project-local build > global concord" || no "expected local build, got: $out"

# 4. $CONCORD_BIN wins over everything.
out=$(resolve "$W/gbin:/usr/bin:/bin" CONCORD_BIN="$W/gbin/concord")
[ "$out" = "$W/gbin/concord" ] && ok "\$CONCORD_BIN overrides all" || no "expected CONCORD_BIN, got: $out"

echo ""
if [ "$fail" = 0 ]; then echo "COORD_SH RESOLUTION: ALL PASS — \$CONCORD_BIN > local build > global PATH > (shell) > empty"; else echo "COORD_SH RESOLUTION: FAILURES"; fi
exit $fail
