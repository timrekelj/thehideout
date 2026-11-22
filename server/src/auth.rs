//! Password hashing, session tokens and request authentication.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use base64::Engine;
use chrono::{DateTime, Utc};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use connectrpc::{ConnectError, RequestContext};
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::AppState;
use crate::convert::{id_from_db, id_to_db};

/// How long a session token stays valid after its last refresh.
pub const SESSION_TTL_SECS: i32 = 30 * 24 * 60 * 60;
/// A session in use has its expiry pushed out at most this often (sliding
/// expiry), so active users stay signed in while the writes stay bounded.
pub const SESSION_REFRESH_AFTER: chrono::Duration = chrono::Duration::days(1);

/// A valid argon2id hash that never matches a real password; verified against
/// when a username is unknown so the response time looks the same.
pub const DUMMY_PASSWORD_HASH: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$BkC5oC+yWwX2FD0lJH6cOQ$g/hvc4UrZJsrTknSwxPBUY6ZFwTpPhSVD9nmLncBR7U";

/// Extracts the token from an `Authorization: Bearer <token>` header.
pub fn bearer_token(ctx: &RequestContext) -> Option<&str> {
    ctx.header("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

/// Best-effort client IP for rate-limit keys (the forwarded address behind a
/// trusted proxy, see `clientip`); `"unknown"` when the server is not served
/// with `into_make_service_with_connect_info`.
pub fn peer_key(ctx: &RequestContext) -> String {
    crate::clientip::from_extensions(ctx.extensions())
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "unknown".to_owned())
}

/// Verifies the session token in `Authorization: Bearer <token>` and returns the user id.
pub async fn authenticate(state: &AppState, ctx: &RequestContext) -> Result<u64, ConnectError> {
    let token = bearer_token(ctx)
        .ok_or_else(|| ConnectError::unauthenticated("missing or malformed authorization header"))?;
    user_id_for_token(state, token).await
}

/// Returns the user id a session token belongs to, if the session still exists.
pub async fn user_id_for_token(state: &AppState, token: &str) -> Result<u64, ConnectError> {
    let token_hash = hash_token(token);
    let rows = state
        .session
        .execute_unpaged(
            "SELECT user_id, created_at, refreshed_at, id, device_name FROM sessions WHERE token_hash = ?",
            (&token_hash,),
        )
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?;
    let (db_user, created_at, refreshed_at, session_id, device_name) = rows
        .maybe_first_row::<(i64, Option<DateTime<Utc>>, Option<DateTime<Utc>>, Option<i64>, Option<String>)>()
        .map_err(internal)?
        .ok_or_else(|| ConnectError::unauthenticated("invalid or expired session token"))?;
    let user_id = id_from_db(db_user);
    if session_id.is_none() && crate::accounts::load(state, user_id).await?.is_none() {
        // Sessions from before session tracking are not listed, so deletion could not revoke them.
        return Err(ConnectError::unauthenticated("invalid or expired session token"));
    }
    // Sliding expiry: rewrite the session rows with a fresh TTL, at most once a day.
    let last_refresh = refreshed_at.or(created_at);
    if last_refresh.is_none_or(|t| Utc::now() - t >= SESSION_REFRESH_AFTER) {
        refresh_session(state, &token_hash, db_user, created_at, session_id, device_name.as_deref()).await;
    }
    if state.seen.due(&token_hash) {
        // Best effort: a failed write only makes "last seen" a bit older.
        if let Err(e) = state
            .session
            .execute_unpaged(
                "INSERT INTO session_seen (token_hash, last_seen_at) VALUES (?, toTimestamp(now())) USING TTL ?",
                (&token_hash, SESSION_TTL_SECS),
            )
            .await
        {
            tracing::debug!(error = %e, "recording session last-seen failed");
        }
    }
    Ok(user_id)
}

/// Pushes a session's expiry out by [`SESSION_TTL_SECS`]: INSERTs (not
/// UPDATEs) every row with a fresh TTL, so the row marker lives on too.
/// Best effort: a failure only means the old expiry stays.
async fn refresh_session(
    state: &AppState,
    token_hash: &str,
    db_user: i64,
    created_at: Option<DateTime<Utc>>,
    session_id: Option<i64>,
    device_name: Option<&str>,
) {
    let now = Utc::now();
    let result = async {
        state
            .session
            .execute_unpaged(
                "INSERT INTO sessions (token_hash, user_id, created_at, id, device_name, refreshed_at) VALUES (?, ?, ?, ?, ?, ?) USING TTL ?",
                (token_hash, db_user, created_at, session_id, device_name, now, SESSION_TTL_SECS),
            )
            .await?;
        if let Some(session_id) = session_id {
            state
                .session
                .execute_unpaged(
                    "INSERT INTO sessions_by_user (user_id, session_id, token_hash, device_name, created_at) VALUES (?, ?, ?, ?, ?) USING TTL ?",
                    (db_user, session_id, token_hash, device_name, created_at, SESSION_TTL_SECS),
                )
                .await?;
        }
        state
            .session
            .execute_unpaged(
                "INSERT INTO session_seen (token_hash, last_seen_at) VALUES (?, ?) USING TTL ?",
                (token_hash, now, SESSION_TTL_SECS),
            )
            .await?;
        Ok::<_, scylla::errors::ExecutionError>(())
    }
    .await;
    if let Err(e) = result {
        tracing::debug!(error = %e, "refreshing a session failed");
    }
}

/// Whether a session still exists. Scylla's TTL deletes the row at expiry, so
/// a missing row means the token is no longer valid; long-lived streams use
/// this to re-check periodically.
pub async fn session_exists(state: &AppState, token_hash: &str) -> Result<bool, ConnectError> {
    Ok(state
        .session
        .execute_unpaged("SELECT user_id FROM sessions WHERE token_hash = ?", (token_hash,))
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .maybe_first_row::<(i64,)>()
        .map_err(internal)?
        .is_some())
}

/// Longest device name kept on a session.
const MAX_DEVICE_NAME: usize = 64;

/// Trims and bounds a client-supplied device name; "" -> "Unknown device".
pub fn device_label(raw: &str) -> String {
    let name: String = raw.trim().chars().filter(|c| !c.is_control()).take(MAX_DEVICE_NAME).collect();
    if name.is_empty() { "Unknown device".to_owned() } else { name }
}

/// Creates a session for `user_id` and returns the plaintext token (only its
/// hash is stored). The session is also listed under the user.
pub async fn create_session(state: &AppState, user_id: u64, device_name: &str) -> Result<String, ConnectError> {
    // No sessions for suspended, deleted or unknown accounts, whatever the sign-in path.
    crate::accounts::load(state, user_id)
        .await?
        .ok_or_else(|| ConnectError::unauthenticated("user no longer exists"))?
        .ensure_not_suspended()?;
    let token = generate_token();
    let token_hash = hash_token(&token);
    let session_id = id_to_db(state.snowflake.next_id());
    let device = device_label(device_name);
    let db_user = id_to_db(user_id);
    state
        .session
        .execute_unpaged(
            "INSERT INTO sessions (token_hash, user_id, created_at, id, device_name, refreshed_at) VALUES (?, ?, toTimestamp(now()), ?, ?, toTimestamp(now())) USING TTL ?",
            (&token_hash, db_user, session_id, &device, SESSION_TTL_SECS),
        )
        .await
        .map_err(internal)?;
    state
        .session
        .execute_unpaged(
            "INSERT INTO sessions_by_user (user_id, session_id, token_hash, device_name, created_at) VALUES (?, ?, ?, ?, toTimestamp(now())) USING TTL ?",
            (db_user, session_id, &token_hash, &device, SESSION_TTL_SECS),
        )
        .await
        .map_err(internal)?;
    Ok(token)
}

/// A listed session.
pub struct SessionRow {
    pub id: u64,
    pub token_hash: String,
    pub device_name: String,
    pub created_at: Option<DateTime<Utc>>,
    pub last_seen_at: Option<DateTime<Utc>>,
}

/// The user's live sessions, newest first (pre-migration sessions are not listed).
pub async fn list_sessions(state: &AppState, user_id: u64) -> Result<Vec<SessionRow>, ConnectError> {
    let rows = state
        .session
        .execute_unpaged(
            "SELECT session_id, token_hash, device_name, created_at FROM sessions_by_user WHERE user_id = ?",
            (id_to_db(user_id),),
        )
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .rows::<(i64, Option<String>, Option<String>, Option<DateTime<Utc>>)>()
        .map_err(internal)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?;
    let mut out = Vec::with_capacity(rows.len());
    for (id, token_hash, device_name, created_at) in rows {
        let token_hash = token_hash.unwrap_or_default();
        let last_seen_at = state
            .session
            .execute_unpaged("SELECT last_seen_at FROM session_seen WHERE token_hash = ?", (&token_hash,))
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .maybe_first_row::<(Option<DateTime<Utc>>,)>()
            .map_err(internal)?
            .and_then(|(t,)| t);
        out.push(SessionRow {
            id: id_from_db(id),
            token_hash,
            device_name: device_name.unwrap_or_default(),
            created_at,
            last_seen_at,
        });
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.id));
    Ok(out)
}

/// Deletes one session of the user.
pub async fn revoke_session(state: &AppState, user_id: u64, session_id: u64, token_hash: &str) -> Result<(), ConnectError> {
    state
        .session
        .execute_unpaged("DELETE FROM sessions WHERE token_hash = ?", (token_hash,))
        .await
        .map_err(internal)?;
    state
        .session
        .execute_unpaged(
            "DELETE FROM sessions_by_user WHERE user_id = ? AND session_id = ?",
            (id_to_db(user_id), id_to_db(session_id)),
        )
        .await
        .map_err(internal)?;
    let _ = state.session.execute_unpaged("DELETE FROM session_seen WHERE token_hash = ?", (token_hash,)).await;
    Ok(())
}

/// Deletes every listed session of the user except `keep` (a token hash);
/// returns how many were deleted.
pub async fn revoke_all_sessions(state: &AppState, user_id: u64, keep: Option<&str>) -> Result<u32, ConnectError> {
    let mut revoked = 0;
    for row in list_sessions(state, user_id).await? {
        if Some(row.token_hash.as_str()) == keep {
            continue;
        }
        revoke_session(state, user_id, row.id, &row.token_hash).await?;
        revoked += 1;
    }
    Ok(revoked)
}

/// Throttles "last seen" writes to one per session per [`SEEN_EVERY`].
#[derive(Default)]
pub struct SeenThrottle {
    last: Mutex<HashMap<String, Instant>>,
}

const SEEN_EVERY: Duration = Duration::from_secs(5 * 60);
const SEEN_MAX_KEYS: usize = 50_000;

impl SeenThrottle {
    /// Whether a write for `token_hash` is due now (and records it).
    pub fn due(&self, token_hash: &str) -> bool {
        let now = Instant::now();
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if last.get(token_hash).is_some_and(|t| now.duration_since(*t) < SEEN_EVERY) {
            return false;
        }
        if last.len() >= SEEN_MAX_KEYS {
            last.retain(|_, t| now.duration_since(*t) < SEEN_EVERY);
            if last.len() >= SEEN_MAX_KEYS {
                last.clear();
            }
        }
        last.insert(token_hash.to_owned(), now);
        true
    }
}

/// 32 random bytes from the OS-seeded CSPRNG, base64url without padding.
pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Hex SHA-256 of a secret (session tokens, invite tokens and codes, one-time
/// codes, recovery codes).
pub fn hash_token(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Hashes a password with argon2id (default params) off the async runtime.
pub async fn hash_password(password: String) -> Result<String, ConnectError> {
    tokio::task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(internal)?
    .map_err(internal)
}

/// Checks a password against a stored hash off the async runtime.
pub async fn verify_password(password: String, hash: String) -> Result<bool, ConnectError> {
    tokio::task::spawn_blocking(move || {
        let parsed = PasswordHash::new(&hash).map_err(|e| e.to_string())?;
        Ok::<_, String>(Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
    })
    .await
    .map_err(internal)?
    .map_err(internal)
}

/// The `[applied]` flag of a lightweight-transaction result (whatever other
/// columns Scylla returns with it).
pub fn lwt_applied(result: scylla::response::query_result::QueryResult) -> Result<bool, ConnectError> {
    let row = result
        .into_rows_result()
        .map_err(internal)?
        .first_row::<scylla::value::Row>()
        .map_err(internal)?;
    Ok(row.columns.first().and_then(|c| c.as_ref()).and_then(|v| v.as_boolean()).unwrap_or(false))
}

/// Logs the real cause and returns an opaque internal error.
pub fn internal(err: impl std::fmt::Display) -> ConnectError {
    tracing::error!(error = %err, "internal error");
    ConnectError::internal("internal error")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dummy_hash_is_valid_and_rejects_guesses() {
        assert!(!verify_password("password".into(), DUMMY_PASSWORD_HASH.into()).await.unwrap());
    }

    #[tokio::test]
    async fn hash_and_verify_round_trip() {
        let hash = hash_password("correct horse".into()).await.unwrap();
        assert!(hash.starts_with("$argon2id$"));
        assert!(verify_password("correct horse".into(), hash.clone()).await.unwrap());
        assert!(!verify_password("wrong horse".into(), hash).await.unwrap());
    }

    #[test]
    fn tokens_are_unique_urlsafe_and_hash_is_hex() {
        let (a, b) = (generate_token(), generate_token());
        assert_ne!(a, b);
        assert_eq!(a.len(), 43);
        assert!(a.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'));
        let h = hash_token(&a);
        assert_eq!(h.len(), 64);
        assert!(h.bytes().all(|c| c.is_ascii_hexdigit()));
    }
}
