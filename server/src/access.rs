//! Membership and access checks shared by the guild, message and DM services.

use chrono::{DateTime, Utc};
use connectrpc::ConnectError;

use crate::AppState;
use crate::auth::internal;
use crate::convert::{id_from_db, id_to_db, user_to_proto};
use crate::proto::thehideout::v1::User;

/// Succeeds if `user_id` belongs to `guild_id`; otherwise `NotFound` so a guild's existence is not leaked.
pub async fn require_guild_member(
    state: &AppState,
    user_id: u64,
    guild_id: u64,
) -> Result<(), ConnectError> {
    if is_member(state, user_id, guild_id).await? {
        Ok(())
    } else {
        Err(ConnectError::not_found("guild not found"))
    }
}

/// How the caller may access a channel: as a member of its guild, or as a
/// participant of the direct-message channel.
pub enum ChannelAccess {
    /// A guild channel; carries the guild the caller is a member of.
    Guild(u64),
    /// A DM channel between the caller and one other user.
    Dm,
}

/// Resolves `channel_id` (guild channel or DM channel) and checks `user_id` may
/// access it. Anything else is `NotFound` so a channel's kind and existence are
/// not leaked to outsiders.
pub async fn require_channel_access(
    state: &AppState,
    user_id: u64,
    channel_id: u64,
) -> Result<ChannelAccess, ConnectError> {
    let not_found = || ConnectError::not_found("channel not found");
    if let Some((guild_id,)) = state
        .session
        .execute_unpaged("SELECT guild_id FROM channels_by_id WHERE id = ?", (id_to_db(channel_id),))
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .maybe_first_row::<(i64,)>()
        .map_err(internal)?
    {
        if is_member(state, user_id, id_from_db(guild_id)).await? {
            return Ok(ChannelAccess::Guild(id_from_db(guild_id)));
        }
        return Err(not_found());
    }
    if let Some((user_a, user_b)) = state
        .session
        .execute_unpaged(
            "SELECT user_a, user_b FROM dm_channels WHERE id = ?",
            (id_to_db(channel_id),),
        )
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .maybe_first_row::<(i64, i64)>()
        .map_err(internal)?
    {
        let caller = id_to_db(user_id);
        if caller == user_a || caller == user_b {
            return Ok(ChannelAccess::Dm);
        }
        return Err(not_found());
    }
    Err(not_found())
}

pub(crate) async fn is_member(
    state: &AppState,
    user_id: u64,
    guild_id: u64,
) -> Result<bool, ConnectError> {
    Ok(state
        .session
        .execute_unpaged(
            "SELECT user_id FROM guild_members WHERE guild_id = ? AND user_id = ?",
            (id_to_db(guild_id), id_to_db(user_id)),
        )
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .maybe_first_row::<(i64,)>()
        .map_err(internal)?
        .is_some())
}

/// Loads a user row by ID for proto conversion.
pub async fn find_user(state: &AppState, db_id: i64) -> Result<Option<User>, ConnectError> {
    Ok(state
        .session
        .execute_unpaged("SELECT username, created_at, avatar FROM users WHERE id = ?", (db_id,))
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .maybe_first_row::<(String, DateTime<Utc>, Option<String>)>()
        .map_err(internal)?
        .map(|(username, created_at, avatar)| user_to_proto(id_from_db(db_id), &username, created_at, avatar)))
}

/// Kind of a guild channel (text for channels stored before voice existed).
pub async fn channel_kind(
    state: &AppState,
    guild_id: u64,
    channel_id: u64,
) -> Result<crate::proto::thehideout::v1::ChannelKind, ConnectError> {
    let row = state
        .session
        .execute_unpaged(
            "SELECT kind FROM channels WHERE guild_id = ? AND id = ?",
            (id_to_db(guild_id), id_to_db(channel_id)),
        )
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .maybe_first_row::<(Option<String>,)>()
        .map_err(internal)?;
    Ok(crate::layout::kind_from_column(row.and_then(|(k,)| k).as_deref()))
}
