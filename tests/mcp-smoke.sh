#!/usr/bin/env bash
# Concord WP12 M3-lean — concord-mcp stdio smoke test.
#
# Drives the MCP server over stdio with a real JSON-RPC handshake (initialize →
# initialized → tools/list → tools/call) and asserts that (1) exactly the enforced
# primitives are exposed as tools, and (2) calls execute against the real store.
# This is the enforced-core-as-typed-MCP surface (WP9-lean), not a broad tool set.
#
# Usage: tests/mcp-smoke.sh
set -euo pipefail

HERE="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${CONCORD_MCP_BIN:-$HERE/target/debug/concord-mcp}"
[ -x "$BIN" ] || ( cd "$HERE" && cargo build -p concord-mcp -q )
[ -x "$BIN" ] || { echo "FATAL: concord-mcp not built at $BIN"; exit 1; }

W=$(mktemp -d "${TMPDIR:-/tmp}/mcp-smoke.XXXXXX"); CD="$W/coord"; SY="$W/SYNC.md"; mkdir -p "$CD"
trap 'rm -rf "$W"' EXIT
fail=0

# Helper: run one MCP session (initialize + initialized + the given request lines).
mcp() {  # <request-json-line>...
  { printf '%s\n' \
     '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
     '{"jsonrpc":"2.0","method":"notifications/initialized"}'
    printf '%s\n' "$@"
  } | CONCORD_DIR="$CD" CONCORD_SYNC="$SY" perl -e 'alarm 15; exec @ARGV' "$BIN" 2>/dev/null || true
}

# Session 1 — discover tools + WRITE state (register + claim).
# (Tool calls within one pipelined batch may be dispatched concurrently — each Store op
#  is atomic, but cross-call ordering isn't guaranteed; so reads happen in session 2.)
OUT=$(mcp \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' \
  '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"register","arguments":{"id":"hub","focus":"coordinator"}}}' \
  '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"claim","arguments":{"id":"hub","area":"kernel/src/main.rs","why":"edit"}}}')
# Session 2 — READ state written by session 1 (deterministic: session 1 has exited).
OUT2=$(mcp \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"verify","arguments":{"id":"hub","area":"kernel/src/main.rs"}}}')

# 1) The enforced primitives are all exposed as tools.
expected=(register heartbeat status claim release verify merge_lock merge_unlock)
missing=""
for t in "${expected[@]}"; do
  echo "$OUT" | grep -q "\"name\":\"$t\"" || missing="$missing $t"
done
if [ -z "$missing" ]; then echo "✓ tools/list exposes all 8 enforced primitives"; else echo "✗ missing tools:$missing"; fail=1; fi

# 2) No commodity-breadth tools leaked in (board/message/thread/search etc.).
if echo "$OUT" | grep -qE '"name":"(board|send|search|thread|message|inbox)"'; then
  echo "✗ unexpected commodity tool exposed (M3-lean should be enforced-only)"; fail=1
else echo "✓ no commodity-breadth tools (enforced-only surface)"; fi

# 3) Calls execute against the real store.
echo "$OUT" | grep -q "registered session 'hub'" && echo "✓ register executed" || { echo "✗ register"; fail=1; }
echo "$OUT" | grep -q "CLAIMED kernel/src/main.rs" && echo "✓ claim executed" || { echo "✗ claim"; fail=1; }
echo "$OUT2" | grep -q "HELD by hub" && echo "✓ verify (session 2) reflects the held lease" || { echo "✗ verify"; fail=1; }

echo ""
if [ "$fail" = 0 ]; then echo "MCP SMOKE: ALL PASS"; else echo "MCP SMOKE: FAILURES"; fi
exit $fail
