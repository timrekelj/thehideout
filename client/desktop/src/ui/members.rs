//! The right-hand member panel. Its width springs open and closed (`motion-reflow`); the content
//! keeps its own fixed width inside a clip, so rows never squash while it moves.
//!
//! In a server the members sit under a header per role (the ones that are somebody's highest, by
//! position, most senior first), then "Members" for those with no role. A name takes the colour
//! of the member's highest role that has one, in the text variant of the palette.

use rune::{col, row, text, El, PointerPress, ScrollSpec, Tx};
use rune_kit::{context_menu, Kit, Menu, MenuItem};
use thehideout_api::User;
use thehideout_design::metrics_of;
use thehideout_design::components::{MemberRow};
use thehideout_design::icons::{PROHIBIT, USER_MINUS};
use thehideout_design::{motion, palette_text, theme_of, type_};

use crate::app::App;
use crate::state::{Menu as MenuState, Place};

pub const WIDTH: f32 = 232.0;

fn people(a: &App) -> Vec<User> {
    let s = &a.state;
    match s.ui.place {
        Place::Guild(g) => s
            .data
            .members
            .get(&g)
            .map(|ids| {
                ids.iter()
                    .filter_map(|i| s.data.users.get(i).cloned())
                    .collect()
            })
            .unwrap_or_default(),
        Place::Home => s
            .current_channel()
            .and_then(|c| s.dm(c))
            .map(|d| d.participants.clone())
            .unwrap_or_default(),
    }
}

fn section_label(k: &Kit, label: String, key: String) -> El {
    let t = theme_of(k);
    row()
        .key(key)
        .w_full()
        .h(28.0)
        .px(8.0)
        .pt(8.0)
        .center_y()
        .child(
            text(label)
                .font(type_::at(k, type_::LABEL))
                .color(t.text_muted.color())
                .max_lines(1),
        )
}

/// `allowed`: the window is wide enough to offer the panel at all.
pub fn build(a: &App, k: &Kit, layer: &mut Vec<El>, allowed: bool) -> El {
    let t = theme_of(k);
    let s = &a.state;
    let open = s.ui.members_open && allowed;
    let owner = s.current_guild().map(|g| g.owner_id);
    let list = people(a);
    let guild = match s.ui.place {
        Place::Guild(g) => Some(g),
        Place::Home => None,
    };
    let mut member = |u: &User| {
        let ink = guild
            .and_then(|g| palette_text(s.name_color(g, u.id).as_key(), t))
            .map(|tone| tone.color());
        let user = u.id;
        // Kick and Ban are offered on a press or a right-click, to those who may and only for
        // someone they outrank.
        let moderate = guild.filter(|g| !u.deleted && s.has_member_menu(*g, user));
        let mut row = MemberRow::new(user.to_string(), u.username.clone())
            .backdrop(t.surface.color())
            .owner(owner == Some(user))
            .image(s.avatar_pixels(user))
            .name_color(ink);
        if moderate.is_some() {
            row = row.on_press(move |a: &mut App| {
                a.state.ui.menu = match a.state.ui.menu {
                    MenuState::Member { user: m, at: None } if m == user => MenuState::None,
                    _ => MenuState::Member { user, at: None },
                }
            });
        }
        let el = row.build(k).key(format!("member-{user}"));
        match moderate {
            Some(g) => menu(a, k, layer, g, user, el),
            None => el,
        }
    };
    let mut rows = col().key("member-rows").w_full().gap(2.0).padding(8.0);
    match guild {
        Some(g) => {
            let groups = s.member_groups(g);
            let grouped = groups.iter().any(|x| x.role.is_some());
            for group in &groups {
                if grouped {
                    let (label, key) = match &group.role {
                        Some(r) => (
                            format!("{} \u{2014} {}", r.name, group.users.len()),
                            format!("members-role-{}", r.id),
                        ),
                        None => (
                            format!("Members \u{2014} {}", group.users.len()),
                            "members-rest".to_owned(),
                        ),
                    };
                    rows = rows.child(section_label(k, label, key));
                }
                for id in &group.users {
                    if let Some(u) = s.data.users.get(id) {
                        rows = rows.child(member(u));
                    }
                }
            }
        }
        None => {
            for u in &list {
                rows = rows.child(member(u));
            }
        }
    }
    let inner = col()
        .key("members-inner")
        .w(WIDTH)
        .h_full()
        .border_sides([0.0, 0.0, 0.0, 1.0], t.border_subtle.color())
        .child(
            row()
                .h(metrics_of(k).header_h())
                .px(16.0)
                .center_y()
                .child(
                    text(format!("Members \u{00b7} {}", list.len()))
                        .font(type_::at(k, type_::CONTROL_STRONG))
                        .color(t.text_muted.color()),
                ),
        )
        .child(s.ui.edges.members.wrap(
            k,
            col()
                .key("members-scroll")
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(rows),
        ));
    let inner = if open {
        inner
    } else {
        inner.inert().a11y_hidden()
    };
    col()
        .key("members")
        .h_full()
        .w(if open { WIDTH } else { 0.0 })
        .clip_rounded(0.0)
        .token_motion(motion::REFLOW)
        .child(inner)
}

/// Wraps a member's row with the menu a press (or a right-click) opens: Kick and Ban.
fn menu(a: &App, k: &Kit, layer: &mut Vec<El>, guild: u64, user: u64, row: El) -> El {
    let s = &a.state;
    let row = row.on_secondary_press(move |a: &mut App, p: PointerPress, _tx: &mut Tx| {
        a.state.ui.menu = MenuState::Member { user, at: Some((p.x as i32, p.y as i32)) };
    });
    let (open, at) = match s.ui.menu {
        MenuState::Member { user: u, at } if u == user => (true, at),
        _ => (false, None),
    };
    let key = format!("menu-member-{user}");
    let menu = match at {
        Some((x, y)) => context_menu(key, row, Some(PointerPress { x: x as f32, y: y as f32 })),
        None => Menu::new(key).trigger(row).open(open),
    };
    let mut menu = menu;
    if s.can_kick(guild, user) {
        menu = menu.item(
            MenuItem::new("Kick")
                .icon(&USER_MINUS)
                .danger(true)
                .on_select(move |a: &mut App| a.act(|s| s.ask_kick(guild, user))),
        );
    }
    if s.can_ban(guild, user) {
        menu = menu.item(
            MenuItem::new("Ban\u{2026}")
                .icon(&PROHIBIT)
                .danger(true)
                .on_select(move |a: &mut App| a.act(|s| s.ask_ban(guild, user))),
        );
    }
    menu.on_close(|a: &mut App| a.state.ui.menu = MenuState::None)
        .min_width(160.0)
        .build(k, layer)
        .role("none")
}
