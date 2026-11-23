#!/usr/bin/env bash
# Builds thehideout.app around a release binary, signs it and packs a .dmg.
#
#   packaging/macos/bundle.sh --binary PATH --version 0.1.0[-rc1] --arch arm64|x86_64 --out DIR
#
# Signing: with APPLE_SIGNING_IDENTITY set (the Developer ID certificate must already be in the
# default keychain, see import-cert.sh) the app and dmg are signed with the hardened runtime and
# a secure timestamp. Without it they are signed ad hoc (`codesign -s -`), which runs locally
# but is not notarizable. Notarization is a separate step: notarize.sh.
#
# Output: DIR/thehideout.app and DIR/thehideout-<version>-macos-<arch>.dmg
set -euo pipefail

# Keep in step with MACOSX_DEPLOYMENT_TARGET in the release workflow.
MIN_MACOS="${MIN_MACOS:-11.0}"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
binary="" version="" arch="" out=""
while [ $# -gt 0 ]; do
  case "$1" in
    --binary) binary="$2"; shift 2 ;;
    --version) version="$2"; shift 2 ;;
    --arch) arch="$2"; shift 2 ;;
    --out) out="$2"; shift 2 ;;
    *) echo "bundle.sh: unknown argument $1" >&2; exit 2 ;;
  esac
done
if [ -z "$binary" ] || [ -z "$version" ] || [ -z "$arch" ] || [ -z "$out" ]; then
  echo "usage: bundle.sh --binary PATH --version VERSION --arch arm64|x86_64 --out DIR" >&2
  exit 2
fi
version="${version#v}"
# CFBundleShortVersionString only allows digits and dots: drop a "-rc1" style suffix.
plist_version="${version%%-*}"

app="$out/thehideout.app"
dmg="$out/thehideout-$version-macos-$arch.dmg"
rm -rf "$app" "$dmg"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

install -m 755 "$binary" "$app/Contents/MacOS/thehideout-desktop"
sed -e "s/@VERSION@/$plist_version/g" -e "s/@MIN_MACOS@/$MIN_MACOS/g" \
  "$here/Info.plist.in" > "$app/Contents/Info.plist"
printf 'APPL????' > "$app/Contents/PkgInfo"
plutil -lint "$app/Contents/Info.plist"

identity="${APPLE_SIGNING_IDENTITY:-}"
if [ -n "$identity" ]; then
  echo "Signing with: $identity"
  codesign --force --deep --options runtime --timestamp \
    --entitlements "$here/entitlements.plist" --sign "$identity" "$app"
else
  echo "::warning::APPLE_SIGNING_IDENTITY not set: signing ad hoc (not notarizable)"
  codesign --force --deep --options runtime \
    --entitlements "$here/entitlements.plist" --sign - "$app"
fi
codesign --verify --deep --strict --verbose=2 "$app"

# The dmg holds the app and an /Applications shortcut to drag it onto.
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
cp -R "$app" "$stage/thehideout.app"
ln -s /Applications "$stage/Applications"
hdiutil create -volname "thehideout" -srcfolder "$stage" -fs HFS+ -format UDZO -ov "$dmg"
if [ -n "$identity" ]; then
  codesign --force --timestamp --sign "$identity" "$dmg"
else
  codesign --force --sign - "$dmg"
fi
codesign --verify --verbose=2 "$dmg"
echo "$dmg"
