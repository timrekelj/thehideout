//! The account lifecycle: anonymizing deletion (own and by an admin),
//! suspension, data export and the background sweeps that keep them honest.
//!
//! **Deletion** keeps the user's messages (shown as "Deleted user") and removes
//! everything that identifies them or belongs to them. Because Scylla has no
//! transactions the work is a list of idempotent [`Step`]s run in the
//! background:
//!
//! 1. [`begin_deletion`] makes the account unusable at once (`deletion_state =
//!    'deleting'`, sessions revoked, an `account_deletions` row) and returns.
//! 2. [`run_deletion`] runs the steps in order. A crash leaves the
//!    `account_deletions` row behind and [`resume_pending`] (at startup and every
//!    few minutes) runs the steps again; every step can be repeated safely.
//! 3. The last step turns the user row into a tombstone (`username = "Deleted
//!    user"`, no credentials) and removes the pending row.
//!
//! Messages are found by scanning the channels the user could have written to
//! (their guilds, guilds they left since this feature exists, their DMs),
//! starting at the time the account was created.

use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use connectrpc::ConnectError;
use scylla::deserialize::row::DeserializeRow;
use scylla::serialize::row::SerializeRow;

use crate::AppState;
use crate::accounts::{self, Account};
use crate::auth::{generate_token, hash_token, internal, lwt_applied};
use crate::convert::{datetime_to_proto, id_from_db, id_to_db, snowflake_datetime};
use crate::events::EventPayload;
use crate::proto::thehideout::v1::{
    Guild, MemberRemovalReason, OwnedGuild, OwnedGuilds, ReactionChange, User,
};
use crate::snowflake::{bucket_of, current_bucket};

/// The name tombstoned accounts carry. Not a valid username (capital letter
/// and a space), so it can never collide with a real one.
pub const DELETED_USERNAME: &str = "Deleted user";
/// How long a data export link works.
pub const EXPORT_TTL: chrono::Duration = chrono::Duration::days(7);
/// One export per user per this many seconds.
pub const EXPORT_COOLDOWN_SECS: i32 = 24 * 60 * 60;
/// Pause between background sweeps.
const SWEEP_EVERY: Duration = Duration::from_secs(10 * 60);

/// The anonymized user other people see.
pub fn deleted_user(id: u64, created_at: DateTime<Utc>) -> User {
    User {
        id,
        username: DELETED_USERNAME.to_owned(),
        created_at: datetime_to_proto(created_at).into(),
        deleted: true,
        ..Default::default()
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

async fn exec(state: &AppState, cql: &'static str, values: impl SerializeRow) -> Result<(), ConnectError> {
    state.session.execute_unpaged(cql, values).await.map_err(internal)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Ownership

/// The guilds `user_id` owns, with their member counts.
pub async fn owned_guilds(state: &AppState, user_id: u64) -> Result<Vec<OwnedGuild>, ConnectError> {
    let mut out = Vec::new();
    for (guild_id,) in rows::<(i64,)>(state, "SELECT guild_id FROM guilds_by_user WHERE user_id = ?", (id_to_db(user_id),)).await? {
        let Some(guild) = crate::services::guild::load_guild(state, guild_id).await? else { continue };
        if guild.owner_id != user_id {
            continue;
        }
        let (members,) = state
            .session
            .execute_unpaged("SELECT COUNT(*) FROM guild_members WHERE guild_id = ?", (guild_id,))
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .first_row::<(i64,)>()
            .map_err(internal)?;
        out.push(OwnedGuild { id: guild.id, name: guild.name, member_count: members.max(0) as u32, ..Default::default() });
    }
    Ok(out)
}

/// FAILED_PRECONDITION listing the guilds that block a deletion.
pub fn owned_guilds_error(guilds: Vec<OwnedGuild>) -> ConnectError {
    let names: Vec<&str> = guilds.iter().map(|g| g.name.as_str()).collect();
    let message = format!("transfer or delete the servers you own first: {}", names.join(", "));
    ConnectError::failed_precondition(message)
        .with_detail(connectrpc::ErrorDetail::from_message("thehideout.v1.OwnedGuilds", &OwnedGuilds { guilds, ..Default::default() }))
}

/// Passes each owned guild to its member with the highest role (ties: the
/// longest member), or deletes it when the user is alone in it.
pub async fn hand_over_guilds(state: &AppState, actor: u64, user_id: u64) -> Result<(), ConnectError> {
    for owned in owned_guilds(state, user_id).await? {
        let members: Vec<(i64, Option<DateTime<Utc>>)> = rows(
            state,
            "SELECT user_id, joined_at FROM guild_members WHERE guild_id = ?",
            (id_to_db(owned.id),),
        )
        .await?;
        let mut best: Option<(i32, std::cmp::Reverse<DateTime<Utc>>, u64)> = None;
        for (member, joined_at) in members {
            let member = id_from_db(member);
            if member == user_id || accounts::load(state, member).await?.is_none() {
                continue;
            }
            let standing = crate::permissions::standing(state, member, owned.id).await?;
            let rank = (standing.top_position.unwrap_or(-1), std::cmp::Reverse(joined_at.unwrap_or_default()), member);
            if best.as_ref().is_none_or(|b| rank > *b) {
                best = Some(rank);
            }
        }
        let Some(guild) = crate::services::guild::load_guild(state, id_to_db(owned.id)).await? else { continue };
        match best {
            None => {
                crate::services::guild::GuildSvc::new(state.clone()).delete_guild_now(actor, &guild).await?;
            }
            Some((_, _, successor)) => transfer(state, actor, &guild, user_id, successor).await?,
        }
    }
    Ok(())
}

async fn transfer(state: &AppState, actor: u64, guild: &Guild, from: u64, to: u64) -> Result<(), ConnectError> {
    let applied = lwt_applied(
        state
            .session
            .execute_unpaged(
                "UPDATE guilds SET owner_id = ? WHERE id = ? IF owner_id = ?",
                (id_to_db(to), id_to_db(guild.id), id_to_db(from)),
            )
            .await
            .map_err(internal)?,
    )?;
    if !applied {
        return Err(ConnectError::aborted("ownership changed: try again"));
    }
    if let Some(updated) = crate::services::guild::load_guild(state, id_to_db(guild.id)).await? {
        state.events.publish_to_guild(guild.id, EventPayload::GuildUpdated(updated));
    }
    crate::moderation::audit(state, actor, "guild.transfer", &format!("guild:{}/user:{to}", guild.id), "account removal").await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Deletion

/// Everyone who shares a guild or a DM with `user_id`, and `user_id`.
async fn audience(state: &AppState, user_id: u64) -> Result<Vec<u64>, ConnectError> {
    let db_id = id_to_db(user_id);
    let mut ids = std::collections::HashSet::from([user_id]);
    for (guild_id,) in rows::<(i64,)>(state, "SELECT guild_id FROM guilds_by_user WHERE user_id = ?", (db_id,)).await? {
        for (member,) in rows::<(i64,)>(state, "SELECT user_id FROM guild_members WHERE guild_id = ?", (guild_id,)).await? {
            ids.insert(id_from_db(member));
        }
    }
    ids.extend(dm_partners(state, user_id).await?.into_iter().map(|(_, partner)| partner));
    Ok(ids.into_iter().collect())
}

/// `(dm_id, other user)` of the user's DMs.
async fn dm_partners(state: &AppState, user_id: u64) -> Result<Vec<(i64, u64)>, ConnectError> {
    let mut out = Vec::new();
    for (dm_id,) in rows::<(i64,)>(state, "SELECT dm_id FROM dms_by_user WHERE user_id = ?", (id_to_db(user_id),)).await? {
        let pair = rows::<(i64, i64)>(state, "SELECT user_a, user_b FROM dm_channels WHERE id = ?", (dm_id,)).await?;
        if let Some((a, b)) = pair.into_iter().next() {
            let other = if id_from_db(a) == user_id { b } else { a };
            out.push((dm_id, id_from_db(other)));
        }
    }
    Ok(out)
}

fn tell_audience(state: &AppState, audience: &[u64], user: &User) {
    for id in audience {
        state.events.notify_user_profile_updated(*id, user.clone());
    }
}

/// Makes the account unusable and queues the cleanup: marks it `deleting`,
/// revokes every session, records it for the resume sweep, sends the farewell
/// email, audit-logs it and tells everyone who shares a guild or DM. Does not
/// start the job (see [`delete_account`]).
pub async fn begin_deletion(state: &AppState, account: &Account, actor: u64, reason: &str) -> Result<(), ConnectError> {
    let db_id = id_to_db(account.id);
    exec(
        state,
        "UPDATE users SET deletion_state = 'deleting', deleted_at = ? WHERE id = ?",
        (Utc::now(), db_id),
    )
    .await?;
    exec(state, "INSERT INTO account_deletions (kind, user_id) VALUES ('pending', ?)", (db_id,)).await?;
    crate::auth::revoke_all_sessions(state, account.id, None).await?;
    crate::voice::leave(state, account.id).await;
    crate::admin::audit(state, actor, "account.delete", &format!("user:{}", account.id), reason).await?;
    if let Some(email) = &account.email {
        state
            .mailer
            .send(crate::email::templates::account_deleted(email, format!("account-deleted-{}", generate_token())));
    }
    let people = audience(state, account.id).await?;
    tell_audience(state, &people, &deleted_user(account.id, account.created_at));
    tracing::info!(user_id = account.id, "account deletion started");
    Ok(())
}

/// [`begin_deletion`] and the background cleanup.
pub async fn delete_account(state: &AppState, account: &Account, actor: u64, reason: &str) -> Result<(), ConnectError> {
    begin_deletion(state, account, actor, reason).await?;
    let (state, user_id) = (state.clone(), account.id);
    tokio::spawn(async move { run_deletion(&state, user_id).await });
    Ok(())
}

/// The stages of the cleanup, in order. Each is idempotent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Frees the username and email address.
    ReleaseNames,
    /// Removes attachments of their messages (the text stays), their reactions and pending uploads.
    RemoveContent,
    /// Leaves every guild (which also clears voice).
    LeaveGuilds,
    /// Drops DM membership; the partners keep the conversation.
    DetachDms,
    /// Withdraws invites they sent that nobody used yet.
    WithdrawInvites,
    /// Deletes passkeys, recovery codes, sessions, avatar, exports and admin rights.
    RemoveCredentials,
    /// Turns the user row into a tombstone and clears the pending marker.
    Finalize,
}

pub const STEPS: [Step; 7] = [
    Step::ReleaseNames,
    Step::RemoveContent,
    Step::LeaveGuilds,
    Step::DetachDms,
    Step::WithdrawInvites,
    Step::RemoveCredentials,
    Step::Finalize,
];

/// Runs the first `count` steps (all of them for `STEPS.len()`); tests use it
/// to stop half-way.
pub async fn run_steps(state: &AppState, user_id: u64, count: usize) -> Result<(), ConnectError> {
    for step in STEPS.into_iter().take(count) {
        run_step(state, user_id, step).await?;
    }
    Ok(())
}

/// Runs every step; a failure is logged and leaves the account pending for the next sweep.
pub async fn run_deletion(state: &AppState, user_id: u64) {
    match run_steps(state, user_id, STEPS.len()).await {
        Ok(()) => tracing::info!(user_id, "account deletion finished"),
        Err(e) => tracing::warn!(user_id, error = %e.message.as_deref().unwrap_or("?"), "account deletion stopped; it resumes at the next sweep"),
    }
}

/// Runs the cleanup of every account still marked pending; returns how many.
pub async fn resume_pending(state: &AppState) -> Result<usize, ConnectError> {
    let pending = rows::<(i64,)>(state, "SELECT user_id FROM account_deletions WHERE kind = 'pending'", ()).await?;
    let count = pending.len();
    for (id,) in pending {
        run_deletion(state, id_from_db(id)).await;
    }
    Ok(count)
}

pub async fn run_step(state: &AppState, user_id: u64, step: Step) -> Result<(), ConnectError> {
    let Some(account) = accounts::load_any(state, user_id).await? else {
        return Ok(());
    };
    if account.deletion_state.is_none() {
        // Never run the cleanup on an account that was not marked.
        return Err(ConnectError::failed_precondition("account is not being deleted"));
    }
    if account.deletion_state.as_deref() == Some("deleted") {
        return exec(state, "DELETE FROM account_deletions WHERE kind = 'pending' AND user_id = ?", (id_to_db(user_id),)).await;
    }
    match step {
        Step::ReleaseNames => release_names(state, &account).await,
        Step::RemoveContent => remove_content(state, &account).await,
        Step::LeaveGuilds => leave_guilds(state, &account).await,
        Step::DetachDms => detach_dms(state, &account).await,
        Step::WithdrawInvites => crate::services::invite::withdraw_by_inviter(state, user_id).await,
        Step::RemoveCredentials => remove_credentials(state, &account).await,
        Step::Finalize => finalize(state, &account).await,
    }
}

async fn release_names(state: &AppState, account: &Account) -> Result<(), ConnectError> {
    let db_id = id_to_db(account.id);
    exec(state, "DELETE FROM users_by_username WHERE username = ? IF user_id = ?", (&account.username, db_id)).await?;
    if let Some(email) = &account.email {
        accounts::release_email(state, email, account.id).await?;
        // Codes waiting for this address (sign-in, reset) and for an email change.
        for purpose in ["login", "reset"] {
            exec(state, "DELETE FROM email_codes WHERE purpose = ? AND subject = ?", (purpose, email)).await?;
        }
    }
    for purpose in ["email_change", "delete"] {
        exec(state, "DELETE FROM email_codes WHERE purpose = ? AND subject = ?", (purpose, account.id.to_string())).await?;
    }
    Ok(())
}

/// A channel the user may have written in.
#[derive(Debug, Clone)]
pub struct ChannelRef {
    pub id: u64,
    /// 0 for a DM channel.
    pub guild_id: u64,
    pub name: String,
    pub guild_name: String,
}

/// Channels to scan for the user's content: guilds they are in or left since
/// this feature exists, and their DMs.
pub async fn candidate_channels(state: &AppState, user_id: u64) -> Result<Vec<ChannelRef>, ConnectError> {
    let db_id = id_to_db(user_id);
    let mut guild_ids: Vec<i64> = rows::<(i64,)>(state, "SELECT guild_id FROM guilds_by_user WHERE user_id = ?", (db_id,))
        .await?
        .into_iter()
        .map(|(g,)| g)
        .collect();
    guild_ids.extend(rows::<(i64,)>(state, "SELECT guild_id FROM user_left_guilds WHERE user_id = ?", (db_id,)).await?.into_iter().map(|(g,)| g));
    guild_ids.sort_unstable();
    guild_ids.dedup();
    let mut out = Vec::new();
    for guild_id in guild_ids {
        let Some(guild) = crate::services::guild::load_guild(state, guild_id).await? else { continue };
        for channel in crate::layout::load(state, guild.id).await?.channels {
            out.push(ChannelRef { id: channel.id, guild_id: guild.id, name: channel.name, guild_name: guild.name.clone() });
        }
    }
    for (dm_id, _) in dm_partners(state, user_id).await? {
        out.push(ChannelRef { id: id_from_db(dm_id), guild_id: 0, name: String::new(), guild_name: String::new() });
    }
    Ok(out)
}

/// A message the user wrote.
pub struct OwnMessage {
    pub id: u64,
    pub guild_id: u64,
    pub content: String,
    pub edited_at: Option<DateTime<Utc>>,
}

fn buckets(user_id: u64, channel_id: u64) -> std::ops::RangeInclusive<i32> {
    bucket_of(user_id).max(bucket_of(channel_id))..=current_bucket()
}

/// The user's messages in a channel, oldest first.
pub async fn own_messages(state: &AppState, user_id: u64, channel_id: u64) -> Result<Vec<OwnMessage>, ConnectError> {
    let mut out = Vec::new();
    for bucket in buckets(user_id, channel_id) {
        let found = rows::<(i64, i64, i64, Option<String>, Option<DateTime<Utc>>)>(
            state,
            "SELECT id, guild_id, author_id, content, edited_at FROM messages WHERE channel_id = ? AND bucket = ? AND id >= ?",
            (id_to_db(channel_id), bucket, id_to_db(user_id)),
        )
        .await?;
        out.extend(found.into_iter().filter(|(_, _, author, _, _)| id_from_db(*author) == user_id).map(
            |(id, guild_id, _, content, edited_at)| OwnMessage {
                id: id_from_db(id),
                guild_id: id_from_db(guild_id),
                content: content.unwrap_or_default(),
                edited_at,
            },
        ));
    }
    out.sort_by_key(|m| m.id);
    Ok(out)
}

/// `(message_id, emoji_key, at)` of the user's reactions in a channel.
pub async fn own_reactions(state: &AppState, user_id: u64, channel_id: u64) -> Result<Vec<(u64, String, i64)>, ConnectError> {
    let mut out = Vec::new();
    for bucket in buckets(user_id, channel_id) {
        let found = rows::<(i64, String, Option<i64>)>(
            state,
            "SELECT message_id, emoji_key, at FROM reactions WHERE channel_id = ? AND bucket = ? AND user_id = ? ALLOW FILTERING",
            (id_to_db(channel_id), bucket, id_to_db(user_id)),
        )
        .await?;
        out.extend(found.into_iter().map(|(m, key, at)| (id_from_db(m), key, at.unwrap_or(0))));
    }
    Ok(out)
}

async fn remove_content(state: &AppState, account: &Account) -> Result<(), ConnectError> {
    let user_id = account.id;
    for channel in candidate_channels(state, user_id).await? {
        for message in own_messages(state, user_id, channel.id).await? {
            let (db_channel, bucket, db_message) = (id_to_db(channel.id), bucket_of(message.id), id_to_db(message.id));
            let attachments = rows::<(i32, Option<String>, Option<String>)>(
                state,
                "SELECT position, file, thumbnail FROM message_attachments WHERE channel_id = ? AND bucket = ? AND message_id = ?",
                (db_channel, bucket, db_message),
            )
            .await?;
            let mut changed = false;
            for (position, file, thumbnail) in attachments {
                // Objects first: until the row says "removed" a repeat finds the keys again.
                let keys: Vec<String> = file.iter().chain(thumbnail.iter()).filter(|k| !k.is_empty()).cloned().collect();
                for key in &keys {
                    crate::moderation::delete_object(state, key).await;
                }
                if file.as_deref().is_some_and(|f| !f.is_empty()) {
                    changed = true;
                }
                exec(
                    state,
                    "UPDATE message_attachments SET removed = true, file = '', thumbnail = null \
                     WHERE channel_id = ? AND bucket = ? AND message_id = ? AND position = ?",
                    (db_channel, bucket, db_message, position),
                )
                .await?;
            }
            if changed && let Err(e) = crate::services::message::publish_message_updated(state, channel.id, message.id).await {
                tracing::warn!(channel_id = channel.id, error = %e.message.as_deref().unwrap_or("?"), "publishing message_updated failed");
            }
        }
        for (message_id, key, _) in own_reactions(state, user_id, channel.id).await? {
            exec(
                state,
                "DELETE FROM reactions WHERE channel_id = ? AND bucket = ? AND message_id = ? AND emoji_key = ? AND user_id = ?",
                (id_to_db(channel.id), bucket_of(message_id), id_to_db(message_id), &key, id_to_db(user_id)),
            )
            .await?;
            if let Some(emoji) = crate::services::message::key_to_emoji(&key) {
                let event = EventPayload::ReactionRemoved(ReactionChange {
                    channel_id: channel.id,
                    message_id,
                    emoji: emoji.into(),
                    user_id,
                    ..Default::default()
                });
                if channel.guild_id == 0 {
                    state.events.publish_to_dm(channel.id, event);
                } else {
                    state.events.publish_to_guild(channel.guild_id, event);
                }
            }
        }
    }
    // Uploads that never became a message.
    let pending = rows::<(Option<String>, Option<String>)>(
        state,
        "SELECT key, thumbnail FROM pending_attachments WHERE uploader = ?",
        (id_to_db(user_id),),
    )
    .await?;
    if let Some(storage) = &state.storage {
        let keys: Vec<String> =
            pending.iter().flat_map(|(a, b)| a.iter().chain(b.iter())).filter(|k| !k.is_empty()).cloned().collect();
        for key in &keys {
            if let Err(e) = storage.delete(key).await {
                tracing::warn!(key, error = %format!("{e:#}"), "deleting pending upload failed");
                return Err(ConnectError::unavailable("media storage is unavailable"));
            }
        }
    }
    exec(state, "DELETE FROM pending_attachments WHERE uploader = ?", (id_to_db(user_id),)).await
}

async fn leave_guilds(state: &AppState, account: &Account) -> Result<(), ConnectError> {
    for (guild_id,) in rows::<(i64,)>(state, "SELECT guild_id FROM guilds_by_user WHERE user_id = ?", (id_to_db(account.id),)).await? {
        let guild_id = id_from_db(guild_id);
        if crate::permissions::standing(state, account.id, guild_id).await?.is_owner {
            tracing::warn!(guild_id, user_id = account.id, "deleted account still owns a guild; leaving it as is");
            continue;
        }
        crate::moderation::remove_member(state, guild_id, account.id, MemberRemovalReason::MEMBER_REMOVAL_REASON_LEFT).await?;
    }
    crate::voice::leave(state, account.id).await;
    Ok(())
}

async fn detach_dms(state: &AppState, account: &Account) -> Result<(), ConnectError> {
    let anonymized = deleted_user(account.id, account.created_at);
    for (dm_id, partner) in dm_partners(state, account.id).await? {
        state.events.notify_user_profile_updated(partner, anonymized.clone());
        exec(state, "DELETE FROM dms_by_user WHERE user_id = ? AND dm_id = ?", (id_to_db(account.id), dm_id)).await?;
    }
    Ok(())
}

async fn remove_credentials(state: &AppState, account: &Account) -> Result<(), ConnectError> {
    let db_id = id_to_db(account.id);
    exec(state, "DELETE FROM passkeys WHERE user_id = ?", (db_id,)).await?;
    exec(state, "DELETE FROM recovery_codes WHERE user_id = ?", (db_id,)).await?;
    crate::auth::revoke_all_sessions(state, account.id, None).await?;
    if let Some(avatar) = account.avatar.as_deref().filter(|a| !a.is_empty()) {
        crate::moderation::delete_object(state, avatar).await;
    }
    // Finished exports of this user.
    for (token_hash, object_key) in
        rows::<(String, Option<String>)>(state, "SELECT token_hash, object_key FROM data_exports_index WHERE kind = 'all'", ()).await?
    {
        let owner = rows::<(Option<i64>,)>(state, "SELECT user_id FROM data_exports WHERE token_hash = ?", (&token_hash,)).await?;
        if owner.first().and_then(|(u,)| *u) != Some(db_id) {
            continue;
        }
        if let (Some(storage), Some(key)) = (&state.storage, object_key)
            && let Err(e) = storage.delete(&key).await
        {
            tracing::warn!(error = %format!("{e:#}"), "deleting the user's export failed");
            return Err(ConnectError::unavailable("media storage is unavailable"));
        }
        exec(state, "DELETE FROM data_exports WHERE token_hash = ?", (&token_hash,)).await?;
        exec(state, "DELETE FROM data_exports_index WHERE kind = 'all' AND token_hash = ?", (&token_hash,)).await?;
    }
    exec(state, "DELETE FROM export_requests WHERE user_id = ?", (db_id,)).await?;
    crate::admin::set_flag(state, account.id, false).await?;
    exec(state, "DELETE FROM user_activity WHERE user_id = ?", (db_id,)).await
}

async fn finalize(state: &AppState, account: &Account) -> Result<(), ConnectError> {
    let db_id = id_to_db(account.id);
    exec(
        state,
        "UPDATE users SET username = ?, password_hash = '', email = null, totp_secret = null, totp_last_step = null, \
         avatar = '', is_admin = false, suspended = null, suspended_until = null, suspended_reason = null, \
         accepted_terms_version = null, accepted_terms_at = null, deletion_state = 'deleted' WHERE id = ?",
        (DELETED_USERNAME, db_id),
    )
    .await?;
    exec(state, "DELETE FROM user_left_guilds WHERE user_id = ?", (db_id,)).await?;
    exec(state, "DELETE FROM account_deletions WHERE kind = 'pending' AND user_id = ?", (db_id,)).await
}

// ---------------------------------------------------------------------------
// Suspension

/// Suspends `target` (revoking sessions and clearing voice) and audit-logs it.
pub async fn suspend(
    state: &AppState,
    actor: u64,
    target: u64,
    reason: &str,
    until: Option<DateTime<Utc>>,
) -> Result<(), ConnectError> {
    exec(
        state,
        "UPDATE users SET suspended = true, suspended_until = ?, suspended_reason = ?, suspended_at = ? WHERE id = ?",
        (until, reason, Utc::now(), id_to_db(target)),
    )
    .await?;
    crate::auth::revoke_all_sessions(state, target, None).await?;
    crate::voice::leave(state, target).await;
    let detail = match until {
        Some(until) => format!("until {} {reason}", until.to_rfc3339()),
        None => reason.to_owned(),
    };
    crate::admin::audit(state, actor, "user.suspend", &format!("user:{target}"), detail.trim()).await?;
    Ok(())
}

/// Lifts a suspension and audit-logs it.
pub async fn unsuspend(state: &AppState, actor: u64, target: u64) -> Result<(), ConnectError> {
    exec(
        state,
        "UPDATE users SET suspended = null, suspended_until = null, suspended_reason = null, suspended_at = null WHERE id = ?",
        (id_to_db(target),),
    )
    .await?;
    crate::admin::audit(state, actor, "user.unsuspend", &format!("user:{target}"), "").await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Terms

/// Records that the user accepted the current terms.
pub async fn record_terms(state: &AppState, user_id: u64) -> Result<(), ConnectError> {
    exec(
        state,
        "UPDATE users SET accepted_terms_version = ?, accepted_terms_at = ? WHERE id = ?",
        (crate::policy::LEGAL_VERSION, Utc::now(), id_to_db(user_id)),
    )
    .await
}

// ---------------------------------------------------------------------------
// Export

/// Claims the user's daily export slot; RESOURCE_EXHAUSTED when taken.
pub async fn claim_export_slot(state: &AppState, user_id: u64) -> Result<(), ConnectError> {
    let applied = lwt_applied(
        state
            .session
            .execute_unpaged(
                "INSERT INTO export_requests (user_id, requested_at) VALUES (?, ?) IF NOT EXISTS USING TTL ?",
                (id_to_db(user_id), Utc::now(), EXPORT_COOLDOWN_SECS),
            )
            .await
            .map_err(internal)?,
    )?;
    if applied { Ok(()) } else { Err(ConnectError::resource_exhausted("you can export your data once every 24 hours")) }
}

/// Builds the export in the background and emails the link.
pub fn queue_export(state: &AppState, account: Account) {
    let state = state.clone();
    tokio::spawn(async move {
        if let Err(e) = run_export(&state, &account).await {
            tracing::warn!(user_id = account.id, error = %e.message.as_deref().unwrap_or("?"), "data export failed");
            // Let them try again.
            let _ = exec(&state, "DELETE FROM export_requests WHERE user_id = ?", (id_to_db(account.id),)).await;
        }
    });
}

/// The user's data as JSON.
pub async fn build_export(state: &AppState, account: &Account) -> Result<serde_json::Value, ConnectError> {
    use serde_json::json;
    let user_id = account.id;
    let base = state.public_base();
    let media = |key: &str| format!("{base}/media/{key}");
    let when = |id: u64| snowflake_datetime(id).to_rfc3339();

    let sessions: Vec<_> = crate::auth::list_sessions(state, user_id)
        .await?
        .into_iter()
        .map(|s| {
            json!({
                "device": s.device_name,
                "created_at": s.created_at.map(|t| t.to_rfc3339()),
                "last_seen_at": s.last_seen_at.map(|t| t.to_rfc3339()),
            })
        })
        .collect();

    let mut memberships = Vec::new();
    for (guild_id, joined_at) in
        rows::<(i64, Option<DateTime<Utc>>)>(state, "SELECT guild_id, joined_at FROM guilds_by_user WHERE user_id = ?", (id_to_db(user_id),)).await?
    {
        let gid = id_from_db(guild_id);
        let Some(guild) = crate::services::guild::load_guild(state, guild_id).await? else { continue };
        let all_roles = crate::permissions::load_roles(state, gid).await?;
        let mine = crate::permissions::member_role_ids(state, gid, user_id).await?;
        let role_names: Vec<&str> = all_roles.iter().filter(|r| mine.contains(&r.id)).map(|r| r.name.as_str()).collect();
        memberships.push(json!({
            "guild_id": gid,
            "guild_name": guild.name,
            "owner": guild.owner_id == user_id,
            "joined_at": joined_at.map(|t| t.to_rfc3339()),
            "roles": role_names,
        }));
    }

    let mut uploads: Vec<serde_json::Value> = Vec::new();
    if let Some(avatar) = account.avatar.as_deref().filter(|a| !a.is_empty()) {
        uploads.push(json!({ "kind": "avatar", "url": media(avatar) }));
    }
    let (mut guild_messages, mut dm_messages, mut reactions) = (Vec::new(), Vec::new(), Vec::new());
    for channel in candidate_channels(state, user_id).await? {
        for message in own_messages(state, user_id, channel.id).await? {
            let attachments = rows::<(Option<String>, Option<String>, Option<i64>, Option<String>, Option<bool>)>(
                state,
                "SELECT filename, content_type, size, file, removed FROM message_attachments WHERE channel_id = ? AND bucket = ? AND message_id = ?",
                (id_to_db(channel.id), bucket_of(message.id), id_to_db(message.id)),
            )
            .await?;
            let attachments: Vec<_> = attachments
                .into_iter()
                .filter(|(_, _, _, file, removed)| !removed.unwrap_or(false) && file.as_deref().is_some_and(|f| !f.is_empty()))
                .map(|(filename, content_type, size, file, _)| {
                    let url = media(&file.unwrap_or_default());
                    uploads.push(json!({ "kind": "attachment", "filename": filename, "url": url, "channel_id": channel.id, "message_id": message.id }));
                    json!({ "filename": filename, "content_type": content_type, "size": size, "url": url })
                })
                .collect();
            let entry = json!({
                "id": message.id,
                "channel_id": channel.id,
                "channel_name": channel.name,
                "guild_id": channel.guild_id,
                "guild_name": channel.guild_name,
                "created_at": when(message.id),
                "edited_at": message.edited_at.map(|t| t.to_rfc3339()),
                "content": message.content,
                "attachments": attachments,
            });
            if channel.guild_id == 0 { dm_messages.push(entry) } else { guild_messages.push(entry) }
        }
        for (message_id, key, at) in own_reactions(state, user_id, channel.id).await? {
            reactions.push(json!({
                "channel_id": channel.id,
                "message_id": message_id,
                "emoji": key,
                "at": DateTime::from_timestamp_millis(at).map(|t| t.to_rfc3339()),
            }));
        }
    }

    // Audit entries by or about the user (their id appears in the target).
    let mut audit = Vec::new();
    let marker = format!("user:{user_id}");
    let mut before = None;
    'pages: loop {
        let page = crate::admin::audit_page(state, before, 200).await?;
        let Some(last) = page.last().map(|r| r.id) else { break };
        for row in page {
            if row.id < user_id {
                break 'pages;
            }
            if row.actor_id == user_id || row.target == marker || row.target.ends_with(&format!("/{marker}")) {
                audit.push(json!({ "id": row.id, "at": when(row.id), "action": row.action, "target": row.target, "reason": row.reason, "by_you": row.actor_id == user_id }));
            }
        }
        before = Some(last);
    }

    Ok(json!({
        "exported_at": Utc::now().to_rfc3339(),
        "profile": {
            "id": user_id,
            "username": account.username,
            "email": account.email,
            "created_at": account.created_at.to_rfc3339(),
            "is_admin": account.is_admin,
            "accepted_terms_version": account.accepted_terms_version,
        },
        "sessions": sessions,
        "guild_memberships": memberships,
        "messages": guild_messages,
        "direct_messages_you_wrote": dm_messages,
        "reactions": reactions,
        "uploads": uploads,
        "audit_log": audit,
    }))
}

async fn run_export(state: &AppState, account: &Account) -> Result<(), ConnectError> {
    let storage = state.storage.as_ref().ok_or_else(|| ConnectError::unavailable("object storage is not configured"))?;
    let email = account.email.clone().ok_or_else(|| ConnectError::failed_precondition("no email address"))?;
    let json = build_export(state, account).await?;
    let body = serde_json::to_vec_pretty(&json).map_err(internal)?;
    let token = generate_token();
    let token_hash = hash_token(&token);
    let object_key = format!("exports/{}", hash_token(&format!("object:{token}")));
    storage.put(&object_key, Bytes::from(body), "application/json").await.map_err(|e| {
        tracing::warn!(error = %format!("{e:#}"), "storing the export failed");
        ConnectError::unavailable("media storage is unavailable")
    })?;
    let expires_at = Utc::now() + EXPORT_TTL;
    // The row outlives the link by a day; the sweeper deletes the object once expired.
    exec(
        state,
        "INSERT INTO data_exports (token_hash, user_id, object_key, expires_at) VALUES (?, ?, ?, ?) USING TTL ?",
        (&token_hash, id_to_db(account.id), &object_key, expires_at, (EXPORT_TTL.num_seconds() + 24 * 3600) as i32),
    )
    .await?;
    exec(
        state,
        "INSERT INTO data_exports_index (kind, token_hash, object_key, expires_at) VALUES ('all', ?, ?, ?)",
        (&token_hash, &object_key, expires_at),
    )
    .await?;
    let link = format!("{}/export/{token}", state.public_base());
    state.mailer.send(crate::email::templates::export_ready(&email, format!("export-{}", generate_token()), &link, EXPORT_TTL.num_days() as u32));
    tracing::info!(user_id = account.id, "data export ready");
    Ok(())
}

/// Deletes the objects of expired exports; returns how many.
pub async fn sweep_exports(state: &AppState) -> Result<usize, ConnectError> {
    let now = Utc::now();
    let mut swept = 0;
    for (token_hash, object_key, expires_at) in
        rows::<(String, Option<String>, Option<DateTime<Utc>>)>(state, "SELECT token_hash, object_key, expires_at FROM data_exports_index WHERE kind = 'all'", ()).await?
    {
        if expires_at.is_some_and(|t| t > now) {
            continue;
        }
        if let (Some(storage), Some(key)) = (&state.storage, &object_key)
            && let Err(e) = storage.delete(key).await
        {
            tracing::warn!(error = %format!("{e:#}"), "deleting an expired export failed");
            continue;
        }
        exec(state, "DELETE FROM data_exports WHERE token_hash = ?", (&token_hash,)).await?;
        exec(state, "DELETE FROM data_exports_index WHERE kind = 'all' AND token_hash = ?", (&token_hash,)).await?;
        swept += 1;
    }
    Ok(swept)
}

/// `GET /export/{token}`: the export file (a bearer link, 7 days).
pub fn routes() -> Router<AppState> {
    Router::new().route("/export/{token}", get(download))
}

async fn download(State(state): State<AppState>, Path(token): Path<String>) -> Response {
    let not_found = || (StatusCode::NOT_FOUND, "not found").into_response();
    if token.len() > 128 {
        return not_found();
    }
    let found = rows::<(Option<String>, Option<DateTime<Utc>>)>(
        &state,
        "SELECT object_key, expires_at FROM data_exports WHERE token_hash = ?",
        (hash_token(&token),),
    )
    .await;
    let Ok(found) = found else {
        return (StatusCode::SERVICE_UNAVAILABLE, "unavailable").into_response();
    };
    let Some((Some(key), Some(expires_at))) = found.into_iter().next() else {
        return not_found();
    };
    if expires_at <= Utc::now() {
        return not_found();
    }
    let Some(storage) = &state.storage else {
        return (StatusCode::SERVICE_UNAVAILABLE, "unavailable").into_response();
    };
    match storage.get(&key).await {
        Ok(Some(object)) => {
            let mut response = Response::new(Body::from_stream(object.stream));
            let headers = response.headers_mut();
            headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
            headers.insert(header::CONTENT_DISPOSITION, HeaderValue::from_static("attachment; filename=\"thehideout-export.json\""));
            headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
            headers.insert(header::CONTENT_LENGTH, object.info.size.into());
            response
        }
        Ok(None) => not_found(),
        Err(e) => {
            tracing::warn!(error = %format!("{e:#}"), "reading an export failed");
            (StatusCode::SERVICE_UNAVAILABLE, "unavailable").into_response()
        }
    }
}

/// Starts the background sweeps: resuming interrupted deletions and removing
/// expired exports, at startup and every few minutes.
pub fn spawn_background(state: &AppState) {
    let state = state.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(SWEEP_EVERY);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = tick.tick() => {}
                _ = state.shutdown.triggered() => return,
            }
            match resume_pending(&state).await {
                Ok(0) => {}
                Ok(n) => tracing::info!(accounts = n, "resumed interrupted account deletions"),
                Err(e) => tracing::warn!(error = %e.message.as_deref().unwrap_or("?"), "resuming account deletions failed"),
            }
            match sweep_exports(&state).await {
                Ok(0) => {}
                Ok(n) => tracing::info!(exports = n, "deleted expired data exports"),
                Err(e) => tracing::warn!(error = %e.message.as_deref().unwrap_or("?"), "export sweep failed"),
            }
        }
    });
}
