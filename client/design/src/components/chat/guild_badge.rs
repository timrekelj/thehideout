//! GuildBadge: the server icon. A rounded square (the avatar radii: 24 `sm`, 32 `md`, 40 `lg`)
//! with initials on a deterministic warm fill. The state lives in a rail to its left: a 4pt
//! dot when unread, a short bar on hover, a tall accent bar when selected. A mention count
//! sits on its bottom-right corner.

use rune::{el, icon, row, text, El, Icon, TextStyle};
use rune_kit::{Kit, State};

use crate::components::avatar::{avatar_tones, initials, tone_index, AVATAR_INK};
use crate::components::util::{paint, Acts, Surface};
use crate::components::MentionPill;
use crate::theme_of;
use crate::tokens::{icon as size, motion, radius, type_};

/// Width of the left rail that holds the indicator.
const RAIL: f32 = 8.0;

pub struct GuildBadge {
    id: String,
    name: String,
    size: u32,
    selected: bool,
    unread: bool,
    mentions: u32,
    state: State,
    acts: Acts,
    image: Option<rune::Pixels>,
}

impl GuildBadge {
    /// `id` picks the colour, `name` gives the initials. `size` is 24, 32 (default) or 40.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> GuildBadge {
        GuildBadge {
            id: id.into(),
            name: name.into(),
            size: 32,
            selected: false,
            unread: false,
            mentions: 0,
            state: State::Rest,
            acts: Acts::default(),
            image: None,
        }
    }
    /// The server's icon, drawn over the initials (which stay while it loads or if it fails).
    pub fn image(mut self, image: Option<rune::Pixels>) -> Self {
        self.image = image;
        self
    }
    pub fn size(mut self, size: u32) -> Self {
        self.size = size;
        self
    }
    pub fn selected(mut self, on: bool) -> Self {
        self.selected = on;
        self
    }
    pub fn unread(mut self, on: bool) -> Self {
        self.unread = on;
        self
    }
    pub fn mentions(mut self, count: u32) -> Self {
        self.mentions = count;
        self
    }
    /// Forces a visual state (gallery and tests).
    pub fn state(mut self, s: State) -> Self {
        self.state = s;
        self
    }
    pub fn on_press<S: 'static>(mut self, f: impl Fn(&mut S) + 'static) -> Self {
        self.acts.press(f);
        self
    }

    pub fn build(self, k: &Kit) -> El {
        let t = theme_of(k);
        let s = self.size as f32;
        let (r, font_px) = match self.size {
            ..=24 => (radius::SM, 10.5),
            25..=32 => (radius::MD, 12.5),
            _ => (radius::LG, 15.0),
        };
        let font = TextStyle {
            size: font_px,
            weight: 600.0,
            line_height: font_px + 2.0,
            tracking: 0.0,
            ..type_::LABEL
        };
        let label = if self.name.is_empty() {
            initials(&self.id)
        } else {
            initials(&self.name)
        };
        let fill = avatar_tones(t.spec.accent)[tone_index(&self.id)].color();
        let mut face = row().key("face").size(s, s).center().radius(r).child(
            text(label)
                .font(font)
                .color(AVATAR_INK.color())
                .a11y_hidden(),
        );
        if let Some(p) = &self.image {
            face = face.child(crate::components::avatar::picture(p, s, r));
        }
        let surface = Surface {
            fill,
            hover: t.hover.color(),
            pressed: t.pressed.color(),
        };
        let face = paint(k, face, surface, self.state, (crate::space::RING_WIDTH, crate::space::RING_OFFSET));
        let rail = indicator(t, s, self.selected, self.unread, self.state == State::Hover);
        tile(
            k,
            Tile {
                label: self.name,
                side: s,
                face,
                rail,
                selected: self.selected,
                unread: self.unread,
                mentions: self.mentions,
                state: self.state,
                acts: self.acts,
            },
        )
    }
}

/// What [`tile`] puts together.
struct Tile {
    label: String,
    side: f32,
    face: El,
    rail: El,
    selected: bool,
    unread: bool,
    mentions: u32,
    state: State,
    acts: Acts,
}

/// The tile's root is the button: it carries the role, the name, the press and keyboard focus, so
/// the element a caller keys (and a menu marks open or closed) is the control itself. The face
/// only paints (its focus ring draws while the root has focus); the selected and unread states
/// are the button's value and description, as the rail bar and the count are hidden inside it.
fn tile(k: &Kit, t: Tile) -> El {
    let face = t.face.focusable(false);
    let mut root = row()
        .role("button")
        .label(t.label)
        .hover_group()
        .w(RAIL + t.side)
        .h(t.side)
        .focusable(t.state != State::Disabled)
        .child(t.rail)
        .child(face);
    if t.selected {
        root = root.a11y_value("selected");
    }
    let note = match (t.mentions, t.unread) {
        (1, _) => Some("1 mention".to_owned()),
        (n, _) if n > 1 => Some(format!("{n} mentions")),
        (_, true) => Some("unread".to_owned()),
        _ => None,
    };
    if let Some(note) = note {
        root = root.description(note);
    }
    if t.mentions > 0 {
        root = root.child(mention_corner(k, t.mentions));
    }
    t.acts.apply(root, true)
}

/// The rail beside a face of side `s`: a tall accent bar when selected, a 4pt dot when unread,
/// a short bar on hover. Shared by every tile in the rail so they read as one family.
fn indicator(
    t: &crate::tokens::theme::Theme,
    s: f32,
    selected: bool,
    unread: bool,
    forced_hover: bool,
) -> El {
    let (h, ink, shown) = if selected {
        ((s * 0.6).round(), t.accent, 1.0)
    } else if unread {
        (4.0, t.text, 1.0)
    } else {
        (
            (s * 0.375).round(),
            t.text,
            if forced_hover { 1.0 } else { 0.0 },
        )
    };
    let mut bar = el()
        .key("rail-bar")
        .w(4.0)
        .h(h)
        .radius(radius::FULL)
        .fill(ink.color())
        .opacity(shown);
    if shown == 0.0 {
        bar = bar.group_opacity(0.0, 1.0);
    }
    el().w(RAIL).h(s).center_y().child(
        bar.token_motion(motion::CONTROL)
            .opacity_motion(motion::CONTROL),
    )
}

/// The mention count on a tile's bottom-right corner: it overhangs the face by 2pt at most, so
/// with the rail's gap it never reaches the next tile.
fn mention_corner(k: &Kit, count: u32) -> El {
    let t = theme_of(k);
    el().key("mentions")
        .border(2.0, t.bg.color())
        .radius(radius::FULL)
        .transition(crate::components::unread::pop_in())
        .child(MentionPill::new(count).build(k))
        .pin(None, None, Some(-2.0), Some(-2.0))
}

/// A rail tile with an icon on a neutral face: Home and "Add a server". Same size, rail and
/// states as [`GuildBadge`]. Selected is the rail bar plus an `accent_subtle` face with an
/// `accent_text` icon; `outline` makes the face a ghost with a border (the add button).
pub struct RailTile {
    glyph: &'static Icon,
    label: String,
    size: u32,
    selected: bool,
    unread: bool,
    mentions: u32,
    outline: bool,
    state: State,
    acts: Acts,
}

impl RailTile {
    /// `glyph_selected` is the fill twin shown when selected.
    pub fn new(
        glyph: &'static Icon,
        label: impl Into<String>,
    ) -> RailTile {
        RailTile {
            glyph,
            label: label.into(),
            size: 40,
            selected: false,
            unread: false,
            mentions: 0,
            outline: false,
            state: State::Rest,
            acts: Acts::default(),
        }
    }
    pub fn size(mut self, size: u32) -> Self {
        self.size = size;
        self
    }
    pub fn selected(mut self, on: bool) -> Self {
        self.selected = on;
        self
    }
    pub fn unread(mut self, on: bool) -> Self {
        self.unread = on;
        self
    }
    pub fn mentions(mut self, count: u32) -> Self {
        self.mentions = count;
        self
    }
    /// A ghost face with a border instead of the neutral fill.
    pub fn outline(mut self, on: bool) -> Self {
        self.outline = on;
        self
    }
    pub fn state(mut self, s: State) -> Self {
        self.state = s;
        self
    }
    pub fn on_press<S: 'static>(mut self, f: impl Fn(&mut S) + 'static) -> Self {
        self.acts.press(f);
        self
    }

    pub fn build(self, k: &Kit) -> El {
        let t = theme_of(k);
        let s = self.size as f32;
        let (fill, ink) = if self.selected {
            (t.accent_subtle.color(), t.accent_text.color())
        } else if self.outline {
            (rune::Color::TRANSPARENT, k.c.icon_muted)
        } else {
            (t.surface_raised.color(), t.text_muted.color())
        };
        let glyph = self.glyph;
        let mut face = row()
            .key("face")
            .size(s, s)
            .center()
            .radius(radius::LG)
            .child(icon(glyph, size::MD).color(ink).a11y_hidden());
        if self.outline {
            face = face.border(1.0, t.control_border.color());
        }
        let surface = Surface {
            fill,
            hover: t.hover.color(),
            pressed: t.pressed.color(),
        };
        let face = paint(k, face, surface, self.state, (crate::space::RING_WIDTH, crate::space::RING_OFFSET));
        let rail = indicator(t, s, self.selected, self.unread, self.state == State::Hover);
        tile(
            k,
            Tile {
                label: self.label,
                side: s,
                face,
                rail,
                selected: self.selected,
                unread: self.unread,
                mentions: self.mentions,
                state: self.state,
                acts: self.acts,
            },
        )
    }
}
