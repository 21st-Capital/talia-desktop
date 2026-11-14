#!/usr/bin/env bash
# Sign, notarise and pack Talia Desktop for Mac in one go:
#   1. sign "Talia Desktop.app" (hardened runtime, secure timestamp)
#   2. have Apple notarise it and staple the ticket to the app
#   3. pack the install .dmg (make-dmg.sh: drag-across window)
#   4. sign the .dmg, notarise it and staple the ticket to it
#
# Usage: contrib/talia/release-macos.sh <built Talia Desktop.app> <out.dmg>
#
# Needs, in this Mac's keychain:
#   - the "Developer ID Application" certificate of team 8SMBP8KCWZ
#     (SIGN_ID overrides the name below)
#   - a notarytool profile made from the team's App Store Connect API key:
#       xcrun notarytool store-credentials talia-notary --key AuthKey_XXXX.p8 \
#         --key-id XXXX --issuer <issuer id>
#     (NOTARY_PROFILE overrides the name)
# and a Python with dmgbuild for make-dmg.sh (PYTHON=...).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SRC="${1:?usage: $0 <Talia Desktop.app> <out.dmg>}"
OUT="${2:?usage: $0 <Talia Desktop.app> <out.dmg>}"
SIGN_ID="${SIGN_ID:-Developer ID Application: Twenty First Capital Cyber Security Consultancy L.L.C (8SMBP8KCWZ)}"
NOTARY_PROFILE="${NOTARY_PROFILE:-talia-notary}"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
APP="$WORK/Talia Desktop.app"
ditto "$SRC" "$APP"

notarise() {
    local file="$1" log="$WORK/notary.log"
    xcrun notarytool submit "$file" --keychain-profile "$NOTARY_PROFILE" --wait --timeout 30m | tee "$log"
    if ! grep -q "status: Accepted" "$log"; then
        local id
        id="$(grep -m1 -oE 'id: [0-9a-f-]{36}' "$log" | cut -d' ' -f2 || true)"
        [ -n "$id" ] && xcrun notarytool log "$id" --keychain-profile "$NOTARY_PROFILE" || true
        echo "notarisation failed for $file" >&2
        exit 1
    fi
}

echo "== 1. sign the app"
codesign --force --options runtime --timestamp --sign "$SIGN_ID" "$APP"
codesign --verify --deep --strict "$APP"

echo "== 2. notarise the app"
ditto -c -k --keepParent "$APP" "$WORK/app.zip"
notarise "$WORK/app.zip"
xcrun stapler staple "$APP"

echo "== 3. pack the install window"
"$ROOT/contrib/talia/make-dmg.sh" "$APP" "$OUT"

echo "== 4. sign and notarise the dmg"
codesign --force --timestamp --sign "$SIGN_ID" "$OUT"
notarise "$OUT"
xcrun stapler staple "$OUT"

spctl --assess --type open --context context:primary-signature --verbose=2 "$OUT"
shasum -a 256 "$OUT"
echo "Ready: $OUT"
