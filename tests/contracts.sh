#!/usr/bin/env bash
# Concord WP12 F5 — enforced signature contracts.
#
# Proves: a contract pins the SIGNATURE (body edits don't break it); a signature change
# is BROKEN (commit/merge gate exit 2); the merge-lock precondition refuses; --update
# renegotiates; release drops it. Fail-open on unreadable symbols.
set -uo pipefail

HERE="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${CONCORD_BIN:-$HERE/target/debug/concord}"
[ -x "$BIN" ] || ( cd "$HERE" && cargo build -p concord -q )
[ -x "$BIN" ] || { echo "FATAL: concord not built at $BIN"; exit 1; }

W=$(mktemp -d "${TMPDIR:-/tmp}/concord-ct.XXXXXX"); trap 'rm -rf "$W"' EXIT
PROJ="$W/proj"; mkdir -p "$PROJ/src"; ( cd "$PROJ" && git init -q )
cat > "$PROJ/src/lib.rs" <<'RS'
pub fn validate(t: &str) -> bool { !t.is_empty() }
pub struct Api { token: String }
RS
fail=0
R() { ( cd "$PROJ" && env -u CONCORD_DIR -u CONCORD_SYNC -u CONCORD_PROJECT -u AIS_COORD_DIR -u AIS_SYNC_FILE -u AIS_PROJECT_DIR "$BIN" "$@" 2>&1 ); }
chk() { if printf '%s' "$2" | grep -qF "$3"; then echo "✓ $1"; else echo "✗ $1 — want '$3' in: $2"; fail=1; fi; }
chkx() { if [ "$2" = "$3" ]; then echo "✓ $1 (exit $2)"; else echo "✗ $1 — exit $2 != $3"; fail=1; fi; }

R init --ids a,b >/dev/null

o=$(R contract a src/lib.rs:validate --with b "agreed"); chk "register pins the signature" "$o" "pub fn validate(t: &str) -> bool"
o=$(R contracts); chk "contracts lists it with parties" "$o" "by a with b"

o=$(R contract-check) && rc=0 || rc=$?; chk "unchanged → OK" "$o" "contracts OK"; chkx "OK exit 0" "$rc" 0

# Body edit must NOT break the contract (signature pinned, not the body) — the key property.
sed -i.bak 's/!t.is_empty()/t.len() > 0/' "$PROJ/src/lib.rs"
o=$(R contract-check) && rc=0 || rc=$?; chk "body edit does NOT break" "$o" "contracts OK"; chkx "body-edit OK exit 0" "$rc" 0

# Signature change BREAKS it (gate exit 2).
sed -i.bak 's/pub fn validate(t: &str)/pub fn validate(t: \&str, n: u32)/' "$PROJ/src/lib.rs"
o=$(R contract-check) && rc=0 || rc=$?; chk "signature change → BROKEN" "$o" "CONTRACT BROKEN"; chkx "broken exit 2" "$rc" 2

# The merge-lock precondition refuses while broken.
o=$(R merge-lock a "merge") && rc=0 || rc=$?; chk "merge-lock refused on broken contract" "$o" "merge-lock refused"; chkx "merge-lock refused exit 2" "$rc" 2
o=$(R merge-lock a "merge" --no-contract-check) && rc=0 || rc=$?; chk "override merges anyway" "$o" "MERGE LOCK"; R merge-unlock a >/dev/null

# Renegotiate (--update) re-pins, gate clears.
o=$(R contract a src/lib.rs:validate --update "renegotiated"); chk "update re-pins signature" "$o" "CONTRACT-UPDATED"
o=$(R contract-check) && rc=0 || rc=$?; chk "after update → OK" "$o" "contracts OK"; chkx "post-update OK exit 0" "$rc" 0

# Fail-open: a contract on a since-deleted symbol is not 'broken'.
R contract a "src/lib.rs:Api" >/dev/null
sed -i.bak '/pub struct Api/d' "$PROJ/src/lib.rs"
o=$(R contract-check) && rc=0 || rc=$?; chkx "fail-open when symbol vanished (not broken)" "$rc" 0

# Release drops it.
o=$(R contract-release a src/lib.rs:validate); chk "release drops the contract" "$o" "CONTRACT-RELEASED"

echo ""
if [ "$fail" = 0 ]; then echo "CONTRACTS: ALL PASS — signature pinned (body-safe), break detected, merge-lock gate, --update, fail-open, release"; else echo "CONTRACTS: FAILURES"; fi
exit $fail
