#!/usr/bin/env bash
# Concord WP12 S2.1 — symbol-level (AST) leases.
#
# Proves the killer capability path-leases cannot express: two sessions hold leases on
# DISJOINT symbols in the SAME file in parallel, while a file path-lease still subsumes
# (bidirectionally) any symbol-lease in it — all ENFORCED (the symbol-lease is a finer
# lease in the same enforced model, unlike wit's advisory symbol locks).
set -euo pipefail

HERE="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${CONCORD_BIN:-$HERE/target/debug/concord}"
[ -x "$BIN" ] || ( cd "$HERE" && cargo build -p concord -q )
[ -x "$BIN" ] || { echo "FATAL: concord not built at $BIN"; exit 1; }

W=$(mktemp -d "${TMPDIR:-/tmp}/concord-sym.XXXXXX"); trap 'rm -rf "$W"' EXIT
PROJ="$W/proj"; mkdir -p "$PROJ/src"; ( cd "$PROJ" && git init -q )
cat > "$PROJ/src/lib.rs" <<'RS'
pub fn foo() -> u32 { 1 }
pub fn bar() -> u32 { 2 }
struct Baz;
pub fn caller() -> u32 { foo() + 1 }
RS
cat > "$PROJ/src/app.ts" <<'TS'
export function greet(): string { return "hi"; }
class Widget {}
TS
cat > "$PROJ/svc.py" <<'PY'
def serve():
    pass

class Server:
    def run(self):
        pass
PY
fail=0
# Run the tool with cwd = the project, so it derives proj/coord purely by convention.
# (F-config: the binary reads no location env, so there is nothing to leak or clear —
# the incident vector is gone by construction.)
run() { ( cd "$PROJ" && "$BIN" "$@" ); }
chk() { if printf '%s' "$2" | grep -qF "$3"; then echo "✓ $1"; else echo "✗ $1 — want '$3' in: $2"; fail=1; fi; }
chkx() { if [ "$2" = "$3" ]; then echo "✓ $1 (exit $2)"; else echo "✗ $1 — exit $2 != $3"; fail=1; fi; }
# `&& rc=0 || rc=$?` keeps a non-zero exit (CONFLICT/OVERLAP=2) from tripping `set -e`.

run init --ids a,b,c >/dev/null

# `symbols` lists the file's symbols.
o=$(run symbols src/lib.rs) && rc=0 || rc=$?; chk "symbols lists foo" "$o" "src/lib.rs:foo"; chk "symbols lists Baz [struct]" "$o" "[struct]"

# a claims foo; b claims bar in the SAME FILE → both succeed (the killer capability).
o=$(run claim a src/lib.rs:foo "edit foo") && rc=0 || rc=$?; chk "a claims foo" "$o" "CLAIMED"; chkx "a claim foo ok" "$rc" 0
o=$(run claim b src/lib.rs:bar "edit bar") && rc=0 || rc=$?; chk "b claims bar (disjoint symbol, SAME file)" "$o" "CLAIMED"; chkx "b claim bar ok" "$rc" 0

# same symbol conflicts.
o=$(run claim c src/lib.rs:foo "want foo too") && rc=0 || rc=$?; chk "c claims foo → CONFLICT (same symbol)" "$o" "CONFLICT"; chkx "c claim foo exit 2" "$rc" 2

# a file path-lease overlaps a held symbol-lease (bidirectional: held symbol blocks path).
o=$(run claim c src/lib.rs "whole file") && rc=0 || rc=$?; chk "c claims the whole file → OVERLAP (subsumes symbols)" "$o" "OVERLAP"; chkx "c file overlap exit 2" "$rc" 2

# advisory note for a symbol that doesn't exist (claims anyway).
# (the advisory note goes to stderr — capture both streams for this check)
o=$(run claim a src/lib.rs:nonexistent "future fn" 2>&1) && rc=0 || rc=$?; chk "claiming a missing symbol notes + still CLAIMED" "$o" "not found"; chk "...and is claimed" "$o" "CLAIMED"

# release a symbol; then the whole-file path-lease is takeable once all symbols freed.
run release a src/lib.rs:foo >/dev/null; run release b src/lib.rs:bar >/dev/null; run release a src/lib.rs:nonexistent >/dev/null
o=$(run claim c src/lib.rs "now free") && rc=0 || rc=$?; chk "after releasing all symbols, file path-lease succeeds" "$o" "CLAIMED"; chkx "c file claim ok" "$rc" 0

# ── S2.2: call-graph DEP_CHAIN advisory + TS/Python symbols ──
run release c src/lib.rs >/dev/null   # free the whole-file lease from the S2.1 section
# a holds foo; b claims caller (which CALLS foo) → advisory DEP_CHAIN note, still CLAIMED.
o=$(run claim a src/lib.rs:foo "edit foo" 2>&1) && rc=0 || rc=$?
o=$(run claim b src/lib.rs:caller "edit caller" 2>&1) && rc=0 || rc=$?
chk "DEP_CHAIN: caller→foo warns (advisory)" "$o" "DEP_CHAIN"
chk "DEP_CHAIN: caller still CLAIMED (enforced lease, advisory warning)" "$o" "CLAIMED"
run release a src/lib.rs:foo >/dev/null; run release b src/lib.rs:caller >/dev/null

# TypeScript + Python symbol extraction via the same `symbols` command.
o=$(run symbols src/app.ts); chk "TS symbols: greet" "$o" "src/app.ts:greet"; chk "TS symbols: Widget [class]" "$o" "[class]"
o=$(run symbols svc.py);     chk "Python symbols: serve" "$o" "svc.py:serve"; chk "Python symbols: run (method)" "$o" "svc.py:run"

echo ""
if [ "$fail" = 0 ]; then echo "SYMBOL LOCKS: ALL PASS — disjoint symbols parallel, path subsumes, enforced; DEP_CHAIN advisory; TS/Python"; else echo "SYMBOL LOCKS: FAILURES"; fi
exit $fail
