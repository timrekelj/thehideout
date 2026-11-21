//! Small helpers shared by the controls.

use rune::{Color, El, el};
use rune_kit::{Kit, State};

/// A panel's outline drawn over its children. `El::border` takes no layout space and paints under
/// the box's own children, so a filled child at the edge (a highlighted message row, a selected
/// table row) would hide it; add this as the panel's last child instead of `.border(..)`. It is
/// inert and hidden from assistive technology.
pub fn frame_over(width: f32, color: Color, radius: f32) -> El {
    el().absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .radius(radius)
        .border(width, color)
        .inert()
        .a11y_hidden()
}

/// `top` over `bottom` (straight alpha), mixed in sRGB as Rune's windows blend by default.
/// Used to paint forced hover/pressed states, which live states get from the runtime.
pub fn over(top: Color, bottom: Color) -> Color {
    let a = top.a + bottom.a * (1.0 - top.a);
    if a <= 0.0 {
        return Color::TRANSPARENT;
    }
    use rune::theme::{srgb_decode as dec, srgb_encode as enc};
    let mix = |t: f32, b: f32| dec((enc(t) * top.a + enc(b) * bottom.a * (1.0 - top.a)) / a);
    Color {
        r: mix(top.r, bottom.r),
        g: mix(top.g, bottom.g),
        b: mix(top.b, bottom.b),
        a,
    }
}

/// The paint for one control surface: its resting fill and the overlays it takes on
/// hover and press.
#[derive(Clone, Copy)]
pub struct Surface {
    pub fill: Color,
    pub hover: Color,
    pub pressed: Color,
}

/// Fills `e` for `state`: forced hover/pressed paint the composite, rest wires the live
/// overlays, disabled dims the control. Adds a focus ring (live on keyboard focus, drawn
/// when `State::Focus` is forced).
pub fn paint(k: &Kit, e: El, s: Surface, state: State, ring: (f32, f32)) -> El {
    let fill = match state {
        State::Hover => over(s.hover, s.fill),
        State::Pressed => over(s.pressed, s.fill),
        _ => s.fill,
    };
    let interactive = state != State::Disabled;
    let mut e = e
        .fill(fill)
        .focusable(interactive)
        .focus_ring(ring.0, ring.1, k.c.border_focus);
    if state == State::Focus {
        e = e.outline(ring.0, ring.1, k.c.border_focus);
    }
    if state == State::Rest {
        e = k.interactive_with(e, s.hover, s.pressed);
    }
    if state == State::Disabled {
        e.layer_opacity(0.5).disabled(true)
    } else {
        e
    }
}

/// Press handlers collected by a builder and attached to the root unless disabled.
#[derive(Default)]
pub struct Acts(Vec<Box<dyn FnOnce(El) -> El>>);

impl Acts {
    pub fn press<S: 'static>(&mut self, f: impl Fn(&mut S) + 'static) {
        self.0.push(Box::new(move |e| e.on_press(f)));
    }

    pub fn apply(self, mut e: El, enabled: bool) -> El {
        if enabled {
            for f in self.0 {
                e = f(e);
            }
        }
        e
    }
}
