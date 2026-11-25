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
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
if [ "${UNIVERSAL:-0}" = "1" ]; then
    cargo build --release -p liana-gui --bin liana-gui --target aarch64-apple-darwin
    # Intel starts at macOS 10.15, the app's minimum (Info.plist). Rust's default
    # for Intel is 10.12, and below 10.14.4 the macOS 27 SDK records CoreMedia as
    # @rpath/libswiftCoreMedia.dylib, which no Mac finds: the app stopped before
    # its window opened on every Intel Mac (decided 2026-11-25).
    MACOSX_DEPLOYMENT_TARGET=10.15 \
        cargo build --release -p liana-gui --bin liana-gui --target x86_64-apple-darwin
    BIN="$OUT/liana-gui-universal"
    lipo -create -output "$BIN" \
        "$TARGET_DIR/aarch64-apple-darwin/release/liana-gui" \
        "$TARGET_DIR/x86_64-apple-darwin/release/liana-gui"
else
    cargo build --release -p liana-gui --bin liana-gui
    BIN="$TARGET_DIR/release/liana-gui"
fi

# The app carries no libraries of its own, so each one it links must sit at a
# fixed path on the Mac. One reached through @rpath is never found and macOS
# refuses to open the app.
for ARCH in $(lipo -archs "$BIN"); do
    if otool -arch "$ARCH" -L "$BIN" | tail -n +2 | grep -q '@rpath/'; then
        echo "error: the $ARCH build links a library no Mac can find:" >&2
        otool -arch "$ARCH" -L "$BIN" | tail -n +2 | grep '@rpath/' >&2
        exit 1
    fi
done

APP="$OUT/Talia Desktop.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
sed "s/VERSION_PLACEHOLDER/$VERSION/g" contrib/talia/macos/Info.plist > "$APP/Contents/Info.plist"
cp contrib/talia/macos/TaliaDesktop.icns "$APP/Contents/Resources/TaliaDesktop.icns"
cp "$BIN" "$APP/Contents/MacOS/Talia Desktop"
cp LICENCE "$APP/Contents/Resources/LICENCE"
echo "Built $APP (version $VERSION)"
