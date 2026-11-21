//! IconButton: a square button with only an icon. Same variants and states as `Button`;
//! the accessible label is required.
//!
//! `selected` (a toggle that is on) is the ghost "selected" look: an `accent_subtle` fill and an
//! `accent_text` icon, no border.
//!
//! Sm is 24x24 with a 14pt icon (`icon::XS`), Md is 32x32 with a 16pt icon (`icon::SM`).

use rune::{El, Icon, icon, row};
use rune_kit::{Kit, State};

use super::button::{Size, Variant, skin};
use super::util::{Acts, paint};
use crate::theme_of;
use crate::tokens::icon as size;

pub struct IconButton {
    glyph: &'static Icon,
    label: String,
    variant: Variant,
    size: Size,
    state: State,
    selected: bool,
    acts: Acts,
}

impl IconButton {
    /// `label` names the button for assistive technology (and a tooltip, if you add one).
    pub fn new(glyph: &'static Icon, label: impl Into<String>) -> IconButton {
        IconButton {
            glyph,
            label: label.into(),
            variant: Variant::Ghost,
            size: Size::Md,
            state: State::Rest,
            selected: false,
            acts: Acts::default(),
        }
    }
    pub fn variant(mut self, v: Variant) -> Self {
        self.variant = v;
        self
    }
    pub fn size(mut self, s: Size) -> Self {
        self.size = s;
        self
    }
    /// A toggle that is on. Only the Ghost variant changes: quiet fill, accent icon.
    pub fn selected(mut self, on: bool) -> Self {
        self.selected = on;
        self
    }
    pub fn state(mut self, s: State) -> Self {
        self.state = s;
        self
    }
    pub fn disabled(mut self, d: bool) -> Self {
        if d {
            self.state = State::Disabled;
        }
        self
    }
    pub fn on_press<S: 'static>(mut self, f: impl Fn(&mut S) + 'static) -> Self {
        self.acts.press(f);
        self
    }

    pub fn build(self, k: &Kit) -> El {
        let mut s = skin(k, self.variant);
        let on = self.selected && self.variant == Variant::Ghost;
        if on {
            s.surface.fill = theme_of(k).accent_subtle.color();
        }
        let m = crate::metrics_of(k);
        let side = self.size.height(m);
        let px = if self.size == Size::Sm {
            m.icon(size::XS)
        } else {
            m.icon(size::SM)
        };
        let ink = if on {
            theme_of(k).accent_text.color()
        } else if self.variant == Variant::Ghost {
            k.c.icon_muted
        } else {
            s.ink
        };
        let mut e = row()
            .role("button")
            .label(self.label)
            .size(side, side)
            .center()
            .radius(self.size.radius());
        if let Some(b) = s.border {
            e = e.border(1.0, b);
        }
        e = paint(k, e, s.surface, self.state, (crate::space::RING_WIDTH, crate::space::RING_OFFSET));
        e = e.child(icon(self.glyph, px).key("icon").color(ink).a11y_hidden());
        self.acts.apply(e, self.state != State::Disabled)
    }
}
