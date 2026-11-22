//! Plain domain types returned by the API; none of them expose generated proto types.

use std::time::SystemTime;

use buffa_types::google::protobuf::Timestamp;

use crate::proto::thehideout::v1 as pb;
use crate::content::{Attachment, Embed, Emoji, Reaction, ReactionEmoji};
use crate::structure::{ChannelIcon, ChannelIconRef, GuildLayout, Member, PaletteColor, Role};

/// A registered user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub id: u64,
    pub username: String,
    pub created_at: SystemTime,
    /// Media key of the avatar (see [`Client::media_url`](crate::Client::media_url)); `None` = no avatar.
    pub avatar: Option<String>,
    /// Whether the user is a site administrator.
    pub is_admin: bool,
    /// The account's email; only known for yourself (and to site admins).
    /// `None` for legacy accounts without one.
    pub email: Option<String>,
    /// The account was deleted and anonymized: `username` is "Deleted user", no avatar. Messages
    /// keep their author id, which resolves to this user.
    pub deleted: bool,
}

/// A guild (server) that groups members and channels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Guild {
    pub id: u64,
    pub name: String,
    pub owner_id: u64,
    /// Code other users pass to [`Client::join_guild`](crate::Client::join_guild).
    pub invite_code: String,
    pub created_at: SystemTime,
    /// Media key of the guild icon; `None` = no icon.
    pub icon: Option<String>,
}

/// What a channel is for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ChannelKind {
    #[default]
    Text,
    Voice,
}

impl ChannelKind {
    fn from_proto(kind: &buffa::EnumValue<pb::ChannelKind>) -> Self {
        match kind.as_known() {
            Some(pb::ChannelKind::CHANNEL_KIND_VOICE) => Self::Voice,
            // Unspecified (older rows and servers) and unknown values are text.
            _ => Self::Text,
        }
    }

    pub(crate) fn to_proto(self) -> pb::ChannelKind {
        match self {
            Self::Text => pb::ChannelKind::CHANNEL_KIND_TEXT,
            Self::Voice => pb::ChannelKind::CHANNEL_KIND_VOICE,
        }
    }
}

/// Who is in a voice channel and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceState {
    pub guild_id: u64,
    pub channel_id: u64,
    pub user_id: u64,
    /// The user muted their own microphone.
    pub self_mute: bool,
    /// The user deafened themself.
    pub self_deaf: bool,
    /// A moderator muted the user (enforced by the server).
    pub server_mute: bool,
    pub self_video: bool,
    pub self_stream: bool,
    /// When the user joined (or was moved into) the channel.
    pub joined_at: SystemTime,
}

/// What a client needs to connect to a voice channel's media room.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceJoin {
    /// LiveKit server URL.
    pub url: String,
    /// LiveKit access token (valid about 10 minutes to connect).
    pub token: String,
    pub channel_id: u64,
}

/// A text or voice channel inside a guild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    pub id: u64,
    pub guild_id: u64,
    pub name: String,
    pub created_at: SystemTime,
    /// Section the channel is in; `None` = top level.
    pub section_id: Option<u64>,
    /// 0-based position within its section (the layout is authoritative).
    pub position: i32,
    /// The Phosphor name of `icon_ref`, `""` for any other icon (kept for older callers).
    pub icon: ChannelIcon,
    /// The authoritative icon.
    pub icon_ref: ChannelIconRef,
    pub color: PaletteColor,
    /// Free-text topic; empty = none.
    pub topic: String,
    /// Text or voice; fixed at creation.
    pub kind: ChannelKind,
}

/// A 1:1 direct-message channel. Its `id` is the `channel_id` for messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DmChannel {
    pub id: u64,
    /// Both participants, ordered by user id ascending.
    pub participants: Vec<User>,
    pub created_at: SystemTime,
}

/// A chat message in a guild channel or a DM channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: u64,
    pub channel_id: u64,
    /// The owning guild; `None` for DM channels.
    pub guild_id: Option<u64>,
    pub author_id: u64,
    pub content: String,
    pub created_at: SystemTime,
    pub edited_at: Option<SystemTime>,
    /// The nonce given to `send_message`; empty if none was given.
    pub nonce: String,
    /// One entry per distinct emoji, ordered by first use.
    pub reactions: Vec<Reaction>,
    pub attachments: Vec<Attachment>,
    /// Link previews; arrive later via [`Event::MessageUpdated`].
    pub embeds: Vec<Embed>,
}

/// One page of messages, newest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page<T> {
    pub items: Vec<T>,
    /// Pass as `before` to fetch the next (older) page; `None` when there are no more.
    pub next_before: Option<u64>,
    pub has_more: bool,
}

/// Why a member is no longer in a guild.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemberRemovalReason {
    /// They left on their own.
    Left,
    /// A moderator kicked them.
    Kicked,
    /// A moderator banned them.
    Banned,
}

impl MemberRemovalReason {
    fn from_proto(reason: &buffa::EnumValue<pb::MemberRemovalReason>) -> Option<Self> {
        match reason.as_known()? {
            pb::MemberRemovalReason::MEMBER_REMOVAL_REASON_LEFT => Some(Self::Left),
            pb::MemberRemovalReason::MEMBER_REMOVAL_REASON_KICKED => Some(Self::Kicked),
            pb::MemberRemovalReason::MEMBER_REMOVAL_REASON_BANNED => Some(Self::Banned),
            pb::MemberRemovalReason::MEMBER_REMOVAL_REASON_UNSPECIFIED => None,
        }
    }
}

/// A real-time event from the gateway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    MessageCreated(Message),
    ChannelCreated(Channel),
    GuildMemberJoined { guild_id: u64, user: User },
    /// A DM channel was opened with the caller.
    DmOpened(DmChannel),
    /// Keep-alive from the server.
    Heartbeat,
    /// A guild's name or icon changed.
    GuildUpdated(Guild),
    /// The caller became a member of a guild (created or joined it, possibly on another
    /// device); the stream now carries its events too.
    GuildAdded(Guild),
    /// A channel's name, icon, colour or topic changed.
    ChannelUpdated(Channel),
    /// A guild's sections and/or channel order changed.
    LayoutUpdated { guild_id: u64, layout: GuildLayout },
    /// A guild's roles changed; always the full list, so replace yours.
    RolesUpdated { guild_id: u64, roles: Vec<Role>, version: u64 },
    /// A member's roles changed.
    MemberUpdated { guild_id: u64, member: Member },
    /// A user changed their profile (e.g. avatar).
    UserUpdated(User),
    /// A guild's custom emojis changed; always the full list, so replace yours.
    EmojisUpdated { guild_id: u64, emojis: Vec<Emoji> },
    /// Someone reacted. Set your own `me` when `user_id` is you.
    ReactionAdded { channel_id: u64, message_id: u64, emoji: ReactionEmoji, user_id: u64 },
    /// Someone removed their reaction.
    ReactionRemoved { channel_id: u64, message_id: u64, emoji: ReactionEmoji, user_id: u64 },
    /// A message changed (embeds resolved, attachment removed); replace your copy.
    /// Its reactions' `me` is not meaningful: keep your local flags.
    MessageUpdated(Message),
    /// Someone joined, left (`state: None`), moved between or changed their state in a
    /// voice channel of `guild_id`.
    VoiceStateUpdate { guild_id: u64, user_id: u64, state: Option<VoiceState> },
    /// The full voice states of `guild_ids`: replace yours for those guilds. Only sent when
    /// someone is in voice, so clear your voice states when a stream opens.
    VoiceSnapshot { guild_ids: Vec<u64>, states: Vec<VoiceState> },
    /// A moderator moved the caller: rejoin with [`Client::join_voice`](crate::Client::join_voice).
    VoiceMoved { guild_id: u64, channel_id: u64 },
    /// A message was deleted (by its author, a moderator or a ban's purge): remove it.
    MessageDeleted { channel_id: u64, message_id: u64 },
    /// A member left or was removed. If it is you, `GuildRemoved` follows (or came just before).
    GuildMemberRemoved { guild_id: u64, user_id: u64, reason: MemberRemovalReason },
    /// You are no longer in this guild (left, kicked, banned, or it was deleted): drop it. The
    /// stream stops delivering its events.
    GuildRemoved { guild_id: u64 },
    /// A channel was deleted; a `LayoutUpdated` follows.
    ChannelDeleted { guild_id: u64, channel_id: u64 },
}

pub(crate) fn time(ts: Option<&Timestamp>) -> SystemTime {
    ts.and_then(|ts| SystemTime::try_from(ts.clone()).ok())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

fn media_key(key: String) -> Option<String> {
    (!key.is_empty()).then_some(key)
}

impl From<pb::User> for User {
    fn from(u: pb::User) -> Self {
        Self {
            id: u.id,
            username: u.username,
            created_at: time(u.created_at.as_option()),
            avatar: media_key(u.avatar),
            is_admin: u.is_admin,
            email: u.email.filter(|e| !e.is_empty()),
            deleted: u.deleted,
        }
    }
}

impl From<pb::Guild> for Guild {
    fn from(g: pb::Guild) -> Self {
        Self {
            id: g.id,
            name: g.name,
            owner_id: g.owner_id,
            invite_code: g.invite_code,
            created_at: time(g.created_at.as_option()),
            icon: media_key(g.icon),
        }
    }
}

impl From<pb::Channel> for Channel {
    fn from(c: pb::Channel) -> Self {
        let icon_ref = ChannelIconRef::from_wire(c.icon_ref.into_option(), &c.icon);
        Self {
            id: c.id,
            guild_id: c.guild_id,
            name: c.name,
            created_at: time(c.created_at.as_option()),
            section_id: c.section_id.filter(|&id| id != 0),
            position: c.position,
            icon: ChannelIcon(icon_ref.phosphor_name().to_owned()),
            icon_ref,
            color: PaletteColor::from_key(&c.color),
            topic: c.topic,
            kind: ChannelKind::from_proto(&c.kind),
        }
    }
}

impl From<pb::VoiceState> for VoiceState {
    fn from(v: pb::VoiceState) -> Self {
        Self {
            guild_id: v.guild_id,
            channel_id: v.channel_id,
            user_id: v.user_id,
            self_mute: v.self_mute,
            self_deaf: v.self_deaf,
            server_mute: v.server_mute,
            self_video: v.self_video,
            self_stream: v.self_stream,
            joined_at: time(v.joined_at.as_option()),
        }
    }
}

impl From<pb::DmChannel> for DmChannel {
    fn from(d: pb::DmChannel) -> Self {
        Self {
            id: d.id,
            participants: d.participants.into_iter().map(User::from).collect(),
            created_at: time(d.created_at.as_option()),
        }
    }
}

impl From<pb::Message> for Message {
    fn from(m: pb::Message) -> Self {
        Self {
            id: m.id,
            channel_id: m.channel_id,
            guild_id: (m.guild_id != 0).then_some(m.guild_id),
            author_id: m.author_id,
            content: m.content,
            created_at: time(m.created_at.as_option()),
            edited_at: m.edited_at.as_option().map(|ts| time(Some(ts))),
            nonce: m.nonce,
            reactions: m.reactions.into_iter().filter_map(Reaction::from_proto).collect(),
            attachments: m.attachments.into_iter().map(Attachment::from).collect(),
            embeds: m.embeds.into_iter().map(Embed::from).collect(),
        }
    }
}

impl Event {
    /// Converts a wire event; `None` if it carries no known payload.
    pub(crate) fn from_proto(event: pb::Event) -> Option<Self> {
        use pb::event::Payload;
        Some(match event.payload? {
            Payload::MessageCreated(m) => Self::MessageCreated((*m).into()),
            Payload::ChannelCreated(c) => Self::ChannelCreated((*c).into()),
            Payload::GuildMemberJoined(j) => Self::GuildMemberJoined {
                guild_id: j.guild_id,
                user: j.user.into_option().map(User::from)?,
            },
            Payload::Heartbeat(_) => Self::Heartbeat,
            Payload::DmOpened(d) => Self::DmOpened((*d).into()),
            Payload::GuildUpdated(g) => Self::GuildUpdated((*g).into()),
            Payload::GuildAdded(g) => Self::GuildAdded((*g).into()),
            Payload::ChannelUpdated(c) => Self::ChannelUpdated((*c).into()),
            Payload::LayoutUpdated(l) => Self::LayoutUpdated {
                guild_id: l.guild_id,
                layout: l.layout.into_option().map(GuildLayout::from)?,
            },
            Payload::RolesUpdated(r) => Self::RolesUpdated {
                guild_id: r.guild_id,
                roles: r.roles.into_iter().map(Role::from).collect(),
                version: r.version,
            },
            Payload::MemberUpdated(m) => Self::MemberUpdated {
                guild_id: m.guild_id,
                member: m.member.into_option().map(Member::from)?,
            },
            Payload::UserUpdated(u) => Self::UserUpdated((*u).into()),
            Payload::EmojisUpdated(e) => Self::EmojisUpdated {
                guild_id: e.guild_id,
                emojis: e.emojis.into_iter().map(Emoji::from).collect(),
            },
            Payload::ReactionAdded(r) => Self::ReactionAdded {
                channel_id: r.channel_id,
                message_id: r.message_id,
                emoji: ReactionEmoji::from_proto(r.emoji.into_option()?)?,
                user_id: r.user_id,
            },
            Payload::ReactionRemoved(r) => Self::ReactionRemoved {
                channel_id: r.channel_id,
                message_id: r.message_id,
                emoji: ReactionEmoji::from_proto(r.emoji.into_option()?)?,
                user_id: r.user_id,
            },
            Payload::MessageUpdated(m) => Self::MessageUpdated(m.message.into_option().map(Message::from)?),
            Payload::VoiceStateUpdate(v) => Self::VoiceStateUpdate {
                guild_id: v.guild_id,
                user_id: v.user_id,
                state: v.state.into_option().map(VoiceState::from),
            },
            Payload::VoiceSnapshot(v) => Self::VoiceSnapshot {
                guild_ids: v.guild_ids,
                states: v.states.into_iter().map(VoiceState::from).collect(),
            },
            Payload::VoiceMoved(v) => Self::VoiceMoved { guild_id: v.guild_id, channel_id: v.channel_id },
            Payload::MessageDeleted(d) => Self::MessageDeleted { channel_id: d.channel_id, message_id: d.message_id },
            Payload::GuildMemberRemoved(r) => Self::GuildMemberRemoved {
                guild_id: r.guild_id,
                user_id: r.user_id,
                // An unspecified reason (a newer server) reads as the mildest.
                reason: MemberRemovalReason::from_proto(&r.reason).unwrap_or(MemberRemovalReason::Left),
            },
            Payload::GuildRemoved(r) => Self::GuildRemoved { guild_id: r.guild_id },
            Payload::ChannelDeleted(c) => Self::ChannelDeleted { guild_id: c.guild_id, channel_id: c.channel_id },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dm_message_has_no_guild_and_unset_edit_time() {
        let m: Message = pb::Message { id: 1, guild_id: 0, ..Default::default() }.into();
        assert_eq!(m.guild_id, None);
        assert_eq!(m.edited_at, None);
        let m: Message = pb::Message { guild_id: 7, ..Default::default() }.into();
        assert_eq!(m.guild_id, Some(7));
    }

    #[test]
    fn user_and_guild_media_keys_empty_is_none() {
        let u: User = pb::User { avatar: "k1".into(), is_admin: true, ..Default::default() }.into();
        assert_eq!(u.avatar.as_deref(), Some("k1"));
        assert!(u.is_admin);
        let u: User = pb::User::default().into();
        assert_eq!((u.avatar, u.is_admin), (None, false));
        let g: Guild = pb::Guild { icon: "i".into(), ..Default::default() }.into();
        assert_eq!(g.icon.as_deref(), Some("i"));
        assert_eq!(Guild::from(pb::Guild::default()).icon, None);
    }

    #[test]
    fn channel_section_zero_is_top_level_and_look_converts() {
        let c: Channel = pb::Channel {
            section_id: Some(0),
            icon: "hash".into(),
            color: "teal".into(),
            topic: "t".into(),
            position: 2,
            ..Default::default()
        }
        .into();
        assert_eq!(c.section_id, None);
        assert_eq!((c.icon.as_str(), c.color, c.topic.as_str(), c.position), ("hash", PaletteColor::Teal, "t", 2));
        let c: Channel = pb::Channel { section_id: Some(5), color: "bogus".into(), ..Default::default() }.into();
        assert_eq!(c.section_id, Some(5));
        assert_eq!(c.color, PaletteColor::None);
        assert!(c.icon.is_default());
        assert_eq!(Channel::from(pb::Channel::default()).section_id, None);
    }

    fn event(payload: pb::event::Payload) -> Option<Event> {
        Event::from_proto(pb::Event { payload: Some(payload), ..Default::default() })
    }

    #[test]
    fn guild_added_maps_to_event() {
        use pb::event::Payload;
        let guild = pb::Guild { id: 7, name: "Den".into(), owner_id: 3, invite_code: "abc".into(), ..Default::default() };
        let Some(Event::GuildAdded(g)) = event(Payload::GuildAdded(Box::new(guild))) else {
            panic!("expected GuildAdded");
        };
        assert_eq!((g.id, g.name.as_str(), g.owner_id, g.invite_code.as_str()), (7, "Den", 3, "abc"));
    }

    #[test]
    fn new_gateway_payloads_map_to_events() {
        use pb::event::Payload;
        let guild = pb::Guild { id: 1, icon: "x".into(), ..Default::default() };
        assert!(matches!(event(Payload::GuildUpdated(Box::new(guild))), Some(Event::GuildUpdated(g)) if g.icon.as_deref() == Some("x")));
        assert!(matches!(
            event(Payload::ChannelUpdated(Box::new(pb::Channel { id: 2, ..Default::default() }))),
            Some(Event::ChannelUpdated(c)) if c.id == 2
        ));
        assert!(matches!(
            event(Payload::UserUpdated(Box::new(pb::User { id: 3, ..Default::default() }))),
            Some(Event::UserUpdated(u)) if u.id == 3
        ));

        let layout = pb::LayoutUpdated { guild_id: 4, layout: pb::GuildLayout { version: 6, ..Default::default() }.into(), ..Default::default() };
        assert!(matches!(
            event(Payload::LayoutUpdated(Box::new(layout))),
            Some(Event::LayoutUpdated { guild_id: 4, layout }) if layout.version == 6
        ));
        // A layout event without a layout is malformed and dropped.
        assert!(event(Payload::LayoutUpdated(Box::default())).is_none());

        let roles = pb::RolesUpdated {
            guild_id: 5,
            roles: vec![pb::Role { id: 1, ..Default::default() }],
            version: 8,
            ..Default::default()
        };
        assert!(matches!(
            event(Payload::RolesUpdated(Box::new(roles))),
            Some(Event::RolesUpdated { guild_id: 5, roles, version: 8 }) if roles.len() == 1
        ));

        let member = pb::MemberUpdated {
            guild_id: 6,
            member: pb::Member { role_ids: vec![9], ..Default::default() }.into(),
            ..Default::default()
        };
        assert!(matches!(
            event(Payload::MemberUpdated(Box::new(member))),
            Some(Event::MemberUpdated { guild_id: 6, member }) if member.role_ids == vec![9]
        ));
        assert!(event(Payload::MemberUpdated(Box::default())).is_none());
    }

    #[test]
    fn channel_icon_ref_is_authoritative_with_legacy_fallback() {
        use pb::channel_icon::Kind;
        let ch = |icon: &str, kind: Option<Kind>| -> Channel {
            pb::Channel {
                icon: icon.into(),
                icon_ref: kind.map(|k| pb::ChannelIcon { kind: Some(k), ..Default::default() }).into(),
                ..Default::default()
            }
            .into()
        };
        let c = ch("", Some(Kind::Unicode("🎮".into())));
        assert_eq!((c.icon_ref, c.icon.is_default()), (ChannelIconRef::Unicode("🎮".into()), true));
        let c = ch("", Some(Kind::CustomEmojiId(5)));
        assert_eq!(c.icon_ref, ChannelIconRef::Custom(5));
        // The ref wins over a stale legacy string and derives `icon`.
        let c = ch("hash", Some(Kind::Phosphor("star".into())));
        assert_eq!((c.icon_ref, c.icon.as_str()), (ChannelIconRef::Phosphor("star".into()), "star"));
        // Old server: only the string is set.
        let c = ch("hash", None);
        assert_eq!((c.icon_ref, c.icon.as_str()), (ChannelIconRef::Phosphor("hash".into()), "hash"));
        assert_eq!(ch("", None).icon_ref, ChannelIconRef::Default);
    }

    #[test]
    fn message_converts_rich_content_and_drops_bad_reactions() {
        let m: Message = pb::Message {
            reactions: vec![
                pb::Reaction { emoji: ReactionEmoji::Unicode("👍".into()).to_proto().into(), count: 2, me: true, ..Default::default() },
                pb::Reaction { count: 1, ..Default::default() },
            ],
            attachments: vec![pb::Attachment { id: 1, file: "k".into(), ..Default::default() }],
            embeds: vec![pb::Embed { url: "https://x".into(), ..Default::default() }],
            ..Default::default()
        }
        .into();
        assert_eq!(m.reactions.len(), 1);
        assert_eq!((m.attachments[0].id, m.embeds[0].url.as_str()), (1, "https://x"));
    }

    #[test]
    fn new_events_map_to_domain_events() {
        use pb::event::Payload;
        let emojis = pb::EmojisUpdated { guild_id: 3, emojis: vec![pb::Emoji { id: 1, ..Default::default() }], ..Default::default() };
        assert!(matches!(
            event(Payload::EmojisUpdated(Box::new(emojis))),
            Some(Event::EmojisUpdated { guild_id: 3, emojis }) if emojis.len() == 1
        ));
        let change = |emoji: Option<ReactionEmoji>| pb::ReactionChange {
            channel_id: 1,
            message_id: 2,
            emoji: emoji.map(|e| e.to_proto()).into(),
            user_id: 4,
            ..Default::default()
        };
        assert_eq!(
            event(Payload::ReactionAdded(Box::new(change(Some(ReactionEmoji::Custom(8)))))),
            Some(Event::ReactionAdded { channel_id: 1, message_id: 2, emoji: ReactionEmoji::Custom(8), user_id: 4 })
        );
        assert_eq!(
            event(Payload::ReactionRemoved(Box::new(change(Some(ReactionEmoji::Unicode("x".into())))))),
            Some(Event::ReactionRemoved { channel_id: 1, message_id: 2, emoji: ReactionEmoji::Unicode("x".into()), user_id: 4 })
        );
        assert!(event(Payload::ReactionAdded(Box::new(change(None)))).is_none());
        let updated = pb::MessageUpdated { message: pb::Message { id: 6, ..Default::default() }.into(), ..Default::default() };
        assert!(matches!(event(Payload::MessageUpdated(Box::new(updated))), Some(Event::MessageUpdated(m)) if m.id == 6));
        assert!(event(Payload::MessageUpdated(Box::default())).is_none());
    }

    #[test]
    fn voice_events_and_channel_kind_convert() {
        use pb::event::Payload;
        let kind = |k: pb::ChannelKind| Channel::from(pb::Channel { kind: k.into(), ..Default::default() }).kind;
        assert_eq!(kind(pb::ChannelKind::CHANNEL_KIND_VOICE), ChannelKind::Voice);
        assert_eq!(kind(pb::ChannelKind::CHANNEL_KIND_TEXT), ChannelKind::Text);
        assert_eq!(kind(pb::ChannelKind::CHANNEL_KIND_UNSPECIFIED), ChannelKind::Text);

        let vs = |user_id| pb::VoiceState { guild_id: 1, channel_id: 2, user_id, server_mute: true, ..Default::default() };
        let joined = pb::VoiceStateUpdate { guild_id: 1, user_id: 3, state: vs(3).into(), ..Default::default() };
        assert!(matches!(
            event(Payload::VoiceStateUpdate(Box::new(joined))),
            Some(Event::VoiceStateUpdate { guild_id: 1, user_id: 3, state: Some(s) }) if s.channel_id == 2 && s.server_mute && !s.self_mute
        ));
        let left = pb::VoiceStateUpdate { guild_id: 1, user_id: 3, ..Default::default() };
        assert!(matches!(
            event(Payload::VoiceStateUpdate(Box::new(left))),
            Some(Event::VoiceStateUpdate { guild_id: 1, user_id: 3, state: None })
        ));
        let snap = pb::VoiceSnapshot { guild_ids: vec![1, 5], states: vec![vs(3), vs(4)], ..Default::default() };
        assert!(matches!(
            event(Payload::VoiceSnapshot(Box::new(snap))),
            Some(Event::VoiceSnapshot { guild_ids, states }) if guild_ids == vec![1, 5] && states.len() == 2
        ));
        let moved = pb::VoiceMoved { guild_id: 1, channel_id: 9, ..Default::default() };
        assert_eq!(event(Payload::VoiceMoved(Box::new(moved))), Some(Event::VoiceMoved { guild_id: 1, channel_id: 9 }));
    }

    #[test]
    fn moderation_events_convert() {
        use pb::event::Payload;
        assert_eq!(
            event(Payload::MessageDeleted(Box::new(pb::MessageDeleted { channel_id: 4, message_id: 5, ..Default::default() }))),
            Some(Event::MessageDeleted { channel_id: 4, message_id: 5 })
        );
        let removed = |reason: pb::MemberRemovalReason| {
            event(Payload::GuildMemberRemoved(Box::new(pb::GuildMemberRemoved {
                guild_id: 1,
                user_id: 2,
                reason: reason.into(),
                ..Default::default()
            })))
        };
        assert_eq!(
            removed(pb::MemberRemovalReason::MEMBER_REMOVAL_REASON_BANNED),
            Some(Event::GuildMemberRemoved { guild_id: 1, user_id: 2, reason: MemberRemovalReason::Banned })
        );
        assert!(matches!(
            removed(pb::MemberRemovalReason::MEMBER_REMOVAL_REASON_KICKED),
            Some(Event::GuildMemberRemoved { reason: MemberRemovalReason::Kicked, .. })
        ));
        assert!(matches!(
            removed(pb::MemberRemovalReason::MEMBER_REMOVAL_REASON_UNSPECIFIED),
            Some(Event::GuildMemberRemoved { reason: MemberRemovalReason::Left, .. })
        ));
        assert_eq!(
            event(Payload::GuildRemoved(Box::new(pb::GuildRemoved { guild_id: 7, ..Default::default() }))),
            Some(Event::GuildRemoved { guild_id: 7 })
        );
        assert_eq!(
            event(Payload::ChannelDeleted(Box::new(pb::ChannelDeleted { guild_id: 7, channel_id: 8, ..Default::default() }))),
            Some(Event::ChannelDeleted { guild_id: 7, channel_id: 8 })
        );
    }
}
