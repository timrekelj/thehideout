//! Maps thehideout's tokens onto `rune_kit::Kit`, so every rune-kit component (TextInput,
//! Switch, Tooltip, ...) inherits the look. Starts from `Kit::from_palette_focus` for the
//! fields we don't care about, then sets every colour, radius, type and motion token
//! explicitly from `tokens`.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Mutex;

use rune::{Color, Shadow};
use rune_kit::{Kit, PaletteFocus, PaletteSeed, StateColors};

use crate::tokens::color::Tone;
use crate::tokens::metrics::Metrics;
use crate::tokens::elevation::{Level, shadows};
use crate::tokens::spec::ThemeSpec;
use crate::tokens::theme::{Theme, theme_for};
use crate::tokens::{motion, radius, type_};

/// The kit for the default look (`ThemeSpec::default()`) drawn `dark`.
pub fn kit(dark: bool) -> &'static Kit {
    kit_for(&ThemeSpec::default(), dark)
}

/// Every kit this module has built, by identity (see [`identity`]), with the theme and metrics it
/// was built from.
static REGISTRY: Mutex<Option<HashMap<usize, (&'static Theme, Metrics)>>> = Mutex::new(None);

thread_local! {
    /// The last `theme_of` answer on this thread: a frame asks for the same kit thousands of times.
    static LAST: Cell<Option<(usize, &'static Theme, Metrics)>> = const { Cell::new(None) };
}

/// The kit for `spec` drawn `dark`, built once per distinct look and metrics (at most
/// `ThemeSpec::KITS`); the theme under it is shared by every metrics of one look.
pub fn kit_for(spec: &ThemeSpec, dark: bool) -> &'static Kit {
    static CACHE: Mutex<Vec<Option<&'static Kit>>> = Mutex::new(Vec::new());
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if cache.is_empty() {
        cache.resize(ThemeSpec::KITS, None);
    }
    cache[spec.kit_index(dark)].get_or_insert_with(|| {
        let theme = theme_for(spec, dark);
        let metrics = spec.metrics();
        let kit: &'static Kit = Box::leak(Box::new(build(theme, metrics)));
        let mut reg = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
        reg.get_or_insert_with(HashMap::new).insert(identity(kit), (theme, metrics));
        kit
    })
}

/// The theme a kit was built from. Components receive a `&Kit` and call this for the semantic
/// tokens, so one kit carries the whole look through the tree with no global to set. A kit not
/// made by `kit_for` falls back to the default theme for its appearance.
pub fn theme_of(k: &Kit) -> &'static Theme {
    entry_of(k).0
}

/// The density and text size a kit was built for (the default for a kit not made by `kit_for`).
/// Components read their sizes from it: `type_::at(k, style)`, `metrics_of(k).row_h()`, ...
pub fn metrics_of(k: &Kit) -> Metrics {
    entry_of(k).1
}

/// What names a kit: the address of its `glyph.dash`, which `build` gives every kit its own copy
/// of (the same icon, never drawn by our components). It lives inside the `Kit`, so a clone of it
/// (a `'static` closure that needs one) still finds its theme and metrics, which the kit's own
/// address could not do.
fn identity(k: &Kit) -> usize {
    k.glyph.dash as *const rune::Icon as usize
}

fn entry_of(k: &Kit) -> (&'static Theme, Metrics) {
    let addr = identity(k);
    if let Some((a, t, m)) = LAST.with(Cell::get)
        && a == addr
    {
        return (t, m);
    }
    let found = REGISTRY
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|r| r.get(&addr).copied());
    let (t, m) = found.unwrap_or_else(|| (crate::tokens::theme::theme(k.dark), Metrics::default()));
    LAST.with(|l| l.set(Some((addr, t, m))));
    (t, m)
}

/// A `spread`-wide stroke just inside the box, so a field's edge sits where a button's border does
/// and (unlike an outer shadow) never grows past it into a clipping parent.
fn ring(color: Color, spread: f32) -> Shadow {
    Shadow {
        spread,
        color,
        inset: true,
        ..Shadow::default()
    }
}

fn state(bg: Tone, fg: Tone, border: Tone) -> StateColors {
    StateColors {
        bg: bg.color(),
        fg: fg.color(),
        border: border.color(),
        icon: fg.color(),
    }
}

fn build(t: &Theme, m: Metrics) -> Kit {
    let dark = t.dark;
    let c = |x: Tone| x.color();
    let mut k = Kit::from_palette_focus(
        PaletteSeed {
            bg: c(t.bg),
            fg: c(t.text),
            accent: c(t.accent),
        },
        PaletteFocus::Custom(c(t.focus)),
    );
    // Inset fills one, two and three steps from the page (the fourth is `border_strong`).
    let layer = |i: usize| c(t.layers[i]);
    let p = &mut k.c;
    p.page = c(t.bg);
    p.bg_base = c(t.bg);
    p.bg_deep = c(t.bg);
    (p.layer_01, p.layer_02, p.layer_03, p.layer_04) =
        (layer(0), layer(1), layer(2), c(t.border_strong));
    p.bg_button_neutral = c(t.surface_raised);
    p.bg_accent = c(t.accent);
    p.bg_contrast = c(t.text);
    p.bg_inverse = c(t.text);
    p.text_base = c(t.text);
    p.text_muted = c(t.text_muted);
    p.text_faint = c(t.text_faint);
    p.text_inverse = c(t.bg);
    p.text_contrast = c(t.bg);
    p.text_accent = c(t.accent_text);
    p.icon_base = c(t.text);
    p.icon_muted = c(t.text_muted);
    p.icon_faint = c(t.text_faint);
    p.icon_inverse = c(t.bg);
    p.icon_contrast = c(t.bg);
    p.icon_accent = c(t.accent_text);
    p.border_muted = c(t.border_subtle);
    p.border_base = c(t.border);
    p.border_strong = c(t.border_strong);
    p.border_focus = c(t.focus);
    p.hover = c(t.hover);
    p.pressed = c(t.pressed);
    p.contrast_hover = c(t.accent_hover);
    p.contrast_pressed = c(t.accent_pressed);
    p.scrim = c(t.scrim);
    // Flat knobs: no depth sheen.
    p.depth_top = Color::TRANSPARENT;
    p.depth_bottom = Color::TRANSPARENT;
    p.knob = c(t.knob);
    p.knob_on = c(t.knob_on);
    p.success = state(t.success_bg, t.success, t.success_border);
    p.warning = state(t.warning_bg, t.warning, t.warning_border);
    p.danger = state(t.danger_bg, t.danger, t.danger_border);
    p.info = state(t.info_bg, t.info, t.info_border);
    p.surface_base = c(t.surface);
    p.surface_weak = c(t.bg);
    p.surface_hover = c(t.hover);
    p.surface_raised = c(t.surface_raised);
    p.surface_raised_base = c(t.surface_raised);
    p.surface_raised_hover = layer(2);
    p.surface_float = c(t.surface_overlay);
    p.surface_disabled = layer(1);
    // Rune-kit's tooltip edge and menu separators: a floating edge, the quietest stroke.
    p.border_weak = c(t.border_subtle);
    p.border_hover = c(t.border_strong);
    p.border_active = c(t.focus);
    p.border_disabled = c(t.border_subtle);
    p.text_strong = c(t.text);
    p.text_weak = c(t.text_muted);
    // Ink on a floating surface (tooltips): the theme's text, not an inverse.
    p.text_invert_strong = c(t.text);
    p.icon_weaker = c(t.text_faint);
    p.std_border = c(t.border);
    p.std_icon = c(t.text_muted);
    p.std_icon_invert = c(t.bg);
    p.std_icon_strong = c(t.text);
    p.std_text = c(t.text);

    // Flat: borders separate, shadows only float. Inputs take a 1px `control_border` ring at rest, inside the box (focus and invalid draw theirs there too).
    k.ring = None;
    let flat = || Vec::<Shadow>::new();
    let pop = shadows(Level::Popover, dark);
    let e = &mut k.elevation;
    (e.raised, e.button_neutral, e.button_contrast, e.elements) = (flat(), flat(), flat(), flat());
    (e.switch_off, e.switch_on, e.knob) = (flat(), flat(), flat());
    (e.floating, e.md) = (pop.clone(), pop);
    e.overlay = shadows(Level::Modal, dark);
    *e = std::mem::take(e).with_input(vec![ring(c(t.control_border), 1.0)]);

    // A private copy of an unused glyph names this kit (see `identity`).
    let dash = k.glyph.dash;
    k.glyph.dash = Box::leak(Box::new(rune::Icon { name: dash.name, view: dash.view, body: dash.body }));
    k.ty = type_::kit(m);
    k.radius = radius::kit();
    k.motion = motion::kit();
    k.blend_over = Some(c(t.bg));
    k.invalid_text = true;
    k.dark = dark;
    k
}
