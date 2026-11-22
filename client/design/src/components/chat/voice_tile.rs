//! VoiceTile: one participant's picture in the voice stage. A rounded button that fills the box
//! its parent gives it (`grow`, `w_full`, a fixed size: the caller decides), showing either live
//! video, letterboxed (`SurfaceFit::Fit`) on a dark back, or the person's avatar centred on a
//! raised wash. The name sits bottom-left on a dim chip, with a mute glyph beside it; speaking is a
//! 2pt `accent` outline drawn just inside the tile (a tile touches its neighbours and the panel
//! edge, so a ring outside would clip), and keyboard focus is the usual inset ring.
//!
//! The tile is a button (it focuses the picture); its accessible name is the `label` ("bruno's
//! camera", "ana's screen") and its state ("speaking", "muted") the description.

use rune::{col, el, icon, row, text, El, Pixels, SurfaceFit};
use rune_kit::{Kit, State};

use super::attachments::{MEDIA_BACK, MEDIA_INK};
use crate::components::util::{frame_over, over, Acts};
use crate::components::Avatar;
use crate::icons::{MICROPHONE_SLASH, MONITOR_ARROW_UP};
use crate::theme_of;
use crate::tokens::color::Tone;
use crate::tokens::{icon as size, radius, type_};

/// The dim behind a name over a picture.
const CHIP: Tone = Tone::new(0.12, 0.01, 55.0).alpha(0.66);

/// What a tile shows.
pub enum TileMedia {
    /// Video: the live image (`None` until the first frame is in, a quiet "Connecting" shows).
    Video(Option<Pixels>),
    /// No video: the person's avatar.
    Avatar {
        id: String,
        name: String,
        image: Option<Pixels>,
    },
}

/// How big the tile is, which sets the avatar and the chip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileSize {
    /// The focused tile or a single one: 96pt avatar.
    Large,
    /// A grid cell: 64pt avatar.
    Medium,
    /// A tile in the strip under the focused one: 40pt avatar, a tighter chip.
    Small,
}

impl TileSize {
    fn avatar(self) -> u32 {
        match self {
            TileSize::Large => 96,
            TileSize::Medium => 64,
            TileSize::Small => 40,
        }
    }
}

pub struct VoiceTile {
    label: String,
    name: String,
    media: TileMedia,
    size: TileSize,
    speaking: bool,
    muted: bool,
    server_mute: bool,
    screen: bool,
    state: State,
    acts: Acts,
}

impl VoiceTile {
    /// `label` is the accessible name ("ana's screen"); `name` is what the chip says.
    pub fn new(label: impl Into<String>, name: impl Into<String>, media: TileMedia) -> VoiceTile {
        VoiceTile {
            label: label.into(),
            name: name.into(),
            media,
            size: TileSize::Medium,
            speaking: false,
            muted: false,
            server_mute: false,
            screen: false,
            state: State::Rest,
            acts: Acts::default(),
        }
    }
    pub fn size(mut self, s: TileSize) -> Self {
        self.size = s;
        self
    }
    /// The accent outline while they talk.
    pub fn speaking(mut self, on: bool) -> Self {
        self.speaking = on;
        self
    }
    /// They muted themselves (a quiet microphone-slash beside the name).
    pub fn muted(mut self, on: bool) -> Self {
        self.muted = on;
        self
    }
    /// A moderator muted them (the danger ink, wins over `muted`).
    pub fn server_mute(mut self, on: bool) -> Self {
        self.server_mute = on;
        self
    }
    /// A screen share: the chip carries the monitor glyph.
    pub fn screen(mut self, on: bool) -> Self {
        self.screen = on;
        self
    }
    /// Forces hover (gallery and tests).
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
        let m = crate::metrics_of(k);
        let ink = MEDIA_INK.color();
        let video = matches!(self.media, TileMedia::Video(_));
        let back = if video {
            MEDIA_BACK.color()
        } else {
            over(t.pressed.color(), t.surface.color())
        };
        // Over video the chip is the media dim and ink; on an avatar tile it follows the theme.
        let (chip_fill, chip_ink) = if video {
            (CHIP.color(), ink)
        } else {
            (t.surface.color(), t.text.color())
        };
        let mut said: Vec<&str> = Vec::new();
        if self.speaking {
            said.push("speaking");
        }
        if self.server_mute {
            said.push("muted by a moderator");
        } else if self.muted {
            said.push("muted");
        }
        let mut tile = el()
            .role("button")
            .label(self.label)
            .min_w(0.0)
            .min_h(0.0)
            .radius(radius::LG)
            .clip_rounded(radius::LG)
            .fill(back)
            .focusable(true)
            .focus_ring(
                crate::space::RING_WIDTH,
                crate::space::RING_INSET,
                k.c.border_focus,
            );
        if !said.is_empty() {
            tile = tile.description(said.join(", "));
        }
        tile = if self.state == State::Hover {
            tile.fill(over(t.hover.color(), back))
        } else {
            tile.hover(t.hover.color()).press(t.pressed.color())
        };
        match self.media {
            TileMedia::Video(p) => {
                // The first frame may be a moment away: a quiet line shows until the picture,
                // which covers the middle of the tile, is drawn over it.
                tile = tile.child(
                    col()
                        .key("waiting")
                        .absolute(0.0, 0.0)
                        .w_full()
                        .h_full()
                        .center()
                        .child(
                            text("Connecting\u{2026}")
                                .font(type_::at(k, type_::CONTROL))
                                .color(Tone::new(0.985, 0.006, 75.0).alpha(0.7).color())
                                .a11y_hidden(),
                        ),
                );
                if let Some(p) = p {
                    tile = tile.child(
                        el().key("video")
                            .absolute(0.0, 0.0)
                            .w_full()
                            .h_full()
                            .pixels_fit(&p, SurfaceFit::Fit)
                            .inert()
                            .a11y_hidden(),
                    );
                }
            }
            TileMedia::Avatar { id, name, image } => {
                tile = tile.child(
                    col()
                        .key("avatar-wrap")
                        .absolute(0.0, 0.0)
                        .w_full()
                        .h_full()
                        .center()
                        .child(
                            Avatar::new(id, name)
                                .size(self.size.avatar())
                                .image(image)
                                .backdrop(t.bg.color())
                                .build(k)
                                .key("avatar")
                                .a11y_hidden(),
                        ),
                );
            }
        }
        // The chip: glyphs, then the name, bottom-left.
        let glyph = m.icon(size::XS);
        let pad = if self.size == TileSize::Small {
            6.0
        } else {
            10.0
        };
        let mut chip = row()
            .key("chip")
            .gap(6.0)
            .center_y()
            .px(8.0)
            .h(m.em(if self.size == TileSize::Small {
                20.0
            } else {
                24.0
            }))
            .radius(radius::SM)
            .fill(chip_fill);
        if self.screen {
            chip = chip.child(icon(&MONITOR_ARROW_UP, glyph).color(chip_ink).a11y_hidden());
        }
        if self.server_mute {
            chip = chip.child(
                icon(&MICROPHONE_SLASH, glyph)
                    .key("server-mute")
                    .color(t.danger.color())
                    .a11y_hidden(),
            );
        } else if self.muted {
            chip = chip.child(
                icon(&MICROPHONE_SLASH, glyph)
                    .key("self-mute")
                    .color(chip_ink)
                    .a11y_hidden(),
            );
        }
        chip = chip.child(
            text(self.name)
                .key("name")
                .font(type_::at(
                    k,
                    if self.size == TileSize::Small {
                        type_::CAPTION
                    } else {
                        type_::CONTROL_STRONG
                    },
                ))
                .color(chip_ink)
                .max_lines(1)
                .shrink()
                .min_w(0.0)
                .a11y_hidden(),
        );
        tile = tile.child(
            col()
                .key("chip-layer")
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .justify_end()
                .padding(pad)
                .child(row().min_w(0.0).child(chip))
                .inert()
                .a11y_hidden(),
        );
        if self.speaking {
            tile = tile.child(
                frame_over(crate::space::RING_WIDTH, t.accent.color(), radius::LG).key("speaking"),
            );
        }
        self.acts.apply(tile, true)
    }
}
