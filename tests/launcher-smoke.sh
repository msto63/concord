#!/usr/bin/env bash
# Concord WP12 S1 — launcher smoke: the Rust concord binary's launcher subcommands
# (start --print / dash / pause / resume / stop), ported from bin/concord.
# `start` uses --print (dry-run) so no real claude session is spawned.
set -euo pipefail

HERE="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${CONCORD_BIN:-$HERE/target/debug/concord}"
[ -x "$BIN" ] || ( cd "$HERE" && cargo build -p concord -q )
[ -x "$BIN" ] || { echo "FATAL: concord not built at $BIN"; exit 1; }

W=$(mktemp -d "${TMPDIR:-/tmp}/concord-launch.XXXXXX"); trap 'rm -rf "$W" "$W/myproj-a"' EXIT
PROJ="$W/myproj"; mkdir -p "$PROJ" "$PROJ-a"; ( cd "$PROJ" && git init -q )
fail=0
cx() { "$BIN" "$@"; }
chk() { if printf '%s' "$2" | grep -qF "$3"; then echo "✓ $1"; else echo "✗ $1 — missing '$3' in: $2"; fail=1; fi; }

cx init --project "$PROJ" --ids hub,a >/dev/null

# start --print: worker role, correct worktree (convention), env, and worker kickoff.
o=$( cd "$PROJ" && cx start a --print 2>&1 )
chk "start a --print: worker role"        "$o" "worker · announces READY"
chk "start a --print: worktree convention" "$o" "myproj-a"
chk "start a --print: binds id via idbind" "$o" "idbind"
chk "start a --print: worker kickoff text" "$o" "You are Concord worker session a"
chk "start a --print: no real spawn"       "$o" "would start session a"

# F4: with telemetry enabled in config, start --print injects the Claude Code OTel env.
mkdir -p "$PROJ-coord"
printf '[telemetry]\nenabled = true\nport = 4319\n' > "$PROJ-coord/config.toml"
o=$( cd "$PROJ" && cx start a --print 2>&1 )
chk "start --print: telemetry env when enabled" "$o" "CLAUDE_CODE_ENABLE_TELEMETRY=1"
chk "start --print: OTLP endpoint to local receiver" "$o" "http://127.0.0.1:4319"
chk "start --print: concord.id resource attr" "$o" "concord.id=a"
rm -f "$PROJ-coord/config.toml"

# start hub --print: coordinator role + coordinator kickoff.
o=$( cd "$PROJ" && cx start hub --print 2>&1 )
chk "start hub --print: coordinator role"  "$o" "coordinator · takes up coordination"
chk "start hub --print: coordinator kickoff" "$o" "You are Concord coordinator session hub"

# dash: status + last prose post per session.
( cd "$PROJ" && cx claim a src/main.rs "edit" >/dev/null; cx sync a hub "STATUS" "progress" >/dev/null )
o=$( cd "$PROJ" && cx dash 2>&1 )
chk "dash: shows lease"            "$o" "src_main.rs"
chk "dash: last prose post"        "$o" "### a → hub"

# pause / resume / stop.
o=$( cd "$PROJ" && cx pause a 2>&1 );  chk "pause"  "$o" "pausiert: a"
# dash shows [PAUSED] while paused
op=$( cd "$PROJ" && cx pause a >/dev/null; cx dash 2>&1 ); chk "dash: [PAUSED] marker" "$op" "[PAUSED]"
o=$( cd "$PROJ" && cx resume a 2>&1 ); chk "resume" "$o" "fortgesetzt: a"
o=$( cd "$PROJ" && cx stop a 2>&1 );   chk "stop"   "$o" "Stop-Signal an a"

echo ""
if [ "$fail" = 0 ]; then echo "LAUNCHER SMOKE: ALL PASS"; else echo "LAUNCHER SMOKE: FAILURES"; fi
exit $fail
