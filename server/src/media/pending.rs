//! Pending attachment uploads: the table of unconsumed uploads, the orphan
//! registry and the sweeper that deletes the objects of uploads nobody used.
//!
//! Protocol (see migration 0007):
//! 1. `POST /attachments` registers the upload in `pending_sweep` (keyed by the
//!    hour it expires), *then* creates the object, *then* writes the
//!    `pending_attachments` row with a TTL. A crash at any point leaves at
//!    worst a sweep row whose objects may not exist.
//! 2. `SendMessage` consumes the pending rows (conditional batch), counts the
//!    bytes in the accounting tables and removes the sweep rows.
//! 3. The sweeper walks the sweep buckets up to the current hour. For each row
//!    past its expiry (plus a grace period) whose pending row is gone and whose
//!    object is not accounted (= no message owns it), it deletes the objects;
//!    either way the sweep row is dropped. Pending uploads are never counted in
//!    storage totals, so sweeping needs no accounting changes.

use chrono::{DateTime, Utc};
use connectrpc::ConnectError;
use scylla::value::Row;

use crate::AppState;
use crate::auth::internal;
use crate::convert::id_to_db;

/// Hour buckets scanned behind the current one when no cursor was stored yet.
const DEFAULT_LOOKBACK_HOURS: i64 = 48;
const CURSOR_NAME: &str = "attachments";

/// Unix hour of `t`, the `pending_sweep` bucket.
pub fn hour_of(t: DateTime<Utc>) -> i64 {
    t.timestamp().div_euclid(3600)
}

/// A pending upload as stored.
#[derive(Debug, Clone)]
pub struct Pending {
    pub id: u64,
    pub channel_id: u64,
    pub key: String,
    pub filename: String,
    pub content_type: String,
    pub size: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub thumbnail: Option<String>,
    pub thumbnail_bytes: u64,
    pub sha256: String,
    pub expires_at: DateTime<Utc>,
    pub sweep_hour: Option<i64>,
}

type PendingRow = (
    i64,
    i64,
    String,
    String,
    String,
    i64,
    Option<i32>,
    Option<i32>,
    Option<String>,
    Option<i64>,
    Option<String>,
    DateTime<Utc>,
    Option<i64>,
);

const PENDING_COLUMNS: &str = "id, channel_id, key, filename, content_type, size, width, height, thumbnail, \
                               thumbnail_bytes, sha256, expires_at, sweep_hour";

fn pending_from_row(row: PendingRow) -> Pending {
    let (id, channel_id, key, filename, content_type, size, width, height, thumbnail, thumbnail_bytes, sha256, expires_at, sweep_hour) = row;
    Pending {
        id: id as u64,
        channel_id: channel_id as u64,
        key,
        filename,
        content_type,
        size: size.max(0) as u64,
        width: width.map(|v| v as u32),
        height: height.map(|v| v as u32),
        thumbnail: thumbnail.filter(|t| !t.is_empty()),
        thumbnail_bytes: thumbnail_bytes.unwrap_or(0).max(0) as u64,
        sha256: sha256.unwrap_or_default(),
        expires_at,
        sweep_hour,
    }
}

/// Number of unexpired pending uploads of `uploader`.
pub async fn count(state: &AppState, uploader: u64) -> Result<usize, ConnectError> {
    let (n,) = state
        .session
        .execute_unpaged("SELECT COUNT(*) FROM pending_attachments WHERE uploader = ?", (id_to_db(uploader),))
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .first_row::<(i64,)>()
        .map_err(internal)?;
    Ok(n.max(0) as usize)
}

/// Registers an upload in the orphan registry (before its object exists).
pub async fn register_sweep(
    state: &AppState,
    id: u64,
    uploader: u64,
    key: &str,
    thumbnail: &str,
    expires_at: DateTime<Utc>,
) -> Result<i64, ConnectError> {
    let hour = hour_of(expires_at);
    state
        .session
        .execute_unpaged(
            "INSERT INTO pending_sweep (hour, id, uploader, key, thumbnail, expires_at) VALUES (?, ?, ?, ?, ?, ?)",
            (hour, id_to_db(id), id_to_db(uploader), key, thumbnail, expires_at),
        )
        .await
        .map_err(internal)?;
    Ok(hour)
}

/// Drops an orphan-registry row (the upload was consumed, or never completed).
pub async fn forget_sweep(state: &AppState, hour: i64, id: u64) {
    if let Err(e) = state
        .session
        .execute_unpaged("DELETE FROM pending_sweep WHERE hour = ? AND id = ?", (hour, id_to_db(id)))
        .await
    {
        tracing::warn!(error = %e, "deleting pending_sweep row failed");
    }
}

/// Writes the pending row; it disappears by itself after `ttl_secs`.
pub async fn insert(state: &AppState, uploader: u64, p: &Pending, ttl_secs: i32) -> Result<(), ConnectError> {
    state
        .session
        .execute_unpaged(
            "INSERT INTO pending_attachments (uploader, id, channel_id, key, filename, content_type, size, width, \
             height, thumbnail, thumbnail_bytes, sha256, expires_at, sweep_hour) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) USING TTL ?",
            (
                id_to_db(uploader),
                id_to_db(p.id),
                id_to_db(p.channel_id),
                &p.key,
                &p.filename,
                &p.content_type,
                p.size as i64,
                p.width.map(|v| v as i32),
                p.height.map(|v| v as i32),
                p.thumbnail.as_deref(),
                p.thumbnail_bytes as i64,
                &p.sha256,
                p.expires_at,
                p.sweep_hour,
                ttl_secs,
            ),
        )
        .await
        .map_err(internal)?;
    Ok(())
}

/// The caller's pending uploads among `ids` (missing ones are simply absent).
pub async fn load(state: &AppState, uploader: u64, ids: &[u64]) -> Result<Vec<Pending>, ConnectError> {
    let ids: Vec<i64> = ids.iter().copied().map(id_to_db).collect();
    Ok(state
        .session
        .execute_unpaged(
            format!("SELECT {PENDING_COLUMNS} FROM pending_attachments WHERE uploader = ? AND id IN ?"),
            (id_to_db(uploader), ids),
        )
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .rows::<PendingRow>()
        .map_err(internal)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?
        .into_iter()
        .map(pending_from_row)
        .collect())
}

/// Consumes the pending uploads `ids` of `uploader` all-or-nothing: one
/// conditional batch in the uploader's partition, applied only if every row
/// still exists. `false` means at least one was already consumed or expired.
pub async fn consume(state: &AppState, uploader: u64, ids: &[u64]) -> Result<bool, ConnectError> {
    use scylla::statement::batch::{Batch, BatchType};
    let mut batch = Batch::new(BatchType::Logged);
    let mut values = Vec::with_capacity(ids.len());
    for id in ids {
        batch.append_statement("DELETE FROM pending_attachments WHERE uploader = ? AND id = ? IF EXISTS");
        values.push((id_to_db(uploader), id_to_db(*id)));
    }
    let result = state.session.batch(&batch, values).await.map_err(internal)?;
    let row = result
        .into_rows_result()
        .map_err(internal)?
        .first_row::<Row>()
        .map_err(internal)?;
    Ok(matches!(row.columns.first(), Some(Some(scylla::value::CqlValue::Boolean(true)))))
}

async fn pending_exists(state: &AppState, uploader: i64, id: i64) -> Result<bool, ConnectError> {
    Ok(state
        .session
        .execute_unpaged("SELECT id FROM pending_attachments WHERE uploader = ? AND id = ?", (uploader, id))
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .rows_num()
        > 0)
}

async fn accounted(state: &AppState, key: &str) -> Result<bool, ConnectError> {
    Ok(state
        .session
        .execute_unpaged("SELECT key FROM media_objects WHERE key = ?", (key,))
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .rows_num()
        > 0)
}

async fn read_cursor(state: &AppState) -> Result<Option<i64>, ConnectError> {
    Ok(state
        .session
        .execute_unpaged("SELECT hour FROM sweep_cursor WHERE name = ?", (CURSOR_NAME,))
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .maybe_first_row::<(Option<i64>,)>()
        .map_err(internal)?
        .and_then(|(h,)| h))
}

/// One sweep pass; returns how many uploads' objects were deleted.
pub async fn sweep_once(state: &AppState) -> Result<usize, ConnectError> {
    let Some(storage) = &state.storage else {
        return Ok(0);
    };
    let now = Utc::now();
    let current = hour_of(now);
    let grace = chrono::Duration::from_std(state.attachments.sweep_grace).unwrap_or_default();
    let start = read_cursor(state).await?.unwrap_or(current - DEFAULT_LOOKBACK_HOURS);

    let mut swept = 0;
    let mut first_unfinished = None;
    for hour in start..=current {
        let rows = state
            .session
            .execute_unpaged("SELECT id, uploader, key, thumbnail, expires_at FROM pending_sweep WHERE hour = ?", (hour,))
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .rows::<(i64, i64, String, Option<String>, DateTime<Utc>)>()
            .map_err(internal)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(internal)?;
        let mut unfinished = false;
        for (id, uploader, key, thumbnail, expires_at) in rows {
            // Not expired yet, or the upload outlived its registry row and its
            // pending row is still live: look again later.
            if expires_at + grace > now || pending_exists(state, uploader, id).await? {
                unfinished = true;
                continue;
            }
            if !accounted(state, &key).await? {
                let mut failed = false;
                for k in std::iter::once(&key).chain(thumbnail.as_ref()) {
                    if let Err(e) = storage.delete(k).await {
                        tracing::warn!(key = %k, error = %format!("{e:#}"), "deleting expired attachment object failed");
                        failed = true;
                    }
                }
                if failed {
                    unfinished = true;
                    continue;
                }
                swept += 1;
            }
            forget_sweep(state, hour, id as u64).await;
        }
        if unfinished && first_unfinished.is_none() {
            first_unfinished = Some(hour);
        }
    }
    let cursor = first_unfinished.unwrap_or(current);
    if cursor != start {
        state
            .session
            .execute_unpaged("INSERT INTO sweep_cursor (name, hour) VALUES (?, ?)", (CURSOR_NAME, cursor))
            .await
            .map_err(internal)?;
    }
    Ok(swept)
}

/// Runs [`sweep_once`] every `sweep_interval` until shutdown.
pub fn spawn_sweeper(state: AppState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(state.attachments.sweep_interval);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = tick.tick() => {}
                _ = state.shutdown.triggered() => return,
            }
            match sweep_once(&state).await {
                Ok(0) => {}
                Ok(n) => tracing::info!(deleted = n, "swept expired attachment uploads"),
                Err(e) => tracing::warn!(error = %e.message.as_deref().unwrap_or("?"), "attachment sweep failed"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hour_buckets_are_unix_hours() {
        let t = DateTime::from_timestamp(3 * 3600 + 59, 0).unwrap();
        assert_eq!(hour_of(t), 3);
        assert_eq!(hour_of(DateTime::from_timestamp(0, 0).unwrap()), 0);
    }
}
