#!/usr/bin/env bash
# Sign, notarise and pack Talia Desktop for Mac in one go:
#   1. sign "Talia Desktop.app" (hardened runtime, secure timestamp)
#   2. have Apple notarise it and staple the ticket to the app
#   3. pack the install .dmg (make-dmg.sh: drag-across window)
#   4. sign the .dmg, notarise it and staple the ticket to it
#
# Usage: contrib/talia/release-macos.sh <built Talia Desktop.app> <out.dmg>
#
# An app that already carries Apple's ticket (stapled) skips steps 1 and 2, so
# a run that stopped while Apple was still checking can resume from the signed
# app it kept.
#
# Keep the Mac unlocked while this runs: the keychain items below (signing key
# and notarytool profile) cannot be read while the screen is locked.
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
KEEP="${OUT%.dmg}-signed.app"
cleanup() {
    local code=$?
    # Apple may still be checking the app: keep the signed copy, whose ticket
    # can be stapled later (xcrun stapler staple) without signing it again.
    if [ "$code" -ne 0 ] && [ ! -e "$KEEP" ] \
        && codesign --verify --strict "$WORK/Talia Desktop.app" 2>/dev/null; then
        ditto "$WORK/Talia Desktop.app" "$KEEP" && echo "kept the signed app: $KEEP" >&2
    fi
    rm -rf "$WORK"
}
trap cleanup EXIT
APP="$WORK/Talia Desktop.app"
ditto "$SRC" "$APP"

notarise() {
    local file="$1" log="$WORK/notary.log"
    xcrun notarytool submit "$file" --keychain-profile "$NOTARY_PROFILE" --wait --timeout 3h | tee "$log"
    if ! grep -q "status: Accepted" "$log"; then
        local id
        id="$(grep -m1 -oE 'id: [0-9a-f-]{36}' "$log" | cut -d' ' -f2 || true)"
        [ -n "$id" ] && xcrun notarytool log "$id" --keychain-profile "$NOTARY_PROFILE" || true
        echo "notarisation failed for $file" >&2
        exit 1
    fi
}

if xcrun stapler validate "$APP" >/dev/null 2>&1; then
    echo "== 1-2. the app is already signed and notarised: skipping"
    codesign --verify --deep --strict "$APP"
else
    echo "== 1. sign the app"
    codesign --force --options runtime --timestamp --sign "$SIGN_ID" "$APP"
    codesign --verify --deep --strict "$APP"

    echo "== 2. notarise the app"
    ditto -c -k --keepParent "$APP" "$WORK/app.zip"
    notarise "$WORK/app.zip"
    xcrun stapler staple "$APP"
fi

echo "== 3. pack the install window"
"$ROOT/contrib/talia/make-dmg.sh" "$APP" "$OUT"

echo "== 4. sign and notarise the dmg"
codesign --force --timestamp --sign "$SIGN_ID" "$OUT"
notarise "$OUT"
xcrun stapler staple "$OUT"

spctl --assess --type open --context context:primary-signature --verbose=2 "$OUT"
shasum -a 256 "$OUT"
echo "Ready: $OUT"
