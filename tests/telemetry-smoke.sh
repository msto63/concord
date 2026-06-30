#!/usr/bin/env bash
# Concord F4 — telemetry receiver + health end-to-end.
#
# Starts concordd with telemetry enabled on a NON-DEFAULT port (14319, so it never clashes
# with a live daemon on 4319) in a scratch coord, POSTs synthetic OTLP/HTTP-JSON metrics for
# three sessions, and asserts `concord status` flags each one: BURN (high token rate),
# REJECT (an edit-tool reject storm), IDLE (telemetry older than idle_min). The receiver
# writes the file before answering 200, but we poll the expected health to stay robust.
set -uo pipefail

HERE="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${CONCORD_BIN:-$HERE/target/debug/concord}"
DAEMON="${CONCORD_DAEMON_BIN:-$HERE/target/debug/concordd}"
[ -x "$BIN" ] || ( cd "$HERE" && cargo build -p concord -q )
[ -x "$DAEMON" ] || ( cd "$HERE" && cargo build -p concordd -q )
command -v curl >/dev/null || { echo "SKIP: curl not available"; exit 0; }

W=$(mktemp -d "${TMPDIR:-/tmp}/concord-tel.XXXXXX")
PROJ="$W/proj"; mkdir -p "$PROJ"; ( cd "$PROJ" && git init -q )
COORD="$W/proj-coord"; mkdir -p "$COORD"
DPID=""
cleanup() { [ -n "$DPID" ] && { kill -KILL "$DPID" 2>/dev/null; wait "$DPID" 2>/dev/null; }; rm -rf "$W"; }
trap cleanup EXIT
fail=0; ok() { echo "✓ $1"; }; no() { echo "✗ $1"; fail=1; }

# Pick a FREE port: probe candidates and take the first where NOTHING already responds, so
# a leftover or live daemon (default 4319, or a stale test daemon) can never intercept our
# POSTs and silently route them to the wrong coord.
PORT=""
for p in 14319 14323 14327 14331 14335 14339; do
  curl -s -o /dev/null --max-time 1 -X POST "http://127.0.0.1:$p/v1/metrics" --data '{}' || { PORT="$p"; break; }
done
[ -n "$PORT" ] || { echo "SKIP: no free telemetry port found"; exit 0; }

# Scratch config: telemetry on, our own port, explicit idle_min so the old-timestamp case
# reliably trips the IDLE threshold.
cat > "$COORD/config.toml" <<TOML
[telemetry]
enabled = true
port = $PORT
idle_min = 15
TOML

# Start the daemon in the project (convention coord = $COORD). `exec` makes the subshell
# BECOME the daemon, so $! is the real concordd pid (a plain `( … ) &` may fork, leaving a
# stray daemon the trap can't kill).
( cd "$PROJ" && exec "$DAEMON" ) >/dev/null 2>&1 &
DPID=$!

# Wait for the receiver to bind (poll the port; no fixed sleep).
bound=0
for _ in $(seq 1 50); do
  curl -s -o /dev/null --max-time 1 -X POST "http://127.0.0.1:$PORT/v1/metrics" --data '{}' && { bound=1; break; }
  perl -e 'select(undef,undef,undef,0.2)'
done
[ "$bound" = 1 ] && ok "telemetry receiver is listening on $PORT" || { no "receiver never bound"; exit 1; }

now=$(date +%s); now_ns=$(( now * 1000000000 )); old_ns=$(( (now - 1200) * 1000000000 ))   # 20 min ago
# OTLP/HTTP-JSON batch: a token.usage sum, a code_edit_tool.decision sum, per session.
post() { curl -s -o /dev/null --max-time 2 -X POST -H 'Content-Type: application/json' \
           "http://127.0.0.1:$PORT/v1/metrics" --data "$1"; }
tok() { printf '{"name":"claude_code.token.usage","sum":{"dataPoints":[{"attributes":[{"key":"type","value":{"stringValue":"output"}}],"timeUnixNano":"%s","asInt":"%s"}]}}' "$1" "$2"; }
rej1() { printf '{"attributes":[{"key":"decision","value":{"stringValue":"reject"}},{"key":"tool_name","value":{"stringValue":"Edit"}}],"timeUnixNano":"%s","asInt":"1"}' "$1"; }
rm_one() { printf '{"resourceMetrics":[{"resource":{"attributes":[{"key":"concord.id","value":{"stringValue":"%s"}}]},"scopeMetrics":[{"metrics":[%s]}]}]}' "$1" "$2"; }

# burn: a recent, very large token count → tokens/min over the window > burn_warn.
burn_body=$(rm_one burn "$(tok "$now_ns" 250000)")
# reject: five recent reject decisions → reject storm.
rejpts="$(rej1 "$now_ns"),$(rej1 "$now_ns"),$(rej1 "$now_ns"),$(rej1 "$now_ns"),$(rej1 "$now_ns")"
reject_metric="{\"name\":\"claude_code.code_edit_tool.decision\",\"sum\":{\"dataPoints\":[$rejpts]}}"
reject_body=$(rm_one reject "$reject_metric")
# idle: a single metric stamped 20 min ago → no recent activity → IDLE.
idle_body=$(rm_one idle "$(tok "$old_ns" 10)")
post "$burn_body"
post "$reject_body"
post "$idle_body"

st() { ( cd "$PROJ" && "$BIN" status 2>/dev/null ); }
# Poll the health surface (robust against any flush/scheduling lag).
want() {  # <id> <FLAG>
  for _ in $(seq 1 25); do
    st | grep -qE "^  $1 +$2 " && { ok "$1 → $2"; return; }
    perl -e 'select(undef,undef,undef,0.2)'
  done
  no "$1 should be $2; status:\n$(st | sed -n '/TELEMETRY/,$p')"
}
want burn BURN
want reject REJECT
want idle IDLE

echo ""
if [ "$fail" = 0 ]; then echo "TELEMETRY SMOKE: ALL PASS — OTLP receiver ingests, status flags BURN/REJECT/IDLE"; else echo "TELEMETRY SMOKE: FAILURES"; fi
exit $fail
