//! The voice stage: what the main panel shows in place of the transcript while we are connected and
//! have opened it (a press on our voice channel's row, or "Watch" on someone's LIVE pill). A header
//! with the way back to the chat, then the tiles: a responsive grid, or, once a tile is focused (a
//! click, or a screen share by itself), that one big with the rest in a strip under it. Also here:
//! the "Share your screen" picker dialog the voice bar's share button opens.
//!
//! A tile is a `VoiceTile`: live video letterboxed in a rounded tile, or the person's avatar. The
//! pictures are `Pixels` the voice sink keeps replacing (`App::video_pixels`); the state subscribes
//! the sink to exactly the tiles on this view (`AppState::settle_video`), so closing it stops the
//! decoding.

use rune::{col, icon, row, text, El};
use rune_kit::Kit;
use thehideout_design::components::{
    Button, Dialog, IconButton, TileMedia, TileSize, Variant, VoiceTile,
};
use thehideout_design::icons::{APP_WINDOW, ARROW_LEFT, MONITOR};
use thehideout_design::{icon as size, metrics_of, radius, theme_of, type_};
use thehideout_voice::{ScreenKind, ScreenSource};

use crate::app::App;
use crate::voice::{Tile, TileKind};

/// The gap between tiles and round the stage.
const GAP: f32 = 8.0;
/// How tall the strip of unfocused tiles is, and the widest one of its tiles.
const STRIP_H: f32 = 124.0;
const STRIP_TILE_W: f32 = 220.0;

/// The whole panel while the stage is open: the header and the tiles.
pub fn build(a: &App, k: &Kit) -> El {
    col()
        .key("voice-stage")
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .h_full()
        .child(header(a, k))
        .child(body(a, k))
}

fn header(a: &App, k: &Kit) -> El {
    let t = theme_of(k);
    let (channel, guild) = a.state.voice_channel_label().unwrap_or_default();
    row()
        .key("stage-header")
        .role("banner")
        .label(format!("Voice, {channel}"))
        .w_full()
        .h(metrics_of(k).header_h())
        .px(16.0)
        .gap(10.0)
        .center_y()
        .child(
            IconButton::new(&ARROW_LEFT, "Back to chat")
                .on_press(|a: &mut App| a.act(|s| s.close_stage()))
                .build(k)
                .key("stage-back"),
        )
        .child(
            row()
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .gap(8.0)
                .center_y()
                .child(
                    text("Voice")
                        .font(type_::at(k, type_::TITLE))
                        .color(t.text.color())
                        .a11y_hidden(),
                )
                .child(
                    text(format!("{channel} / {guild}"))
                        .font(type_::at(k, type_::CONTROL))
                        .color(t.text_muted.color())
                        .max_lines(1)
                        .shrink()
                        .min_w(0.0)
                        .a11y_hidden(),
                ),
        )
}

fn body(a: &App, k: &Kit) -> El {
    let s = &a.state;
    let t = theme_of(k);
    let tiles = s.stage_tiles();
    let focus = s.stage_focus(&tiles);
    let mut body = col()
        .key("stage-body")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .px(GAP + 4.0)
        .pb(GAP + 4.0)
        .gap(GAP);
    let any_video = tiles.iter().any(|t| t.track.is_some());
    match focus.and_then(|(u, kind)| tiles.iter().find(|t| t.user == u && t.kind == kind)) {
        Some(big) => {
            // The big tile takes what the strip leaves; it may shrink so the strip never runs out
            // of the panel.
            body = body.child(
                tile(a, k, big, TileSize::Large)
                    .w_full()
                    .grow(1.0)
                    .shrink()
                    .min_h(0.0),
            );
            let rest: Vec<&Tile> = tiles.iter().filter(|t| *t != big).collect();
            if !rest.is_empty() {
                // Equal 16:9 tiles, left-aligned under the big one, gapped like the grid.
                let mut strip = row().key("stage-strip").w_full().h(STRIP_H).gap(GAP);
                for t in rest {
                    strip = strip.child(tile(a, k, t, TileSize::Small).size(STRIP_TILE_W, STRIP_H));
                }
                body = body.child(strip);
            }
        }
        None => {
            let cols = grid_columns(tiles.len());
            let mut it = tiles.iter().peekable();
            while it.peek().is_some() {
                let mut line = row().w_full().grow(1.0).min_h(0.0).gap(GAP);
                for t in it.by_ref().take(cols) {
                    line = line.child(
                        tile(a, k, t, TileSize::Medium)
                            .grow(1.0)
                            .shrink()
                            .min_w(0.0)
                            .h_full(),
                    );
                }
                body = body.child(line);
            }
        }
    }
    if !any_video {
        body = body.child(
            text(
                "Nobody has video on. Turn on your camera or share your screen from the voice bar.",
            )
            .key("stage-hint")
            .font(type_::at(k, type_::CAPTION))
            .color(t.text_muted.color())
            .w_full()
            .align(rune::Align::Center),
        );
    }
    body
}

/// Columns for `n` tiles: one alone, two across up to four, then three, then four.
pub fn grid_columns(n: usize) -> usize {
    match n {
        0 | 1 => 1,
        2..=4 => 2,
        5..=9 => 3,
        _ => 4,
    }
}

/// One tile: its picture (video, or avatar), its name and state, and the press that focuses it.
fn tile(a: &App, k: &Kit, tile: &Tile, size: TileSize) -> El {
    let s = &a.state;
    let user = tile.user;
    let me = user == s.my_id();
    let name = if me {
        "You".to_owned()
    } else {
        s.user_name(user)
    };
    let st = s.voice_state_of(user);
    let media = match &tile.track {
        Some(track) => TileMedia::Video(a.video_pixels(track)),
        None => TileMedia::Avatar {
            id: user.to_string(),
            name: s.user_name(user),
            image: s.avatar_pixels(user),
        },
    };
    let who = if me {
        "Your".to_owned()
    } else {
        format!("{}\u{2019}s", s.user_name(user))
    };
    let (label, shown) = match tile.kind {
        TileKind::Camera => (format!("{who} camera"), name),
        TileKind::Screen => (format!("{who} screen"), format!("{who} screen")),
        TileKind::Avatar => (
            if me {
                "You".to_owned()
            } else {
                s.user_name(user)
            },
            name,
        ),
    };
    let kind = tile.kind;
    let mut t = VoiceTile::new(label, shown, media)
        .size(size)
        .screen(kind == TileKind::Screen)
        .on_press(move |a: &mut App| a.act(|s| s.focus_tile(user, kind)));
    if kind != TileKind::Screen {
        t = t
            .speaking(s.is_speaking(user))
            .muted(st.is_some_and(|v| v.self_mute || v.self_deaf))
            .server_mute(st.is_some_and(|v| v.server_mute));
    }
    let key = match kind {
        TileKind::Camera => format!("tile-camera-{user}"),
        TileKind::Screen => format!("tile-screen-{user}"),
        TileKind::Avatar => format!("tile-avatar-{user}"),
    };
    t.build(k).key(key)
}

// ---- the share picker ------------------------------------------------------------------------

fn close(a: &mut App) {
    a.act(|s| s.close_screen_picker());
}

/// The "Share your screen" dialog (nothing while it is closed): the screens first, then the windows.
pub fn picker(a: &App, k: &Kit, layer: &mut Vec<El>) {
    if !a.state.voice.picker {
        return;
    }
    let t = theme_of(k);
    let choices = a.state.share_choices();
    let mut body = col().key("share-list").w_full().gap(4.0);
    if a.state.voice.screen_sources.is_none() {
        body = body.child(
            text("Looking for screens and windows\u{2026}")
                .key("share-loading")
                .font(type_::at(k, type_::CONTROL))
                .color(t.text_muted.color()),
        );
    } else if choices.is_empty() {
        body = body.child(
            text("Nothing to share was found. Allow Screen Recording for this app in your system settings, then try again.")
                .key("share-empty")
                .font(type_::at(k, type_::CONTROL))
                .color(t.text_muted.color())
                .wrap()
                .w_full(),
        );
    }
    let mut last: Option<ScreenKind> = None;
    for src in choices {
        if last != Some(src.kind) {
            last = Some(src.kind);
            body = body.child(
                text(if src.kind == ScreenKind::Screen {
                    "Screens"
                } else {
                    "Windows"
                })
                .key(if src.kind == ScreenKind::Screen {
                    "share-screens"
                } else {
                    "share-windows"
                })
                .font(type_::at(k, type_::LABEL))
                .color(t.text_muted.color())
                .pt(if last.is_some() && src.kind == ScreenKind::Window {
                    8.0
                } else {
                    0.0
                })
                .pb(2.0),
            );
        }
        body = body.child(source_row(k, src));
    }
    Dialog::new("dlg-share", "Share your screen")
        .description("Pick a screen or a window. Everyone in the voice channel can watch it.")
        .open(true)
        .child(body)
        .footer(
            Button::new("Cancel")
                .variant(Variant::Secondary)
                .on_press(close)
                .build(k)
                .key("share-cancel"),
        )
        .on_close(close)
        .build(k, layer);
}

fn source_row(k: &Kit, src: ScreenSource) -> El {
    let t = theme_of(k);
    let m = metrics_of(k);
    let glyph = if src.kind == ScreenKind::Screen {
        &MONITOR
    } else {
        &APP_WINDOW
    };
    let id = src.id;
    let title = src.title.clone();
    let kind = if src.kind == ScreenKind::Screen {
        "screen"
    } else {
        "window"
    };
    let pick = src.clone();
    row()
        .key(format!("share-source-{id}"))
        .role("button")
        .label(format!("{title}, {kind}"))
        .w_full()
        .h(m.row_h() + 8.0)
        .px(12.0)
        .gap(12.0)
        .center_y()
        .radius(radius::MD)
        .focusable(true)
        .focus_ring(
            thehideout_design::space::RING_WIDTH,
            thehideout_design::space::RING_INSET,
            k.c.border_focus,
        )
        .hover(t.hover.color())
        .press(t.pressed.color())
        .on_press(move |a: &mut App| a.act(|s| s.start_screen_share(pick.clone())))
        .child(
            icon(glyph, m.icon(size::MD))
                .color(k.c.icon_muted)
                .a11y_hidden(),
        )
        .child(
            text(title)
                .font(type_::at(k, type_::CONTROL))
                .color(t.text.color())
                .max_lines(1)
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .a11y_hidden(),
        )
}
