#!/usr/bin/env bash
# Concord WP12 M4.1 — `concord install-hooks` smoke.
#
# Proves the embedded hook scripts (include_str!'d into the binary) materialize into
# <coord>/hooks/ byte-identically, with the right exec bits, AND that the wiring step
# merges ~/.claude/settings.json (run against a FAKE HOME so the real one is untouched —
# the env-leak incident lesson applied to $HOME).
set -euo pipefail

HERE="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${CONCORD_BIN:-$HERE/target/debug/concord}"
[ -x "$BIN" ] || ( cd "$HERE" && cargo build -p concord -q )
[ -x "$BIN" ] || { echo "FATAL: concord not built at $BIN"; exit 1; }

W=$(mktemp -d "${TMPDIR:-/tmp}/concord-ih.XXXXXX"); trap 'rm -rf "$W"' EXIT
PROJ="$W/proj"; mkdir -p "$PROJ"; ( cd "$PROJ" && git init -q )
COORD="$W/proj-coord"
fail=0
# cwd = project (convention-derive), cleared env, FAKE HOME for the settings wiring.
run() { ( cd "$PROJ" && env -u CONCORD_DIR -u CONCORD_SYNC -u CONCORD_PROJECT \
          -u AIS_COORD_DIR -u AIS_SYNC_FILE -u AIS_PROJECT_DIR HOME="$W/home" "$BIN" "$@" ); }
chk() { if printf '%s' "$2" | grep -qF "$3"; then echo "✓ $1"; else echo "✗ $1 — want '$3' in: $2"; fail=1; fi; }

mkdir -p "$W/home/.claude"
echo '{"existingKey":"keepme","statusLine":{"type":"command","command":"old"}}' > "$W/home/.claude/settings.json"
run init >/dev/null

# Default install-hooks wires settings (Unix). Should succeed and report the laydown.
o=$(run install-hooks) && rc=0 || rc=$?
chk "install-hooks writes 9 files" "$o" "9 hook files"
chk "install-hooks exit 0" "$rc" 0

# Files present in <coord>/hooks/.
for f in lib.sh session-start.sh user-prompt.sh post-tool.sh pre-tool.sh statusline.sh install.sh uninstall.sh shared-regions; do
  [ -f "$COORD/hooks/$f" ] || { echo "✗ missing $f"; fail=1; }
done
[ "$fail" = 0 ] && echo "✓ all 9 hook files materialized"

# Byte-identical to the repo source.
diff -q "$HERE/hooks/lib.sh" "$COORD/hooks/lib.sh" >/dev/null && echo "✓ lib.sh byte-identical" || { echo "✗ lib.sh differs"; fail=1; }
diff -q "$HERE/hooks/shared-regions" "$COORD/hooks/shared-regions" >/dev/null && echo "✓ shared-regions byte-identical" || { echo "✗ shared-regions differs"; fail=1; }

# Exec bits: .sh executable, shared-regions not.
[ -x "$COORD/hooks/lib.sh" ] && echo "✓ lib.sh executable" || { echo "✗ lib.sh not executable"; fail=1; }
[ -x "$COORD/hooks/shared-regions" ] && { echo "✗ shared-regions should NOT be executable"; fail=1; } || echo "✓ shared-regions not executable"

# Settings wired (Unix): the 4 hook keys + statusLine present, existing key preserved.
S="$W/home/.claude/settings.json"
if command -v python3 >/dev/null 2>&1; then
  python3 - "$S" <<'PY' && echo "✓ settings.json wired (hooks + statusLine, existing key kept)" || { echo "✗ settings wiring incomplete"; exit 1; }
import sys, json
d = json.load(open(sys.argv[1]))
assert d.get("existingKey") == "keepme", "existing key dropped"
assert d.get("statusLine"), "no statusLine"
hooks = d.get("hooks", {})
for k in ("SessionStart","UserPromptSubmit","PostToolUse","PreToolUse"):
    assert k in hooks, f"missing hook {k}"
PY
else
  echo "… python3 absent — skipped settings-content check"
fi

# --no-wire leaves settings untouched: re-point HOME to a fresh dir and verify no write.
mkdir -p "$W/home2/.claude"; echo '{"k":1}' > "$W/home2/.claude/settings.json"
o=$( cd "$PROJ" && env -u CONCORD_DIR HOME="$W/home2" "$BIN" install-hooks --no-wire ) && rc=0 || rc=$?
chk "--no-wire reports skip" "$o" "no-wire"
[ "$(cat "$W/home2/.claude/settings.json")" = '{"k":1}' ] && echo "✓ --no-wire left settings.json untouched" || { echo "✗ --no-wire mutated settings"; fail=1; }

echo ""
if [ "$fail" = 0 ]; then echo "INSTALL-HOOKS: ALL PASS — embedded scripts materialize byte-identically, exec bits, settings wired, --no-wire safe"; else echo "INSTALL-HOOKS: FAILURES"; fi
exit $fail
