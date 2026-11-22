//! Removal and cleanup behind the moderation and structure RPCs: taking a
//! member out of a guild, deleting messages with everything that hangs off
//! them, and the background purges of deleted channels and guilds.
//!
//! Deletion model (Scylla has no cascading deletes):
//! - What makes something unreachable is deleted synchronously (the message
//!   row, the `channels` / `channels_by_id` rows, the `guilds` and membership
//!   rows), so reads fail at once.
//! - What hangs off it (reactions, embeds, attachment rows, stored objects, a
//!   channel's message partitions, a guild's roles and emojis) is removed
//!   afterwards, in the background for the big ones. Everything is idempotent
//!   and best effort: a failure is logged and leaves unreachable rows or
//!   objects behind (their accounting rows remain, so the admin file list can
//!   still find and delete the files). Nothing re-runs a purge after a crash.

use std::time::{SystemTime, UNIX_EPOCH};

use connectrpc::ConnectError;
use scylla::statement::batch::{Batch, BatchType};

use crate::AppState;
use crate::access::is_member;
use crate::auth::internal;
use crate::convert::{id_from_db, id_to_db};
use crate::events::EventPayload;
use crate::media::{self, files};
use crate::proto::thehideout::v1::{ChannelDeleted, GuildMemberRemoved, MemberRemovalReason, MessageDeleted};
use crate::snowflake::{self, bucket_of, current_bucket};

/// Longest span of a ban's message purge.
pub const MAX_DELETE_MESSAGE_SECONDS: u32 = 7 * 24 * 60 * 60;
/// Longest ban reason, in characters.
pub const MAX_REASON_CHARS: usize = 512;
/// How long a send's nonce is remembered for deduplication.
pub const NONCE_TTL_SECS: i32 = 60;

/// Whether `user_id` is banned from `guild_id`.
pub async fn is_banned(state: &AppState, guild_id: u64, user_id: u64) -> Result<bool, ConnectError> {
    Ok(state
        .session
        .execute_unpaged(
            "SELECT user_id FROM guild_bans WHERE guild_id = ? AND user_id = ?",
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

/// Audit entry for a moderation action. A failed write is logged, not
/// returned: the action itself has already happened.
pub async fn audit(state: &AppState, actor: u64, action: &str, target: &str, reason: &str) {
    if let Err(e) = crate::admin::audit(state, actor, action, target, reason).await {
        tracing::warn!(action, target, error = %e.message.as_deref().unwrap_or("?"), "writing the audit entry failed");
    }
}

/// Removes `user_id` from the guild: membership and roles go, the user leaves
/// voice, the other members hear `guild_member_removed` and the user's open
/// streams stop following the guild (`guild_removed`). Returns whether they
/// were a member.
pub async fn remove_member(
    state: &AppState,
    guild_id: u64,
    user_id: u64,
    reason: MemberRemovalReason,
) -> Result<bool, ConnectError> {
    if !is_member(state, user_id, guild_id).await? {
        return Ok(false);
    }
    let (guild, user) = (id_to_db(guild_id), id_to_db(user_id));
    let mut batch = Batch::new(BatchType::Logged);
    batch.append_statement("DELETE FROM guild_members WHERE guild_id = ? AND user_id = ?");
    batch.append_statement("DELETE FROM guilds_by_user WHERE user_id = ? AND guild_id = ?");
    batch.append_statement("DELETE FROM member_roles WHERE guild_id = ? AND user_id = ?");
    state
        .session
        .batch(&batch, ((guild, user), (user, guild), (guild, user)))
        .await
        .map_err(internal)?;
    crate::stats::member_removed(state, guild_id).await;
    crate::voice::on_member_removed(state, guild_id, user_id).await;
    state.events.publish_to_guild(
        guild_id,
        EventPayload::GuildMemberRemoved(GuildMemberRemoved {
            guild_id,
            user_id,
            reason: reason.into(),
            ..Default::default()
        }),
    );
    state.events.notify_user_removed_from_guild(user_id, guild_id);
    tracing::info!(guild_id, user_id, ?reason, "member removed from guild");
    Ok(true)
}

/// Deletes one stored object and its accounting. Best effort.
pub async fn delete_object(state: &AppState, key: &str) {
    if let Some(storage) = &state.storage
        && let Err(e) = storage.delete(key).await
    {
        tracing::warn!(key, error = %format!("{e:#}"), "deleting object failed");
    }
    media::delete_record(state, key).await;
}

async fn delete_objects(state: &AppState, keys: Vec<String>) {
    for key in keys.iter().filter(|k| !k.is_empty()) {
        delete_object(state, key).await;
    }
}

/// A table whose rows hang off messages, keyed `((channel_id, bucket), message_id, ..)`.
struct Hanging {
    table: &'static str,
    /// Columns holding media keys, if any.
    keys: Option<(&'static str, &'static str)>,
}

const REACTIONS: Hanging = Hanging { table: "reactions", keys: None };
const ATTACHMENTS: Hanging = Hanging { table: "message_attachments", keys: Some(("file", "thumbnail")) };
const EMBEDS: Hanging = Hanging { table: "message_embeds", keys: Some(("thumbnail", "author_icon")) };
const EMBED_MEDIA: Hanging = Hanging { table: "message_embed_media", keys: Some(("file", "poster")) };

/// Deletes the rows of `tables` for one message (`Some`) or a whole partition
/// (`None`) and then the objects they named.
async fn purge_tables(
    state: &AppState,
    channel_id: u64,
    bucket: i32,
    message_id: Option<u64>,
    tables: &[&Hanging],
) -> Result<(), ConnectError> {
    let channel = id_to_db(channel_id);
    let scope = if message_id.is_some() { " AND message_id = ?" } else { "" };
    let mut keys: Vec<String> = Vec::new();
    for t in tables {
        if let Some((a, b)) = t.keys {
            let query = format!("SELECT {a}, {b} FROM {} WHERE channel_id = ? AND bucket = ?{scope}", t.table);
            let result = match message_id {
                Some(m) => state.session.execute_unpaged(query, (channel, bucket, id_to_db(m))).await,
                None => state.session.execute_unpaged(query, (channel, bucket)).await,
            }
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?;
            for (x, y) in result.rows::<(Option<String>, Option<String>)>().map_err(internal)?.flatten() {
                keys.extend(x.into_iter().chain(y));
            }
        }
        let delete = format!("DELETE FROM {} WHERE channel_id = ? AND bucket = ?{scope}", t.table);
        match message_id {
            Some(m) => state.session.execute_unpaged(delete, (channel, bucket, id_to_db(m))).await,
            None => state.session.execute_unpaged(delete, (channel, bucket)).await,
        }
        .map_err(internal)?;
    }
    delete_objects(state, keys).await;
    Ok(())
}

/// Deletes a message and everything that hangs off it: reactions, embeds
/// (rows and stored files) and attachments (rows and stored files).
pub async fn purge_message(state: &AppState, channel_id: u64, message_id: u64) -> Result<(), ConnectError> {
    state
        .session
        .execute_unpaged(
            "DELETE FROM messages WHERE channel_id = ? AND bucket = ? AND id = ?",
            (id_to_db(channel_id), bucket_of(message_id), id_to_db(message_id)),
        )
        .await
        .map_err(internal)?;
    purge_tables(state, channel_id, bucket_of(message_id), Some(message_id), &[&REACTIONS, &ATTACHMENTS, &EMBEDS, &EMBED_MEDIA]).await
}

/// Deletes only the embeds of a message (rows and stored files), as before
/// they are resolved again after an edit.
pub async fn purge_embeds(state: &AppState, channel_id: u64, message_id: u64) -> Result<(), ConnectError> {
    purge_tables(state, channel_id, bucket_of(message_id), Some(message_id), &[&EMBEDS, &EMBED_MEDIA]).await
}

/// Deletes every message of a channel with all that hangs off them.
pub async fn purge_channel(state: &AppState, channel_id: u64) {
    for bucket in bucket_of(channel_id)..=current_bucket() {
        let result = async {
            purge_tables(state, channel_id, bucket, None, &[&REACTIONS, &ATTACHMENTS, &EMBEDS, &EMBED_MEDIA]).await?;
            state
                .session
                .execute_unpaged("DELETE FROM messages WHERE channel_id = ? AND bucket = ?", (id_to_db(channel_id), bucket))
                .await
                .map_err(internal)?;
            Ok::<_, ConnectError>(())
        }
        .await;
        if let Err(e) = result {
            tracing::warn!(channel_id, bucket, error = %e.message.as_deref().unwrap_or("?"), "purging channel messages failed");
        }
    }
}

/// Publishes `message_deleted` to the audience of the channel (`guild_id` 0 = DM).
pub fn publish_message_deleted(state: &AppState, guild_id: u64, channel_id: u64, message_id: u64) {
    let event = EventPayload::MessageDeleted(MessageDeleted { channel_id, message_id, ..Default::default() });
    if guild_id == 0 {
        state.events.publish_to_dm(channel_id, event);
    } else {
        state.events.publish_to_guild(guild_id, event);
    }
}

/// Deletes the messages `user_id` wrote in `channel_ids` during the last
/// `seconds`, publishing `message_deleted` for each. Background work of a ban.
pub async fn purge_author_messages(state: &AppState, guild_id: u64, channel_ids: &[u64], user_id: u64, seconds: u32) {
    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let since_ms = now_ms.saturating_sub(snowflake::EPOCH_MS).saturating_sub(u64::from(seconds) * 1000);
    let since_id = snowflake::make_id(since_ms, 0, 0);
    let mut deleted = 0usize;
    for &channel_id in channel_ids {
        for bucket in bucket_of(since_id).max(bucket_of(channel_id))..=current_bucket() {
            let found = async {
                state
                    .session
                    .execute_unpaged(
                        "SELECT id, author_id FROM messages WHERE channel_id = ? AND bucket = ? AND id >= ?",
                        (id_to_db(channel_id), bucket, id_to_db(since_id)),
                    )
                    .await
                    .map_err(internal)?
                    .into_rows_result()
                    .map_err(internal)?
                    .rows::<(i64, i64)>()
                    .map_err(internal)?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(internal)
            }
            .await;
            let rows = match found {
                Ok(rows) => rows,
                Err(e) => {
                    tracing::warn!(channel_id, bucket, error = %e.message.as_deref().unwrap_or("?"), "listing messages to purge failed");
                    continue;
                }
            };
            for (id, author) in rows {
                if id_from_db(author) != user_id {
                    continue;
                }
                let id = id_from_db(id);
                match purge_message(state, channel_id, id).await {
                    Ok(()) => {
                        deleted += 1;
                        publish_message_deleted(state, guild_id, channel_id, id);
                    }
                    Err(e) => tracing::warn!(channel_id, message_id = id, error = %e.message.as_deref().unwrap_or("?"), "purging a message failed"),
                }
            }
        }
    }
    tracing::info!(guild_id, user_id, deleted, "ban message purge finished");
}

/// Removes what is left of a deleted guild: its channels' messages, sections,
/// roles, emojis, bans, invites, counters and every stored file counted
/// against it. The guild, membership and channel-lookup rows are already gone.
pub async fn purge_deleted_guild(state: &AppState, guild_id: u64, channel_ids: Vec<u64>) {
    let guild = id_to_db(guild_id);
    // The index lists every object of the guild (attachments, embeds, emojis, icon).
    let scope = format!("guild:{guild_id}");
    let mut after: Option<files::Cursor> = None;
    loop {
        let page = match files::page(state, &scope, after.as_ref(), 100).await {
            Ok(page) => page,
            Err(e) => {
                tracing::warn!(guild_id, error = %e.message.as_deref().unwrap_or("?"), "listing the guild's files failed");
                break;
            }
        };
        let Some(last) = page.last().cloned() else { break };
        for (_, key) in &page {
            delete_object(state, key).await;
        }
        after = Some(last);
    }
    for channel_id in channel_ids {
        purge_channel(state, channel_id).await;
    }
    for cql in [
        "DELETE FROM channels WHERE guild_id = ?",
        "DELETE FROM sections WHERE guild_id = ?",
        "DELETE FROM roles WHERE guild_id = ?",
        "DELETE FROM member_roles WHERE guild_id = ?",
        "DELETE FROM guild_versions WHERE guild_id = ?",
        "DELETE FROM emojis WHERE guild_id = ?",
        "DELETE FROM emoji_names WHERE guild_id = ?",
        "DELETE FROM guild_bans WHERE guild_id = ?",
        "DELETE FROM guild_channel_quota WHERE guild_id = ?",
    ] {
        if let Err(e) = state.session.execute_unpaged(cql, (guild,)).await {
            tracing::warn!(guild_id, cql, error = %e, "deleting guild rows failed");
        }
    }
    for cql in ["DELETE FROM invites_by_scope WHERE scope = ?", "DELETE FROM counters WHERE scope = ?"] {
        let scope = if cql.contains("counters") { format!("guild:{guild_id}") } else { scope.clone() };
        if let Err(e) = state.session.execute_unpaged(cql, (scope,)).await {
            tracing::warn!(guild_id, cql, error = %e, "deleting guild rows failed");
        }
    }
    tracing::info!(guild_id, "guild purge finished");
}

/// Publishes `channel_deleted` to the guild.
pub fn publish_channel_deleted(state: &AppState, guild_id: u64, channel_id: u64) {
    state
        .events
        .publish_to_guild(guild_id, EventPayload::ChannelDeleted(ChannelDeleted { guild_id, channel_id, ..Default::default() }));
}
