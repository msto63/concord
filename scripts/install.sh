#!/usr/bin/env bash
# Concord installer (M4.2). Detects the platform, downloads the matching release archive
# from GitHub, verifies its checksum, and installs the three binaries (concord, concordd,
# concord-mcp) into ~/.local/bin (override with CONCORD_INSTALL_DIR).
#
#   curl --proto '=https' --tlsv1.2 -LsSf \
#     https://raw.githubusercontent.com/msto63/concord/main/scripts/install.sh | sh
#
# Equivalent to a `dist`-generated installer; hand-authored so it ships without `dist`.
# Unix only (macOS/Linux). On Windows use the .zip from the GitHub Release, or `cargo install`.
set -euo pipefail

REPO="msto63/concord"
DEST="${CONCORD_INSTALL_DIR:-$HOME/.local/bin}"
TAG="${CONCORD_VERSION:-latest}"

err() { echo "concord-install: $*" >&2; exit 1; }

# ── detect target triple ──
os="$(uname -s)"; arch="$(uname -m)"
case "$os" in
  Darwin) case "$arch" in
            arm64) target="aarch64-apple-darwin" ;;
            x86_64) target="x86_64-apple-darwin" ;;
            *) err "unsupported macOS arch: $arch" ;;
          esac ;;
  Linux)  case "$arch" in
            x86_64) target="x86_64-unknown-linux-gnu" ;;
            *) err "unsupported Linux arch: $arch (try \`cargo install --git https://github.com/$REPO\`)" ;;
          esac ;;
  *) err "unsupported OS: $os (Windows: use the .zip release or \`cargo install\`)" ;;
esac

# ── resolve the download URL ──
base="https://github.com/$REPO/releases"
if [ "$TAG" = latest ]; then
  url="$base/latest/download/concord-$target.tar.gz"
else
  url="$base/download/$TAG/concord-$target.tar.gz"
fi
echo "concord-install: target $target ← $url"

tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
arc="$tmp/concord.tar.gz"
curl --proto '=https' --tlsv1.2 -fLsS "$url" -o "$arc" || err "download failed: $url"

# ── verify checksum if the .sha256 is published ──
if curl --proto '=https' --tlsv1.2 -fLsS "$url.sha256" -o "$arc.sha256" 2>/dev/null; then
  want="$(awk '{print $1}' "$arc.sha256")"
  got="$(shasum -a 256 "$arc" | awk '{print $1}')"
  [ "$want" = "$got" ] || err "checksum mismatch (want $want, got $got)"
  echo "concord-install: checksum OK"
fi

# ── extract + install ──
tar xzf "$arc" -C "$tmp"
mkdir -p "$DEST"
for b in concord concordd concord-mcp; do
  src="$tmp/concord-$target/$b"
  [ -f "$src" ] || err "binary $b missing from archive"
  install -m 0755 "$src" "$DEST/$b"
done

echo "concord-install: installed concord, concordd, concord-mcp → $DEST"
case ":$PATH:" in
  *":$DEST:"*) ;;
  *) echo "concord-install: add $DEST to your PATH (e.g. export PATH=\"$DEST:\$PATH\")" ;;
esac
echo "concord-install: next → \`concord init --with-hooks\` in your repo"
