//! Sidebar rows: [`NavRow`] (icon or 20pt avatar + label, for Home, Friends, DMs) and [`ChannelRow`]
//! (`#` + name). Both show selected, unread and muted, with an optional trailing unread dot or
//! mention pill.
//!
//! Selected: the highlight is its own element (`matched`), so when the selection moves the
//! highlight travels to the new row on `motion::REFLOW` instead of fading in place. Rows that
//! share a list share a `.highlight_group(..)`. Chrome icons are the regular weight and selection is the
//! highlight, stronger text and an `accent_text` icon (a channel's own tint wins).

use rune::{el, icon, row, text, El, Icon, Transition};
use rune_kit::{Kit, State};

use crate::components::util::{over, Acts};
use crate::components::{Avatar, MentionPill, UnreadDot};
use crate::icons::HASH;
use crate::theme_of;
use crate::tokens::color::Tone;
use crate::tokens::{icon as size, motion, radius, type_};

/// The width of the leading glyph column of a sidebar row (an icon, an emoji or a 20pt avatar, centred in it);
/// a section header's chevron sits in the same column.
pub const GLYPH_W: f32 = 20.0;

struct Row {
    label: String,
    glyph: &'static Icon,
    /// A user (id, name) drawn as a 20pt avatar in place of the icon.
    avatar: Option<String>,
    selected: bool,
    unread: bool,
    muted: bool,
    mentions: u32,
    state: State,
    group: String,
    acts: Acts,
    /// A palette colour for the icon (channels); the selected highlight takes a faint wash of it.
    tint: Option<Tone>,
    /// A control shown on hover at the row's right edge (a "more" button).
    trailing: Option<El>,
    /// Replaces the label as the element key, so a rename keeps the row (and its slide) alive.
    id: Option<String>,
    /// An emoji drawn in place of the icon (a channel whose icon is an emoji).
    art: Option<El>,
    /// A voice channel we are connected to: `accent_text` ink and a "connected" value, with no
    /// highlight (a voice channel is never the selected row; that is the open text channel).
    connected: bool,
}

impl Row {
    fn new(label: String, glyph: &'static Icon) -> Row {
        Row {
            label,
            glyph,
            avatar: None,
            selected: false,
            unread: false,
            muted: false,
            mentions: 0,
            state: State::Rest,
            group: "rows".into(),
            acts: Acts::default(),
            tint: None,
            trailing: None,
            id: None,
            art: None,
            connected: false,
        }
    }

    fn build(mut self, k: &Kit) -> El {
        let t = theme_of(k);
        let strong = self.selected || self.connected || (self.unread && !self.muted);
        let ink = if self.selected {
            t.text
        } else if self.connected {
            t.accent_text
        } else if self.muted {
            t.text_faint
        } else if self.unread {
            t.text
        } else {
            t.text_muted
        };
        let icon_ink = if let Some(tone) = self.tint {
            tone.alpha(if self.muted { 0.55 } else { 1.0 })
        } else if self.selected || self.connected {
            t.accent_text
        } else if self.muted {
            t.text_faint
        } else if self.unread {
            t.text
        } else {
            t.text_muted
        };
        let glyph = self.glyph;
        let font = if strong {
            type_::at(k, type_::CONTROL_STRONG)
        } else {
            type_::at(k, type_::CONTROL)
        };

        let mut e = row()
            .key(format!(
                "{}-row-{}",
                self.group,
                self.id.as_deref().unwrap_or(&self.label)
            ))
            .role("button")
            .label(self.label.clone())
            .w_full()
            .h(crate::metrics_of(k).row_h())
            .px(8.0)
            .gap(8.0)
            .center_y()
            .radius(radius::MD)
            .focusable(true)
            .focus_ring(
                crate::space::RING_WIDTH,
                crate::space::RING_INSET,
                k.c.border_focus,
            )
            .token_motion(motion::CONTROL);
        // A button carries no selected state on every platform, so the selection is its value
        // ("general, selected, button") and unread or mentions its description.
        if self.selected {
            e = e.a11y_value("selected");
        } else if self.connected {
            e = e.a11y_value("connected");
        }
        match (self.mentions, self.unread && !self.muted) {
            (0, false) => {}
            (0, true) => e = e.description("unread"),
            (1, _) => e = e.description("1 mention"),
            (n, _) => e = e.description(format!("{n} mentions")),
        }
        if self.selected {
            // The shared element: unkeyed rows of other lists never match it.
            let hl = el()
                .key(format!("{}-highlight", self.group))
                .matched(format!("{}-highlight", self.group))
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .radius(radius::MD)
                .fill(match self.tint {
                    Some(tone) => over(tone.alpha(0.12).color(), t.pressed.color()),
                    None => t.pressed.color(),
                })
                .token_motion(motion::REFLOW)
                .transition(Transition::opacity())
                .a11y_hidden();
            e = e.child(hl);
        } else if self.state == State::Hover {
            e = e.fill(over(t.hover.color(), rune::Color::TRANSPARENT));
        } else if self.state == State::Rest {
            e = e.hover(t.hover.color()).press(t.pressed.color());
        }
        // Icons, emoji and avatars share one 20pt column, so labels start at one x whatever leads.
        let lead = match (&self.avatar, self.art.take()) {
            (_, Some(art)) => row()
                .key("icon")
                .size(size::SM, size::SM)
                .center()
                .child(art),
            (Some(id), None) => Avatar::new(id.clone(), self.label.clone())
                .size(20)
                .backdrop(t.bg.color())
                .build(k)
                .key("avatar"),
            (None, None) => icon(glyph, crate::metrics_of(k).icon(size::SM))
                .key("icon")
                .color(icon_ink.color())
                .a11y_hidden(),
        };
        e = e.child(row().w(GLYPH_W).center().child(lead));
        e = e.child(
            text(self.label)
                .key("label")
                .font(font)
                .color(ink.color())
                .grow(1.0)
                .shrink()
                .max_lines(1)
                .a11y_hidden(),
        );
        if self.mentions > 0 {
            e = e.child(MentionPill::new(self.mentions).build(k));
        } else if self.unread && !self.muted && !self.selected {
            e = e.child(UnreadDot::build(k));
        }
        if let Some(more) = self.trailing {
            e = e.hover_group().child(
                row()
                    .key("more")
                    .pin(Some(0.0), Some(4.0), Some(0.0), Some(0.0))
                    .justify_end()
                    .center_y()
                    .group_opacity(0.0, 1.0)
                    .child(more),
            );
        }
        self.acts.apply(e, true)
    }
}

macro_rules! row_api {
    ($t:ident) => {
        impl $t {
            pub fn selected(mut self, on: bool) -> Self {
                self.0.selected = on;
                self
            }
            pub fn unread(mut self, on: bool) -> Self {
                self.0.unread = on;
                self
            }
            pub fn muted(mut self, on: bool) -> Self {
                self.0.muted = on;
                self
            }
            pub fn mentions(mut self, count: u32) -> Self {
                self.0.mentions = count;
                self
            }
            /// Forces hover (gallery and tests). At rest the runtime drives it.
            pub fn state(mut self, s: State) -> Self {
                self.0.state = s;
                self
            }
            /// Rows of one list share a group, so only they exchange the selection highlight.
            /// Defaults to `"rows"`; give each list in a window its own.
            pub fn highlight_group(mut self, group: impl Into<String>) -> Self {
                self.0.group = group.into();
                self
            }
            pub fn on_press<S: 'static>(mut self, f: impl Fn(&mut S) + 'static) -> Self {
                self.0.acts.press(f);
                self
            }
            pub fn build(self, k: &Kit) -> El {
                self.0.build(k)
            }
        }
    };
}

/// A nav row: an icon and a label.
pub struct NavRow(Row);

impl NavRow {
    pub fn new(label: impl Into<String>, glyph: &'static Icon) -> NavRow {
        NavRow(Row::new(label.into(), glyph))
    }
}
impl NavRow {
    /// A row for a person: their 20pt avatar (colour from `id`, initials from `label`) instead of
    /// an icon, so a conversation list reads as people.
    pub fn person(label: impl Into<String>, id: impl Into<String>) -> NavRow {
        let mut r = Row::new(label.into(), &HASH);
        r.avatar = Some(id.into());
        NavRow(r)
    }
}
row_api!(NavRow);

/// A channel row: `#` and a name.
pub struct ChannelRow(Row);

impl ChannelRow {
    pub fn new(name: impl Into<String>) -> ChannelRow {
        ChannelRow(Row::new(name.into(), &HASH))
    }
}
row_api!(ChannelRow);

impl ChannelRow {
    /// A Phosphor icon in place of `#` (the channel's own).
    pub fn icon(mut self, glyph: &'static Icon) -> Self {
        self.0.glyph = glyph;
        self
    }
    /// An emoji (a built element of about the icon's size) in place of the icon: the channel's
    /// Unicode or custom emoji icon.
    pub fn emoji(mut self, art: El) -> Self {
        self.0.art = Some(art);
        self
    }
    /// Tints the icon with a palette colour and washes the selected highlight with it.
    pub fn tint(mut self, tone: Option<Tone>) -> Self {
        self.0.tint = tone;
        self
    }
    /// A built control that fades in at the row's right edge on hover (an `IconButton`).
    pub fn trailing(mut self, control: El) -> Self {
        self.0.trailing = Some(control);
        self
    }
    /// A voice channel we are connected to: accent ink, no highlight (see `Row::connected`).
    pub fn connected(mut self, on: bool) -> Self {
        self.0.connected = on;
        self
    }
    /// A stable id for the element key (the channel id), so renames and moves keep the row.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.0.id = Some(id.into());
        self
    }
}
