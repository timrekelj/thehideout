//! The main screen: rail, sidebar and the inset panel holding the header, transcript, composer
//! and the member panel. Works from 800x600 up; the transcript takes whatever width is left.

use rune::{El, col, el, icon, row, text};
use rune_kit::Kit;
use thehideout_design::metrics_of;
use thehideout_design::components::util::frame_over;
use thehideout_design::components::{Avatar, Button, ChannelHeader, IconButton, Size, Variant};
use thehideout_design::icons::{GEAR, HASH, USERS, X, catalog};
use thehideout_design::{palette_color, radius, theme_of, type_};

use super::{admin, composer, members, rail, server_settings, settings, sidebar, transcript, voice_stage};
use crate::account::SecKind;
use crate::app::App;
use crate::state::{DialogKind, Place};

/// What the window's width allows: the sidebar is 240 from 1100pt up and 208 below, and the member
/// panel is only offered from 1000pt (below that the transcript would be too narrow).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub sidebar: f32,
    pub members: bool,
    /// The window's height, for what must fit in it (a dialog's body scrolls past it).
    pub height: f32,
    pub width: f32,
}

impl Layout {
    pub fn of(width: f32, height: f32) -> Layout {
        Layout {
            sidebar: if width >= 1100.0 { 240.0 } else { 208.0 },
            members: width >= 1000.0,
            height,
            width,
        }
    }
}

impl Default for Layout {
    /// Before the first layout is known: the roomy one.
    fn default() -> Layout {
        Layout::of(f32::MAX, f32::MAX)
    }
}

/// Space between the window edge and the panel; the rail, sidebar and panel all start below the
/// top one, so their headers and footers share y.
const INSET: f32 = 8.0;

/// Height of the strip the window's controls sit in (macOS traffic lights, Windows caption
/// buttons). `bar` below is what the platform actually reserves of it: 0 where the native frame
/// stays.
pub const TITLEBAR: f32 = 40.0;

/// `bar`: the titlebar's height (`Cx::chrome_insets().height`). The shell starts below it, so the
/// rail's Home button and the sidebar header never sit under the traffic lights; `app::view`
/// lays the drag strip over that band.
pub fn build(a: &App, k: &Kit, layer: &mut Vec<El>, bar: f32, lay: Layout) -> El {
    if let Some(section) = a.state.ui.settings {
        return settings::build(a, k, layer, section, bar);
    }
    if a.state.ui.server_settings.is_some() {
        return server_settings::build(a, k, layer, bar);
    }
    // The admin dashboard replaces the shell the same way; below 1000pt the window is narrow
    // enough to drop the secondary columns of its tables.
    if a.state.ui.admin.is_some() {
        return admin::build(a, k, layer, bar, !lay.members);
    }
    let t = theme_of(k);
    let panel = row()
        .key("main-panel")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .fill(t.surface.color())
        .radius(radius::LG)
        .clip_rounded(radius::LG);
    // The voice stage takes the whole panel (members included) while it is open.
    let panel = if a.state.stage_open() {
        panel.child(voice_stage::build(a, k))
    } else {
        panel
            .child(
                col()
                    .key("chat")
                    .grow(1.0)
                    .shrink()
                    .min_w(0.0)
                    .h_full()
                    .child(header(a, k, lay))
                    .children(email_banner(a, k))
                    .child(transcript::build(a, k, layer))
                    .child(composer::build(a, k)),
            )
            .child(members::build(a, k, layer, lay.members))
    }
    .child(frame_over(1.0, t.border_subtle.color(), radius::LG));
    let body = row()
        .key("shell-body")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .pt(INSET)
        .pb(INSET)
        .child(rail::build(a, k, layer))
        .child(sidebar::build(a, k, layer, lay.sidebar))
        .child(col().grow(1.0).min_w(0.0).h_full().pr(INSET).child(panel));
    col()
        .key("shell")
        .w_full()
        .h_full()
        .fill(t.bg.color())
        .pt(bar)
        .child(body)
}

/// A legacy account (no email) is asked to add one: a quiet wash under the header, with the
/// way to do it and a dismiss for this session.
fn email_banner(a: &App, k: &Kit) -> Option<El> {
    if !a.state.wants_email() {
        return None;
    }
    let t = theme_of(k);
    let glyph = &thehideout_design::icons::ENVELOPE_SIMPLE;
    Some(
        row()
            .key("email-banner")
            .role("status")
            .w_full()
            .px(12.0)
            .child(
                row()
                    .w_full()
                    .px(12.0)
                    .py(8.0)
                    .gap(10.0)
                    .center_y()
                    .radius(radius::MD)
                    .fill(t.info_bg.color())
                    .child(icon(glyph, thehideout_design::icon::SM).color(t.info.color()).a11y_hidden())
                    .child(
                        text("Add an email to your account, so you can sign in with a code and get back in if you forget your password.")
                            .font(type_::at(k, type_::CONTROL))
                            .color(t.text.color())
                            .wrap()
                            .grow(1.0)
                            .shrink(),
                    )
                    .child(
                        Button::new("Add email")
                            .variant(Variant::Secondary)
                            .size(Size::Sm)
                            .on_press(|a: &mut App| a.act(|s| s.open_sec(SecKind::Email { sent: false })))
                            .build(k)
                            .key("email-banner-add"),
                    )
                    .child(
                        IconButton::new(&X, "Dismiss")
                            .size(Size::Sm)
                            .on_press(|a: &mut App| a.act(|s| s.dismiss_email_banner()))
                            .build(k)
                            .key("email-banner-dismiss"),
                    ),
            ),
    )
}

fn header(a: &App, k: &Kit, lay: Layout) -> El {
    let t = theme_of(k);
    let s = &a.state;
    let open = s.ui.members_open && lay.members;
    let toggle = IconButton::new(&USERS, if open { "Hide members" } else { "Show members" })
        .selected(open)
        .size(thehideout_design::components::Size::Md)
        .on_press(|a: &mut App| a.state.ui.members_open = !a.state.ui.members_open)
        .build(k)
        .key("members-toggle");
    let name = s.current_channel_name();
    match (s.ui.place, name) {
        (Place::Guild(g), Some(name)) => {
            let mut h = ChannelHeader::new(name);
            if let Some(c) = s.current_guild_channel() {
                let glyph = catalog::by_name(c.icon.as_str()).unwrap_or(&HASH);
                h = h.glyph(glyph, palette_color(c.color.as_key(), t));
                if let Some(art) = super::emoji::channel_art(a, k, c, 18.0) {
                    h = h.emoji(art);
                }
                if !c.topic.is_empty() {
                    h = h.topic(c.topic.clone());
                }
                if s.can_manage_channels(g) {
                    let id = c.id;
                    h = h.action(
                        IconButton::new(&GEAR, "Channel settings")
                            .size(thehideout_design::components::Size::Md)
                            .on_press(move |a: &mut App| {
                                a.act(|s| {
                                    s.open_dialog(DialogKind::ChannelSettings {
                                        guild: g,
                                        channel: id,
                                    })
                                })
                            })
                            .build(k)
                            .key("channel-settings-button"),
                    );
                }
            }
            (if lay.members { h.action(toggle) } else { h }).build(k)
        }
        (Place::Home, Some(name)) => {
            let id = s
                .current_channel()
                .and_then(|c| s.dm(c))
                .and_then(|d| s.dm_peer(d))
                .map_or(0, |u| u.id);
            row()
                .role("banner")
                .label(name.clone())
                .w_full()
                .h(metrics_of(k).header_h())
                .px(16.0)
                .gap(10.0)
                .center_y()
                .child(
                    Avatar::new(id.to_string(), name.clone())
                        .size(24)
                        .image(s.avatar_pixels(id))
                        .backdrop(t.surface.color())
                        .build(k),
                )
                .child(
                    text(name)
                        .font(type_::at(k, type_::TITLE))
                        .color(t.text.color())
                        .grow(1.0),
                )
        }
        // Home with nothing chosen: the empty state below says it all; a bar over it is clutter.
        (Place::Home, None) => el().key("header-none").h(0.0),
        _ => row()
            .role("banner")
            .label("No channel")
            .w_full()
            .h(metrics_of(k).header_h()),
    }
}
