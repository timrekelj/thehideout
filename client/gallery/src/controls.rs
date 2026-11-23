//! The `controls` page: every basic control, variants x states.

use rune::{El, col, row, text};
use rune_kit::{Kit, State};
use thehideout_design::components::tooltip::tooltip;
use thehideout_design::components::{
    Avatar, Badge, BadgeVariant, Button, Checkbox, IconButton, InviteCodeInput, Kbd, MentionPill, Presence, Segmented, Size,
    TextInput, UnreadDot, Variant, switch,
};
use thehideout_design::icons::{GEAR, PLUS, TRASH};
use thehideout_design::{radius, theme_of, type_};

use crate::G;
use crate::ui::*;

const STATES: [&str; 5] = ["Default", "Hover", "Pressed", "Focus", "Disabled"];
const FORCED: [State; 5] = [
    State::Rest,
    State::Hover,
    State::Pressed,
    State::Focus,
    State::Disabled,
];
const VARIANTS: [(&str, Variant); 4] = [
    ("Primary", Variant::Primary),
    ("Secondary", Variant::Secondary),
    ("Ghost", Variant::Ghost),
    ("Danger", Variant::Danger),
];

pub fn page(s: &G, k: &Kit, layer: &mut Vec<El>) -> El {
    let left = col()
        .gap(20.0)
        .w(680.0)
        .child(section(
            k,
            "Button, medium (32)",
            "Primary is the one main action per view. radius-md.",
            buttons(k, Size::Md),
        ))
        .child(section(
            k,
            "Button, small (24)",
            "radius-sm. For toolbars and dense rows.",
            buttons(k, Size::Sm),
        ))
        .child(section(
            k,
            "Button with icon",
            "Icon takes the label's ink.",
            icon_buttons_with_label(k),
        ))
        .child(section(
            k,
            "IconButton",
            "Always has an accessible label. Ghost by default.",
            icon_buttons(k),
        ))
        .child(section(
            k,
            "Avatar",
            "Rounded squares from the radius scale; colour is a hash of the user id.",
            avatars(k),
        ));
    let right = col()
        .gap(20.0)
        .w(436.0)
        .child(section(k, "TextInput", "From rune-kit, themed through our Kit. 1px control_border ring at rest; hover washes it, focus swaps the ring for the focus colour.", inputs(s, k)))
        .child(section(k, "InviteCodeInput", "XXXX-XXXX, one box per character over a single text field: type, Backspace and paste (a pasted invite link works) fill the boxes. Crockford letters only; the next box takes the focus ring.", invite_codes(s, k)))
        .child(section(k, "Segmented", "No border: a bg-filled container, the selected segment a filled raised tile. 32pt.", segmented(k)))
        .child(section(k, "Switch", "24x16, 4pt corners. Accent when on.", switches(k)))
        .child(section(k, "Checkbox", "16x16, radius-sm. Accent fill when on.", checkboxes(k)))
        .child(section(k, "Badge", "18pt chips; washes for neutral/accent, status tokens for the rest.", badges(k)))
        .child(section(k, "Unread, mentions, Kbd, Tooltip", "Unread is quiet, a mention is loud. Red is for errors only.", misc(s, k, layer)));
    col()
        .gap(24.0)
        .child(page_title(
            k,
            "Controls",
            "Basic controls, variants by state. Hover, pressed and focus are forced for review.",
        ))
        .child(row().gap(20.0).child(left).child(right))
}

fn buttons(k: &Kit, size: Size) -> El {
    let names: Vec<&str> = VARIANTS.iter().map(|v| v.0).collect();
    grid(k, 76.0, 96.0, &names, &STATES, |r, c| {
        Button::new("Button")
            .variant(VARIANTS[r].1)
            .size(size)
            .state(FORCED[c])
            .build(k)
    })
}

fn icon_buttons_with_label(k: &Kit) -> El {
    let mut r = row().gap(12.0).flex_wrap().center_y();
    for (name, v) in VARIANTS {
        r = r.child(Button::new(name).variant(v).icon(&PLUS).build(k));
    }
    let mut r2 = row().gap(12.0).flex_wrap().center_y();
    for (name, v) in VARIANTS {
        r2 = r2.child(
            Button::new(name)
                .variant(v)
                .size(Size::Sm)
                .icon(&PLUS)
                .build(k),
        );
    }
    col().gap(12.0).child(r).child(r2)
}

fn icon_buttons(k: &Kit) -> El {
    let names: Vec<&str> = VARIANTS.iter().map(|v| v.0).collect();
    let glyph = |r: usize| match r {
        0 => &PLUS,
        1 => &GEAR,
        _ => &TRASH,
    };
    let mut g = grid(k, 76.0, 96.0, &names, &STATES, |r, c| {
        IconButton::new(glyph(r), VARIANTS[r].0)
            .variant(VARIANTS[r].1)
            .state(FORCED[c])
            .build(k)
    });
    let mut small = row()
        .gap(8.0)
        .center_y()
        .child(row().w(76.0).child(label(k, "Small (24)")));
    for (_, v) in VARIANTS {
        small = small.child(
            row().w(96.0).child(
                IconButton::new(&GEAR, "Settings")
                    .variant(v)
                    .size(Size::Sm)
                    .build(k),
            ),
        );
    }
    g = g.child(small);
    g
}

fn avatars(k: &Kit) -> El {
    let t = theme_of(k);
    let sizes = [20u32, 24, 32, 40];
    let presence = [
        ("None", Presence::None),
        ("Online", Presence::Online),
        ("Idle", Presence::Idle),
        ("Offline", Presence::Offline),
    ];
    let names: Vec<String> = sizes.iter().map(|s| format!("{s}")).collect();
    let rows: Vec<&str> = names.iter().map(String::as_str).collect();
    let cols: Vec<&str> = presence.iter().map(|p| p.0).collect();
    let grid_ = grid(k, 76.0, 96.0, &rows, &cols, |r, c| {
        Avatar::new("user-ana", "Ana Kovac")
            .size(sizes[r])
            .presence(presence[c].1)
            .backdrop(t.surface.color())
            .build(k)
    });
    let people = [
        "Ana Kovac",
        "Bruno",
        "Cvetka Z",
        "Dario M",
        "Eva",
        "Filip",
        "Gaja H",
        "Hugo",
    ];
    let mut r = row().gap(10.0).flex_wrap();
    for (i, n) in people.iter().enumerate() {
        r = r.child(Avatar::new(format!("user-{i}-{n}"), *n).size(40).build(k));
    }
    col()
        .gap(16.0)
        .child(grid_)
        .child(muted(
            k,
            "Deterministic fill per user id (eight warm tones):",
        ))
        .child(r)
}

fn inputs(s: &G, k: &Kit) -> El {
    let field = |i: usize, caption: &str, st: State, invalid: bool, ph: &str| {
        col().gap(6.0).child(muted(k, caption.to_owned())).child(
            TextInput::new(&s.edits[i])
                .placeholder(ph.to_owned())
                .state(st)
                .invalid(invalid)
                .label(caption.to_owned())
                .on_edit(move |g: &mut G| &mut g.edits[i])
                .build(k),
        )
    };
    col()
        .gap(14.0)
        .child(field(
            0,
            "Default (placeholder)",
            State::Rest,
            false,
            "Message #general",
        ))
        .child(field(1, "Filled", State::Rest, false, ""))
        .child(field(2, "Hover", State::Hover, false, "Search people"))
        .child(field(3, "Focused", State::Focus, false, "Type a message"))
        .child(field(4, "Invalid", State::Rest, true, "Invite code"))
        .child(field(
            5,
            "Disabled",
            State::Disabled,
            false,
            "Locked channel",
        ))
}

fn invite_codes(s: &G, k: &Kit) -> El {
    let field = |i: usize, caption: &str, invalid: bool, disabled: bool| {
        col().gap(6.0).child(muted(k, caption.to_owned())).child(
            InviteCodeInput::new(&s.codes[i])
                .key(format!("invite-code-{i}"))
                .label(caption.to_owned())
                .invalid(invalid)
                .disabled(disabled)
                .on_edit(move |g: &mut G| &mut g.codes[i])
                .build(k),
        )
    };
    col()
        .gap(14.0)
        .child(field(0, "Empty", false, false))
        .child(field(1, "Partial", false, false))
        .child(field(2, "Full", false, false))
        .child(field(3, "Error", true, false))
}

fn segmented(k: &Kit) -> El {
    col()
        .gap(12.0)
        .w_full()
        .child(Segmented::new(&["Log in", "Sign up"], 0).label("Log in or sign up").build(k))
        .child(Segmented::new(&["Day", "Week", "Month"], 1).label("Range").build(k))
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(IconButton::new(&GEAR, "Settings").selected(true).build(k))
                .child(label(k, "IconButton selected: quiet fill, accent icon")),
        )
}

fn switches(k: &Kit) -> El {
    let states = ["Default", "Hover", "Focus", "Disabled"];
    let forced = [State::Rest, State::Hover, State::Focus, State::Disabled];
    grid(k, 56.0, 74.0, &["Off", "On"], &states, |r, c| {
        switch(r == 1)
            .label("Switch")
            .hide_label()
            .state(forced[c])
            .build(k)
    })
}

fn checkboxes(k: &Kit) -> El {
    let states = ["Default", "Hover", "Focus", "Disabled"];
    let forced = [State::Rest, State::Hover, State::Focus, State::Disabled];
    grid(k, 56.0, 74.0, &["Off", "On", "Mixed"], &states, |r, c| {
        Checkbox::new(r == 1)
            .mixed(r == 2)
            .label("Checkbox")
            .hide_label()
            .state(forced[c])
            .build(k)
    })
}

fn badges(k: &Kit) -> El {
    let mut r = row().gap(8.0).flex_wrap().center_y();
    for (n, v) in [
        ("Neutral", BadgeVariant::Neutral),
        ("Accent", BadgeVariant::Accent),
        ("Success", BadgeVariant::Success),
        ("Warning", BadgeVariant::Warning),
        ("Danger", BadgeVariant::Danger),
        ("Info", BadgeVariant::Info),
    ] {
        r = r.child(Badge::new(n).variant(v).build(k));
    }
    r
}

fn misc(s: &G, k: &Kit, layer: &mut Vec<El>) -> El {
    let t = theme_of(k);
    let channel = |name: &str, bold: bool, tail: El| {
        let font = if bold {
            type_::BODY_STRONG
        } else {
            type_::BODY
        };
        let ink = if bold { t.text } else { t.text_muted };
        row()
            .w_full()
            .h(28.0)
            .px(8.0)
            .gap(8.0)
            .center_y()
            .radius(radius::MD)
            .child(text(format!("# {name}")).font(font).color(ink.color()))
            .child(row().grow(1.0))
            .child(tail)
    };
    let rows = col()
        .gap(2.0)
        .child(channel("general", false, row().h(1.0)))
        .child(channel("random", true, UnreadDot::build(k)))
        .child(channel("dev", true, MentionPill::new(3).build(k)))
        .child(channel("plans", true, MentionPill::new(42).build(k)))
        .child(channel("memes", true, MentionPill::new(128).build(k)));
    let kbds = row()
        .gap(14.0)
        .center_y()
        .child(Kbd::new(&["\u{2318}", "K"]).build(k))
        .child(Kbd::new(&["Ctrl", "Shift", "P"]).build(k))
        .child(Kbd::new(&["Esc"]).build(k))
        .child(Kbd::new(&["\u{21B5}"]).build(k));
    let trigger = IconButton::new(&GEAR, "Settings")
        .variant(Variant::Secondary)
        .build(k);
    let tip = tooltip(
        k,
        "settings-tip",
        trigger,
        "User settings",
        &["\u{2318}", ","],
        s.tooltip_open,
        layer,
    );
    col().gap(16.0).child(rows).child(kbds).child(
        row()
            .h(76.0)
            .pt(32.0)
            .pl(88.0)
            .gap(16.0)
            .center_y()
            .child(tip)
            .child(
                muted(
                    k,
                    "Tooltip: standard appearance, overlay surface, popover shadow.",
                )
                .wrap()
                .shrink(),
            ),
    )
}
