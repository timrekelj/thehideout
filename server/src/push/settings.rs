//! Notification settings and the rule that turns them into "push or not".

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use connectrpc::ConnectError;
use scylla::deserialize::row::DeserializeRow;
use scylla::serialize::row::SerializeRow;

use crate::AppState;
use crate::auth::internal;
use crate::convert::{datetime_to_proto, id_from_db, id_to_db};
use crate::proto::thehideout::v1::{NotificationLevel, NotificationOverride, NotificationSettings};

/// Overrides one user may keep (guilds and channels together).
pub const MAX_OVERRIDES: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Overrides only: defer to the less specific setting.
    Inherit,
    All,
    Mentions,
    Nothing,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Inherit => "inherit",
            Level::All => "all",
            Level::Mentions => "mentions",
            Level::Nothing => "nothing",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "inherit" => Level::Inherit,
            "all" => Level::All,
            "mentions" => Level::Mentions,
            "nothing" => Level::Nothing,
            _ => return None,
        })
    }

    pub fn from_proto(level: NotificationLevel) -> Self {
        match level {
            NotificationLevel::NOTIFICATION_LEVEL_ALL => Level::All,
            NotificationLevel::NOTIFICATION_LEVEL_MENTIONS => Level::Mentions,
            NotificationLevel::NOTIFICATION_LEVEL_NOTHING => Level::Nothing,
            _ => Level::Inherit,
        }
    }

    pub fn to_proto(self) -> NotificationLevel {
        match self {
            Level::Inherit => NotificationLevel::NOTIFICATION_LEVEL_INHERIT,
            Level::All => NotificationLevel::NOTIFICATION_LEVEL_ALL,
            Level::Mentions => NotificationLevel::NOTIFICATION_LEVEL_MENTIONS,
            Level::Nothing => NotificationLevel::NOTIFICATION_LEVEL_NOTHING,
        }
    }
}

/// The setting of one guild or channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Override {
    pub level: Level,
    pub muted_until: Option<DateTime<Utc>>,
}

impl Override {
    fn muted(&self, now: DateTime<Utc>) -> bool {
        self.muted_until.is_some_and(|t| t > now)
    }

    /// An override with nothing in it is removed instead of stored.
    pub fn is_empty(&self) -> bool {
        self.level == Level::Inherit && self.muted_until.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub dms_enabled: bool,
    pub guild_default: Level,
    pub show_previews: bool,
    pub guilds: BTreeMap<u64, Override>,
    pub channels: BTreeMap<u64, Override>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { dms_enabled: true, guild_default: Level::Mentions, show_previews: true, guilds: BTreeMap::new(), channels: BTreeMap::new() }
    }
}

impl Settings {
    /// What a message in `channel_id` notifies about. The most specific
    /// setting wins: a muted channel is silent; a channel level beats the
    /// guild's; a muted guild is silent unless the channel has its own level;
    /// a guild level beats the global default. A DM channel falls back to
    /// `dms_enabled` (all messages or nothing).
    pub fn effective(&self, guild_id: Option<u64>, channel_id: u64, now: DateTime<Utc>) -> Level {
        if let Some(channel) = self.channels.get(&channel_id) {
            if channel.muted(now) {
                return Level::Nothing;
            }
            if channel.level != Level::Inherit {
                return channel.level;
            }
        }
        match guild_id {
            Some(guild_id) => {
                if let Some(guild) = self.guilds.get(&guild_id) {
                    if guild.muted(now) {
                        return Level::Nothing;
                    }
                    if guild.level != Level::Inherit {
                        return guild.level;
                    }
                }
                self.guild_default
            }
            None => if self.dms_enabled { Level::All } else { Level::Nothing },
        }
    }

    pub fn to_proto(&self) -> NotificationSettings {
        let list = |map: &BTreeMap<u64, Override>| {
            map.iter()
                .map(|(id, o)| NotificationOverride {
                    id: *id,
                    level: o.level.to_proto().into(),
                    muted_until: o.muted_until.map(datetime_to_proto).into(),
                    ..Default::default()
                })
                .collect::<Vec<_>>()
        };
        NotificationSettings {
            dms_enabled: self.dms_enabled,
            guild_default: self.guild_default.to_proto().into(),
            show_previews: self.show_previews,
            guilds: list(&self.guilds),
            channels: list(&self.channels),
            ..Default::default()
        }
    }
}

async fn rows<R>(state: &AppState, cql: &'static str, values: impl SerializeRow) -> Result<Vec<R>, ConnectError>
where
    R: for<'f, 'm> DeserializeRow<'f, 'm>,
{
    state
        .session
        .execute_unpaged(cql, values)
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .rows::<R>()
        .map_err(internal)?
        .collect::<Result<Vec<R>, _>>()
        .map_err(internal)
}

/// The user's settings; defaults for anything never set.
pub async fn load(state: &AppState, user_id: u64) -> Result<Settings, ConnectError> {
    let db_user = id_to_db(user_id);
    let mut settings = Settings::default();
    if let Some((dms, default, previews)) = rows::<(Option<bool>, Option<String>, Option<bool>)>(
        state,
        "SELECT dms_enabled, guild_default, show_previews FROM notification_settings WHERE user_id = ?",
        (db_user,),
    )
    .await?
    .into_iter()
    .next()
    {
        settings.dms_enabled = dms.unwrap_or(true);
        settings.show_previews = previews.unwrap_or(true);
        if let Some(level) = default.as_deref().and_then(Level::parse).filter(|l| *l != Level::Inherit) {
            settings.guild_default = level;
        }
    }
    for (scope, id, level, muted_until) in rows::<(String, i64, Option<String>, Option<DateTime<Utc>>)>(
        state,
        "SELECT scope, scope_id, level, muted_until FROM notification_overrides WHERE user_id = ?",
        (db_user,),
    )
    .await?
    {
        let o = Override { level: level.as_deref().and_then(Level::parse).unwrap_or(Level::Inherit), muted_until };
        match scope.as_str() {
            "guild" => settings.guilds.insert(id_from_db(id), o),
            "channel" => settings.channels.insert(id_from_db(id), o),
            _ => None,
        };
    }
    Ok(settings)
}

/// A change to the stored settings; unset fields stay as they are.
#[derive(Default)]
pub struct Update {
    pub dms_enabled: Option<bool>,
    pub guild_default: Option<Level>,
    pub show_previews: Option<bool>,
    pub guilds: Vec<(u64, Override)>,
    pub channels: Vec<(u64, Override)>,
}

/// Writes `update` and returns the resulting settings. `RESOURCE_EXHAUSTED`
/// when the user would keep more than [`MAX_OVERRIDES`] overrides.
pub async fn save(state: &AppState, user_id: u64, update: &Update) -> Result<Settings, ConnectError> {
    let db_user = id_to_db(user_id);
    let mut settings = load(state, user_id).await?;
    for (scope, id, o) in update.guilds.iter().map(|(i, o)| ("guild", *i, o)).chain(update.channels.iter().map(|(i, o)| ("channel", *i, o))) {
        let map = if scope == "guild" { &mut settings.guilds } else { &mut settings.channels };
        if o.is_empty() {
            map.remove(&id);
        } else {
            map.insert(id, *o);
        }
    }
    if settings.guilds.len() + settings.channels.len() > MAX_OVERRIDES {
        return Err(ConnectError::resource_exhausted("too many notification overrides; reset some first"));
    }
    let now = Utc::now();
    if let Some(v) = update.dms_enabled {
        settings.dms_enabled = v;
    }
    if let Some(v) = update.guild_default {
        settings.guild_default = v;
    }
    if let Some(v) = update.show_previews {
        settings.show_previews = v;
    }
    state
        .session
        .execute_unpaged(
            "INSERT INTO notification_settings (user_id, dms_enabled, guild_default, show_previews, updated_at) VALUES (?, ?, ?, ?, ?)",
            (db_user, settings.dms_enabled, settings.guild_default.as_str(), settings.show_previews, now),
        )
        .await
        .map_err(internal)?;
    for (scope, id, o) in update.guilds.iter().map(|(i, o)| ("guild", *i, o)).chain(update.channels.iter().map(|(i, o)| ("channel", *i, o))) {
        if o.is_empty() {
            state
                .session
                .execute_unpaged("DELETE FROM notification_overrides WHERE user_id = ? AND scope = ? AND scope_id = ?", (db_user, scope, id_to_db(id)))
                .await
                .map_err(internal)?;
        } else {
            state
                .session
                .execute_unpaged(
                    "INSERT INTO notification_overrides (user_id, scope, scope_id, level, muted_until) VALUES (?, ?, ?, ?, ?)",
                    (db_user, scope, id_to_db(id), o.level.as_str(), o.muted_until),
                )
                .await
                .map_err(internal)?;
        }
    }
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    const G: u64 = 10;
    const C: u64 = 20;

    fn o(level: Level) -> Override {
        Override { level, muted_until: None }
    }

    fn muted(level: Level, ahead: i64) -> Override {
        Override { level, muted_until: Some(Utc::now() + Duration::seconds(ahead)) }
    }

    #[test]
    fn defaults_are_dms_on_guilds_mentions_only() {
        let s = Settings::default();
        assert_eq!(s.effective(Some(G), C, Utc::now()), Level::Mentions);
        assert_eq!(s.effective(None, C, Utc::now()), Level::All);
        assert_eq!(Settings { dms_enabled: false, ..Default::default() }.effective(None, C, Utc::now()), Level::Nothing);
    }

    #[test]
    fn channel_beats_guild_beats_global() {
        let mut s = Settings { guild_default: Level::Nothing, ..Default::default() };
        assert_eq!(s.effective(Some(G), C, Utc::now()), Level::Nothing);
        s.guilds.insert(G, o(Level::Mentions));
        assert_eq!(s.effective(Some(G), C, Utc::now()), Level::Mentions);
        s.channels.insert(C, o(Level::All));
        assert_eq!(s.effective(Some(G), C, Utc::now()), Level::All);
        s.channels.insert(C, o(Level::Inherit));
        assert_eq!(s.effective(Some(G), C, Utc::now()), Level::Mentions);
        // A channel in another guild is untouched.
        assert_eq!(s.effective(Some(G + 1), C + 1, Utc::now()), Level::Nothing);
    }

    #[test]
    fn mutes_silence_until_they_lapse() {
        let now = Utc::now();
        let mut s = Settings { guild_default: Level::All, ..Default::default() };
        s.guilds.insert(G, muted(Level::Inherit, 3600));
        assert_eq!(s.effective(Some(G), C, now), Level::Nothing);
        // A channel with its own level punches through a guild mute...
        s.channels.insert(C, o(Level::All));
        assert_eq!(s.effective(Some(G), C, now), Level::All);
        // ...but a muted channel stays silent whatever its level.
        s.channels.insert(C, muted(Level::All, 3600));
        assert_eq!(s.effective(Some(G), C, now), Level::Nothing);
        // Lapsed mutes are ignored.
        s.channels.insert(C, muted(Level::Inherit, -5));
        s.guilds.insert(G, muted(Level::Inherit, -5));
        assert_eq!(s.effective(Some(G), C, now), Level::All);
    }

    #[test]
    fn dm_channel_override_wins_over_dms_enabled() {
        let mut s = Settings { dms_enabled: false, ..Default::default() };
        s.channels.insert(C, o(Level::All));
        assert_eq!(s.effective(None, C, Utc::now()), Level::All);
        s.channels.insert(C, muted(Level::All, 60));
        assert_eq!(s.effective(None, C, Utc::now()), Level::Nothing);
    }

    #[test]
    fn levels_round_trip_through_text_and_proto() {
        for l in [Level::Inherit, Level::All, Level::Mentions, Level::Nothing] {
            assert_eq!(Level::parse(l.as_str()), Some(l));
            assert_eq!(Level::from_proto(l.to_proto()), l);
        }
        assert_eq!(Level::parse("loud"), None);
        assert_eq!(Level::from_proto(NotificationLevel::NOTIFICATION_LEVEL_UNSPECIFIED), Level::Inherit);
    }
}
