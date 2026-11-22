//! Basic controls. Where rune-kit has the control, we re-export it (it inherits our look from
//! [`crate::kit()`]); where it doesn't fit our language we write our own.
//!
//! | Control | Source |
//! |---|---|
//! | Button, IconButton | own (flat, accent primary) |
//! | TextInput | `rune_kit::TextInput` |
//! | Switch | `rune_kit::Switch` in its `.web()` look (24x16, 4pt corners) |
//! | Checkbox | own (accent fill when on) |
//! | Avatar, Badge, UnreadDot, MentionPill, Kbd | own |
//! | Tooltip | `rune_kit::Tooltip`, standard appearance, via [`tooltip::tooltip`] |
//! | CodeInput, QrCode | own (one-time codes; authenticator setup) |
//! | Slider, LevelMeter | own (a 0..1 control; a live level bar with an optional threshold marker) |

pub mod avatar;
pub mod badge;
pub mod button;
pub mod chart;
pub mod chat;
pub mod checkbox;
pub mod code_input;
pub mod icon_button;
pub mod kbd;
pub mod level_meter;
pub mod preview;
pub mod qr;
pub mod scroll_edges;
pub mod slider;
pub mod tooltip;
pub mod unread;
pub mod util;

pub use avatar::{Avatar, Presence};
pub use badge::{Badge, BadgeVariant};
pub use button::{Button, Size, Variant};
pub use chart::{
    ActivityChart, BarSegment, ChartDay, StackedBar, StatCard, compact, group_digits, nice_max, nice_ticks,
};
pub use checkbox::Checkbox;
pub use code_input::{CodeInput, digits_only};
pub use icon_button::IconButton;
pub use kbd::Kbd;
pub use level_meter::LevelMeter;
pub use preview::ThemePreview;
pub use qr::QrCode;
pub use rune_kit::{State, TextInput};
pub use scroll_edges::{ScrollEdges, scroll_edges};
pub use slider::{Slider, THUMB, slider_fraction};
pub use unread::{MentionPill, UnreadDot};

/// A `rune_kit::Switch` in the look thehideout uses: compact, crisp, accent when on.
pub fn switch(on: bool) -> rune_kit::Switch {
    rune_kit::Switch::new(on).web()
}

pub use chat::*;
