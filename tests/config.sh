#!/usr/bin/env bash
# Concord WP12 F-config — config.toml + retired env vars.
#
# Proves: init drops a sample config.toml; config values take effect (coordinator,
# overlap_policy, strict); --coord bootstrap; and a legacy env var is honored with a
# deprecation warning (not a break).
set -uo pipefail

HERE="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${CONCORD_BIN:-$HERE/target/debug/concord}"
[ -x "$BIN" ] || ( cd "$HERE" && cargo build -p concord -q )
[ -x "$BIN" ] || { echo "FATAL: concord not built at $BIN"; exit 1; }

W=$(mktemp -d "${TMPDIR:-/tmp}/concord-cfg.XXXXXX"); trap 'rm -rf "$W"' EXIT
PROJ="$W/proj"; mkdir -p "$PROJ/src"; ( cd "$PROJ" && git init -q )
COORD="$W/proj-coord"
fail=0
# cwd = project (convention-derive) + a controlled HOME so the user-global config is absent.
# (F-config: the binary reads no location env, so there is nothing to leak or clear.)
R() { ( cd "$PROJ" && env HOME="$W/home" "$BIN" "$@" ); }
chk() { if printf '%s' "$2" | grep -qF "$3"; then echo "✓ $1"; else echo "✗ $1 — want '$3' in: $2"; fail=1; fi; }
chkx() { if [ "$2" = "$3" ]; then echo "✓ $1 (exit $2)"; else echo "✗ $1 — exit $2 != $3"; fail=1; fi; }

R init --ids a,b >/dev/null
[ -f "$COORD/config.toml" ] && echo "✓ init drops a sample config.toml" || { echo "✗ no sample config.toml"; fail=1; }

# Default coordinator is hub (sample config is all-commented → defaults apply).
o=$(R escalate a high "x"); chk "default coordinator = hub" "$o" "→ hub"

# Set a custom coordinator + strict overlap in config → takes effect.
cat > "$COORD/config.toml" <<'TOML'
[leases]
strict = true
[escalation]
coordinator = "K"
TOML
o=$(R escalate a high "y"); chk "config coordinator override (K)" "$o" "→ K"

# strict=true (P1): an un-leased file is DENIED by check-lease (no --strict flag needed).
o=$(R check-lease a src/free.rs) && rc=0 || rc=$?; chk "config strict=true → DENY un-leased" "$o" "DENY"; chkx "strict deny exit 2" "$rc" 2
# With strict back to false, the same edit is allowed.
printf '[leases]\nstrict = false\n' > "$COORD/config.toml"
o=$(R check-lease a src/free.rs) && rc=0 || rc=$?; chk "config strict=false → ALLOW un-leased" "$o" "ALLOW"; chkx "non-strict allow exit 0" "$rc" 0

# Malformed config.toml does not crash — warns and falls back to defaults.
printf 'this is [[[ not toml = =\n' > "$COORD/config.toml"
o=$(R status 2>&1) && rc=0 || rc=$?; chk "malformed config warns" "$o" "ignoring malformed"; chkx "malformed config does not crash" "$rc" 0
printf '' > "$COORD/config.toml"

# --coord bootstrap: resolve a coord dir explicitly, run from an unrelated cwd.
o=$( cd "$W" && env HOME="$W/home" "$BIN" --coord "$COORD" status 2>&1 ); chk "--coord bootstrap resolves the coord" "$o" "$COORD"

# F-config: legacy location env is IGNORED (no ambient authority). With a bogus CONCORD_DIR
# set, the binary still resolves by CONVENTION (cwd=$PROJ → $COORD) and never reads the env.
o=$( cd "$PROJ" && env CONCORD_DIR="$W/BOGUS-coord" HOME="$W/home" "$BIN" paths 2>&1 )
# Convention resolves the coord as <proj>-coord (suffix-checked; macOS canonicalizes
# /var → /private/var, so don't compare the full prefix). The bogus env must not appear.
chk "legacy CONCORD_DIR is ignored — convention resolves <proj>-coord" "$o" "proj-coord"
[ "${o#*BOGUS}" = "$o" ] && echo "✓ bogus CONCORD_DIR absent from resolved paths" || { echo "✗ bogus env leaked"; fail=1; }

echo ""
if [ "$fail" = 0 ]; then echo "CONFIG: ALL PASS — sample config, overrides take effect, strict, malformed-safe, --coord, legacy-env ignored"; else echo "CONFIG: FAILURES"; fi
exit $fail
