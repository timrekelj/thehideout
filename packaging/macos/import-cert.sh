#!/usr/bin/env bash
# Imports the Developer ID certificate into a temporary keychain for the rest of the job.
# Reads APPLE_CERTIFICATE_P12_BASE64 and APPLE_CERTIFICATE_PASSWORD; run on a CI runner only.
set -euo pipefail

: "${APPLE_CERTIFICATE_P12_BASE64:?}" "${APPLE_CERTIFICATE_PASSWORD:?}"
keychain="${RUNNER_TEMP:-/tmp}/signing.keychain-db"
keychain_password="$(uuidgen)"
p12="${RUNNER_TEMP:-/tmp}/certificate.p12"

printf '%s' "$APPLE_CERTIFICATE_P12_BASE64" | base64 --decode > "$p12"
security create-keychain -p "$keychain_password" "$keychain"
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
security import "$p12" -k "$keychain" -P "$APPLE_CERTIFICATE_PASSWORD" -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$keychain_password" "$keychain" >/dev/null
# Put it first in the search list so codesign finds the identity.
# shellcheck disable=SC2046
security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')
rm -f "$p12"
security find-identity -v -p codesigning "$keychain"
