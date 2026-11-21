//! The guild rail: Home on top, a 4pt gap (no rule), one badge per server (unread dot, mention count) and
//! the "+" menu directly under the last server. Home and every server share the rail bar for
//! the selected state; nothing in here travels.

use rune::{col, el, El, ScrollSpec};
use rune_kit::{Kit, Menu, MenuItem};
use thehideout_design::components::{GuildBadge, RailTile};
use thehideout_design::icons::{HOUSE, LINK, PLUS};
use thehideout_design::space::RING;

use crate::app::App;
use crate::state::{DialogKind, Menu as MenuState, Place};

/// The rail's width: the tile plus 12 either side.
pub fn width(k: &Kit) -> f32 {
    thehideout_design::metrics_of(k).rail_tile() + 24.0
}
/// Space between tiles: the mention pill overhangs a face by 2pt, so 12 leaves 10 clear.
const GAP: f32 = 12.0;

pub fn build(a: &App, k: &Kit, layer: &mut Vec<El>) -> El {
    let s = &a.state;
    let face = thehideout_design::metrics_of(k).rail_tile();
    let mut guilds = col().key("guilds").w_full().gap(GAP).items_start().pb(4.0);
    for g in &s.data.guilds {
        let u = s.guild_unread(g.id);
        let id = g.id;
        let badge = GuildBadge::new(g.id.to_string(), g.name.clone())
            .size(face as u32)
            .image(s.icon_pixels(g.id))
            .selected(s.ui.place == Place::Guild(id))
            .unread(u.count > 0)
            .mentions(u.mentions)
            .on_press(move |a: &mut App| a.act(|s| s.go_guild(id)))
            .build(k)
            .key(format!("guild-{id}"));
        guilds = guilds.child(badge);
    }
    let plus = RailTile::new(&PLUS, "Add a server")
        .size(face as u32)
        .outline(true)
        .on_press(|a: &mut App| {
            a.state.ui.menu = if a.state.ui.menu == MenuState::Add {
                MenuState::None
            } else {
                MenuState::Add
            }
        })
        .build(k)
        .key("rail-add");
    let plus = Menu::new("rail-menu")
        .trigger(plus)
        .item(
            MenuItem::new("Create server")
                .icon(&PLUS)
                .on_select(|a: &mut App| a.act(|s| s.open_dialog(DialogKind::CreateGuild))),
        )
        .item(
            MenuItem::new("Join server")
                .icon(&LINK)
                .on_select(|a: &mut App| a.act(|s| s.open_dialog(DialogKind::JoinGuild))),
        )
        .open(s.ui.menu == MenuState::Add)
        .placement(rune_kit::Placement::RightStart)
        .on_close(|a: &mut App| a.state.ui.menu = MenuState::None)
        .build(k, layer)
        // The menu's anchor box is layout only: the tile inside is what assistive technology finds.
        .role("none");
    guilds = guilds.child(plus);

    // Same 4pt inset as the tiles' rail; the divider is a face wide and centred on the faces.
    col()
        .key("rail")
        .w(width(k))
        .h_full()
        .pt(4.0)
        .pl(4.0)
        .gap(GAP)
        .items_start()
        .child(home(a, k))
        .child(el().key("rail-divider").w(face).h(4.0))
        .child(
            // The scroll view clips, so it carries `RING` of room round the tiles for their focus
            // rings; the negative margin takes it back so the tiles stay where they were.
            col()
                .key("guilds-scroll")
                .grow(1.0)
                .min_h(0.0)
                .w(width(k) - 4.0)
                .ml(-RING)
                .mt(-RING)
                .pt(RING)
                .pl(RING)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(guilds),
        )
}

/// Home: the same tile as a server, a house on a neutral face.
fn home(a: &App, k: &Kit) -> El {
    let u = a.state.home_unread();
    let tile = RailTile::new(&HOUSE, "Home, direct messages")
        .size(thehideout_design::metrics_of(k).rail_tile() as u32)
        .selected(a.state.ui.place == Place::Home)
        .unread(u.count > 0)
        .mentions(u.mentions)
        .on_press(|a: &mut App| a.act(|s| s.go_home()))
        .build(k);
    tile.key("home")
}
