#!/usr/bin/env bash
# Run the whole Concord test suite locally.
#
#   bash tests/all.sh              # every shell smoke test (auto-discovered)
#   bash tests/all.sh --with-cargo # cargo test --workspace first, then the smokes
#
# Smokes are auto-discovered (tests/*.sh) so a new one is picked up with no list to keep in
# sync. Each smoke isolates its own coordination state (a scratch dir + a cleared CONCORD_*
# env); this runner only sequences them and reports pass/fail. `make test` runs --with-cargo.
set -uo pipefail

HERE="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$HERE"
LOGS=$(mktemp -d "${TMPDIR:-/tmp}/concord-suite.XXXXXX"); trap 'rm -rf "$LOGS"' EXIT
fail=0; pass=0

if [ "${1:-}" = "--with-cargo" ]; then
  echo "══ cargo test --workspace ══"
  if cargo test --workspace --quiet; then echo "✓ cargo test"; else echo "✗ cargo test"; fail=$((fail + 1)); fi
  echo ""
fi

echo "══ shell smoke tests ══"
for t in tests/*.sh; do
  name=$(basename "$t" .sh)
  [ "$name" = "all" ] && continue
  if bash "$t" >"$LOGS/$name.log" 2>&1; then
    echo "✓ $name"; pass=$((pass + 1))
  else
    echo "✗ $name"; sed 's/^/    /' "$LOGS/$name.log" | tail -4; fail=$((fail + 1))
  fi
done

echo ""
echo "smokes: $pass passed, $fail failed"
[ "$fail" = 0 ] && echo "SUITE: ALL PASS" || { echo "SUITE: FAILURES"; exit 1; }
