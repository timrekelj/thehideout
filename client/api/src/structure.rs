//! Roles, permissions, members, sections and layout: the guild "structure" domain types.

use std::ops::{BitOr, BitOrAssign};
use std::time::SystemTime;

use crate::proto::thehideout::v1 as pb;
use crate::types::{time, User};

/// A set of permission flags (the wire `uint64`). Unknown bits are preserved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Permissions(pub u64);

impl Permissions {
    pub const NONE: Self = Self(0);
    /// Implies every other permission.
    pub const ADMINISTRATOR: Self = Self(1);
    /// Update guild name and icon.
    pub const MANAGE_SERVER: Self = Self(2);
    /// Create / update channels and sections, and change the layout.
    pub const MANAGE_CHANNELS: Self = Self(4);
    /// Create / update / delete / reorder roles and assign them to members.
    pub const MANAGE_ROLES: Self = Self(8);
    /// Create / rename / delete custom emojis.
    pub const MANAGE_EMOJIS: Self = Self(16);
    /// Invite people by email into the guild.
    pub const INVITE_PEOPLE: Self = Self(32);
    /// Join voice channels. Every member has it (see [`Permissions::BASELINE`]).
    pub const CONNECT: Self = Self(64);
    /// Publish a microphone in voice channels. Every member has it.
    pub const SPEAK: Self = Self(128);
    /// Publish a camera and share the screen. Every member has it.
    pub const VIDEO: Self = Self(256);
    /// Server-mute other members in voice channels (subject to the role hierarchy).
    pub const MUTE_MEMBERS: Self = Self(512);
    /// Move and disconnect other members in voice channels (subject to the role hierarchy).
    pub const MOVE_MEMBERS: Self = Self(1024);
    /// What every member has without any role: CONNECT, SPEAK and VIDEO.
    pub const BASELINE: Self = Self(64 | 128 | 256);

    pub const fn bits(self) -> u64 {
        self.0
    }

    /// Whether every flag of `other` is set. `ADMINISTRATOR` implies all
    /// flags, so it satisfies any `other`; the empty set is always contained.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & Self::ADMINISTRATOR.0 != 0 || self.0 & other.0 == other.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl BitOr for Permissions {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for Permissions {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl From<u64> for Permissions {
    fn from(bits: u64) -> Self {
        Self(bits)
    }
}

impl From<Permissions> for u64 {
    fn from(p: Permissions) -> Self {
        p.0
    }
}

/// One of the 12 palette keys used for role and channel colours, or `None`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum PaletteColor {
    #[default]
    None,
    Slate,
    Red,
    Orange,
    Amber,
    Yellow,
    Lime,
    Green,
    Teal,
    Cyan,
    Blue,
    Indigo,
    Pink,
}

impl PaletteColor {
    /// The 12 real colours, in palette order (excludes `None`).
    pub const ALL: [Self; 12] = [
        Self::Slate,
        Self::Red,
        Self::Orange,
        Self::Amber,
        Self::Yellow,
        Self::Lime,
        Self::Green,
        Self::Teal,
        Self::Cyan,
        Self::Blue,
        Self::Indigo,
        Self::Pink,
    ];

    /// Parses a wire key; `""` and unknown keys become `None`.
    pub fn from_key(key: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|c| c.as_key() == key)
            .unwrap_or(Self::None)
    }

    /// The wire key (`""` for `None`).
    pub const fn as_key(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Slate => "slate",
            Self::Red => "red",
            Self::Orange => "orange",
            Self::Amber => "amber",
            Self::Yellow => "yellow",
            Self::Lime => "lime",
            Self::Green => "green",
            Self::Teal => "teal",
            Self::Cyan => "cyan",
            Self::Blue => "blue",
            Self::Indigo => "indigo",
            Self::Pink => "pink",
        }
    }
}

impl From<&str> for PaletteColor {
    fn from(key: &str) -> Self {
        Self::from_key(key)
    }
}

impl From<PaletteColor> for &'static str {
    fn from(c: PaletteColor) -> Self {
        c.as_key()
    }
}

/// A Phosphor icon name such as `"game-controller"`; `""` = default (derive a
/// hash icon from the channel).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ChannelIcon(pub String);

impl ChannelIcon {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn is_default(&self) -> bool {
        self.0.is_empty()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for ChannelIcon {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// A channel's icon: the authoritative three-way form of [`ChannelIcon`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub enum ChannelIconRef {
    /// No icon chosen; derive a hash icon from the channel.
    #[default]
    Default,
    /// A Phosphor icon name such as `"game-controller"`.
    Phosphor(String),
    /// A single Unicode emoji.
    Unicode(String),
    /// A custom emoji of the channel's guild.
    Custom(u64),
}

impl ChannelIconRef {
    /// The deprecated string mirror: the Phosphor name, `""` otherwise.
    pub fn phosphor_name(&self) -> &str {
        match self {
            Self::Phosphor(name) => name,
            _ => "",
        }
    }

    /// Reads the wire form; an unset icon falls back to the legacy `icon` string.
    pub(crate) fn from_wire(icon_ref: Option<pb::ChannelIcon>, legacy: &str) -> Self {
        use pb::channel_icon::Kind;
        match icon_ref.and_then(|i| i.kind) {
            Some(Kind::Phosphor(name)) => Self::Phosphor(name),
            Some(Kind::Unicode(e)) => Self::Unicode(e),
            Some(Kind::CustomEmojiId(id)) => Self::Custom(id),
            None if !legacy.is_empty() => Self::Phosphor(legacy.to_owned()),
            None => Self::Default,
        }
    }

    pub(crate) fn to_proto(&self) -> pb::ChannelIcon {
        use pb::channel_icon::Kind;
        pb::ChannelIcon {
            kind: match self {
                Self::Default => None,
                Self::Phosphor(name) => Some(Kind::Phosphor(name.clone())),
                Self::Unicode(e) => Some(Kind::Unicode(e.clone())),
                Self::Custom(id) => Some(Kind::CustomEmojiId(*id)),
            },
            ..Default::default()
        }
    }
}

impl From<&ChannelIcon> for ChannelIconRef {
    fn from(icon: &ChannelIcon) -> Self {
        if icon.is_default() { Self::Default } else { Self::Phosphor(icon.0.clone()) }
    }
}

/// A role within a guild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Role {
    pub id: u64,
    pub guild_id: u64,
    pub name: String,
    pub color: PaletteColor,
    /// Seniority; higher = more senior.
    pub position: i32,
    pub permissions: Permissions,
    pub created_at: SystemTime,
}

/// A user's membership of a guild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub user: User,
    pub role_ids: Vec<u64>,
    pub joined_at: SystemTime,
}

/// A named group of channels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub id: u64,
    pub guild_id: u64,
    pub name: String,
    pub position: i32,
}

/// The channels of one section in display order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutEntry {
    /// `None` = top level (unsectioned channels).
    pub section_id: Option<u64>,
    pub channel_ids: Vec<u64>,
}

/// The ordered layout of a guild's channel list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GuildLayout {
    /// Pass as `expected_version` to `update_layout`.
    pub version: u64,
    /// Sections in display order.
    pub sections: Vec<Section>,
    pub entries: Vec<LayoutEntry>,
}

/// Roles of a guild, most senior first, with the version for `reorder_roles`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoleList {
    pub roles: Vec<Role>,
    pub version: u64,
}

impl From<pb::Role> for Role {
    fn from(r: pb::Role) -> Self {
        Self {
            id: r.id,
            guild_id: r.guild_id,
            name: r.name,
            color: PaletteColor::from_key(&r.color),
            position: r.position,
            permissions: Permissions(r.permissions),
            created_at: time(r.created_at.as_option()),
        }
    }
}

impl From<pb::Member> for Member {
    fn from(m: pb::Member) -> Self {
        Self {
            user: m
                .user
                .into_option()
                .map(User::from)
                .unwrap_or_else(|| User::from(pb::User::default())),
            role_ids: m.role_ids,
            joined_at: time(m.joined_at.as_option()),
        }
    }
}

impl From<pb::Section> for Section {
    fn from(s: pb::Section) -> Self {
        Self {
            id: s.id,
            guild_id: s.guild_id,
            name: s.name,
            position: s.position,
        }
    }
}

impl From<pb::LayoutEntry> for LayoutEntry {
    fn from(e: pb::LayoutEntry) -> Self {
        Self {
            section_id: (e.section_id != 0).then_some(e.section_id),
            channel_ids: e.channel_ids,
        }
    }
}

impl From<&LayoutEntry> for pb::LayoutEntry {
    fn from(e: &LayoutEntry) -> Self {
        Self {
            section_id: e.section_id.unwrap_or(0),
            channel_ids: e.channel_ids.clone(),
            ..Default::default()
        }
    }
}

impl From<pb::GuildLayout> for GuildLayout {
    fn from(l: pb::GuildLayout) -> Self {
        Self {
            version: l.version,
            sections: l.sections.into_iter().map(Section::from).collect(),
            entries: l.entries.into_iter().map(LayoutEntry::from).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permissions_contains_or_and_admin() {
        let p = Permissions::MANAGE_CHANNELS | Permissions::MANAGE_ROLES;
        assert_eq!(p.bits(), 12);
        assert!(p.contains(Permissions::MANAGE_ROLES));
        assert!(p.contains(Permissions::MANAGE_CHANNELS | Permissions::MANAGE_ROLES));
        assert!(!p.contains(Permissions::MANAGE_SERVER));
        assert!(!p.contains(Permissions::MANAGE_SERVER | Permissions::MANAGE_ROLES));
        assert!(p.contains(Permissions::NONE));
        assert!(Permissions::NONE.contains(Permissions::NONE));
        assert!(!Permissions::NONE.contains(Permissions::MANAGE_ROLES));
        // Administrator implies everything, including unknown bits.
        assert!(Permissions::ADMINISTRATOR.contains(Permissions(1 << 40)));
        let mut q = Permissions::NONE;
        q |= Permissions::MANAGE_SERVER;
        assert_eq!(q, Permissions::MANAGE_SERVER);
        assert_eq!(Permissions::from(u64::MAX).bits(), u64::MAX);
    }

    #[test]
    fn palette_round_trips_and_unknown_is_none() {
        for color in PaletteColor::ALL {
            assert_ne!(color.as_key(), "");
            assert_eq!(PaletteColor::from_key(color.as_key()), color);
        }
        assert_eq!(PaletteColor::ALL.len(), 12);
        assert_eq!(PaletteColor::from_key(""), PaletteColor::None);
        assert_eq!(PaletteColor::None.as_key(), "");
        assert_eq!(PaletteColor::from_key("magenta"), PaletteColor::None);
        assert_eq!(PaletteColor::from_key("Red"), PaletteColor::None);
        assert_eq!(PaletteColor::from("teal"), PaletteColor::Teal);
    }

    #[test]
    fn channel_icon_default() {
        assert!(ChannelIcon::default().is_default());
        let icon = ChannelIcon::new("game-controller");
        assert!(!icon.is_default());
        assert_eq!(icon.as_str(), "game-controller");
    }

    #[test]
    fn role_converts_color_and_permissions() {
        let r: Role = pb::Role {
            id: 1,
            color: "pink".into(),
            permissions: 10,
            position: 3,
            ..Default::default()
        }
        .into();
        assert_eq!(r.color, PaletteColor::Pink);
        assert_eq!(
            r.permissions,
            Permissions::MANAGE_SERVER | Permissions::MANAGE_ROLES
        );
        assert_eq!(r.position, 3);
        let r: Role = pb::Role {
            color: "nope".into(),
            ..Default::default()
        }
        .into();
        assert_eq!(r.color, PaletteColor::None);
    }

    #[test]
    fn member_carries_roles_and_user() {
        let m: Member = pb::Member {
            user: pb::User {
                id: 5,
                username: "a".into(),
                ..Default::default()
            }
            .into(),
            role_ids: vec![1, 2],
            ..Default::default()
        }
        .into();
        assert_eq!(m.user.id, 5);
        assert_eq!(m.role_ids, vec![1, 2]);
    }

    #[test]
    fn layout_section_zero_is_top_level() {
        let layout: GuildLayout = pb::GuildLayout {
            version: 4,
            sections: vec![pb::Section {
                id: 9,
                guild_id: 1,
                name: "s".into(),
                position: 0,
                ..Default::default()
            }],
            entries: vec![
                pb::LayoutEntry {
                    section_id: 0,
                    channel_ids: vec![1, 2],
                    ..Default::default()
                },
                pb::LayoutEntry {
                    section_id: 9,
                    channel_ids: vec![3],
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
        .into();
        assert_eq!(layout.version, 4);
        assert_eq!(
            layout.sections[0],
            Section {
                id: 9,
                guild_id: 1,
                name: "s".into(),
                position: 0
            }
        );
        assert_eq!(
            layout.entries[0],
            LayoutEntry {
                section_id: None,
                channel_ids: vec![1, 2]
            }
        );
        assert_eq!(layout.entries[1].section_id, Some(9));
    }

    #[test]
    fn layout_entry_to_proto_maps_none_to_zero() {
        let top = pb::LayoutEntry::from(&LayoutEntry {
            section_id: None,
            channel_ids: vec![1],
        });
        assert_eq!(top.section_id, 0);
        assert_eq!(top.channel_ids, vec![1]);
        let sec = pb::LayoutEntry::from(&LayoutEntry {
            section_id: Some(7),
            channel_ids: vec![],
        });
        assert_eq!(sec.section_id, 7);
    }
}
