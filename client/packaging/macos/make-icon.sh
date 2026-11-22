#!/bin/sh
# Regenerates AppIcon.icns from make-icon.swift (the committed .icns is what builds use).
set -eu
cd "$(dirname "$0")"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
swift make-icon.swift "$tmp/master.png"
set="$tmp/AppIcon.iconset"
mkdir "$set"
for s in 16 32 128 256 512; do
  sips -z $s $s "$tmp/master.png" --out "$set/icon_${s}x${s}.png" >/dev/null
  sips -z $((s * 2)) $((s * 2)) "$tmp/master.png" --out "$set/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$set" -o AppIcon.icns
echo "wrote $(pwd)/AppIcon.icns"
