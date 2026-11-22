//! Site analytics without scans: counters maintained where things are
//! created, read by the admin dashboard.
//!
//! Everything lives in the `counters` table (`scope`, `name`): scope `site`
//! (users, guilds, channels, messages), `guild:<id>` (members, channels,
//! messages), `user:<id>` (messages) and `day:<YYYY-MM-DD>` in UTC (messages,
//! signups, active_users). Updates are best effort: a failed counter write is
//! logged and never fails the user's action.
//!
//! "Active" means the user sent a message or opened a gateway stream that UTC
//! day. Each user is counted once per day through a marker row in
//! `daily_active` written with `INSERT ... IF NOT EXISTS USING TTL` (2 days);
//! an in-memory map of "already marked today" spares the LWT on every message.
//!
//! Upgrading: [`backfill`] runs at startup and, once (marker `counters_backfilled`
//! in `server_meta`), counts the existing users, guilds, channels, members and
//! messages with full scans and adds the difference to the counters.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;

use chrono::{DateTime, Utc};
use connectrpc::ConnectError;
use scylla::value::Counter;

use crate::AppState;
use crate::auth::internal;
use crate::convert::{id_from_db, id_to_db, snowflake_datetime};

/// How long an "active today" marker lives.
const ACTIVE_MARKER_TTL_SECS: i32 = 2 * 24 * 60 * 60;

/// In-memory live state: open gateway streams and which users were already
/// marked active today.
#[derive(Default)]
pub struct Live {
    pub connections: AtomicU64,
    marked: Mutex<HashMap<u64, String>>,
}

/// `YYYY-MM-DD` of a time, in UTC.
pub fn day_of(t: DateTime<Utc>) -> String {
    t.format("%Y-%m-%d").to_string()
}

pub fn today() -> String {
    day_of(Utc::now())
}

/// Adds `delta` to a counter; best effort.
pub async fn bump(state: &AppState, scope: &str, name: &str, delta: i64) {
    if let Err(e) = state
        .session
        .execute_unpaged("UPDATE counters SET value = value + ? WHERE scope = ? AND name = ?", (Counter(delta), scope, name))
        .await
    {
        tracing::warn!(scope, name, error = %e, "counter update failed");
    }
}

/// All counters of a scope (missing ones are absent).
pub async fn scope(state: &AppState, scope: &str) -> Result<HashMap<String, i64>, ConnectError> {
    Ok(state
        .session
        .execute_unpaged("SELECT name, value FROM counters WHERE scope = ?", (scope,))
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .rows::<(String, Counter)>()
        .map_err(internal)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?
        .into_iter()
        .map(|(name, value)| (name, value.0.max(0)))
        .collect())
}

pub async fn user_signed_up(state: &AppState) {
    bump(state, "site", "users", 1).await;
    bump(state, &format!("day:{}", today()), "signups", 1).await;
}

pub async fn guild_created(state: &AppState) {
    bump(state, "site", "guilds", 1).await;
}

pub async fn channel_created(state: &AppState, guild_id: u64) {
    bump(state, "site", "channels", 1).await;
    bump(state, &format!("guild:{guild_id}"), "channels", 1).await;
}

pub async fn member_added(state: &AppState, guild_id: u64) {
    bump(state, &format!("guild:{guild_id}"), "members", 1).await;
}

/// A member left or was removed.
pub async fn member_removed(state: &AppState, guild_id: u64) {
    bump(state, &format!("guild:{guild_id}"), "members", -1).await;
}

pub async fn channel_deleted(state: &AppState, guild_id: u64) {
    bump(state, "site", "channels", -1).await;
    bump(state, &format!("guild:{guild_id}"), "channels", -1).await;
}

/// A guild was deleted with `channels` channels.
pub async fn guild_deleted(state: &AppState, channels: usize) {
    bump(state, "site", "guilds", -1).await;
    bump(state, "site", "channels", -(channels as i64)).await;
}

/// A message was stored (`guild_id` is 0 for DMs).
pub async fn message_sent(state: &AppState, guild_id: u64, user_id: u64) {
    bump(state, "site", "messages", 1).await;
    bump(state, &format!("day:{}", today()), "messages", 1).await;
    bump(state, &format!("user:{user_id}"), "messages", 1).await;
    if guild_id != 0 {
        bump(state, &format!("guild:{guild_id}"), "messages", 1).await;
    }
    touch_user(state, user_id).await;
}

/// Records that the user is active now (message or gateway stream).
pub async fn touch_user(state: &AppState, user_id: u64) {
    let now = Utc::now();
    let day = day_of(now);
    if let Err(e) = state
        .session
        .execute_unpaged("INSERT INTO user_activity (user_id, last_active_at) VALUES (?, ?)", (id_to_db(user_id), now))
        .await
    {
        tracing::warn!(user_id, error = %e, "recording user activity failed");
    }
    {
        let mut marked = state.live.marked.lock().unwrap_or_else(|e| e.into_inner());
        if marked.get(&user_id) == Some(&day) {
            return;
        }
        marked.insert(user_id, day.clone());
        // Forget yesterday's users so the map stays bounded.
        marked.retain(|_, d| *d == day);
    }
    let applied = async {
        let (applied, _, _) = state
            .session
            .execute_unpaged(
                "INSERT INTO daily_active (day, user_id) VALUES (?, ?) IF NOT EXISTS USING TTL ?",
                (&day, id_to_db(user_id), ACTIVE_MARKER_TTL_SECS),
            )
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .first_row::<(bool, Option<String>, Option<i64>)>()
            .map_err(internal)?;
        Ok::<_, ConnectError>(applied)
    }
    .await;
    match applied {
        Ok(true) => bump(state, &format!("day:{day}"), "active_users", 1).await,
        Ok(false) => {}
        Err(_) => {
            // Let the next activity retry.
            state.live.marked.lock().unwrap_or_else(|e| e.into_inner()).remove(&user_id);
        }
    }
}

/// Counts the existing data once and adds what the counters lack. Returns
/// whether it ran.
pub async fn backfill(state: &AppState) -> Result<bool, ConnectError> {
    let done = state
        .session
        .execute_unpaged("SELECT value FROM server_meta WHERE name = 'counters_backfilled'", ())
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .maybe_first_row::<(Option<String>,)>()
        .map_err(internal)?
        .is_some_and(|(v,)| v.as_deref() == Some("done"));
    if done {
        return Ok(false);
    }
    // Idempotent (it adds only the difference), so an interrupted run or two
    // servers starting at once just converge.
    backfill_counts(state).await?;
    Ok(true)
}

async fn scan<T: for<'f, 'm> scylla::deserialize::row::DeserializeRow<'f, 'm>>(state: &AppState, cql: &'static str) -> Result<Vec<T>, ConnectError> {
    let result = state.session.execute_unpaged(cql, ()).await.map_err(internal)?.into_rows_result().map_err(internal)?;
    let rows = result.rows::<T>().map_err(internal)?.collect::<Result<Vec<T>, _>>().map_err(internal)?;
    Ok(rows)
}

async fn backfill_counts(state: &AppState) -> Result<(), ConnectError> {
    // (scope, name) -> what the data says.
    let mut want: BTreeMap<(String, String), i64> = BTreeMap::new();
    let mut add = |scope: String, name: &str, n: i64| *want.entry((scope, name.to_owned())).or_default() += n;

    for (id, created_at) in scan::<(i64, Option<DateTime<Utc>>)>(state, "SELECT id, created_at FROM users").await? {
        add("site".into(), "users", 1);
        let created_at = created_at.unwrap_or_else(|| snowflake_datetime(id_from_db(id)));
        add(format!("day:{}", day_of(created_at)), "signups", 1);
    }
    for (_,) in scan::<(i64,)>(state, "SELECT id FROM guilds").await? {
        add("site".into(), "guilds", 1);
    }
    for (guild_id,) in scan::<(i64,)>(state, "SELECT guild_id FROM channels").await? {
        add("site".into(), "channels", 1);
        add(format!("guild:{}", id_from_db(guild_id)), "channels", 1);
    }
    for (guild_id,) in scan::<(i64,)>(state, "SELECT guild_id FROM guild_members").await? {
        add(format!("guild:{}", id_from_db(guild_id)), "members", 1);
    }
    let today = today();
    let mut active: HashSet<(String, u64)> = HashSet::new();
    for (id, guild_id, author_id) in scan::<(i64, i64, i64)>(state, "SELECT id, guild_id, author_id FROM messages").await? {
        let (id, author) = (id_from_db(id), id_from_db(author_id));
        let day = day_of(snowflake_datetime(id));
        add("site".into(), "messages", 1);
        add(format!("day:{day}"), "messages", 1);
        add(format!("user:{author}"), "messages", 1);
        if guild_id != 0 {
            add(format!("guild:{}", id_from_db(guild_id)), "messages", 1);
        }
        active.insert((day, author));
    }
    for (day, author) in &active {
        add(format!("day:{day}"), "active_users", 1);
        if *day == today {
            // Mark today's authors so they are not counted again when active.
            let _ = state
                .session
                .execute_unpaged(
                    "INSERT INTO daily_active (day, user_id) VALUES (?, ?) USING TTL ?",
                    (day, id_to_db(*author), ACTIVE_MARKER_TTL_SECS),
                )
                .await;
        }
    }

    let mut scopes: HashMap<String, HashMap<String, i64>> = HashMap::new();
    for ((scope_name, name), n) in want {
        if !scopes.contains_key(&scope_name) {
            let current = scope(state, &scope_name).await?;
            scopes.insert(scope_name.clone(), current);
        }
        let delta = n - scopes[&scope_name].get(&name).copied().unwrap_or(0);
        if delta != 0 {
            bump(state, &scope_name, &name, delta).await;
        }
    }
    state
        .session
        .execute_unpaged("INSERT INTO server_meta (name, value) VALUES ('counters_backfilled', 'done')", ())
        .await
        .map_err(internal)?;
    tracing::info!("analytics counters backfilled from existing data");
    Ok(())
}
