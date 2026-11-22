//! Avatar: initials on a deterministic warm colour picked from the user's id, with an optional
//! presence dot. Rounded squares (not circles) so avatars share the radius scale:
//! 20/24 use `radius-sm`, 32 `radius-md`, 40 `radius-lg`.

use rune::{el, icon, text, El};
use rune_kit::Kit;

use crate::theme_of;
use crate::tokens::accent::AccentPreset;
use crate::tokens::color::Tone;
use crate::tokens::{radius, type_};

/// The eight avatar fills of an accent preset, one per hash bucket (`accent.rs` defines them).
/// All have the same lightness (0.48) so white initials clear 4.5:1 on each, and they look the
/// same in dark and light.
pub fn avatar_tones(accent: AccentPreset) -> [Tone; 8] {
    accent.palette().avatars
}

/// What a deleted account is called. Its avatar is neutral: no initials, and a colour that
/// belongs to no one (the id would give it a hue, which would tie it to a name that is gone).
pub const DELETED_USER: &str = "Deleted user";

/// Ink on every avatar fill.
pub const AVATAR_INK: Tone = Tone::new(0.985, 0.006, 75.0);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Presence {
    #[default]
    None,
    Online,
    Idle,
    Offline,
}

/// Which of the eight fills `id` gets: FNV-1a, so it never changes between runs or platforms.
pub fn tone_index(id: &str) -> usize {
    let mut h: u32 = 0x811c_9dc5;
    for b in id.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h as usize % 8
}

/// Up to two initials from `name` (first letters of the first two words).
pub fn initials(name: &str) -> String {
    let mut it = name.split_whitespace().filter_map(|w| w.chars().next());
    let first = it.next();
    let second = it.next();
    first
        .into_iter()
        .chain(second)
        .flat_map(char::to_uppercase)
        .collect()
}

pub struct Avatar {
    id: String,
    name: String,
    size: u32,
    presence: Presence,
    backdrop: Option<rune::Color>,
    image: Option<rune::Pixels>,
}

/// The picture of an avatar or badge over its initials: `Fill` crops it to the square. A picture
/// that is still decoding (or never will) paints nothing, so the initials below show through.
pub fn picture(pixels: &rune::Pixels, side: f32, radius: f32) -> rune::El {
    el().key("picture")
        .absolute(0.0, 0.0)
        .size(side, side)
        .radius(radius)
        .pixels_fit(pixels, rune::SurfaceFit::Fill)
        .a11y_hidden()
}

impl Avatar {
    /// `id` picks the colour; `name` gives the initials. `size` is 20, 24, 32 or 40.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Avatar {
        Avatar {
            id: id.into(),
            name: name.into(),
            size: 32,
            presence: Presence::None,
            backdrop: None,
            image: None,
        }
    }
    /// The user's picture, drawn over the initials (which stay while it loads or if it fails).
    pub fn image(mut self, image: Option<rune::Pixels>) -> Self {
        self.image = image;
        self
    }
    pub fn size(mut self, size: u32) -> Self {
        self.size = size;
        self
    }
    pub fn presence(mut self, p: Presence) -> Self {
        self.presence = p;
        self
    }
    /// The surface the avatar sits on, so the presence dot's cut-out matches. Defaults to `bg`.
    pub fn backdrop(mut self, c: rune::Color) -> Self {
        self.backdrop = Some(c);
        self
    }

    pub fn build(self, k: &Kit) -> El {
        let t = theme_of(k);
        let s = self.size as f32;
        let (r, font_px) = match self.size {
            ..=20 => (radius::SM, 9.0),
            21..=24 => (radius::SM, 10.5),
            25..=32 => (radius::MD, 12.5),
            _ => (radius::LG, 16.0),
        };
        let font = rune::TextStyle {
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
        let deleted = self.name == DELETED_USER;
        let mut e = el()
            .role("img")
            .label(self.name.clone())
            .size(s, s)
            .center()
            .radius(r);
        e = if deleted {
            e.fill(t.border.color()).child(
                icon(&crate::icons::USER_CIRCLE, (s * 0.62).round())
                    .color(t.text_muted.color())
                    .a11y_hidden(),
            )
        } else {
            e.fill(avatar_tones(t.spec.accent)[tone_index(&self.id)].color())
                .child(
                    text(label)
                        .font(font)
                        .color(AVATAR_INK.color())
                        .a11y_hidden(),
                )
        };
        if let Some(p) = &self.image {
            e = e.child(picture(p, s, r));
        }
        if self.presence != Presence::None {
            let ring = 2.0;
            let d = match self.size {
                ..=20 => 8.0,
                21..=24 => 10.0,
                25..=32 => 12.0,
                _ => 14.0,
            };
            let colour = match self.presence {
                Presence::Online => t.success,
                Presence::Idle => t.warning,
                _ => t.text_faint,
            };
            let backdrop = self.backdrop.unwrap_or(k.c.page);
            let mut dot = el()
                .key("presence")
                .size(d, d)
                .radius(radius::FULL)
                .fill(colour.color())
                .border(ring, backdrop)
                .absolute(s - d + ring, s - d + ring);
            let inner = d - 2.0 * ring;
            if self.presence == Presence::Offline && inner >= 6.0 {
                let hole = inner - 4.0;
                dot = dot
                    .center()
                    .child(el().size(hole, hole).radius(radius::FULL).fill(backdrop));
            }
            e = e.child(dot);
        }
        e
    }
}
