//! The voice bar: a card over the user strip while we are in (or joining) a voice channel. It says
//! whether we are connected, in which channel and server, shows the connection quality, and holds
//! the Disconnect button. The mute and deafen toggles live in the user strip (`sidebar.rs`), always
//! there like other chat apps.
//!
//! The camera and screen-share buttons come in a second row (`media_buttons`), only while connected:
//! the camera toggles our video, and the share button opens the picker of screens and windows (it
//! turns into "Stop sharing" while a share runs).

use rune::{col, icon, row, text, El};
use rune_kit::Kit;
use thehideout_design::components::util::frame_over;
use thehideout_design::components::IconButton;
use thehideout_design::icons::{
    MONITOR_ARROW_UP, PHONE_DISCONNECT, VIDEO_CAMERA, VIDEO_CAMERA_SLASH, WIFI_HIGH, WIFI_LOW, WIFI_MEDIUM,
};
use thehideout_design::{icon as size, metrics_of, radius, theme_of, type_};

use crate::app::App;
use crate::voice::VoiceStatus;
use thehideout_voice_types::Quality;

/// The bar, or nothing while we are not in voice.
pub fn build(a: &App, k: &Kit) -> Option<El> {
    let s = &a.state;
    let status_now = s.voice_status()?;
    let connected = status_now == VoiceStatus::Connected;
    let t = theme_of(k);
    let m = metrics_of(k);
    let (channel, guild) = s.voice_channel_label().unwrap_or_default();
    let status = match status_now {
        VoiceStatus::Connected => "Voice connected",
        VoiceStatus::Connecting => "Connecting\u{2026}",
        VoiceStatus::Reconnecting => "Reconnecting\u{2026}",
    };
    let poor = connected && matches!(s.voice.quality, Some(Quality::Poor | Quality::Lost));
    let ink = if connected { t.success } else { t.text_muted };
    // The slot for the connection's quality: the engine's report picks the glyph (good until it
    // reports); not connected, a faint low glyph holds the place.
    let quality = match (connected, s.voice.quality) {
        (false, _) => icon(&WIFI_LOW, m.icon(size::XS)).color(t.text_faint.color()),
        (true, Some(Quality::Poor | Quality::Lost)) => {
            icon(&WIFI_LOW, m.icon(size::XS)).color(t.danger.color())
        }
        (true, Some(Quality::Good)) => icon(&WIFI_MEDIUM, m.icon(size::XS)).color(t.warning.color()),
        (true, _) => icon(&WIFI_HIGH, m.icon(size::XS)).color(t.success.color()),
    }
    .key("quality")
    .a11y_hidden();
    let info = col()
        .key("voice-info")
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .gap(2.0)
        .child(
            row()
                .gap(6.0)
                .center_y()
                .child(
                    text(status)
                        .key("voice-status")
                        .font(type_::at(k, type_::CONTROL_STRONG))
                        .color(ink.color())
                        .max_lines(1)
                        .a11y_hidden(),
                )
                .child(quality),
        )
        .child(
            text(format!("{channel} / {guild}"))
                .key("voice-where")
                .font(type_::at(k, type_::CAPTION))
                .color(t.text_muted.color())
                .max_lines(1)
                .a11y_hidden(),
        );
    // In push-to-talk mode the bar says which key opens the mic, or that there is none yet.
    let info = if s.push_to_talk() {
        let caps = s
            .voice_shortcut(crate::voice::VoiceKey::PushToTalk)
            .map(|c| c.keycaps().join(if cfg!(target_os = "macos") { "" } else { "+" }));
        let (line, ink) = match caps {
            Some(c) => (format!("Push to talk: {c}"), t.text_muted),
            None => ("Push to talk: no key set".to_owned(), t.warning),
        };
        info.child(
            text(line)
                .key("voice-ptt")
                .font(type_::at(k, type_::CAPTION))
                .color(ink.color())
                .max_lines(1),
        )
    } else {
        info
    };
    let disconnect = IconButton::new(&PHONE_DISCONNECT, "Disconnect")
        .ink(t.danger.color())
        .hover_fill(t.danger_bg.color())
        .on_press(|a: &mut App| a.act(|s| s.leave_voice()))
        .build(k)
        .key("voice-disconnect");
    let mut bar = col()
        .key("voice-bar")
        .role("group")
        .label(format!(
            "{}, {channel}, {guild}{}",
            match status_now {
                VoiceStatus::Connected => "Voice connected",
                VoiceStatus::Connecting => "Connecting to voice",
                VoiceStatus::Reconnecting => "Reconnecting to voice",
            },
            if poor { ", poor connection" } else { "" }
        ))
        .w_full()
        .px(12.0)
        .py(8.0)
        .gap(6.0)
        .radius(radius::LG)
        .fill(t.surface.color())
        .child(
            row()
                .key("voice-main")
                .w_full()
                .gap(8.0)
                .center_y()
                .child(info)
                .child(disconnect),
        );
    let extra = media_buttons(a, k);
    if !extra.is_empty() {
        let mut line = row().key("voice-media").w_full().gap(4.0).center_y();
        for b in extra {
            line = line.child(b);
        }
        bar = bar.child(line);
    }
    // A hairline edge, as every inset panel has (light `surface` is barely off `bg`).
    bar = bar.child(frame_over(1.0, t.border_subtle.color(), radius::LG));
    Some(col().key("voice-zone").w_full().px(8.0).pb(8.0).child(bar))
}

/// The camera and screen-share buttons, once connected (nothing while joining or reconnecting:
/// there is no room to publish into).
fn media_buttons(a: &App, k: &Kit) -> Vec<El> {
    if a.state.voice_status() != Some(VoiceStatus::Connected) {
        return Vec::new();
    }
    let v = &a.state.voice;
    let camera = IconButton::new(
        if v.camera { &VIDEO_CAMERA } else { &VIDEO_CAMERA_SLASH },
        if v.camera { "Turn off camera" } else { "Turn on camera" },
    )
    .selected(v.camera)
    .on_press(|a: &mut App| a.act(|s| s.toggle_camera()))
    .build(k)
    .key("voice-camera");
    let sharing = v.sharing.is_some();
    let share = IconButton::new(&MONITOR_ARROW_UP, if sharing { "Stop sharing" } else { "Share screen" })
        .selected(sharing)
        .on_press(|a: &mut App| a.act(|s| s.toggle_screen_share()))
        .build(k)
        .key("voice-share");
    vec![camera, share]
}
