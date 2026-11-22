#!/bin/sh
# Regenerates the Swift and Kotlin API clients from /proto. Needs `buf` and network access to the
# BSR (remote plugins). With --check it fails when the committed output is stale (CI).
set -eu
cd "$(dirname "$0")/../.."
swift_out=mobile/ios/Packages/HideoutAPI/Sources/HideoutAPI/Generated
kotlin_out=mobile/android/api/generated
if [ "${1:-}" = "--check" ]; then
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  cp -R "$swift_out" "$tmp/swift"
  cp -R "$kotlin_out" "$tmp/kotlin"
  buf generate --template mobile/api/buf.gen.yaml
  if diff -r "$tmp/swift" "$swift_out" >/dev/null && diff -r "$tmp/kotlin" "$kotlin_out" >/dev/null; then
    echo "generated API clients are up to date"
  else
    echo "generated API clients are stale: run mobile/api/generate.sh and commit" >&2
    exit 1
  fi
else
  buf generate --template mobile/api/buf.gen.yaml
fi
