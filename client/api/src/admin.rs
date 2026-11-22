//! Site-admin domain types (see `Client::admin_*` methods).

use std::time::SystemTime;

use crate::proto::thehideout::v1 as pb;
use crate::types::{time, Guild, User};

/// Bytes of stored media per category.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StorageBreakdown {
    pub attachments: u64,
    pub emojis: u64,
    pub guild_icons: u64,
    /// Always 0 for a single guild.
    pub avatars: u64,
    pub embeds: u64,
}

impl StorageBreakdown {
    /// Sum of all categories.
    pub fn total(&self) -> u64 {
        self.attachments + self.emojis + self.guild_icons + self.avatars + self.embeds
    }
}

impl From<pb::StorageBreakdown> for StorageBreakdown {
    fn from(s: pb::StorageBreakdown) -> Self {
        Self { attachments: s.attachments, emojis: s.emojis, guild_icons: s.guild_icons, avatars: s.avatars, embeds: s.embeds }
    }
}

/// Site-wide totals; "today" is the current UTC day.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overview {
    pub users: u64,
    pub guilds: u64,
    pub channels: u64,
    pub messages: u64,
    pub storage_bytes_total: u64,
    pub storage_bytes_avatars: u64,
    pub storage_bytes_guild_icons: u64,
    pub live_connections: u64,
    pub messages_today: u64,
    pub signups_today: u64,
    pub active_users_today: u64,
    /// Global storage per category.
    pub storage: StorageBreakdown,
}

/// One day of activity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityPoint {
    /// UTC date, `YYYY-MM-DD`.
    pub date: String,
    pub messages: u64,
    pub signups: u64,
    pub active_users: u64,
}

/// A guild with usage numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminGuildRow {
    pub guild: Guild,
    pub member_count: u64,
    pub channel_count: u64,
    pub message_count: u64,
    pub storage_bytes: u64,
    /// Per-category storage (`avatars` is 0).
    pub storage: StorageBreakdown,
}

/// A user with usage numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminUserRow {
    pub user: User,
    pub guild_count: u64,
    pub message_count: u64,
    pub storage_bytes: u64,
    /// `None` if the user was never active.
    pub last_active_at: Option<SystemTime>,
    /// The user is suspended right now.
    pub suspended: bool,
    /// When the suspension ends; `None` = until an admin lifts it.
    pub suspended_until: Option<SystemTime>,
    pub suspended_reason: String,
    /// The account was deleted (anonymized) or is being deleted.
    pub deleted: bool,
}

/// One page of admin rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paged<T> {
    pub items: Vec<T>,
    /// Pass as `page_token` for the next page; `None` on the last page.
    pub next_page_token: Option<String>,
}

/// Category of a stored file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AdminFileKind {
    Attachment,
    Emoji,
    GuildIcon,
    Avatar,
    EmbedThumbnail,
    /// A kind this client does not know.
    Unknown,
}

impl AdminFileKind {
    fn from_proto(kind: pb::AdminFileKind) -> Self {
        use pb::AdminFileKind as K;
        match kind {
            K::ADMIN_FILE_KIND_ATTACHMENT => Self::Attachment,
            K::ADMIN_FILE_KIND_EMOJI => Self::Emoji,
            K::ADMIN_FILE_KIND_GUILD_ICON => Self::GuildIcon,
            K::ADMIN_FILE_KIND_AVATAR => Self::Avatar,
            K::ADMIN_FILE_KIND_EMBED_THUMBNAIL => Self::EmbedThumbnail,
            _ => Self::Unknown,
        }
    }

    /// `None` for [`Unknown`](Self::Unknown) (= "all kinds" as a filter).
    pub(crate) fn to_proto(self) -> Option<pb::AdminFileKind> {
        use pb::AdminFileKind as K;
        Some(match self {
            Self::Attachment => K::ADMIN_FILE_KIND_ATTACHMENT,
            Self::Emoji => K::ADMIN_FILE_KIND_EMOJI,
            Self::GuildIcon => K::ADMIN_FILE_KIND_GUILD_ICON,
            Self::Avatar => K::ADMIN_FILE_KIND_AVATAR,
            Self::EmbedThumbnail => K::ADMIN_FILE_KIND_EMBED_THUMBNAIL,
            Self::Unknown => return None,
        })
    }
}

/// A stored file with its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminFileRow {
    /// Media key.
    pub key: String,
    pub kind: AdminFileKind,
    /// Original name (attachments only); empty otherwise.
    pub filename: String,
    pub bytes: u64,
    pub content_type: String,
    pub guild: Option<Guild>,
    pub owner: Option<User>,
    pub channel_id: Option<u64>,
    pub message_id: Option<u64>,
    pub created_at: SystemTime,
}

impl From<pb::AdminFileRow> for AdminFileRow {
    fn from(r: pb::AdminFileRow) -> Self {
        Self {
            key: r.key,
            kind: r.kind.as_known().map(AdminFileKind::from_proto).unwrap_or(AdminFileKind::Unknown),
            filename: r.filename,
            bytes: r.bytes,
            content_type: r.content_type,
            guild: r.guild.into_option().map(Guild::from),
            owner: r.owner.into_option().map(User::from),
            channel_id: r.channel_id,
            message_id: r.message_id,
            created_at: time(r.created_at.as_option()),
        }
    }
}

/// One audit record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    pub id: u64,
    /// The admin who acted.
    pub actor: Option<User>,
    /// E.g. `file.delete`, `admin.grant`, `admin.revoke`.
    pub action: String,
    pub target: String,
    pub reason: String,
    pub created_at: SystemTime,
}

impl From<pb::AuditEntry> for AuditEntry {
    fn from(e: pb::AuditEntry) -> Self {
        Self {
            id: e.id,
            actor: e.actor.into_option().map(User::from),
            action: e.action,
            target: e.target,
            reason: e.reason,
            created_at: time(e.created_at.as_option()),
        }
    }
}

pub(crate) fn next_token(token: String) -> Option<String> {
    (!token.is_empty()).then_some(token)
}

impl From<pb::GetOverviewResponse> for Overview {
    fn from(o: pb::GetOverviewResponse) -> Self {
        Self {
            users: o.users,
            guilds: o.guilds,
            channels: o.channels,
            messages: o.messages,
            storage_bytes_total: o.storage_bytes_total,
            storage_bytes_avatars: o.storage_bytes_avatars,
            storage_bytes_guild_icons: o.storage_bytes_guild_icons,
            live_connections: o.live_connections,
            messages_today: o.messages_today,
            signups_today: o.signups_today,
            active_users_today: o.active_users_today,
            storage: o.storage.into_option().map(Into::into).unwrap_or_default(),
        }
    }
}

impl From<pb::ActivityPoint> for ActivityPoint {
    fn from(p: pb::ActivityPoint) -> Self {
        Self {
            date: p.date,
            messages: p.messages,
            signups: p.signups,
            active_users: p.active_users,
        }
    }
}

impl AdminGuildRow {
    pub(crate) fn from_proto(r: pb::AdminGuildRow) -> Option<Self> {
        Some(Self {
            guild: r.guild.into_option().map(Guild::from)?,
            member_count: r.member_count,
            channel_count: r.channel_count,
            message_count: r.message_count,
            storage_bytes: r.storage_bytes,
            storage: r.storage.into_option().map(Into::into).unwrap_or_default(),
        })
    }
}

impl AdminUserRow {
    pub(crate) fn from_proto(r: pb::AdminUserRow) -> Option<Self> {
        Some(Self {
            user: r.user.into_option().map(User::from)?,
            guild_count: r.guild_count,
            message_count: r.message_count,
            storage_bytes: r.storage_bytes,
            last_active_at: r.last_active_at.as_option().map(|ts| time(Some(ts))),
            suspended: r.suspended,
            suspended_until: r.suspended_until.as_option().map(|ts| time(Some(ts))),
            suspended_reason: r.suspended_reason,
            deleted: r.deleted,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_token_empty_is_none() {
        assert_eq!(next_token(String::new()), None);
        assert_eq!(next_token("abc".into()), Some("abc".into()));
    }

    #[test]
    fn overview_copies_fields() {
        let o: Overview = pb::GetOverviewResponse {
            users: 1,
            guilds: 2,
            active_users_today: 9,
            ..Default::default()
        }
        .into();
        assert_eq!((o.users, o.guilds, o.active_users_today), (1, 2, 9));
    }

    #[test]
    fn rows_require_their_entity_and_map_optional_time() {
        assert!(AdminGuildRow::from_proto(pb::AdminGuildRow::default()).is_none());
        assert!(AdminUserRow::from_proto(pb::AdminUserRow::default()).is_none());
        let row = AdminUserRow::from_proto(pb::AdminUserRow {
            user: pb::User {
                id: 3,
                is_admin: true,
                ..Default::default()
            }
            .into(),
            guild_count: 2,
            ..Default::default()
        })
        .unwrap();
        assert!(row.user.is_admin);
        assert_eq!(row.last_active_at, None);
        assert_eq!(row.guild_count, 2);
    }

    #[test]
    fn activity_point_copies() {
        let p: ActivityPoint = pb::ActivityPoint {
            date: "2026-01-02".into(),
            messages: 4,
            signups: 1,
            active_users: 2,
            ..Default::default()
        }
        .into();
        assert_eq!(p.date, "2026-01-02");
        assert_eq!(p.active_users, 2);
    }

    #[test]
    fn storage_breakdown_converts_and_sums() {
        let o: Overview = pb::GetOverviewResponse {
            storage: pb::StorageBreakdown { attachments: 1, emojis: 2, guild_icons: 3, avatars: 4, embeds: 5, ..Default::default() }.into(),
            ..Default::default()
        }
        .into();
        assert_eq!((o.storage.attachments, o.storage.embeds, o.storage.total()), (1, 5, 15));
        assert_eq!(Overview::from(pb::GetOverviewResponse::default()).storage, StorageBreakdown::default());
        let row = AdminGuildRow::from_proto(pb::AdminGuildRow {
            guild: pb::Guild::default().into(),
            storage: pb::StorageBreakdown { emojis: 7, ..Default::default() }.into(),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(row.storage.emojis, 7);
    }

    #[test]
    fn file_row_and_kind_convert() {
        use pb::AdminFileKind as K;
        for kind in [AdminFileKind::Attachment, AdminFileKind::Emoji, AdminFileKind::GuildIcon, AdminFileKind::Avatar, AdminFileKind::EmbedThumbnail] {
            assert_eq!(AdminFileKind::from_proto(kind.to_proto().unwrap()), kind);
        }
        assert_eq!(AdminFileKind::Unknown.to_proto(), None);
        assert_eq!(AdminFileKind::from_proto(K::ADMIN_FILE_KIND_UNSPECIFIED), AdminFileKind::Unknown);
        let r: AdminFileRow = pb::AdminFileRow {
            key: "k".into(),
            kind: K::ADMIN_FILE_KIND_EMOJI.into(),
            bytes: 9,
            channel_id: Some(2),
            owner: pb::User { id: 3, ..Default::default() }.into(),
            ..Default::default()
        }
        .into();
        assert_eq!((r.key.as_str(), r.kind, r.bytes, r.channel_id, r.message_id), ("k", AdminFileKind::Emoji, 9, Some(2), None));
        assert_eq!((r.owner.map(|u| u.id), r.guild), (Some(3), None));
    }

    #[test]
    fn audit_entry_converts() {
        let e: AuditEntry = pb::AuditEntry {
            id: 1,
            actor: pb::User { id: 2, ..Default::default() }.into(),
            action: "file.delete".into(),
            target: "k".into(),
            reason: "spam".into(),
            ..Default::default()
        }
        .into();
        assert_eq!((e.id, e.actor.map(|u| u.id), e.action.as_str(), e.reason.as_str()), (1, Some(2), "file.delete", "spam"));
    }
}
