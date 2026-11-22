//! Channel and section layout of a guild: loading, normalising and writing.
//!
//! Stored `position`s are only a sort hint. The authoritative order is derived
//! by [`build`]: sections by `(position, id)`, channels per container by
//! `(position, id)`, with null columns (rows written before migration 0004)
//! counting as unsectioned / position 0, i.e. ordered by id. Channels pointing
//! at a missing section fall back to the top level. Concurrent writers can
//! therefore at worst produce ties, never an invalid layout.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use connectrpc::ConnectError;
use scylla::statement::batch::{Batch, BatchType};

use crate::AppState;
use crate::auth::internal;
use crate::convert::{datetime_to_proto, id_from_db, id_to_db};
use crate::proto::thehideout::v1::{Channel, ChannelIcon, ChannelKind, GuildLayout, LayoutEntry, Section, channel_icon};
use crate::versions;

/// Sections one guild may have. Enforced by a count before insert, so racing
/// creators can overshoot by a few; the cap only bounds growth.
pub const MAX_SECTIONS_PER_GUILD: usize = 50;
const WRITE_CHUNK: usize = 100;

/// A channel's icon; absence (`None` where used) is the default icon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IconRef {
    Phosphor(String),
    Unicode(String),
    /// A custom emoji id of the channel's guild.
    Custom(u64),
}

impl IconRef {
    /// `(icon_kind, icon_value)` as stored in the `channels` table.
    pub fn columns(&self) -> (&'static str, String) {
        match self {
            Self::Phosphor(name) => ("phosphor", name.clone()),
            Self::Unicode(emoji) => ("unicode", emoji.clone()),
            Self::Custom(id) => ("custom", id.to_string()),
        }
    }

    /// Columns for an optional icon; "none" records an explicit default so the
    /// legacy `icon` column is not consulted again.
    pub fn columns_of(icon: Option<&Self>) -> (&'static str, String) {
        icon.map_or(("none", String::new()), Self::columns)
    }

    /// Reads the stored columns. Rows from before migration 0006 have no kind
    /// and fall back to the legacy `icon` column (a Phosphor name, "" = none).
    pub fn from_columns(kind: Option<&str>, value: Option<&str>, legacy_icon: &str) -> Option<Self> {
        match (kind, value) {
            (Some("phosphor"), Some(v)) => Some(Self::Phosphor(v.to_owned())),
            (Some("unicode"), Some(v)) => Some(Self::Unicode(v.to_owned())),
            (Some("custom"), Some(v)) => v.parse().ok().map(Self::Custom),
            (Some(_), _) => None,
            (None, _) => (!legacy_icon.is_empty()).then(|| Self::Phosphor(legacy_icon.to_owned())),
        }
    }

    /// The deprecated `Channel.icon` mirror: the Phosphor name, else "".
    pub fn legacy_mirror(icon: Option<&Self>) -> &str {
        match icon {
            Some(Self::Phosphor(name)) => name,
            _ => "",
        }
    }

    pub fn to_proto(&self) -> ChannelIcon {
        let kind = match self {
            Self::Phosphor(name) => channel_icon::Kind::Phosphor(name.clone()),
            Self::Unicode(emoji) => channel_icon::Kind::Unicode(emoji.clone()),
            Self::Custom(id) => channel_icon::Kind::CustomEmojiId(*id),
        };
        ChannelIcon {
            kind: Some(kind),
            ..Default::default()
        }
    }
}

pub struct ChannelRow {
    pub id: u64,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub section_id: Option<u64>,
    pub position: i32,
    pub icon: Option<IconRef>,
    pub color: String,
    pub topic: String,
    /// Text or voice; rows from before migration 0012 are text.
    pub kind: ChannelKind,
}

/// The `kind` column value of a channel kind ("text" for everything but voice).
pub fn kind_column(kind: ChannelKind) -> &'static str {
    if kind == ChannelKind::CHANNEL_KIND_VOICE { "voice" } else { "text" }
}

/// Reads the `kind` column; null (old rows) and unknown values are text.
pub fn kind_from_column(kind: Option<&str>) -> ChannelKind {
    match kind {
        Some("voice") => ChannelKind::CHANNEL_KIND_VOICE,
        _ => ChannelKind::CHANNEL_KIND_TEXT,
    }
}

pub struct SectionRow {
    pub id: u64,
    pub name: String,
    pub position: i32,
}

pub struct GuildLayoutData {
    /// Ordered by id (creation order).
    pub channels: Vec<ChannelRow>,
    /// Ordered by `(position, id)`.
    pub sections: Vec<SectionRow>,
}

type RawChannel = (
    i64,
    Option<String>,
    DateTime<Utc>,
    Option<i64>,
    Option<i32>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

pub async fn load(state: &AppState, guild_id: u64) -> Result<GuildLayoutData, ConnectError> {
    let db_guild = id_to_db(guild_id);
    let mut channels = state
        .session
        .execute_unpaged(
            "SELECT id, name, created_at, section_id, position, icon, color, topic, icon_kind, icon_value, kind \
             FROM channels WHERE guild_id = ? ORDER BY id ASC",
            (db_guild,),
        )
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .rows::<RawChannel>()
        .map_err(internal)?
        .map(|r| {
            r.map(|(id, name, created_at, section, position, icon, color, topic, icon_kind, icon_value, kind)| ChannelRow {
                id: id_from_db(id),
                name: name.unwrap_or_default(),
                created_at,
                section_id: section.map(id_from_db),
                position: position.unwrap_or(0),
                icon: IconRef::from_columns(icon_kind.as_deref(), icon_value.as_deref(), &icon.unwrap_or_default()),
                color: color.unwrap_or_default(),
                topic: topic.unwrap_or_default(),
                kind: kind_from_column(kind.as_deref()),
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?;
    channels.sort_by_key(|c| c.id);

    let mut sections = state
        .session
        .execute_unpaged("SELECT id, name, position FROM sections WHERE guild_id = ?", (db_guild,))
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .rows::<(i64, Option<String>, Option<i32>)>()
        .map_err(internal)?
        .map(|r| {
            r.map(|(id, name, position)| SectionRow {
                id: id_from_db(id),
                name: name.unwrap_or_default(),
                position: position.unwrap_or(0),
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?;
    sections.sort_by_key(|s| (s.position, s.id));
    Ok(GuildLayoutData { channels, sections })
}

/// The normalised layout plus every channel with its effective section and position.
pub struct Built {
    pub layout: GuildLayout,
    /// Ordered by id.
    pub channels: Vec<Channel>,
}

pub fn build(guild_id: u64, data: &GuildLayoutData, version: u64) -> Built {
    let known: HashSet<u64> = data.sections.iter().map(|s| s.id).collect();
    let effective = |c: &ChannelRow| c.section_id.filter(|s| known.contains(s));

    let mut per_container: HashMap<u64, Vec<&ChannelRow>> = HashMap::new();
    for c in &data.channels {
        per_container.entry(effective(c).unwrap_or(0)).or_default().push(c);
    }
    for list in per_container.values_mut() {
        list.sort_by_key(|c| (c.position, c.id));
    }

    let mut positions: HashMap<u64, i32> = HashMap::new();
    let mut entries = Vec::with_capacity(data.sections.len() + 1);
    for container in std::iter::once(0).chain(data.sections.iter().map(|s| s.id)) {
        let list = per_container.remove(&container).unwrap_or_default();
        for (i, c) in list.iter().enumerate() {
            positions.insert(c.id, i as i32);
        }
        entries.push(LayoutEntry {
            section_id: container,
            channel_ids: list.iter().map(|c| c.id).collect(),
            ..Default::default()
        });
    }

    let channels = data
        .channels
        .iter()
        .map(|c| channel_to_proto(guild_id, c, effective(c), positions[&c.id]))
        .collect();
    let sections = data
        .sections
        .iter()
        .enumerate()
        .map(|(i, s)| Section {
            id: s.id,
            guild_id,
            name: s.name.clone(),
            position: i as i32,
            ..Default::default()
        })
        .collect();
    Built {
        layout: GuildLayout {
            version,
            sections,
            entries,
            ..Default::default()
        },
        channels,
    }
}

pub fn channel_to_proto(guild_id: u64, c: &ChannelRow, section_id: Option<u64>, position: i32) -> Channel {
    Channel {
        id: c.id,
        guild_id,
        name: c.name.clone(),
        created_at: datetime_to_proto(c.created_at).into(),
        section_id,
        position,
        icon: IconRef::legacy_mirror(c.icon.as_ref()).to_owned(),
        icon_ref: c.icon.as_ref().map(IconRef::to_proto).into(),
        color: c.color.clone(),
        topic: c.topic.clone(),
        kind: c.kind.into(),
        ..Default::default()
    }
}

/// Reads the version first, then the data: a concurrent write can only make
/// the version look older than the data, which a later `UpdateLayout` rejects.
pub async fn load_built(state: &AppState, guild_id: u64) -> Result<Built, ConnectError> {
    let version = versions::get(state, guild_id, versions::LAYOUT).await?;
    let data = load(state, guild_id).await?;
    Ok(build(guild_id, &data, version))
}

/// Sets `(section, position)` of the given channels, in single-partition batches.
pub async fn write_channel_positions(
    state: &AppState,
    guild_id: u64,
    assignments: &[(u64, Option<u64>, i32)],
) -> Result<(), ConnectError> {
    for chunk in assignments.chunks(WRITE_CHUNK) {
        let mut batch = Batch::new(BatchType::Unlogged);
        let mut values = Vec::with_capacity(chunk.len());
        for &(id, section, position) in chunk {
            batch.append_statement("UPDATE channels SET section_id = ?, position = ? WHERE guild_id = ? AND id = ?");
            values.push((section.map(id_to_db), position, id_to_db(guild_id), id_to_db(id)));
        }
        state.session.batch(&batch, values).await.map_err(internal)?;
    }
    Ok(())
}

/// Sets the position of the given sections, in single-partition batches.
pub async fn write_section_positions(
    state: &AppState,
    guild_id: u64,
    assignments: &[(u64, i32)],
) -> Result<(), ConnectError> {
    for chunk in assignments.chunks(WRITE_CHUNK) {
        let mut batch = Batch::new(BatchType::Unlogged);
        let mut values = Vec::with_capacity(chunk.len());
        for &(id, position) in chunk {
            batch.append_statement("UPDATE sections SET position = ? WHERE guild_id = ? AND id = ?");
            values.push((position, id_to_db(guild_id), id_to_db(id)));
        }
        state.session.batch(&batch, values).await.map_err(internal)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(id: u64, section: Option<u64>, position: i32) -> ChannelRow {
        ChannelRow {
            id,
            name: format!("c{id}"),
            created_at: DateTime::<Utc>::default(),
            section_id: section,
            position,
            icon: None,
            color: String::new(),
            topic: String::new(),
            kind: ChannelKind::CHANNEL_KIND_TEXT,
        }
    }

    #[test]
    fn legacy_and_orphaned_channels_get_a_valid_layout() {
        let data = GuildLayoutData {
            // 5 points at a section that no longer exists; 1, 2, 3 are legacy (position 0).
            channels: vec![ch(1, None, 0), ch(2, None, 0), ch(3, None, 0), ch(4, Some(9), 1), ch(5, Some(77), 0)],
            sections: vec![SectionRow { id: 9, name: "s".into(), position: 4 }],
        };
        let built = build(1, &data, 3);
        assert_eq!(built.layout.version, 3);
        assert_eq!(built.layout.entries[0].section_id, 0);
        assert_eq!(built.layout.entries[0].channel_ids, vec![1, 2, 3, 5]);
        assert_eq!(built.layout.entries[1].channel_ids, vec![4]);
        assert_eq!(built.layout.sections[0].position, 0);
        assert_eq!(built.channels[4].section_id, None);
        assert_eq!(built.channels[3].section_id, Some(9));
        assert_eq!(built.channels[3].position, 0);
    }
}
