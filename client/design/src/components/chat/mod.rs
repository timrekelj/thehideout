//! Chat building blocks: data in, callbacks out, no app state types. Each takes the `Kit`
//! like the basic controls do, and every motion comes from `tokens::motion`. See the README's
//! "Chat components" for the motion table and what never animates.
//!
//! | Component | Source |
//! |---|---|
//! | [`GuildBadge`], [`RailTile`] | `guild_badge` |
//! | [`Segmented`] | `segmented` |
//! | [`NavRow`], [`ChannelRow`], [`SidebarSection`] | `nav_row`, `section` |
//! | [`VoiceParticipantRow`], [`VoiceTile`] | `voice_row`, `voice_tile` |
//! | [`MessageGroup`], [`ChatMessage`] | `message`, `body`, `markdown` |
//! | [`DateDivider`], [`NewMessagesDivider`] | `dividers` |
//! | [`Composer`] | `composer` |
//! | [`EmojiPicker`], [`EmojiSuggest`], [`EmojiSet`] | `emoji_picker`, `emoji` |
//! | [`ChannelHeader`], [`MemberRow`], [`EmptyState`] | `header`, `member_row`, `empty_state` |
//! | [`Dialog`], [`Toast`], [`ConnectionStatus`] | `dialog`, `toast`, `connection` |

// Heights are `Metrics` (density and text size): `metrics_of(k).header_h()` (48 / 40) for every
// panel header, so their titles share a baseline and the scroll-edge hairlines under them sit on
// one y; `.footer_h()` (64 / 52) for the zone under the transcript and the sidebar's user strip,
// for the same reason; `.row_h()` (32 / 28) for a sidebar or member row.

pub mod attachments;
pub mod body;
pub mod composer;
pub mod connection;
pub mod dialog;
pub mod dividers;
pub mod embed;
pub mod emoji;
pub mod emoji_picker;
pub mod empty_state;
pub mod guild_badge;
pub mod header;
pub mod markdown;
pub mod member_row;
pub mod message;
pub mod segmented;
pub mod nav_row;
pub mod pickers;
pub mod section;
pub mod toast;
pub mod voice_row;
pub mod voice_tile;

pub use attachments::{AudioRow, Cell, Chip, ChipState, FileCard, FileKind, GridImage, LightboxView, VideoCard, VideoUi, Wire, file_kind, human_size, image_grid, image_grid_el, lightbox, tray, tray_note};
pub use composer::Composer;
pub use connection::{ConnectionPill, ConnectionStatus};
pub use dialog::{Dialog, dialog_field};
pub use dividers::{DateDivider, NewMessagesDivider};
pub use embed::{EmbedCard, Provider as EmbedProvider};
pub use emoji::{EmojiImage, EmojiSet, Pick, ServerEmoji, Suggestion};
pub use emoji_picker::{EmojiPicker, EmojiPickerState, EmojiSuggest, Scope as EmojiScope};
pub use empty_state::EmptyState;
pub use guild_badge::{GuildBadge, RailTile};
pub use header::ChannelHeader;
pub use member_row::MemberRow;
pub use message::{ChatMessage, DeliveryState, GROUP_WINDOW_SECS, MAX_REACTIONS, MessageGroup, ReactionChip, ReactionGlyph, collapses_into};
pub use nav_row::{ChannelRow, NavRow};
pub use pickers::{ColorSwatches, IconGrid, PickerTabs};
pub use segmented::Segmented;
pub use section::SidebarSection;
pub use toast::{Toast, ToastKind};
pub use voice_row::VoiceParticipantRow;
pub use voice_tile::{TileMedia, TileSize, VoiceTile};
