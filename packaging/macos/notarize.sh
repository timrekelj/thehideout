#!/usr/bin/env bash
# Submits a signed .dmg to Apple's notary service, waits, and staples the ticket.
#   packaging/macos/notarize.sh path/to/thehideout.dmg
# Reads APPLE_ID, APPLE_APP_SPECIFIC_PASSWORD and APPLE_TEAM_ID.
set -euo pipefail

dmg="${1:?usage: notarize.sh DMG}"
: "${APPLE_ID:?}" "${APPLE_APP_SPECIFIC_PASSWORD:?}" "${APPLE_TEAM_ID:?}"

xcrun notarytool submit "$dmg" --wait --timeout 30m \
  --apple-id "$APPLE_ID" --password "$APPLE_APP_SPECIFIC_PASSWORD" --team-id "$APPLE_TEAM_ID"
xcrun stapler staple "$dmg"
xcrun stapler validate "$dmg"
