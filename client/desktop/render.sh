#!/bin/sh
# Renders every desktop scene to desktop/snapshots/*.png (headless, same renderer as the window,
# fake backend; `--at 1` lets the scroll-edge lines finish their fade). Run from anywhere: ./desktop/render.sh [extra rune flags, e.g. --scale 2]
set -e
cd "$(dirname "$0")/.."
cargo build -p thehideout-desktop
BIN=target/debug/thehideout-desktop
OUT=desktop/snapshots
mkdir -p "$OUT"
for mode in "" "-light"; do
  for scene in emoji-picker emoji-autocomplete reactions jumbo-emoji server-settings-emojis channel-icon-emoji-tabs; do
    $BIN --scene "$scene$mode" --size 1280x800 --at 1 --snap "$OUT/$scene$mode.png" "$@"
  done
  for scene in auth-signin auth-signup-error auth-code auth-second-factor auth-invite-lookup auth-invite-accept auth-passkey-waiting add-email-banner settings-security-totp-setup settings-security-recovery-codes invite-by-email admin-invites main main-members home-dms home-empty toast empty-channel dialog-create-server reconnecting sections channel-settings icon-picker-search reorder-menu main-avatars server-settings-overview server-settings-roles server-settings-roles-limited server-settings-members profile-dialog admin-overview admin-servers admin-files admin-files-delete admin-users admin-audit admin-unavailable; do
    $BIN --scene "$scene$mode" --size 1280x800 --at 1 --snap "$OUT/$scene$mode.png" "$@"
  done
  for scene in attachments-composer attachments-messages attachments-images lightbox video-embed; do
    $BIN --scene "$scene$mode" --size 1280x800 --at 1 --snap "$OUT/$scene$mode.png" "$@"
  done
  # Embeds are tall: a taller window shows whole cards.
  for scene in embeds embeds-gallery embeds-collapsed; do
    $BIN --scene "$scene$mode" --size 1280x1300 --at 1 --snap "$OUT/$scene$mode.png" "$@"
  done
  for scene in voice-sidebar voice-connected voice-server-muted voice-participant-menu voice-stage voice-stage-screen voice-screen-picker voice-sidebar-live dialog-create-voice; do
    $BIN --scene "$scene$mode" --size 1280x800 --at 1 --snap "$OUT/$scene$mode.png" "$@"
  done
  # The roles editor is long: a taller window shows the new voice switches at its end.
  $BIN --scene "server-settings-roles-voice$mode" --size 1280x1100 --at 1 --snap "$OUT/server-settings-roles-voice$mode.png" "$@"
  $BIN --scene "main-small$mode" --size 800x600 --at 1 --snap "$OUT/main-small$mode.png" "$@"
done
# The Appearance, Voice & audio and Security pages are tall: a taller window shows all of them.
for mode in "" "-light"; do
  for scene in settings-appearance settings-voice settings-voice-ptt settings-security; do
    $BIN --scene "$scene$mode" --size 1280x1250 --at 1 --snap "$OUT/$scene$mode.png" "$@"
  done
done
# Non-default looks, for review: accent x background x tint (see scenes.rs).
for scene in emoji-picker-ocean channel-settings-ocean main-compact main-compact-light main-text-125 main-text-125-light main-ocean-cool main-coral-paper-light main-graphite-deep; do
  $BIN --scene "$scene" --size 1280x800 --at 1 --snap "$OUT/$scene.png" "$@"
done
# Keyboard-focus review: a scene can't ask for a keyboard ring, but a rune `--script` can press keys.
# `focus SCENE KEYS NAME [SIZE]`: KEYS is words, each `N` (Tab N times), `-N` (Shift+Tab N times),
# `@key` (click the element with that key) or a key name (`Home`), so the ring sits on a control
# against a clipping edge. To pick N, dump the
# scene (`$BIN --scene S --tree t.json --at 1`) and count the nodes whose "actions" include "focus"
# in document order (a roving group such as a grid counts every cell, though Tab visits one).
SCRIPT="$(mktemp)"
trap 'rm -f "$SCRIPT"' EXIT
FLAGS="$*"
focus() {
  echo "$2" | awk '{
    t = 0.1; printf "[";
    for (w = 1; w <= NF; w++) {
      if ($w ~ /^@/) { printf "%s{\"t\":%.3f,\"type\":\"tap\",\"key\":\"%s\"}", (t > 0.1 ? "," : ""), t, substr($w, 2); t += 0.2; continue }
      if ($w ~ /^-?[0-9]+$/) { n = ($w < 0 ? -$w : $w); name = "Tab"; shift = ($w < 0) } else { n = 1; name = $w; shift = 0 }
      for (i = 0; i < n; i++) { printf "%s{\"t\":%.3f,\"type\":\"key\",\"name\":\"%s\",\"shift\":%s}", (t > 0.1 ? "," : ""), t, name, (shift ? "true" : "false"); t += 0.002 }
    }
    print "]" }' > "$SCRIPT"
  $BIN --scene "$1" --size "${4:-1280x800}" --script "$SCRIPT" --at 1 --snap "$OUT/$3.png" $FLAGS
}
for mode in "" "-light"; do
  focus "main$mode" 2 "focus-rail$mode"
  focus "main$mode" 5 "focus-rail-add$mode"
  focus "main$mode" 6 "focus-sidebar-header$mode"
  focus "main$mode" 7 "focus-sidebar-row$mode"
  focus "channel-settings$mode" 4 "focus-dialog$mode"
  focus "channel-settings$mode" 10 "focus-icon-grid$mode"
  focus "channel-settings$mode" "@swatch-blue Home" "focus-dialog-swatch$mode"
  focus "server-settings-roles$mode" 7 "focus-roles$mode"
  focus "admin-servers$mode" 9 "focus-admin-row$mode"
done
