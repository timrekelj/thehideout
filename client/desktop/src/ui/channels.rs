//! The sidebar's channel list for a guild: top-level channels, then each section as a
//! collapsible group. Channels show their own icon and palette colour. Users who may manage
//! channels also get the structure controls: drag a row (or a section) to move it, or use the
//! same moves from the row's "..." button and right-click menu (the keyboard and screen-reader
//! way, and how a drag is undone). Others see none of it.

use rune::{col, Anim, Drag, El, Phase, PointerPress, Tx};
use rune_kit::{context_menu, Kit, Menu, MenuEntry, MenuItem};
use thehideout_design::components::{ChannelRow, IconButton, SidebarSection, Size};
use thehideout_design::icons::{
    catalog, ARROW_DOWN, ARROW_UP, DOTS_THREE, PENCIL, PLUS, TRASH,
};
use thehideout_design::{palette_color, theme_of};

use crate::app::App;
use crate::layout::{self, Item};
use crate::state::{AppState, DialogKind, Menu as MenuState};

const SECTION_GAP: f32 = 8.0;
/// Points a press must travel before it is a drag, not a click.
const SLOP: f32 = 4.0;

pub fn build(a: &App, k: &Kit, layer: &mut Vec<El>, guild: u64) -> El {
    let s = &a.state;
    let manage = s.can_manage_channels(guild);
    let mut list = col()
        .key("channel-list")
        .w_full()
        .padding(8.0)
        .gap(SECTION_GAP);
    let Some(layout) = s.layout(guild) else {
        return list.child(group(a, k, layer, guild, None, manage));
    };
    if !layout::channels_in(layout, None).is_empty() || layout.sections.is_empty() {
        list = list.child(group(a, k, layer, guild, None, manage));
    }
    for section in &layout.sections {
        list = list.child(section_el(
            a,
            k,
            layer,
            guild,
            section.id,
            &section.name,
            manage,
        ));
    }
    list
}

/// The rows of one container (`None` = the top level, with no header).
fn group(
    a: &App,
    k: &Kit,
    layer: &mut Vec<El>,
    guild: u64,
    section: Option<u64>,
    manage: bool,
) -> El {
    let s = &a.state;
    let ids: Vec<u64> = match s.layout(guild) {
        Some(l) => layout::channels_in(l, section).to_vec(),
        None => s.ordered_channels(guild).iter().map(|c| c.id).collect(),
    };
    let mut c = col().key("channels-top").w_full().gap(2.0);
    for id in ids {
        if let Some(row) = channel_row(a, k, layer, guild, id, manage) {
            c = c.child(row);
        }
    }
    c
}

fn section_el(
    a: &App,
    k: &Kit,
    layer: &mut Vec<El>,
    guild: u64,
    id: u64,
    name: &str,
    manage: bool,
) -> El {
    let s = &a.state;
    let open = s.section_open(guild, id);
    let mut sec = SidebarSection::new(name, open)
        .on_toggle(move |a: &mut App| a.act(|s| s.toggle_section(guild, id)));
    if manage {
        sec = sec
            .add_label("Create channel here")
            .on_add(move |a: &mut App| a.act(|s| s.open_create_channel_in(guild, id)))
            .more(more_button(a, k, layer, MenuTarget::Section(id), guild))
            .decorate_header(move |h| {
                h.on_secondary_press(move |a: &mut App, p: PointerPress, _tx: &mut Tx| {
                    a.state.ui.menu = MenuState::Section {
                        id,
                        at: Some((p.x as i32, p.y as i32)),
                    };
                })
                .on_drag(move |a: &mut App, d: &mut Drag, tx: &mut Tx| {
                    drag(a, guild, Item::Section(id), d, tx)
                })
            });
    }
    if open {
        let ids = s
            .layout(guild)
            .map(|l| layout::channels_in(l, Some(id)).to_vec())
            .unwrap_or_default();
        for cid in ids {
            if let Some(row) = channel_row(a, k, layer, guild, cid, manage) {
                sec = sec.child(row);
            }
        }
    }
    let mut el = sec.build(k).key(format!("section-{id}"));
    let (shift, lifted) = section_shift(s, guild, id);
    if shift != 0.0 {
        el = el.offset(0.0, shift);
    }
    if lifted {
        el = el.raise().offset_motion(Anim::INSTANT);
    }
    // The header's own menu opens over the header, wherever it was asked for.
    section_menu(a, k, layer, guild, id, el)
}

fn channel_row(
    a: &App,
    k: &Kit,
    layer: &mut Vec<El>,
    guild: u64,
    id: u64,
    manage: bool,
) -> Option<El> {
    let s = &a.state;
    let c = s.channel(guild, id)?;
    let t = theme_of(k);
    let u = s.unread_of(id);
    let glyph = catalog::by_name(c.icon.as_str()).unwrap_or(&thehideout_design::icons::HASH);
    let mut row = ChannelRow::new(c.name.clone())
        .highlight_group(format!("channels-{guild}"))
        .icon(glyph)
        .tint(palette_color(c.color.as_key(), t))
        .selected(s.current_channel() == Some(id))
        .unread(u.count > 0)
        .mentions(u.mentions)
        .on_press(move |a: &mut App| a.act(|s| s.select_channel(id)));
    // An emoji icon replaces the glyph; a custom one that is gone leaves the plain `#`.
    if let Some(art) = super::emoji::channel_art(a, k, c, 16.0) {
        row = row.emoji(art);
    }
    if manage {
        row = row.trailing(more_button(a, k, layer, MenuTarget::Channel(id), guild));
    }
    let mut el = row.build(k);
    if manage {
        el = el
            .on_secondary_press(move |a: &mut App, p: PointerPress, _tx: &mut Tx| {
                a.state.ui.menu = MenuState::Channel {
                    id,
                    at: Some((p.x as i32, p.y as i32)),
                };
            })
            .on_drag(move |a: &mut App, d: &mut Drag, tx: &mut Tx| {
                drag(a, guild, Item::Channel(id), d, tx)
            });
    }
    let (shift, lifted) = channel_shift(s, guild, id);
    if shift != 0.0 {
        el = el.offset(0.0, shift);
    }
    if lifted {
        // Follows the pointer exactly, on an opaque wash, over the rows it passes.
        el = k.elevate(
            el.raise().offset_motion(Anim::INSTANT).fill(
                thehideout_design::components::util::over(t.pressed.color(), t.bg.color()),
            ),
            &k.elevation.raised,
        );
    }
    Some(if manage {
        channel_menu(a, k, layer, guild, id, el)
    } else {
        el
    })
}

fn drag(a: &mut App, guild: u64, item: Item, d: &mut Drag, tx: &mut Tx) {
    match d.phase {
        Phase::Down => {}
        Phase::Move => {
            if a.state.ui.drag.is_some() || d.dy.abs() >= SLOP {
                // The rows it passes spring aside; it follows the pointer exactly.
                tx.animated();
                a.state.drag_item(guild, item, d.dy);
            }
        }
        Phase::Up => {
            let cancelled = d.cancelled();
            tx.animated();
            a.act(|s| s.end_drag(cancelled));
        }
    }
}

/// How far a channel row sits from its place during a drag, and whether it is the one held.
fn channel_shift(s: &AppState, guild: u64, id: u64) -> (f32, bool) {
    let Some(d) =
        s.ui.drag
            .filter(|d| d.guild == guild && matches!(d.item, Item::Channel(_)))
    else {
        return (0.0, false);
    };
    if d.item == Item::Channel(id) {
        return (d.dy, true);
    }
    let rows = s.sidebar_items(guild);
    let (Some(from), Some(to), Some(at)) = (
        rows.iter().position(|&i| i == d.item),
        s.drop_slot(),
        rows.iter().position(|&i| i == Item::Channel(id)),
    ) else {
        return (0.0, false);
    };
    let shift = if from < at && at <= to {
        -s.ui.look.spec.metrics().row_pitch()
    } else if to <= at && at < from {
        s.ui.look.spec.metrics().row_pitch()
    } else {
        0.0
    };
    (shift, false)
}

/// The same for a whole section group while a section is dragged.
fn section_shift(s: &AppState, guild: u64, id: u64) -> (f32, bool) {
    let Some(d) =
        s.ui.drag
            .filter(|d| d.guild == guild && matches!(d.item, Item::Section(_)))
    else {
        return (0.0, false);
    };
    if d.item == Item::Section(id) {
        return (d.dy, true);
    }
    let Some((from, to, height)) = s.section_drag() else {
        return (0.0, false);
    };
    let Some(at) = s
        .layout(guild)
        .and_then(|l| l.sections.iter().position(|x| x.id == id))
    else {
        return (0.0, false);
    };
    let shift = if from < at && at <= to {
        -height
    } else if to <= at && at < from {
        height
    } else {
        0.0
    };
    (shift, false)
}

// ---- menus -----------------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum MenuTarget {
    Channel(u64),
    Section(u64),
}

/// The "..." button; it opens the same menu a right-click does, anchored under itself.
fn more_button(a: &App, k: &Kit, layer: &mut Vec<El>, target: MenuTarget, guild: u64) -> El {
    let (key, label) = match target {
        MenuTarget::Channel(id) => (format!("channel-more-{id}"), "Channel menu"),
        MenuTarget::Section(id) => (format!("section-more-{id}"), "Section menu"),
    };
    let open = match (a.state.ui.menu, target) {
        (
            MenuState::Channel { id, at: None } | MenuState::MoveTo { id, at: None },
            MenuTarget::Channel(t),
        ) => id == t,
        (MenuState::Section { id, at: None }, MenuTarget::Section(t)) => id == t,
        _ => false,
    };
    let button = IconButton::new(&DOTS_THREE, label)
        .size(Size::Sm)
        .selected(open)
        .on_press(move |a: &mut App| {
            a.state.ui.menu = match (open, target) {
                (true, _) => MenuState::None,
                (false, MenuTarget::Channel(id)) => MenuState::Channel { id, at: None },
                (false, MenuTarget::Section(id)) => MenuState::Section { id, at: None },
            }
        })
        .build(k)
        .key(key.clone());
    let menu = match target {
        MenuTarget::Channel(id) => channel_items(a, Menu::new(format!("menu-{key}")), guild, id),
        MenuTarget::Section(id) => section_items(a, Menu::new(format!("menu-{key}")), guild, id),
    };
    menu.trigger(button)
        .open(open)
        .on_close(|a: &mut App| a.state.ui.menu = MenuState::None)
        .min_width(180.0)
        .build(k, layer)
        .role("none")
}

fn point(at: Option<(i32, i32)>) -> Option<PointerPress> {
    at.map(|(x, y)| PointerPress {
        x: x as f32,
        y: y as f32,
    })
}

/// Wraps a channel row so a right-click opens the menu where it landed.
fn channel_menu(a: &App, k: &Kit, layer: &mut Vec<El>, guild: u64, id: u64, row: El) -> El {
    let at = match a.state.ui.menu {
        MenuState::Channel { id: m, at } | MenuState::MoveTo { id: m, at } if m == id => at,
        _ => None,
    };
    let menu = channel_items(
        a,
        context_menu(format!("menu-channel-{id}"), row, point(at)),
        guild,
        id,
    );
    menu.on_close(|a: &mut App| a.state.ui.menu = MenuState::None)
        .min_width(180.0)
        .build(k, layer)
        .role("none")
}

fn section_menu(a: &App, k: &Kit, layer: &mut Vec<El>, guild: u64, id: u64, group: El) -> El {
    if !a.state.can_manage_channels(guild) {
        return group;
    }
    let at = match a.state.ui.menu {
        MenuState::Section { id: m, at } if m == id => at,
        _ => None,
    };
    let menu = section_items(
        a,
        context_menu(format!("menu-section-{id}"), group, point(at)),
        guild,
        id,
    );
    menu.on_close(|a: &mut App| a.state.ui.menu = MenuState::None)
        .min_width(180.0)
        .build(k, layer)
        .role("none")
}

/// Move up, Move down, Move to section, Edit channel. (Deleting a channel is not in the contract.)
fn channel_items(a: &App, menu: Menu, guild: u64, id: u64) -> Menu {
    let s = &a.state;
    let layout = s.layout(guild);
    let can = |delta: i32| layout.is_some_and(|l| layout::can_step_channel(l, id, delta));
    let here = layout.and_then(|l| layout::section_of(l, id));
    let mut targets: Vec<MenuEntry> = Vec::new();
    if let Some(l) = layout {
        let mut dests: Vec<(Option<u64>, String)> = vec![(None, "Top level".to_owned())];
        dests.extend(l.sections.iter().map(|x| (Some(x.id), x.name.clone())));
        for (dest, name) in dests.into_iter().filter(|(d, _)| Some(*d) != here) {
            targets.push(MenuEntry::item(MenuItem::new(name).on_select(
                move |a: &mut App| {
                    a.act(|s| {
                        s.ui.menu = MenuState::None;
                        s.move_channel_to_section(guild, id, dest);
                    })
                },
            )));
        }
    }
    let sub_open = matches!(s.ui.menu, MenuState::MoveTo { id: m, .. } if m == id);
    let at = match s.ui.menu {
        MenuState::Channel { id: m, at } | MenuState::MoveTo { id: m, at } if m == id => at,
        _ => None,
    };
    menu.item(
        MenuItem::new("Move up")
            .icon(&ARROW_UP)
            .disabled(!can(-1))
            .on_select(move |a: &mut App| {
                a.act(|s| {
                    s.ui.menu = MenuState::None;
                    s.step_channel_by(guild, id, -1)
                })
            }),
    )
    .item(
        MenuItem::new("Move down")
            .icon(&ARROW_DOWN)
            .disabled(!can(1))
            .on_select(move |a: &mut App| {
                a.act(|s| {
                    s.ui.menu = MenuState::None;
                    s.step_channel_by(guild, id, 1)
                })
            }),
    )
    .item(
        MenuItem::new("Move to section")
            .icon(&thehideout_design::icons::FOLDER_SIMPLE)
            .disabled(targets.is_empty())
            .submenu(targets, sub_open)
            .on_select(move |a: &mut App| a.state.ui.menu = MenuState::MoveTo { id, at }),
    )
    .separator()
    .item(
        MenuItem::new("Edit channel\u{2026}")
            .icon(&PENCIL)
            .on_select(move |a: &mut App| {
                a.act(|s| s.open_dialog(DialogKind::ChannelSettings { guild, channel: id }))
            }),
    )
}

/// Rename, Move up, Move down, Delete section, Create channel here.
fn section_items(a: &App, menu: Menu, guild: u64, id: u64) -> Menu {
    let l = a.state.layout(guild);
    let at = l.and_then(|l| l.sections.iter().position(|x| x.id == id));
    let count = l.map_or(0, |l| l.sections.len());
    menu.item(
        MenuItem::new("Rename section\u{2026}")
            .icon(&PENCIL)
            .on_select(move |a: &mut App| {
                a.act(|s| s.open_dialog(DialogKind::RenameSection { guild, section: id }))
            }),
    )
    .item(
        MenuItem::new("Move up")
            .icon(&ARROW_UP)
            .disabled(at.is_none_or(|i| i == 0))
            .on_select(move |a: &mut App| {
                a.act(|s| {
                    s.ui.menu = MenuState::None;
                    s.step_section_by(guild, id, -1)
                })
            }),
    )
    .item(
        MenuItem::new("Move down")
            .icon(&ARROW_DOWN)
            .disabled(at.is_none_or(|i| i + 1 >= count))
            .on_select(move |a: &mut App| {
                a.act(|s| {
                    s.ui.menu = MenuState::None;
                    s.step_section_by(guild, id, 1)
                })
            }),
    )
    .item(
        MenuItem::new("Create channel here")
            .icon(&PLUS)
            .on_select(move |a: &mut App| a.act(|s| s.open_create_channel_in(guild, id))),
    )
    .separator()
    .item(
        MenuItem::new("Delete section\u{2026}")
            .icon(&TRASH)
            .danger(true)
            .on_select(move |a: &mut App| {
                a.act(|s| s.open_dialog(DialogKind::DeleteSection { guild, section: id }))
            }),
    )
}
