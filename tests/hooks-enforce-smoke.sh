#!/usr/bin/env bash
# Concord WP12 F1 — harness-native enforcement hooks smoke.
#
# Asserts the enforcement-critical hook paths actually fire (the ADR-0003 risk-mitigation:
# "a smoke test asserts the deny path"): A1 PreToolUse deny, A6 PostToolUse out-of-scope
# audit, A3 Stop anti-going-dark, A4 PreCompact snapshot — all fail-open.
set -uo pipefail

HERE="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${CONCORD_BIN:-$HERE/target/debug/concord}"
[ -x "$BIN" ] || ( cd "$HERE" && cargo build -p concord -q )
[ -x "$BIN" ] || { echo "FATAL: concord not built at $BIN"; exit 1; }

W=$(mktemp -d "${TMPDIR:-/tmp}/concord-f1.XXXXXX"); trap 'rm -rf "$W"' EXIT
PROJ="$W/proj"; mkdir -p "$PROJ/src"; ( cd "$PROJ" && git init -q )
COORD="$W/proj-coord"; SYNC="$W/proj-SESSION-SYNC.md"
fail=0
# F-config: hooks derive COORD/SYNC purely from their own location (<coord>/hooks/). So
# materialize them into the scratch coord — exactly as `install-hooks` does — and invoke
# THOSE copies: _libdir=$COORD/hooks ⇒ COORD=$COORD, SYNC=$W/proj-SESSION-SYNC.md by
# convention. Identity is still the explicit CONCORD_ID override (a per-call launch knob).
mkdir -p "$COORD/hooks"; cp "$HERE"/hooks/*.sh "$COORD/hooks/"
R() { ( cd "$PROJ" && "$BIN" "$@" ); }
# Run a hook as session $1 with stdin from $2-payload; cwd = PROJ; explicit id override only.
hook() { ( cd "$PROJ" && env CONCORD_ID="$1" CONCORD_BIN="$BIN" bash "$COORD/hooks/$2" ); }
ok() { echo "✓ $1"; }; no() { echo "✗ $1"; fail=1; }

R init --ids a,b >/dev/null
R claim a src/x.rs "edit" >/dev/null

# ── A1: PreToolUse deny ──
out=$(printf '{"tool_name":"Edit","tool_input":{"file_path":"%s/src/x.rs"}}' "$PROJ" | hook b pre-tool.sh)
printf '%s' "$out" | python3 -c 'import json,sys; d=json.load(sys.stdin); assert d["hookSpecificOutput"]["permissionDecision"]=="deny"' 2>/dev/null \
  && ok "A1: b's edit of a's leased file is DENIED" || no "A1 deny missing (got: $out)"
out=$(printf '{"tool_name":"Edit","tool_input":{"file_path":"%s/src/free.rs"}}' "$PROJ" | hook b pre-tool.sh)
[ -z "$out" ] && ok "A1: b's edit of a free file is allowed" || no "A1 should allow free file (got: $out)"
out=$(printf '{"tool_name":"Edit","tool_input":{"file_path":"%s/src/x.rs"}}' "$PROJ" | hook a pre-tool.sh)
[ -z "$out" ] && ok "A1: a's edit of its OWN leased file is allowed" || no "A1 should allow own lease (got: $out)"
# fail-open: NO concord available at all → allow. A minimal PATH (/usr/bin:/bin) keeps
# git/python3/perl but excludes any globally-installed `concord` (the new PATH fallback in
# lib.sh) so this genuinely exercises the no-binary case on any machine.
out=$(printf '{"tool_name":"Edit","tool_input":{"file_path":"%s/src/x.rs"}}' "$PROJ" | ( cd "$PROJ" && env -i PATH=/usr/bin:/bin CONCORD_ID=b CONCORD_BIN=/nonexistent bash "$COORD/hooks/pre-tool.sh" ))
[ -z "$out" ] && ok "A1: fail-open when no concord is available" || no "A1 should fail-open (got: $out)"

# ── A6: PostToolUse out-of-scope-write audit (log, no block) ──
count_viol() { grep -c "out-of-scope-write" "$COORD/intents.jsonl" 2>/dev/null | head -1; }
before=$(count_viol); before=${before:-0}
out=$(printf '{"tool_name":"Edit","tool_input":{"file_path":"%s/src/x.rs"},"tool_output":"ok"}' "$PROJ" | hook b post-tool.sh)
[ -z "$out" ] && ok "A6: PostToolUse does not block (audit-only)" || no "A6 must not emit a decision (got: $out)"
after=$(count_viol); after=${after:-0}
[ "$after" -gt "$before" ] && ok "A6: out-of-scope write by b was logged as a violation" || no "A6 should log the violation ($before→$after)"

# ── A3: Stop anti-going-dark ──
# No directive to b yet → allow stop.
out=$(printf '{"stop_hook_active":false}' | hook b stop.sh)
[ -z "$out" ] && ok "A3: no pending directive → stop allowed" || no "A3 should allow when nothing pending (got: $out)"
# A coordinator directive addressed to b appears → block.
printf '\n### hub → b  (GO: do the thing)\nbody\n' >> "$SYNC"
out=$(printf '{"stop_hook_active":false}' | hook b stop.sh)
printf '%s' "$out" | python3 -c 'import json,sys; assert json.load(sys.stdin)["decision"]=="block"' 2>/dev/null \
  && ok "A3: un-ACK'd directive → stop BLOCKED" || no "A3 should block on pending directive (got: $out)"
# Loop-guard: stop_hook_active=true → allow even with pending directive.
out=$(printf '{"stop_hook_active":true}' | hook b stop.sh)
[ -z "$out" ] && ok "A3: stop_hook_active guard prevents an endless turn" || no "A3 loop-guard failed (got: $out)"
# After b posts its own line (ACK watermark), the directive is no longer pending.
printf '\n### b → hub  (ACK: on it)\n' >> "$SYNC"
out=$(printf '{"stop_hook_active":false}' | hook b stop.sh)
[ -z "$out" ] && ok "A3: after b's own post (ACK watermark), stop allowed" || no "A3 watermark failed (got: $out)"

# ── A4: PreCompact snapshot ──
out=$(printf '{"trigger":"auto"}' | hook a pre-compact.sh)
printf '%s' "$out" | python3 -c 'import json,sys; d=json.load(sys.stdin); assert "additionalContext" in d["hookSpecificOutput"]' 2>/dev/null \
  && ok "A4: PreCompact emits additionalContext" || no "A4 additionalContext missing (got: $out)"
[ -f "$COORD/state/a.precompact" ] && ok "A4: PreCompact wrote the state snapshot file" || no "A4 state file missing"
grep -q "src_x.rs" "$COORD/state/a.precompact" 2>/dev/null && ok "A4: snapshot records a's held lease" || no "A4 snapshot should list the lease"

echo ""
if [ "$fail" = 0 ]; then echo "F1 ENFORCE HOOKS: ALL PASS — A1 deny, A6 audit, A3 anti-dark, A4 snapshot, fail-open"; else echo "F1 ENFORCE HOOKS: FAILURES"; fi
exit $fail
