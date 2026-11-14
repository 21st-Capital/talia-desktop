#!/usr/bin/env bash
# Pack "Talia Desktop.app" into the install .dmg: the app, an Applications
# shortcut and the drag-across background (contrib/talia/macos/dmg/).
# Usage: contrib/talia/make-dmg.sh <path to Talia Desktop.app> <out.dmg> [volume name]
# Needs a Python with dmgbuild (pip install dmgbuild); PYTHON can point at it.
# Sign and notarise the app BEFORE packing it.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
APP="$(cd "$(dirname "${1:?usage: $0 <app> <out.dmg> [volume name]}")" && pwd)/$(basename "$1")"
OUT="${2:?usage: $0 <app> <out.dmg> [volume name]}"
NAME="${3:-Talia Desktop}"
PYTHON="${PYTHON:-python3}"
DIR="$ROOT/contrib/talia/macos/dmg"
[ -d "$APP" ] || { echo "no app at $APP" >&2; exit 1; }
[ ! -e "/Volumes/$NAME" ] || { echo "/Volumes/$NAME is already mounted; eject it first" >&2; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# 1. Read-write image with the layout and the background.
"$PYTHON" -m dmgbuild -s "$DIR/settings.py" -D here="$DIR" -D app="$APP" -D format=UDRW "$NAME" "$WORK/rw.dmg"
# 2. Background link that macOS 26 accepts (see background_bookmark.py).
MOUNT="$(hdiutil attach -nobrowse -owners off "$WORK/rw.dmg" | grep -o '/Volumes/.*' | tail -1)"
"$PYTHON" "$DIR/background_bookmark.py" "$MOUNT"
hdiutil detach "$MOUNT" -quiet
# 3. Compressed, read-only result.
rm -f "$OUT"
hdiutil convert "$WORK/rw.dmg" -format UDZO -imagekey zlib-level=9 -o "$OUT" -quiet
hdiutil verify -quiet "$OUT"
echo "Built $OUT"
