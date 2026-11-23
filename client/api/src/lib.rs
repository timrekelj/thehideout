//! Typed Rust client for the thehideout server.
//!
//! UI apps use this crate instead of talking to the generated Connect/proto
//! code: every public type is a plain domain type ([`User`], [`Guild`],
//! [`Channel`], [`DmChannel`], [`Message`], [`Event`]), timestamps are
//! [`std::time::SystemTime`], ids are `u64`, and failures are an [`ApiError`].
//! The crate does not own a runtime; call it from your app's tokio runtime.
//!
//! # Request/response calls
//!
//! ```no_run
//! # async fn demo() -> Result<(), thehideout_api::ApiError> {
//! use thehideout_api::Client;
//!
//! let client = Client::new("http://127.0.0.1:8080");
//! // Accounts are invite-only: `accept_invite(code, ..)` creates one.
//! let (token, me) = match client.login("alice", "correct horse battery").await? {
//!     thehideout_api::LoginOutcome::Session { token, user } => (token, user),
//!     thehideout_api::LoginOutcome::SecondFactor { challenge, .. } => {
//!         client.complete_second_factor(&challenge, thehideout_api::SecondFactor::Totp("123456")).await?
//!     }
//! };
//! // Persist `token`; next launch: `Client::new(url).with_token(token)`.
//!
//! let guild = client.create_guild("friends").await?;
//! let channels = client.list_channels(guild.id).await?;
//! client.send_message(channels[0].id, "hello", Some("local-1")).await?;
//! let page = client.list_messages(channels[0].id, None, 50).await?;
//! if page.has_more {
//!     let older = client.list_messages(channels[0].id, page.next_before, 50).await?;
//! }
//! # let _ = me; Ok(()) }
//! ```
//!
//! DM channels share the message API: pass [`DmChannel::id`] as the
//! `channel_id` of `send_message` / `list_messages`.
//!
//! # Roles, layout, media and admin
//!
//! [`Client::list_roles`] / `create_role` / `update_role` / `reorder_roles` /
//! `set_member_roles` manage [`Role`]s ([`Permissions`], [`PaletteColor`]);
//! [`Client::list_members`] returns [`Member`]s with their role ids.
//! [`Client::list_channels_with_layout`] returns the channels and the
//! [`GuildLayout`] (sections plus channel order); `create_section`,
//! `update_section`, `delete_section` and `update_layout` edit it, and
//! `update_channel` / `update_guild` change a channel's look or a guild's name.
//! Version mismatches (layout, role order) fail with [`ApiError::Conflict`]:
//! refetch and retry. Images: `User::avatar` / `Guild::icon` are media keys;
//! [`Client::media_url`] / [`Client::fetch_media`] load them, and
//! `upload_avatar` / `upload_guild_icon` send up to [`MAX_MEDIA_BYTES`].
//! Site admins ([`User::is_admin`]) use the `admin_*` calls.
//!
//! # Accounts and sign-in
//!
//! Accounts are invite-only: [`Client::create_invite`] (site admins, or
//! guild members with [`Permissions::INVITE_PEOPLE`]),
//! [`Client::lookup_invite`] / [`Client::accept_invite`] for the invitee.
//! Sign in with [`Client::login`] (username or email + password),
//! [`Client::request_email_code`] / [`Client::verify_email_code`], or a passkey
//! ([`Client::begin_passkey_login`] / [`Client::finish_passkey_login`]); the
//! first two may return [`LoginOutcome::SecondFactor`]. Account security:
//! password, email, TOTP, passkeys and [`Client::list_sessions`].
//!
//! # Voice
//!
//! Voice channels have [`Channel::kind`] = [`ChannelKind::Voice`]. [`Client::join_voice`]
//! returns a [`VoiceJoin`] (LiveKit url and token); `leave_voice`, `update_voice_state`
//! and the moderation calls (`server_mute_member`, `move_member`, `disconnect_member`)
//! sit next to it. Who is where arrives as [`Event::VoiceStateUpdate`] /
//! [`Event::VoiceSnapshot`]; [`Event::VoiceMoved`] asks you to rejoin.
//!
//! # Push notifications
//!
//! The mobile apps register their APNs / FCM token with [`Client::register_device`] and tell the
//! server when they are backgrounded with [`Client::set_client_state`]. The notification
//! settings ([`NotificationSettings`], per account, guild and channel) are read and changed with
//! [`Client::notification_settings`] / [`Client::update_notification_settings`]; a change made on
//! one session arrives on the others as [`Event::NotificationSettingsUpdated`].
//!
//! # Real-time updates
//!
//! [`Gateway::spawn`] subscribes on the current tokio runtime and keeps the
//! subscription alive: it reconnects with capped exponential backoff and
//! jitter, treats silence longer than 2.5 heartbeat intervals as a dead
//! connection, and reports everything through the callback as a
//! [`GatewayUpdate`]. After a reconnect, events missed in between are not
//! replayed; refetch the visible channel's messages on
//! [`GatewayUpdate::Connected`]. The gateway stops for good with
//! [`GatewayUpdate::AuthExpired`] when the token is rejected, or when the
//! [`GatewayHandle`] is shut down or dropped.
//!
//! ```no_run
//! # fn demo(client: thehideout_api::Client) {
//! use thehideout_api::{Gateway, GatewayUpdate};
//!
//! let handle = Gateway::spawn(client, |update| match update {
//!     GatewayUpdate::Event(event) => println!("{event:?}"),
//!     other => println!("{other:?}"),
//! });
//! // ... later
//! handle.shutdown();
//! # }
//! ```

mod account;
mod admin;
mod client;
mod content;
mod client_ext;
mod error;
#[cfg(target_arch = "wasm32")]
mod fetch;
mod gateway;
mod lifecycle;
mod moderation;
mod notifications;
mod runtime;
mod state;
mod structure;
mod transport;
mod types;
mod upload;
mod voice;

pub(crate) mod proto {
    connectrpc::include_generated!();
}

pub use account::{
    DeviceSession, Invite, InvitePreview, InviteStatus, InviteTerms, LoginOutcome, NewInvite, Passkey, PasskeyCeremony, SecondFactor,
    SecondFactorMethod, SecuritySummary, TotpSetup,
};
pub use admin::{
    ActivityPoint, AdminFileKind, AdminFileRow, AdminGuildRow, AdminUserRow, AuditEntry, Overview, Paged, StorageBreakdown,
};
pub use client::{CLIENT_HEADER, Client, client_header_value, DEFAULT_MAX_UPLOAD_BYTES, MAX_EMOJI_BYTES, MAX_MEDIA_BYTES, MAX_MESSAGE_CHARS};
pub use content::{Attachment, Embed, EmbedKind, EmbedMedia, EmbedMediaKind, Emoji, PendingAttachment, Reaction, ReactionEmoji};
pub use error::{ApiError, OwnedGuild, update_required};
pub use notifications::{
    DeviceRegistration, NotificationLevel, NotificationOverride, NotificationSettings, NotificationSettingsUpdate, PushPlatform,
};
pub use lifecycle::{DeleteCredentials, TermsStatus};
#[cfg(target_arch = "wasm32")]
pub use fetch::FetchTransport;
pub use gateway::{Gateway, GatewayConfig, GatewayHandle, GatewayUpdate};
pub use structure::{Ban, ChannelIcon, ChannelIconRef, GuildLayout, LayoutEntry, Member, PaletteColor, Permissions, Role, RoleList, Section};
pub use types::{Channel, ChannelKind, DmChannel, Event, Guild, MemberRemovalReason, Message, Page, User, VoiceJoin, VoiceState};
