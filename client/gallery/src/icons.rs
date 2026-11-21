//! The `icons` page: our Phosphor icons (regular for chrome, fill for user-chosen), how a selected icon is shown, the
//! size tokens, and the icons in context (sidebar nav, channel list, composer).

use rune::{El, Icon, col, el, icon, row, text};
use rune_kit::Kit;
use thehideout_design::components::{IconButton, Size, Variant};
use thehideout_design::icons;
use thehideout_design::type_::{CAPTION, CONTROL, CONTROL_STRONG, LABEL};
use thehideout_design::{icon as size, radius, theme_of};

use crate::ui::*;

pub fn page(k: &Kit) -> El {
    col()
        .gap(20.0)
        .child(page_title(
            k,
            "Icons",
            "Phosphor (MIT). The app's own icons are regular (outline); only icons users choose, like channel icons, are fill.",
        ))
        .child(section(
            k,
            "Catalog",
            "Regular weight at 16pt (icon::SM) in text_muted. Add one in tools/gen_icons.py.",
            catalog(k),
        ))
        .child(
            row()
                .gap(20.0)
                .items_start()
                .child(
                    section(
                        k,
                        "Selected state",
                        "The weight never changes: rest is text_muted, selected is accent_text on the row highlight.",
                        fills(k),
                    )
                    .w(560.0),
                )
                .child(section(k, "Sizes", "icon::XS inside 24pt controls, SM the default, MD standalone.", sizes(k)).w(556.0)),
        )
        .child(section(
            k,
            "Channel palette",
            "The 12 palette keys as icon ink (palette_color, theme-aware, 3:1 on bg, surface and the selected row in every look), plus the picker's swatches.",
            palette(k),
        ))
        .child(section(
            k,
            "Full Phosphor catalog",
            "icons::catalog: every Phosphor fill icon with search tags (what channel icons are chosen from). The first 120 of them.",
            full_catalog(k),
        ))
        .child(section(k, "In context", "Real components: nav, channel list, composer, icon buttons.", context(k)))
}

/// Every icon at 16pt in `text_muted`, with its name.
fn catalog(k: &Kit) -> El {
    let t = theme_of(k);
    let mut g = row().gap(4.0).flex_wrap();
    for ic in icons::ALL.iter() {
        g = g.child(
            col()
                .w(130.0)
                .gap(6.0)
                .py(10.0)
                .items_center()
                .child(icon(ic, size::SM).color(t.text_muted.color()))
                .child(
                    text(ic.name.to_owned())
                        .font(CAPTION)
                        .color(t.text_muted.color()),
                ),
        );
    }
    g
}

/// Each icon at rest (text_muted) and selected (accent_text on the highlight).
fn fills(k: &Kit) -> El {
    let t = theme_of(k);
    let glyphs: [&'static Icon; 6] = [
        &icons::HOUSE,
        &icons::BELL,
        &icons::CHAT,
        &icons::USERS,
        &icons::GEAR,
        &icons::LOCK,
    ];
    let mut r = row().gap(8.0).flex_wrap();
    for rest in glyphs {
        r = r.child(
            col()
                .w(76.0)
                .gap(8.0)
                .py(8.0)
                .items_center()
                .child(
                    row()
                        .gap(10.0)
                        .center_y()
                        .child(icon(rest, size::SM).color(t.text_muted.color()))
                        .child(
                            el().size(28.0, 28.0)
                                .center()
                                .radius(radius::MD)
                                .fill(t.pressed.color())
                                .child(icon(rest, size::SM).color(t.accent_text.color())),
                        ),
                )
                .child(
                    text(rest.name.to_owned())
                        .font(CAPTION)
                        .color(t.text_muted.color()),
                ),
        );
    }
    r
}

/// The three icon sizes, on one glyph, with their token names.
fn sizes(k: &Kit) -> El {
    let t = theme_of(k);
    let mut r = row().gap(24.0).items_end();
    for (name, px) in [
        ("XS 14", size::XS),
        ("SM 16", size::SM),
        ("MD 20", size::MD),
    ] {
        r = r.child(
            col()
                .gap(8.0)
                .items_center()
                .child(
                    row()
                        .gap(6.0)
                        .center_y()
                        .child(icon(&icons::GEAR, px).color(t.text.color()))
                        .child(icon(&icons::BELL, px).color(t.text.color()))
                        .child(icon(&icons::CHAT, px).color(t.text.color())),
                )
                .child(
                    text(name.to_owned())
                        .font(CAPTION)
                        .color(t.text_muted.color()),
                ),
        );
    }
    r
}

fn panel(k: &Kit, w: f32) -> El {
    let t = theme_of(k);
    col()
        .w(w)
        .gap(2.0)
        .padding(8.0)
        .radius(radius::LG)
        .fill(t.bg.color())
        .border(1.0, t.border_subtle.color())
}

/// A 28pt row: icon, label, selected = raised fill, strong text, accent icon.
fn nav_row(k: &Kit, glyph: &'static Icon, name: &str, selected: bool) -> El {
    let t = theme_of(k);
    let (ink, icon_ink, face) = if selected {
        (t.text.color(), t.accent_text.color(), CONTROL_STRONG)
    } else {
        (t.text_muted.color(), t.text_muted.color(), CONTROL)
    };
    let mut r = row()
        .w_full()
        .h(28.0)
        .px(8.0)
        .gap(8.0)
        .center_y()
        .radius(radius::MD)
        .child(icon(glyph, size::SM).color(icon_ink))
        .child(text(name.to_owned()).font(face).color(ink));
    if selected {
        r = r.fill(t.surface_raised.color());
    }
    r
}

fn section_label(k: &Kit, s: &str) -> El {
    text(s.to_owned())
        .font(LABEL)
        .color(theme_of(k).text_faint.color())
        .px(8.0)
        .py(4.0)
}

fn context(k: &Kit) -> El {
    let t = theme_of(k);
    let sidebar = panel(k, 232.0)
        .child(nav_row(k, &icons::HOUSE, "Home", true))
        .child(nav_row(k, &icons::BELL, "Activity", false))
        .child(nav_row(k, &icons::CHAT, "Direct messages", false))
        .child(nav_row(k, &icons::USERS, "People", false))
        .child(nav_row(k, &icons::GEAR, "Settings", false));
    // Selected channel: raised row, stronger text, accent icon.
    let channels = panel(k, 232.0)
        .child(section_label(k, "Channels"))
        .child(nav_row(k, &icons::HASH, "general", true))
        .child(nav_row(k, &icons::HASH, "random", false))
        .child(nav_row(k, &icons::HASH, "design-review", false))
        .child(nav_row(k, &icons::LOCK, "leads", false));

    let buttons = row()
        .gap(8.0)
        .center_y()
        .child(IconButton::new(&icons::GEAR, "Settings").build(k))
        .child(
            IconButton::new(&icons::PLUS, "New")
                .variant(Variant::Secondary)
                .build(k),
        )
        .child(
            IconButton::new(&icons::SEND, "Send")
                .variant(Variant::Primary)
                .build(k),
        )
        .child(
            IconButton::new(&icons::TRASH, "Delete")
                .variant(Variant::Danger)
                .build(k),
        )
        .child(
            IconButton::new(&icons::GEAR, "Settings")
                .size(Size::Sm)
                .build(k),
        )
        .child(
            IconButton::new(&icons::PLUS, "New")
                .variant(Variant::Secondary)
                .size(Size::Sm)
                .build(k),
        )
        .child(
            IconButton::new(&icons::SEND, "Send")
                .variant(Variant::Primary)
                .size(Size::Sm)
                .build(k),
        )
        .child(
            IconButton::new(&icons::TRASH, "Delete")
                .variant(Variant::Danger)
                .size(Size::Sm)
                .build(k),
        );

    let composer = row()
        .w_full()
        .h(44.0)
        .padding(6.0)
        .gap(4.0)
        .center_y()
        .radius(radius::LG)
        .fill(t.surface_raised.color())
        .border(1.0, t.control_border.color())
        .child(IconButton::new(&icons::PLUS, "Add").build(k))
        .child(IconButton::new(&icons::PAPERCLIP, "Attach").build(k))
        .child(IconButton::new(&icons::SMILEY, "Emoji").build(k))
        .child(
            row().grow(1.0).padding(4.0).child(
                text("Message #general".to_owned())
                    .font(CONTROL)
                    .color(t.text_faint.color()),
            ),
        )
        .child(
            IconButton::new(&icons::SEND, "Send")
                .variant(Variant::Primary)
                .build(k),
        );

    col()
        .gap(16.0)
        .child(
            row()
                .gap(16.0)
                .items_start()
                .child(sidebar)
                .child(channels)
                .child(col().gap(16.0).grow(1.0).child(buttons).child(el().h(1.0))),
        )
        .child(composer)
}

/// The 12 palette tones as icon ink on the page, each with a sample icon and its key, then the
/// picker's swatches.
fn palette(k: &Kit) -> El {
    let t = theme_of(k);
    let samples = ["hash", "code", "megaphone", "calendar-check", "dice-five", "music-notes", "game-controller", "paint-brush", "books", "smiley", "house", "rocket"];
    let mut r = row().gap(4.0).flex_wrap();
    for (key, name) in thehideout_design::PALETTE_KEYS.iter().zip(samples) {
        let tone = thehideout_design::palette_color(key, t).unwrap_or(t.text_muted);
        let glyph = icons::catalog::by_name(name).unwrap_or(&icons::HASH);
        r = r.child(
            col()
                .w(88.0)
                .gap(6.0)
                .py(10.0)
                .items_center()
                .child(icon(glyph, size::MD).color(tone.color()))
                .child(text((*key).to_owned()).font(CAPTION).color(t.text_muted.color())),
        );
    }
    col()
        .gap(12.0)
        .child(r)
        .child(thehideout_design::components::ColorSwatches::new("teal").build(k))
}

fn full_catalog(k: &Kit) -> El {
    let t = theme_of(k);
    let mut g = row().gap(2.0).flex_wrap();
    for c in icons::catalog::all().take(120) {
        g = g.child(el().size(36.0, 36.0).center().radius(radius::MD).child(icon(&c.icon, size::MD).color(t.text_muted.color())));
    }
    col()
        .gap(8.0)
        .child(text(format!("{} icons", icons::catalog::len())).font(CONTROL).color(t.text.color()))
        .child(g)
}
