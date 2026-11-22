#!/bin/bash
# Builds thehideout.app and thehideout-<version>.dmg for macOS (arm64).
#
#   ./packaging/macos/build.sh          ad-hoc signed (runs on this Mac; Gatekeeper warns elsewhere)
#
# Developer ID signing, set before running:
#   APPLE_SIGNING_IDENTITY   e.g. "Developer ID Application: Name (TEAMID)"
# plus notarization (and stapling), either with a notarytool keychain profile:
#   NOTARY_PROFILE           created with `xcrun notarytool store-credentials`
# or with an Apple ID:
#   APPLE_ID, APPLE_APP_PASSWORD (app-specific password), APPLE_TEAM_ID
#
# Needs cargo-packager (`cargo install cargo-packager --locked`). Output: client/dist/macos/.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
client=$(cd "$here/../.." && pwd)
cd "$client"

target=${CARGO_TARGET_DIR:-$client/target}
out=$client/dist/macos
ents=$here/Entitlements.plist
version=$(cargo metadata --no-deps --format-version 1 | python3 -c \
  'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]=="thehideout-desktop"))')
app=$out/thehideout.app
dmg=$out/thehideout-$version.dmg

identity=${APPLE_SIGNING_IDENTITY:-}
notarize_args=()
if [ -n "${NOTARY_PROFILE:-}" ]; then
  notarize_args=(--keychain-profile "$NOTARY_PROFILE")
elif [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_APP_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; then
  notarize_args=(--apple-id "$APPLE_ID" --password "$APPLE_APP_PASSWORD" --team-id "$APPLE_TEAM_ID")
fi
if [ ${#notarize_args[@]} -gt 0 ] && [ -z "$identity" ]; then
  echo "notarization needs APPLE_SIGNING_IDENTITY too" >&2
  exit 1
fi

notarize() { # notarize <file>: submit, wait, fail unless accepted
  xcrun notarytool submit "$1" "${notarize_args[@]}" --wait
}

echo "==> cargo build --release"
cargo build --release -p thehideout-desktop

echo "==> bundling $app"
rm -rf "$out"
mkdir -p "$out"
# cargo-packager signs and notarizes by itself when these are set; this script does it below.
env -u APPLE_SIGNING_IDENTITY -u APPLE_CERTIFICATE -u APPLE_CERTIFICATE_PASSWORD \
    -u APPLE_ID -u APPLE_PASSWORD -u APPLE_TEAM_ID \
  cargo packager --release -p thehideout-desktop -f app --out-dir "$out" --binaries-dir "$target/release"
[ -d "$app" ] || { echo "cargo-packager did not produce $app" >&2; exit 1; }

echo "==> signing"
if [ -n "$identity" ]; then
  sign=(--sign "$identity" --timestamp)
else
  echo "    (ad-hoc: set APPLE_SIGNING_IDENTITY for a Developer ID signature)"
  sign=(--sign -)
fi
codesign --force --deep --options runtime --entitlements "$ents" "${sign[@]}" "$app"
codesign --verify --deep --strict --verbose=2 "$app"

if [ ${#notarize_args[@]} -gt 0 ]; then
  echo "==> notarizing the app"
  zip=$out/thehideout-notarize.zip
  ditto -c -k --keepParent "$app" "$zip"
  notarize "$zip"
  rm -f "$zip"
  xcrun stapler staple "$app"
fi

echo "==> $dmg"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
hdiutil create -volname thehideout -srcfolder "$stage" -ov -format UDZO "$dmg" >/dev/null
if [ -n "$identity" ]; then
  codesign --force "${sign[@]}" "$dmg"
fi
if [ ${#notarize_args[@]} -gt 0 ]; then
  echo "==> notarizing the dmg"
  notarize "$dmg"
  xcrun stapler staple "$dmg"
fi

echo "done:"
echo "  $app"
echo "  $dmg"
