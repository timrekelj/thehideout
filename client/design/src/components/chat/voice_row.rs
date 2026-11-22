//! VoiceParticipantRow: one person in a voice channel, listed under the channel's row in the
//! sidebar. A `row_h()` row indented so its 20pt avatar starts where the channel's name does
//! (`LEAD` = 8pt padding + the `GLYPH_W` glyph column + 8pt gap), the name in the member's role
//! colour, and trailing state glyphs: a server mute (danger ink: somebody else did it), a
//! self mute and a deafen (quiet ink), a video-camera when their camera is on and a small danger "LIVE"
//! pill while they share their screen. Speaking is a 2pt `accent` ring round the avatar, drawn
//! outside it like every focus ring (1pt clear), so the row's own edges never clip it.
//!
//! The row is a button (the app opens the person's menu from it). Its state reads aloud as the
//! description ("muted", "deafened", "muted by a moderator", "speaking", "muted for you").

use rune::{icon, row, text, El};
use rune_kit::{Kit, State};

use super::nav_row::GLYPH_W;
use crate::components::util::{over, Acts};
use crate::components::{Avatar, Badge, BadgeVariant};
use crate::icons::{MICROPHONE_SLASH, SPEAKER_SLASH, VIDEO_CAMERA};
use crate::theme_of;
use crate::tokens::{icon as size, radius, type_};

/// Where the avatar starts: under the channel name (8pt padding, the glyph column, 8pt gap).
pub const LEAD: f32 = 8.0 + GLYPH_W + 8.0;

pub struct VoiceParticipantRow {
    id: String,
    name: String,
    speaking: bool,
    self_mute: bool,
    self_deaf: bool,
    server_mute: bool,
    local_mute: bool,
    camera: bool,
    live: bool,
    state: State,
    backdrop: Option<rune::Color>,
    image: Option<rune::Pixels>,
    name_color: Option<rune::Color>,
    acts: Acts,
    watch: Acts,
}

impl VoiceParticipantRow {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> VoiceParticipantRow {
        VoiceParticipantRow {
            id: id.into(),
            name: name.into(),
            speaking: false,
            self_mute: false,
            self_deaf: false,
            server_mute: false,
            local_mute: false,
            camera: false,
            live: false,
            state: State::Rest,
            backdrop: None,
            image: None,
            name_color: None,
            acts: Acts::default(),
            watch: Acts::default(),
        }
    }
    /// The accent ring round the avatar while they talk.
    pub fn speaking(mut self, on: bool) -> Self {
        self.speaking = on;
        self
    }
    /// They muted themselves (quiet microphone-slash).
    pub fn self_mute(mut self, on: bool) -> Self {
        self.self_mute = on;
        self
    }
    /// They deafened themselves (quiet speaker-slash, next to the mute they imply).
    pub fn self_deaf(mut self, on: bool) -> Self {
        self.self_deaf = on;
        self
    }
    /// A moderator muted them (microphone-slash in the danger ink; it wins over a self mute).
    pub fn server_mute(mut self, on: bool) -> Self {
        self.server_mute = on;
        self
    }
    /// We muted them for ourselves: the name reads fainter.
    pub fn local_mute(mut self, on: bool) -> Self {
        self.local_mute = on;
        self
    }
    /// Their camera is on (a quiet video-camera glyph after the name).
    pub fn camera(mut self, on: bool) -> Self {
        self.camera = on;
        self
    }
    /// They are sharing their screen: a small "LIVE" pill (danger wash) after the name.
    pub fn live(mut self, on: bool) -> Self {
        self.live = on;
        self
    }
    /// A press on the LIVE pill (the pointer's way to watch the share; the keyboard's is the
    /// row's menu, so the pill is not a control of its own).
    pub fn on_watch<S: 'static>(mut self, f: impl Fn(&mut S) + 'static) -> Self {
        self.watch.press(f);
        self
    }
    pub fn image(mut self, image: Option<rune::Pixels>) -> Self {
        self.image = image;
        self
    }
    /// The name's colour (their highest coloured role's text tone, see `palette_text`).
    pub fn name_color(mut self, c: Option<rune::Color>) -> Self {
        self.name_color = c;
        self
    }
    /// The surface the row sits on, for the avatar's cut-outs. Defaults to `bg`.
    pub fn backdrop(mut self, c: rune::Color) -> Self {
        self.backdrop = Some(c);
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
        let ink = if self.local_mute {
            t.text_faint.color()
        } else {
            self.name_color.unwrap_or_else(|| t.text_muted.color())
        };
        let mut avatar = Avatar::new(self.id, self.name.clone())
            .size(20)
            .image(self.image)
            .backdrop(self.backdrop.unwrap_or(t.bg.color()))
            .build(k)
            .key("avatar");
        if self.speaking {
            avatar = avatar.outline(
                crate::space::RING_WIDTH,
                crate::space::RING_OFFSET,
                t.accent.color(),
            );
        }
        let mut said: Vec<&str> = Vec::new();
        if self.speaking {
            said.push("speaking");
        }
        if self.server_mute {
            said.push("muted by a moderator");
        } else if self.self_mute && !self.self_deaf {
            said.push("muted");
        }
        if self.self_deaf {
            said.push("deafened");
        }
        if self.local_mute {
            said.push("muted for you");
        }
        if self.camera {
            said.push("camera on");
        }
        if self.live {
            said.push("sharing their screen");
        }
        let mut e = row()
            .role("button")
            .label(self.name.clone())
            .w_full()
            .h(m.row_h())
            .pl(LEAD)
            .pr(8.0)
            .gap(8.0)
            .center_y()
            .radius(radius::MD)
            .focusable(true)
            .focus_ring(
                crate::space::RING_WIDTH,
                crate::space::RING_INSET,
                k.c.border_focus,
            );
        if !said.is_empty() {
            e = e.description(said.join(", "));
        }
        e = if self.state == State::Hover {
            e.fill(over(t.hover.color(), rune::Color::TRANSPARENT))
        } else {
            e.hover(t.hover.color()).press(t.pressed.color())
        };
        e = e.child(row().w(20.0).center().child(avatar)).child(
            text(self.name)
                .key("label")
                .font(type_::at(k, type_::CONTROL))
                .color(ink)
                .max_lines(1)
                .grow(1.0)
                .shrink()
                .a11y_hidden(),
        );
        let glyph = m.icon(size::XS);
        if self.live {
            let pill = Badge::new("LIVE")
                .variant(BadgeVariant::Danger)
                .build(k)
                .key("live")
                .a11y_hidden();
            e = e.child(self.watch.apply(pill, true));
        }
        if self.camera {
            e = e.child(
                icon(&VIDEO_CAMERA, glyph)
                    .key("camera")
                    .color(t.text_faint.color())
                    .a11y_hidden(),
            );
        }
        if self.server_mute {
            e = e.child(
                icon(&MICROPHONE_SLASH, glyph)
                    .key("server-mute")
                    .color(t.danger.color())
                    .a11y_hidden(),
            );
        } else if self.self_mute {
            e = e.child(
                icon(&MICROPHONE_SLASH, glyph)
                    .key("self-mute")
                    .color(t.text_faint.color())
                    .a11y_hidden(),
            );
        }
        if self.self_deaf {
            e = e.child(
                icon(&SPEAKER_SLASH, glyph)
                    .key("self-deaf")
                    .color(t.text_faint.color())
                    .a11y_hidden(),
            );
        }
        self.acts.apply(e, true)
    }
}
