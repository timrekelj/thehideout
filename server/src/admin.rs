//! Site administration: the admin flag and its bootstrap, the audit log and
//! the startup jobs (counter and file-index backfills).

use chrono::{DateTime, Utc};
use connectrpc::{ConnectError, RequestContext};

use crate::AppState;
use crate::auth::{authenticate, internal};
use crate::convert::{id_from_db, id_to_db, user_to_proto};
use crate::proto::thehideout::v1::User;
use crate::snowflake::{bucket_of, current_bucket};

/// Normalises a username the way sign-up does.
pub fn normalize_username(raw: &str) -> String {
    raw.trim().to_lowercase()
}

/// Parses `THEHIDEOUT_ADMINS`: comma-separated usernames.
pub fn parse_admins(raw: &str) -> Vec<String> {
    raw.split(',').map(normalize_username).filter(|s| !s.is_empty()).collect()
}

/// Loads a user including `is_admin` and `email` (use only for the user
/// themself and for admin views; other users never see them).
pub async fn load_user(state: &AppState, id: u64) -> Result<Option<User>, ConnectError> {
    Ok(state
        .session
        .execute_unpaged("SELECT username, created_at, avatar, is_admin, email, deletion_state FROM users WHERE id = ?", (id_to_db(id),))
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .maybe_first_row::<(String, DateTime<Utc>, Option<String>, Option<bool>, Option<String>, Option<String>)>()
        .map_err(internal)?
        .map(|(username, created_at, avatar, is_admin, email, deletion_state)| {
            if deletion_state.is_some_and(|s| !s.is_empty()) {
                return crate::lifecycle::deleted_user(id, created_at);
            }
            let mut user = user_to_proto(id, &username, created_at, avatar);
            user.is_admin = is_admin.unwrap_or(false);
            user.email = email.filter(|e| !e.is_empty());
            user
        }))
}

/// Succeeds with the caller's id if they are a site admin: PERMISSION_DENIED
/// otherwise, UNAUTHENTICATED without a valid session.
pub async fn require_admin(state: &AppState, ctx: &RequestContext) -> Result<u64, ConnectError> {
    let user_id = authenticate(state, ctx).await?;
    let is_admin = state
        .session
        .execute_unpaged("SELECT is_admin FROM users WHERE id = ?", (id_to_db(user_id),))
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .maybe_first_row::<(Option<bool>,)>()
        .map_err(internal)?
        .and_then(|(a,)| a)
        .unwrap_or(false);
    if is_admin {
        Ok(user_id)
    } else {
        Err(ConnectError::permission_denied("site admin rights required"))
    }
}

/// Writes the flag on the user row and in the `site_admins` list.
pub async fn set_flag(state: &AppState, user_id: u64, is_admin: bool) -> Result<(), ConnectError> {
    use scylla::statement::batch::{Batch, BatchType};
    let db_id = id_to_db(user_id);
    let mut batch = Batch::new(BatchType::Logged);
    batch.append_statement("UPDATE users SET is_admin = ? WHERE id = ?");
    if is_admin {
        batch.append_statement("INSERT INTO site_admins (kind, user_id) VALUES ('admin', ?)");
    } else {
        batch.append_statement("DELETE FROM site_admins WHERE kind = 'admin' AND user_id = ?");
    }
    state.session.batch(&batch, ((is_admin, db_id), (db_id,))).await.map_err(internal)?;
    Ok(())
}

/// Ids of all admins.
pub async fn admin_ids(state: &AppState) -> Result<Vec<u64>, ConnectError> {
    Ok(state
        .session
        .execute_unpaged("SELECT user_id FROM site_admins WHERE kind = 'admin'", ())
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .rows::<(i64,)>()
        .map_err(internal)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?
        .into_iter()
        .map(|(id,)| id_from_db(id))
        .collect())
}

/// Promotes the listed usernames that exist; returns how many were promoted.
pub async fn promote_usernames(state: &AppState, usernames: &[String]) -> Result<usize, ConnectError> {
    let mut promoted = 0;
    for name in usernames {
        let Some((id,)) = state
            .session
            .execute_unpaged("SELECT user_id FROM users_by_username WHERE username = ?", (name,))
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .maybe_first_row::<(i64,)>()
            .map_err(internal)?
        else {
            continue;
        };
        set_flag(state, id_from_db(id), true).await?;
        promoted += 1;
    }
    Ok(promoted)
}

/// Startup jobs: promote `THEHIDEOUT_ADMINS`, backfill the analytics
/// counters (once) and the file size index (if empty).
pub async fn startup(state: &AppState) -> Result<(), ConnectError> {
    let admins: Vec<String> = state.admins.iter().cloned().collect();
    let promoted = promote_usernames(state, &admins).await?;
    if promoted > 0 {
        tracing::info!(promoted, "promoted configured admins");
    }
    crate::stats::backfill(state).await?;
    crate::media::files::backfill_index(state).await?;
    Ok(())
}

/// Appends an audit entry; returns its id.
pub async fn audit(state: &AppState, actor_id: u64, action: &str, target: &str, reason: &str) -> Result<u64, ConnectError> {
    let id = state.snowflake.next_id();
    state
        .session
        .execute_unpaged(
            "INSERT INTO audit_log (bucket, id, actor_id, action, target, reason) VALUES (?, ?, ?, ?, ?, ?)",
            (bucket_of(id), id_to_db(id), id_to_db(actor_id), action, target, reason),
        )
        .await
        .map_err(internal)?;
    tracing::info!(actor_id, action, target, "admin action");
    Ok(id)
}

/// A stored audit row.
pub struct AuditRow {
    pub id: u64,
    pub actor_id: u64,
    pub action: String,
    pub target: String,
    pub reason: String,
}

/// Up to `limit` entries newest first, strictly older than `before`.
pub async fn audit_page(state: &AppState, before: Option<u64>, limit: usize) -> Result<Vec<AuditRow>, ConnectError> {
    let mut out = Vec::new();
    let start = before.map_or_else(current_bucket, bucket_of);
    for bucket in (0..=start).rev() {
        let want = (limit - out.len()) as i32;
        let result = match before.filter(|_| bucket == start) {
            Some(before) => {
                state
                    .session
                    .execute_unpaged(
                        "SELECT id, actor_id, action, target, reason FROM audit_log WHERE bucket = ? AND id < ? LIMIT ?",
                        (bucket, id_to_db(before), want),
                    )
                    .await
            }
            None => {
                state
                    .session
                    .execute_unpaged("SELECT id, actor_id, action, target, reason FROM audit_log WHERE bucket = ? LIMIT ?", (bucket, want))
                    .await
            }
        }
        .map_err(internal)?;
        let rows = result
            .into_rows_result()
            .map_err(internal)?
            .rows::<(i64, i64, Option<String>, Option<String>, Option<String>)>()
            .map_err(internal)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(internal)?;
        out.extend(rows.into_iter().map(|(id, actor, action, target, reason)| AuditRow {
            id: id_from_db(id),
            actor_id: id_from_db(actor),
            action: action.unwrap_or_default(),
            target: target.unwrap_or_default(),
            reason: reason.unwrap_or_default(),
        }));
        if out.len() >= limit {
            break;
        }
    }
    Ok(out)
}
