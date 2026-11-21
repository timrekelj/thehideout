//! Button: four variants, two sizes, five states, optional leading icon.
//!
//! | Size | Height | Pad X | Radius | Label |
//! |---|---|---|---|---|
//! | Sm | 24 | 8 | `radius-sm` | label 12 |
//! | Md | 32 | 12 | `radius-md` | control 13 |

use rune::{El, Icon, icon, row, text};
use rune_kit::{Kit, State};

use super::util::{Acts, Surface, paint};
use crate::{metrics_of, theme_of};
use crate::tokens::icon as size;
use crate::tokens::{radius, type_};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Variant {
    /// The one main action in a view: accent fill.
    #[default]
    Primary,
    /// Everything else: raised surface with a border.
    Secondary,
    /// Quiet, in toolbars and rows: no fill until hovered.
    Ghost,
    /// Destructive: solid danger fill.
    Danger,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Size {
    Sm,
    #[default]
    Md,
}

impl Size {
    pub(super) fn height(self, m: crate::Metrics) -> f32 {
        match self {
            Size::Sm => 24.0,
            Size::Md => m.control_h(),
        }
    }
    pub(super) fn radius(self) -> f32 {
        match self {
            Size::Sm => radius::SM,
            Size::Md => radius::MD,
        }
    }
}

/// What a variant paints: surface, border and ink.
pub(super) struct Skin {
    pub surface: Surface,
    pub border: Option<rune::Color>,
    pub ink: rune::Color,
}

pub(super) fn skin(k: &Kit, v: Variant) -> Skin {
    let t = theme_of(k);
    let c = |x: crate::tokens::color::Tone| x.color();
    let clear = rune::Color::TRANSPARENT;
    match v {
        Variant::Primary => Skin {
            surface: Surface {
                fill: c(t.accent),
                hover: c(t.accent_hover),
                pressed: c(t.accent_pressed),
            },
            border: None,
            ink: c(t.accent_fg),
        },
        Variant::Secondary => Skin {
            surface: Surface {
                fill: c(t.surface_raised),
                hover: c(t.hover),
                pressed: c(t.pressed),
            },
            border: Some(c(t.control_border)),
            ink: c(t.text),
        },
        Variant::Ghost => Skin {
            surface: Surface {
                fill: clear,
                hover: c(t.hover),
                pressed: c(t.pressed),
            },
            border: None,
            ink: c(t.text),
        },
        Variant::Danger => Skin {
            surface: Surface {
                fill: c(t.danger_solid),
                hover: c(t.accent_hover),
                pressed: c(t.accent_pressed),
            },
            border: None,
            ink: c(t.on_danger),
        },
    }
}

pub struct Button {
    label: String,
    variant: Variant,
    size: Size,
    icon: Option<&'static Icon>,
    state: State,
    acts: Acts,
}

impl Button {
    pub fn new(label: impl Into<String>) -> Button {
        Button {
            label: label.into(),
            variant: Variant::Primary,
            size: Size::Md,
            icon: None,
            state: State::Rest,
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
    /// A leading icon, drawn in the label's ink.
    pub fn icon(mut self, i: &'static Icon) -> Self {
        self.icon = Some(i);
        self
    }
    /// Forces a visual state (gallery and tests). At rest the runtime drives hover and press.
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
        let s = skin(k, self.variant);
        let m = metrics_of(k);
        let (px, font, icon_px) = match self.size {
            Size::Sm => (8.0, m.font(type_::LABEL), m.icon(size::XS)),
            Size::Md => (12.0, m.font(type_::CONTROL_STRONG), m.icon(size::SM)),
        };
        let mut e = row()
            .role("button")
            .label(self.label.clone())
            .h(self.size.height(m))
            .px(px)
            .gap(6.0)
            .center()
            .radius(self.size.radius());
        if let Some(b) = s.border {
            e = e.border(1.0, b);
        }
        e = paint(k, e, s.surface, self.state, (crate::space::RING_WIDTH, crate::space::RING_OFFSET));
        if let Some(i) = self.icon {
            e = e.child(icon(i, icon_px).key("icon").color(s.ink).a11y_hidden());
        }
        e = e.child(
            text(self.label)
                .key("label")
                .font(font)
                .color(s.ink)
                .a11y_hidden(),
        );
        self.acts.apply(e, self.state != State::Disabled)
    }
}
