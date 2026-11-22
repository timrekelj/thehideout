//! The sidebar: the server's name (a menu), its channels, or Home's conversations, and the user
//! strip. Switching server crossfades the body (`motion-control`); the selection highlight
//! travels between rows of one list (`motion-reflow`, from the components).

use rune::{El, ScrollSpec, SpringPair, Transition, Tx, col, icon, row, text};
use rune_kit::{Kit, Menu, MenuItem, Placement};
use thehideout_design::components::tooltip::tooltip_at;
use thehideout_design::metrics_of;
use thehideout_design::components::{
    Avatar, ConnectionPill, ConnectionStatus, IconButton, NavRow, Presence,
    SidebarSection,
};
use thehideout_design::icons::{
    CHEVRON_DOWN, FOLDER_PLUS, GEAR, HEADPHONES, MICROPHONE, MICROPHONE_SLASH, PLUS, SIGN_OUT,
    USER_PLUS,
};
use thehideout_design::{icon as size, motion, theme_of, type_};

use crate::app::App;
use crate::state::{DialogKind, Menu as MenuState, Place};

/// `width` comes from the window (`shell::Layout`): 240 when there is room, 208 when not.
pub fn build(a: &App, k: &Kit, layer: &mut Vec<El>, width: f32) -> El {
    let t = theme_of(k);
    let place = match a.state.ui.place {
        Place::Home => "home".to_owned(),
        Place::Guild(g) => format!("guild-{g}"),
    };
    let fade = Transition::opacity().springs(SpringPair::new(motion::CONTROL, motion::CONTROL));
    let content = match a.state.ui.place {
        Place::Home => home_rows(a, k),
        Place::Guild(g) => super::channels::build(a, k, layer, g),
    };
    let body = col()
        .key("sidebar-body")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .child(
            col()
                .key(format!("sidebar-{place}"))
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .transition(fade)
                .child(
                    a.state.ui.edges.sidebar.wrap(
                        k,
                        col()
                            .key(format!("sidebar-scroll-{place}"))
                            .scroll(ScrollSpec {
                                stick_bottom: false,
                            })
                            .child(content),
                    ),
                ),
        );
    col()
        .key("sidebar")
        .w(width)
        .h_full()
        .fill(t.bg.color())
        .child(header(a, k, layer, width))
        .child(body)
        .children(super::voice_bar::build(a, k))
        .child(user_strip(a, k, layer))
}

fn header(a: &App, k: &Kit, layer: &mut Vec<El>, width: f32) -> El {
    let t = theme_of(k);
    let Some(g) = a.state.current_guild() else {
        return row()
            .key("sidebar-header")
            .w_full()
            .h(metrics_of(k).header_h())
            .px(16.0)
            .center_y()
            .child(
                text("Direct messages")
                    .font(type_::at(k, type_::TITLE))
                    .color(t.text.color()),
            );
    };
    let (id, manage) = (g.id, a.state.can_manage_channels(g.id));
    let trigger = row()
        .key("guild-header")
        .role("button")
        .label(format!("{}, server menu", g.name))
        .w(width)
        .h(metrics_of(k).header_h())
        .px(16.0)
        .gap(8.0)
        .center_y()
        .focusable(true)
        .focus_ring(2.0, -2.0, k.c.border_focus)
        .hover(t.hover.color())
        .on_press(|a: &mut App| {
            a.state.ui.menu = if a.state.ui.menu == MenuState::Guild {
                MenuState::None
            } else {
                MenuState::Guild
            }
        })
        .child(
            text(g.name.clone())
                .font(type_::at(k, type_::TITLE))
                .color(t.text.color())
                .max_lines(1)
                .grow(1.0)
                .shrink()
                .a11y_hidden(),
        )
        .child(
            icon(&CHEVRON_DOWN, size::SM)
                .color(t.text_muted.color())
                .a11y_hidden(),
        );
    let mut menu = Menu::new("guild-menu")
        .trigger(trigger)
        .item(
            MenuItem::new("Invite people")
                .icon(&USER_PLUS)
                .on_select(move |a: &mut App| {
                    a.act(|s| s.open_dialog(DialogKind::Invite { guild: id }))
                }),
        );
    // Only for those who may: inviting by email makes accounts.
    if a.state.can_invite(id) {
        menu = menu.item(
            MenuItem::new("Invite people by email\u{2026}")
                .icon(&thehideout_design::icons::ENVELOPE_SIMPLE)
                .on_select(move |a: &mut App| {
                    a.act(|s| s.open_dialog(DialogKind::InviteByEmail { guild: id }))
                }),
        );
    }
    let menu = menu
        .item(
            MenuItem::new("Create channel")
                .icon(&PLUS)
                .disabled(!manage)
                .on_select(move |a: &mut App| {
                    a.act(|s| s.open_dialog(DialogKind::CreateChannel { guild: id }))
                }),
        )
        .item(
            MenuItem::new("Create section")
                .icon(&FOLDER_PLUS)
                .disabled(!manage)
                .on_select(move |a: &mut App| {
                    a.act(|s| s.open_dialog(DialogKind::CreateSection { guild: id }))
                }),
        )
        .item(
            MenuItem::new("Server settings\u{2026}")
                .icon(&thehideout_design::icons::GEAR_SIX)
                .disabled(!a.state.can_open_server_settings(id))
                .on_select(move |a: &mut App| a.act(|s| s.open_server_settings(id))),
        );
    // Members may leave; the owner must hand the server over or delete it first.
    let menu = if a.state.can_leave_guild(id) {
        menu.separator().item(
            MenuItem::new("Leave server\u{2026}")
                .icon(&thehideout_design::icons::SIGN_OUT)
                .danger(true)
                .on_select(move |a: &mut App| a.act(|s| s.open_dialog(DialogKind::LeaveGuild { guild: id }))),
        )
    } else {
        menu
    };
    let menu = menu
        .open(a.state.ui.menu == MenuState::Guild)
        .min_width(width - 16.0)
        .on_close(|a: &mut App| a.state.ui.menu = MenuState::None)
        .build(k, layer)
        .role("none");
    row().w_full().child(menu)
}

fn home_rows(a: &App, k: &Kit) -> El {
    let t = theme_of(k);
    let s = &a.state;
    let mut list = SidebarSection::new("Direct messages", s.ui.dms_open)
        .on_toggle(|a: &mut App| a.state.ui.dms_open = !a.state.ui.dms_open);
    for dm in &s.data.dms {
        let name = s
            .dm_peer(dm)
            .map_or_else(|| "Unknown".to_owned(), |u| u.username.clone());
        let u = s.unread_of(dm.id);
        let id = dm.id;
        list = list.child(
            NavRow::person(name, dm_peer_id(s, dm))
                .highlight_group("dms")
                .selected(s.current_channel() == Some(id))
                .unread(u.count > 0)
                .mentions(u.mentions)
                .on_press(move |a: &mut App| a.act(|s| s.select_channel(id)))
                .build(k),
        );
    }
    let new_dm = NavRow::new("New message", &PLUS)
        .highlight_group("home-actions")
        .on_press(|a: &mut App| a.act(|s| s.open_dialog(DialogKind::NewDm)))
        .build(k)
        .key("new-dm");
    let mut c = col()
        .key("dm-list")
        .w_full()
        .padding(8.0)
        .gap(8.0)
        .child(new_dm)
        .child(list.build(k));
    if s.data.dms.is_empty() {
        c = c.child(
            text("No conversations yet.")
                .font(type_::at(k, type_::CONTROL))
                .color(t.text_faint.color())
                .px(8.0),
        );
    }
    c
}

/// The id that colours the other person's avatar (the same one the transcript uses).
fn dm_peer_id(s: &crate::state::AppState, dm: &thehideout_api::DmChannel) -> String {
    s.dm_peer(dm).map_or_else(String::new, |u| u.id.to_string())
}

/// The strip under the sidebar: your avatar (its dot is the connection), your name, and a gear
/// menu. `FOOTER_H` tall, like the composer zone, so the edge lines above them line up. The
/// connection pill appears only while something is wrong.
fn user_strip(a: &App, k: &Kit, layer: &mut Vec<El>) -> El {
    let t = theme_of(k);
    let me = a.state.me();
    let name = me.map_or_else(String::new, |u| u.username.clone());
    let id = me.map_or_else(String::new, |u| u.id.to_string());
    let status = a.state.ui.connection;
    let presence = match status {
        ConnectionStatus::Connected => Presence::Online,
        ConnectionStatus::Connecting | ConnectionStatus::Reconnecting(_) => Presence::Idle,
        ConnectionStatus::Offline => Presence::Offline,
    };
    let (mute, deafen) = voice_toggles(a, k, layer);
    let gear = IconButton::new(&GEAR, "Account menu")
        .selected(a.state.ui.menu == MenuState::User)
        .on_press(|a: &mut App| {
            a.state.ui.menu = if a.state.ui.menu == MenuState::User {
                MenuState::None
            } else {
                MenuState::User
            }
        })
        .build(k)
        .key("user-menu-button");
    let mut menu = Menu::new("user-menu")
        .trigger(gear)
        .item(
            MenuItem::new("Profile\u{2026}")
                .icon(&thehideout_design::icons::USER_CIRCLE)
                .on_select(|a: &mut App| a.act(|s| s.open_profile())),
        )
        .item(
            MenuItem::new("Settings\u{2026}")
                .icon(&GEAR)
                .on_select(|a: &mut App| a.act(|s| s.open_settings())),
        );
    if a.state.is_admin() {
        menu = menu.item(
            MenuItem::new("Admin\u{2026}")
                .icon(&thehideout_design::icons::SHIELD)
                .shortcut(if cfg!(target_os = "macos") {
                    "\u{21e7}\u{2318}A"
                } else {
                    "Ctrl+Shift+A"
                })
                .on_select(|a: &mut App| a.act(|s| s.open_admin())),
        );
    }
    let menu = menu
        .item(
            MenuItem::new("Security\u{2026}")
                .icon(&thehideout_design::icons::LOCK)
                .on_select(|a: &mut App| a.act(|s| s.open_security())),
        )
        .item(
            MenuItem::new("Sign out")
                .icon(&SIGN_OUT)
                .on_select(|a: &mut App| a.act(|s| s.sign_out())),
        )
        .open(a.state.ui.menu == MenuState::User)
        .placement(rune_kit::Placement::TopEnd)
        .min_width(160.0)
        .on_close(|a: &mut App| a.state.ui.menu = MenuState::None)
        .build(k, layer)
        .role("none");
    let mut who = col()
        .key("who")
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .gap(2.0)
        .child(
            text(name.clone())
                .font(type_::at(k, type_::CONTROL_STRONG))
                .color(t.text.color())
                .max_lines(1),
        );
    if status != ConnectionStatus::Connected {
        who = who.child(
            row()
                .key("connection")
                .child(ConnectionPill::new(status).build(k)),
        );
    }
    row()
        .key("user-strip")
        .w_full()
        .h(metrics_of(k).footer_h())
        .px(12.0)
        .gap(10.0)
        .center_y()
        .child(
            Avatar::new(id, name)
                .size(32)
                .image(a.state.avatar_pixels(a.state.my_id()))
                .presence(presence)
                .backdrop(t.bg.color())
                .build(k),
        )
        .child(who)
        .child(mute)
        .child(deafen)
        .child(menu)
}

/// The mute and deafen toggles of the user strip, always there (in voice or not: the flags apply
/// to the next channel joined). A flag that is on reads in the danger ink with the matching name
/// ("Unmute", "Undeafen"). While a moderator has muted us the mute button is disabled and says so
/// in a tooltip: our own flag changes nothing then.
fn voice_toggles(a: &App, k: &Kit, layer: &mut Vec<El>) -> (El, El) {
    let t = theme_of(k);
    let v = &a.state.voice;
    let server_muted = a.state.i_am_server_muted();
    let muted = v.self_mute || server_muted;
    let mut mute = IconButton::new(
        if muted { &MICROPHONE_SLASH } else { &MICROPHONE },
        if v.self_mute || server_muted { "Unmute" } else { "Mute" },
    )
    .disabled(server_muted)
    .on_press(|a: &mut App| a.act(|s| s.toggle_mute()));
    if muted {
        mute = mute.ink(t.danger.color());
    }
    let mut mute = mute.build(k).key("voice-mute");
    if server_muted {
        mute = mute.description("Muted by a moderator");
    } else if a.state.push_to_talk() {
        mute = mute.description("Push to talk");
    }
    let mute = tooltip_at(
        k,
        "voice-mute-tip",
        row()
            .key("voice-mute-wrap")
            .on_hover_change(|a: &mut App, on: bool, _tx: &mut Tx| a.state.voice.mute_tip = on)
            .child(mute),
        "Muted by a moderator",
        &[],
        server_muted && v.mute_tip,
        Placement::Top,
        layer,
    );
    let mut deafen = IconButton::new(
        &HEADPHONES,
        if v.self_deaf { "Undeafen" } else { "Deafen" },
    )
    .on_press(|a: &mut App| a.act(|s| s.toggle_deaf()));
    if v.self_deaf {
        deafen = deafen.ink(t.danger.color());
    }
    (mute, deafen.build(k).key("voice-deafen"))
}
