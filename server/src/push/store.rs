//! Device storage. Every device row carries the session TTL and is renewed
//! when the app registers again; signing out, account deletion and suspension
//! delete devices explicitly, and a device whose session no longer exists is
//! dropped the next time a push would go to it.

use chrono::{DateTime, Utc};
use connectrpc::ConnectError;
use scylla::deserialize::row::DeserializeRow;
use scylla::serialize::row::SerializeRow;

use super::{Device, Platform};
use crate::AppState;
use crate::auth::{SESSION_TTL_SECS, internal};
use crate::convert::{id_from_db, id_to_db};

/// Devices one user may have registered.
pub const MAX_DEVICES: usize = 10;

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

type DeviceRow = (i64, Option<String>, Option<String>, Option<String>, Option<String>, Option<bool>);

fn to_device(user_id: u64, (id, session_hash, platform, token, locale, sandbox): DeviceRow) -> Option<Device> {
    Some(Device {
        id: id_from_db(id),
        user_id,
        session_hash: session_hash.unwrap_or_default(),
        platform: Platform::parse(platform.as_deref()?)?,
        token: token.filter(|t| !t.is_empty())?,
        locale: locale.filter(|l| !l.is_empty()),
        sandbox: sandbox.unwrap_or(false),
    })
}

/// What the app told us when it registered.
pub struct Registration<'a> {
    pub platform: Platform,
    pub token: &'a str,
    pub app_version: &'a str,
    pub device_name: &'a str,
    pub locale: Option<&'a str>,
    pub sandbox: bool,
}

/// Stores (or refreshes) a device for `session_hash` and returns its id. A
/// token has one owner: registering it again keeps the id, and a token another
/// account held moves to the caller. A session keeps one device per platform
/// (a rotated token replaces the old one).
pub async fn register(state: &AppState, user_id: u64, session_hash: &str, reg: &Registration<'_>) -> Result<u64, ConnectError> {
    let platform = reg.platform.as_str();
    let db_user = id_to_db(user_id);
    let existing = rows::<(i64, i64)>(state, "SELECT user_id, device_id FROM push_devices_by_token WHERE platform = ? AND push_token = ?", (platform, reg.token))
        .await?
        .into_iter()
        .next();
    let mut id = None;
    if let Some((owner, device_id)) = existing {
        if owner == db_user {
            id = Some(device_id);
        } else {
            delete_device(state, id_from_db(owner), id_from_db(device_id)).await?;
        }
    }
    // The session's other devices of this platform are replaced by this one.
    for (device_id,) in rows::<(i64,)>(state, "SELECT device_id FROM push_devices_by_session WHERE session_hash = ?", (session_hash,)).await? {
        if Some(device_id) == id {
            continue;
        }
        let old = rows::<(Option<String>,)>(state, "SELECT platform FROM push_devices WHERE user_id = ? AND id = ?", (db_user, device_id)).await?;
        if old.first().and_then(|(p,)| p.as_deref()) == Some(platform) {
            delete_device(state, user_id, id_from_db(device_id)).await?;
        }
    }
    let id = match id {
        Some(id) => id,
        None => {
            let count = rows::<(i64,)>(state, "SELECT id FROM push_devices WHERE user_id = ?", (db_user,)).await?.len();
            if count >= MAX_DEVICES {
                return Err(ConnectError::resource_exhausted("too many registered devices; sign out of one first"));
            }
            id_to_db(state.snowflake.next_id())
        }
    };
    let now = Utc::now();
    exec(
        state,
        "INSERT INTO push_devices (user_id, id, session_hash, platform, push_token, app_version, device_name, locale, sandbox, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) USING TTL ?",
        (db_user, id, session_hash, platform, reg.token, reg.app_version, reg.device_name, reg.locale, reg.sandbox, now, now, SESSION_TTL_SECS),
    )
    .await?;
    exec(
        state,
        "INSERT INTO push_devices_by_token (platform, push_token, user_id, device_id) VALUES (?, ?, ?, ?) USING TTL ?",
        (platform, reg.token, db_user, id, SESSION_TTL_SECS),
    )
    .await?;
    exec(
        state,
        "INSERT INTO push_devices_by_session (session_hash, device_id, user_id) VALUES (?, ?, ?) USING TTL ?",
        (session_hash, id, db_user, SESSION_TTL_SECS),
    )
    .await?;
    Ok(id_from_db(id))
}

/// The user's registered devices.
pub async fn list(state: &AppState, user_id: u64) -> Result<Vec<Device>, ConnectError> {
    Ok(rows::<DeviceRow>(state, "SELECT id, session_hash, platform, push_token, locale, sandbox FROM push_devices WHERE user_id = ?", (id_to_db(user_id),))
        .await?
        .into_iter()
        .filter_map(|row| to_device(user_id, row))
        .collect())
}

/// Removes one device (all three rows); a no-op for an unknown id.
pub async fn delete_device(state: &AppState, user_id: u64, device_id: u64) -> Result<(), ConnectError> {
    let (db_user, db_id) = (id_to_db(user_id), id_to_db(device_id));
    let row = rows::<(Option<String>, Option<String>, Option<String>)>(
        state,
        "SELECT session_hash, platform, push_token FROM push_devices WHERE user_id = ? AND id = ?",
        (db_user, db_id),
    )
    .await?
    .into_iter()
    .next();
    if let Some((session_hash, platform, token)) = row {
        if let (Some(platform), Some(token)) = (platform, token) {
            // Only while the token still points at this device.
            exec(state, "DELETE FROM push_devices_by_token WHERE platform = ? AND push_token = ? IF device_id = ?", (platform, token, db_id)).await?;
        }
        if let Some(session_hash) = session_hash {
            exec(state, "DELETE FROM push_devices_by_session WHERE session_hash = ? AND device_id = ?", (session_hash, db_id)).await?;
        }
    }
    exec(state, "DELETE FROM push_devices WHERE user_id = ? AND id = ?", (db_user, db_id)).await
}

/// Removes the device with this token, if the user has it.
pub async fn delete_by_token(state: &AppState, user_id: u64, token: &str) -> Result<(), ConnectError> {
    for device in list(state, user_id).await? {
        if device.token == token {
            delete_device(state, user_id, device.id).await?;
        }
    }
    Ok(())
}

/// Removes the devices of a session (sign-out, revocation).
pub async fn delete_for_session(state: &AppState, session_hash: &str) -> Result<(), ConnectError> {
    for (device_id, user_id) in rows::<(i64, i64)>(state, "SELECT device_id, user_id FROM push_devices_by_session WHERE session_hash = ?", (session_hash,)).await? {
        delete_device(state, id_from_db(user_id), id_from_db(device_id)).await?;
    }
    Ok(())
}

/// Removes every device and every notification setting of the user (account deletion).
pub async fn delete_all_for_user(state: &AppState, user_id: u64) -> Result<(), ConnectError> {
    for device in list(state, user_id).await? {
        delete_device(state, user_id, device.id).await?;
    }
    // Rows whose columns were all expired or partly written.
    exec(state, "DELETE FROM push_devices WHERE user_id = ?", (id_to_db(user_id),)).await?;
    exec(state, "DELETE FROM notification_settings WHERE user_id = ?", (id_to_db(user_id),)).await?;
    exec(state, "DELETE FROM notification_overrides WHERE user_id = ?", (id_to_db(user_id),)).await
}

/// When the device was last registered (tests and diagnostics).
pub async fn updated_at(state: &AppState, user_id: u64, device_id: u64) -> Result<Option<DateTime<Utc>>, ConnectError> {
    Ok(rows::<(Option<DateTime<Utc>>,)>(state, "SELECT updated_at FROM push_devices WHERE user_id = ? AND id = ?", (id_to_db(user_id), id_to_db(device_id)))
        .await?
        .into_iter()
        .next()
        .and_then(|(t,)| t))
}
