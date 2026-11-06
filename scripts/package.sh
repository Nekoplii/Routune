#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$SCRIPT_DIR"

echo "=== Building Routune Release Artifacts ==="
cargo build --release -p routune-daemon

echo "=== Building Frontend Dist ==="
npm --prefix routune-gui ci || npm --prefix routune-gui install
npm --prefix routune-gui run build

echo "=== Building GUI Binary ==="
cargo build --release -p routune-gui

DIST_DIR="$SCRIPT_DIR/dist"
STAGE_DIR="$DIST_DIR/routune-linux-x86_64"

rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR"

echo "=== Staging Release Files ==="
cp "target/release/routune-daemon" "$STAGE_DIR/"
cp "target/release/routune-gui" "$STAGE_DIR/"
cp "install.sh" "$STAGE_DIR/"
cp "packaging/routune.service" "$STAGE_DIR/"
cp "packaging/routune.desktop" "$STAGE_DIR/"
cp "packaging/icons/routune.png" "$STAGE_DIR/"
cp "README.md" "$STAGE_DIR/"
cp "LICENSE" "$STAGE_DIR/"

chmod +x "$STAGE_DIR/routune-daemon" "$STAGE_DIR/routune-gui" "$STAGE_DIR/install.sh"

echo "=== Generating Tarball ==="
cd "$DIST_DIR"
tar -czf "routune-linux-x86_64.tar.gz" -C "$DIST_DIR" "routune-linux-x86_64"
sha256sum "routune-linux-x86_64.tar.gz" > "routune-linux-x86_64.tar.gz.sha256"

echo "=== Packaging Complete ==="
ls -lh "$DIST_DIR/routune-linux-x86_64.tar.gz"
cat "$DIST_DIR/routune-linux-x86_64.tar.gz.sha256"
