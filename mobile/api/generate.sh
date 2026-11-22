#!/bin/sh
# Regenerates the Swift and Kotlin API clients from /proto. Needs `buf` and network access to the
# BSR (remote plugins). Pass --check to fail when the committed output is stale (CI).
set -eu
cd "$(dirname "$0")/../.."
if [ "${1:-}" = "--check" ]; then
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  git ls-files -z mobile/ios/Packages/HideoutAPI/Sources/HideoutAPI/Generated mobile/android/api/generated | xargs -0 -I{} sh -c 'mkdir -p "$1/$(dirname "{}")" && cp "{}" "$1/{}"' _ "$tmp"
  buf generate --template mobile/api/buf.gen.yaml
  diff -r "$tmp/mobile/ios/Packages/HideoutAPI/Sources/HideoutAPI/Generated" mobile/ios/Packages/HideoutAPI/Sources/HideoutAPI/Generated >/dev/null \
    && diff -r "$tmp/mobile/android/api/generated" mobile/android/api/generated >/dev/null \
    || { echo "generated API clients are stale: run mobile/api/generate.sh" >&2; exit 1; }
  echo "generated API clients are up to date"
else
  buf generate --template mobile/api/buf.gen.yaml
fi
