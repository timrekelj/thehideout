//! Checkbox: a 16x16 box with `radius-sm` corners. Off: strong border on the page. On:
//! accent fill with accent ink. Mixed: accent fill with a dash.

use rune::{El, el, icon, row, text};
use rune_kit::{Kit, State};

use super::util::{Acts, Surface, over};
use crate::theme_of;
use crate::tokens::{radius, type_};

pub struct Checkbox {
    checked: bool,
    mixed: bool,
    label: Option<String>,
    hide_label: bool,
    state: State,
    acts: Acts,
}

impl Checkbox {
    pub fn new(checked: bool) -> Checkbox {
        Checkbox {
            checked,
            mixed: false,
            label: None,
            hide_label: false,
            state: State::Rest,
            acts: Acts::default(),
        }
    }
    pub fn label(mut self, l: impl Into<String>) -> Self {
        self.label = Some(l.into());
        self
    }
    /// Keeps the label for assistive technology only.
    pub fn hide_label(mut self) -> Self {
        self.hide_label = true;
        self
    }
    /// The mixed (indeterminate) look.
    pub fn mixed(mut self, m: bool) -> Self {
        self.mixed = m;
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
    pub fn on_toggle<S: 'static>(mut self, f: impl Fn(&mut S) + 'static) -> Self {
        self.acts.press(f);
        self
    }

    pub fn build(self, k: &Kit) -> El {
        let t = theme_of(k);
        let on = self.checked || self.mixed;
        let clear = rune::Color::TRANSPARENT;
        let (fill, border) = if on {
            (t.accent.color(), t.accent.color())
        } else {
            (clear, t.border_strong.color())
        };
        // Hover lightens or darkens whatever is under it; on the accent fill use the accent's own overlay.
        let overlay = if on {
            t.accent_hover.color()
        } else {
            t.hover.color()
        };
        let surface = Surface {
            fill,
            hover: overlay,
            pressed: if on {
                t.accent_pressed.color()
            } else {
                t.pressed.color()
            },
        };
        let shown = match self.state {
            State::Hover => over(surface.hover, surface.fill),
            State::Pressed => over(surface.pressed, surface.fill),
            _ => fill,
        };
        let glyph = if self.mixed {
            k.glyph.dash
        } else {
            k.glyph.check
        };
        let mut control = el()
            .key("control")
            .size(16.0, 16.0)
            .center()
            .radius(radius::SM)
            .fill(shown)
            .border(1.0, border)
            .child(
                icon(glyph, 10.0)
                    .key("mark")
                    .color(t.accent_fg.color())
                    .opacity(if on { 1.0 } else { 0.0 }),
            );
        if self.state == State::Rest {
            control = k.interactive_with(control, surface.hover, surface.pressed);
        }
        let disabled = self.state == State::Disabled;
        control = control.focus_ring(crate::space::RING_WIDTH, crate::space::RING_OFFSET, k.c.border_focus);
        if self.state == State::Focus {
            control = control.outline(crate::space::RING_WIDTH, crate::space::RING_OFFSET, k.c.border_focus);
        }
        let mut root = row()
            .role("checkbox")
            .gap(8.0)
            .center_y()
            .child(control.a11y_hidden());
        root = if self.mixed {
            root.checked_mixed()
        } else {
            root.checked(self.checked)
        };
        if let Some(l) = self.label {
            root = root.label(l.clone());
            if !self.hide_label {
                root = root.child(
                    text(l)
                        .key("label")
                        .font(type_::at(k, type_::CONTROL))
                        .color(t.text.color())
                        .a11y_hidden(),
                );
            }
        }
        if disabled {
            root = root.layer_opacity(0.5);
        }
        self.acts
            .apply(root.focusable(!disabled).disabled(disabled), !disabled)
    }
}
