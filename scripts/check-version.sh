#!/usr/bin/env bash
# Enforce version discipline. Fails (non-zero) unless:
#   - VERSION is valid semver (X.Y.Z)
#   - the latest released CHANGELOG entry equals VERSION
#   - `concord version` reports the same VERSION
#   - on a tag build (GITHUB_REF_NAME=vX.Y.Z), the tag equals v$VERSION
# Run by CI on every push/PR (see .github/workflows/ci.yml) and recommended as a
# local pre-push hook (see CONTRIBUTING.md → Release discipline).
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fail() { echo "✗ version check: $*" >&2; exit 1; }

[ -f "$ROOT/VERSION" ]      || fail "VERSION file missing"
[ -f "$ROOT/CHANGELOG.md" ] || fail "CHANGELOG.md missing"

ver="$(tr -d '[:space:]' < "$ROOT/VERSION")"
echo "$ver" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$' || fail "VERSION '$ver' is not semver (X.Y.Z)"

# First '## [X.Y.Z]' heading in the CHANGELOG, ignoring '## [Unreleased]'.
cl="$(grep -m1 -oE '^## \[[0-9]+\.[0-9]+\.[0-9]+\]' "$ROOT/CHANGELOG.md" | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' || true)"
[ -n "$cl" ] || fail "no released '## [X.Y.Z]' heading in CHANGELOG.md"
[ "$ver" = "$cl" ] || fail "VERSION ($ver) != latest CHANGELOG entry ($cl) — bump one or the other"

# Post Rust-migration: the version flows VERSION → Cargo.toml → the Rust binary
# (env!("CARGO_PKG_VERSION")). Verify the Cargo.toml workspace version, and the built
# binary's reported version when one is present. (The shell bin/concord is frozen under
# bin/legacy/ and no longer the version source.)
cargo_ver="$(grep -m1 -E '^version = ' "$ROOT/Cargo.toml" | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')"
[ "$cargo_ver" = "$ver" ] || fail "Cargo.toml version ($cargo_ver) != VERSION ($ver) — bump one or the other"
for b in "$ROOT/target/release/concord" "$ROOT/target/debug/concord"; do
  if [ -x "$b" ]; then
    cv="$("$b" version 2>/dev/null | awk '{print $NF}')"
    [ "$cv" = "$ver" ] || fail "\`concord version\` ($cv) != VERSION ($ver) — rebuild after bumping"
    break
  fi
done

ref="${GITHUB_REF_NAME:-}"
if echo "$ref" | grep -qE '^v[0-9]'; then
  [ "$ref" = "v$ver" ] || fail "tag $ref != v$ver"
fi

echo "✓ version OK: $ver (VERSION, CHANGELOG, and \`concord version\` agree)"
