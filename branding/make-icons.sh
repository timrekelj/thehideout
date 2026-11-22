#!/bin/bash
# Regenerates every platform icon from branding/TheHideout.png (macOS tools only: swift, iconutil).
#
#   branding/make-icons.sh
#
# Writes client/packaging/macos/AppIcon.icns and branding/out/{macos,ios,android,web}/.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
out=$here/out
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

rm -rf "$out"
mkdir -p "$out"
swiftc -O "$here/make-icons.swift" -o "$work/make-icons"
"$work/make-icons" "$here/TheHideout.png" "$out" "$work"
iconutil -c icns "$work/AppIcon.iconset" -o "$root/client/packaging/macos/AppIcon.icns"
echo "wrote $root/client/packaging/macos/AppIcon.icns"
