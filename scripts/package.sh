#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$SCRIPT_DIR"

RAW_VERSION="${1:-${VERSION:-${GITHUB_REF_NAME:-0.0.0-dev}}}"
VERSION="${RAW_VERSION#v}"
ARCH="${ARCH:-$(uname -m)}"

echo "=== Packaging Routune v${VERSION} (linux-${ARCH}) ==="

echo "=== Building Routune Release Artifacts ==="
cargo build --release -p routune-daemon

echo "=== Building Frontend Dist ==="
npm --prefix routune-gui ci || npm --prefix routune-gui install
npm --prefix routune-gui run build

echo "=== Building GUI Binary ==="
cargo build --release -p routune-gui --features custom-protocol

DIST_DIR="$SCRIPT_DIR/dist"
STAGE_DIR="$DIST_DIR/routune-${VERSION}-linux-${ARCH}"

rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR"

echo "=== Staging Release Files ==="
cp "target/release/routune-daemon" "$STAGE_DIR/"
cp "target/release/routune-gui" "$STAGE_DIR/"
ln -sf "routune-gui" "$STAGE_DIR/routune"
cp "install.sh" "$STAGE_DIR/"
cp "packaging/routune.service" "$STAGE_DIR/"
cp "packaging/routune.desktop" "$STAGE_DIR/"
cp "packaging/icons/routune.png" "$STAGE_DIR/"
cp "README.md" "$STAGE_DIR/"
cp "LICENSE" "$STAGE_DIR/"

chmod +x "$STAGE_DIR/routune-daemon" "$STAGE_DIR/routune-gui" "$STAGE_DIR/install.sh"

echo "=== Generating Tarball ==="
cd "$DIST_DIR"
TARBALL_NAME="routune-${VERSION}-linux-${ARCH}.tar.gz"
tar -czf "$TARBALL_NAME" -C "$DIST_DIR" "routune-${VERSION}-linux-${ARCH}"
sha256sum "$TARBALL_NAME" > "${TARBALL_NAME}.sha256"

# Create generic fallback copy
cp "$TARBALL_NAME" "routune-linux-${ARCH}.tar.gz"
cp "${TARBALL_NAME}.sha256" "routune-linux-${ARCH}.tar.gz.sha256"

echo "=== Packaging Complete ==="
ls -lh "$DIST_DIR/$TARBALL_NAME"
cat "$DIST_DIR/${TARBALL_NAME}.sha256"
