#!/usr/bin/env bash
# Build "Talia Desktop.app" for this Mac's architecture (or a universal app with UNIVERSAL=1).
# Usage: contrib/talia/build-macos-app.sh <output dir>
# Signing and notarisation are separate steps (see contrib/talia/README.md).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="${1:?usage: $0 <output dir>}"
VERSION="$(grep -m1 '^version' "$ROOT/liana-gui/Cargo.toml" | sed -E 's/.*"([0-9]+)\.([0-9]+)\..*/\1.\2/')"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"

cd "$ROOT"
if [ "${UNIVERSAL:-0}" = "1" ]; then
    cargo build --release -p liana-gui --bin liana-gui --target aarch64-apple-darwin
    cargo build --release -p liana-gui --bin liana-gui --target x86_64-apple-darwin
    BIN="$OUT/liana-gui-universal"
    lipo -create -output "$BIN" \
        target/aarch64-apple-darwin/release/liana-gui \
        target/x86_64-apple-darwin/release/liana-gui
else
    cargo build --release -p liana-gui --bin liana-gui
    BIN="target/release/liana-gui"
fi

APP="$OUT/Talia Desktop.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
sed "s/VERSION_PLACEHOLDER/$VERSION/g" contrib/talia/macos/Info.plist > "$APP/Contents/Info.plist"
cp contrib/talia/macos/TaliaDesktop.icns "$APP/Contents/Resources/TaliaDesktop.icns"
cp "$BIN" "$APP/Contents/MacOS/Talia Desktop"
cp LICENCE "$APP/Contents/Resources/LICENCE"
echo "Built $APP (version $VERSION)"
