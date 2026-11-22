//! Accounts: creation (invite acceptance, CLI, test fixtures), lookups by
//! username or email, and the validation rules shared by the auth RPCs.

use chrono::{DateTime, Utc};
use connectrpc::ConnectError;

use crate::AppState;
use crate::auth::{hash_password, internal};
use crate::convert::{id_from_db, id_to_db, snowflake_datetime, user_to_proto};
use crate::proto::thehideout::v1::User;

pub const PASSWORD_LEN: std::ops::RangeInclusive<usize> = 8..=128;
pub const USERNAME_LEN: std::ops::RangeInclusive<usize> = 2..=32;
const MAX_EMAIL_LEN: usize = 254;

/// Trims and lowercases a username so lookups are case-insensitive.
pub fn normalize_username(raw: &str) -> String {
    raw.trim().to_lowercase()
}

/// Matches `^[a-z0-9_.]{2,32}$` on an already normalized username.
pub fn valid_username(username: &str) -> bool {
    USERNAME_LEN.contains(&username.len())
        && username
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.')
}

pub fn check_username(username: &str) -> Result<(), ConnectError> {
    if valid_username(username) {
        Ok(())
    } else {
        Err(ConnectError::invalid_argument("username must be 2-32 characters of a-z, 0-9, '_' or '.'"))
    }
}

pub fn check_password(password: &str) -> Result<(), ConnectError> {
    if PASSWORD_LEN.contains(&password.chars().count()) {
        Ok(())
    } else {
        Err(ConnectError::invalid_argument("password must be 8-128 characters"))
    }
}

/// Trims and lowercases an email address.
pub fn normalize_email(raw: &str) -> String {
    raw.trim().to_lowercase()
}

/// A deliberately loose syntax check on a normalized address: one `@`, a
/// non-empty local part, a dotted domain, no whitespace or angle brackets.
pub fn valid_email(email: &str) -> bool {
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    email.len() <= MAX_EMAIL_LEN
        && !local.is_empty()
        && !domain.contains('@')
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && email.chars().all(|c| !c.is_whitespace() && !c.is_control() && !"<>,;\"()[]\\".contains(c))
}

pub fn check_email(email: &str) -> Result<(), ConnectError> {
    if valid_email(email) {
        Ok(())
    } else {
        Err(ConnectError::invalid_argument("invalid email address"))
    }
}

/// `alice@example.com` -> `a***@example.com`.
pub fn mask_email(email: &str) -> String {
    match email.split_once('@') {
        Some((local, domain)) => {
            let first: String = local.chars().take(1).collect();
            format!("{first}***@{domain}")
        }
        None => "***".to_owned(),
    }
}

/// A user row with the authentication columns.
#[derive(Debug, Clone)]
pub struct Account {
    pub id: u64,
    pub username: String,
    /// `None` for passwordless accounts.
    pub password_hash: Option<String>,
    /// `None` for legacy accounts.
    pub email: Option<String>,
    pub totp_secret: Option<String>,
    pub totp_last_step: i64,
    pub created_at: DateTime<Utc>,
    pub avatar: Option<String>,
    pub is_admin: bool,
    /// `deleting` / `deleted` once account deletion started.
    pub deletion_state: Option<String>,
    pub suspended: bool,
    pub suspended_until: Option<DateTime<Utc>>,
    pub suspended_reason: Option<String>,
    /// The terms version the user accepted last, if any.
    pub accepted_terms_version: Option<String>,
}

impl Account {
    /// The suspension in force now (reason, end), if any. A suspension with
    /// an `until` in the past is over.
    pub fn active_suspension(&self) -> Option<(String, Option<DateTime<Utc>>)> {
        if !self.suspended || self.suspended_until.is_some_and(|until| until <= Utc::now()) {
            return None;
        }
        Some((self.suspended_reason.clone().unwrap_or_default(), self.suspended_until))
    }

    /// Fails with the sign-in refusal (`AccountSuspended` detail) when suspended.
    pub fn ensure_not_suspended(&self) -> Result<(), ConnectError> {
        match self.active_suspension() {
            Some((reason, until)) => Err(suspended_error(&reason, until)),
            None => Ok(()),
        }
    }

    /// `Some(version)` while the user still has to accept the current terms.
    pub fn terms_update_required(&self) -> Option<String> {
        (self.accepted_terms_version.as_deref() != Some(crate::policy::LEGAL_VERSION))
            .then(|| crate::policy::LEGAL_VERSION.to_owned())
    }

    /// The user as shown to themself (with `is_admin` and `email`).
    pub fn to_proto(&self) -> User {
        let mut user = user_to_proto(self.id, &self.username, self.created_at, self.avatar.clone());
        user.is_admin = self.is_admin;
        user.email = self.email.clone();
        user.deleted = self.deletion_state.is_some();
        user
    }

    pub fn has_totp(&self) -> bool {
        self.totp_secret.is_some()
    }
}

type AccountRow = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<DateTime<Utc>>,
    Option<String>,
    Option<bool>,
    Option<String>,
    Option<bool>,
    Option<DateTime<Utc>>,
    Option<String>,
    Option<String>,
);

/// The sign-in refusal of a suspended account: PERMISSION_DENIED with an
/// `AccountSuspended` detail.
pub fn suspended_error(reason: &str, until: Option<DateTime<Utc>>) -> ConnectError {
    let detail = crate::proto::thehideout::v1::AccountSuspended {
        reason: reason.to_owned(),
        until: until.map(crate::convert::datetime_to_proto).into(),
        ..Default::default()
    };
    ConnectError::permission_denied("Your account is suspended")
        .with_detail(connectrpc::ErrorDetail::from_message("thehideout.v1.AccountSuspended", &detail))
}

/// Loads the account with id `id`; `None` for unknown and for deleted (or
/// being deleted) accounts, which are gone as far as every caller is concerned.
pub async fn load(state: &AppState, id: u64) -> Result<Option<Account>, ConnectError> {
    Ok(load_any(state, id).await?.filter(|a| a.deletion_state.is_none()))
}

/// Like [`load`], including accounts whose deletion started (the cleanup job's view).
pub async fn load_any(state: &AppState, id: u64) -> Result<Option<Account>, ConnectError> {
    let row = state
        .session
        .execute_unpaged(
            "SELECT username, password_hash, email, totp_secret, totp_last_step, created_at, avatar, is_admin, \
             deletion_state, suspended, suspended_until, suspended_reason, accepted_terms_version FROM users WHERE id = ?",
            (id_to_db(id),),
        )
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .maybe_first_row::<AccountRow>()
        .map_err(internal)?;
    Ok(row.and_then(|(username, hash, email, totp, step, created_at, avatar, is_admin, deletion_state, suspended, suspended_until, suspended_reason, accepted)| {
        Some(Account {
            id,
            username: username?,
            password_hash: hash.filter(|h| !h.is_empty()),
            email: email.filter(|e| !e.is_empty()),
            totp_secret: totp.filter(|s| !s.is_empty()),
            totp_last_step: step.unwrap_or(0),
            created_at: created_at.unwrap_or_else(|| snowflake_datetime(id)),
            avatar,
            is_admin: is_admin.unwrap_or(false),
            deletion_state: deletion_state.filter(|s| !s.is_empty()),
            suspended: suspended.unwrap_or(false),
            suspended_until,
            suspended_reason,
            accepted_terms_version: accepted.filter(|v| !v.is_empty()),
        })
    }))
}

async fn id_by(state: &AppState, cql: &'static str, key: &str) -> Result<Option<u64>, ConnectError> {
    Ok(state
        .session
        .execute_unpaged(cql, (key,))
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .maybe_first_row::<(i64,)>()
        .map_err(internal)?
        .map(|(id,)| id_from_db(id)))
}

pub async fn id_by_username(state: &AppState, username: &str) -> Result<Option<u64>, ConnectError> {
    id_by(state, "SELECT user_id FROM users_by_username WHERE username = ?", username).await
}

pub async fn id_by_email(state: &AppState, email: &str) -> Result<Option<u64>, ConnectError> {
    id_by(state, "SELECT user_id FROM users_by_email WHERE email = ?", email).await
}

/// The account a login name refers to: an email if it contains `@`, else a username.
pub async fn find_by_login(state: &AppState, login: &str) -> Result<Option<Account>, ConnectError> {
    let login = login.trim().to_lowercase();
    let id = if login.contains('@') { id_by_email(state, &login).await? } else { id_by_username(state, &login).await? };
    match id {
        Some(id) => load(state, id).await,
        None => Ok(None),
    }
}

pub async fn find_by_email(state: &AppState, email: &str) -> Result<Option<Account>, ConnectError> {
    match id_by_email(state, &normalize_email(email)).await? {
        Some(id) => load(state, id).await,
        None => Ok(None),
    }
}

/// Claims `email` for `user_id`; false if another account holds it.
pub async fn claim_email(state: &AppState, email: &str, user_id: u64) -> Result<bool, ConnectError> {
    let (applied, _, holder) = state
        .session
        .execute_unpaged(
            "INSERT INTO users_by_email (email, user_id) VALUES (?, ?) IF NOT EXISTS",
            (email, id_to_db(user_id)),
        )
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .first_row::<(bool, Option<String>, Option<i64>)>()
        .map_err(internal)?;
    // Claiming an address the user already holds is fine.
    Ok(applied || holder == Some(id_to_db(user_id)))
}

/// Releases `email` if `user_id` holds it.
pub async fn release_email(state: &AppState, email: &str, user_id: u64) -> Result<(), ConnectError> {
    state
        .session
        .execute_unpaged("DELETE FROM users_by_email WHERE email = ? IF user_id = ?", (email, id_to_db(user_id)))
        .await
        .map_err(internal)?;
    Ok(())
}

/// A new account.
pub struct NewUser<'a> {
    pub username: &'a str,
    /// `None` = passwordless.
    pub password: Option<&'a str>,
    /// `None` only for test fixtures of legacy accounts.
    pub email: Option<&'a str>,
}

/// Creates an account: validates, hashes the password, writes the row and
/// claims the username and email. A taken username or email fails with
/// ALREADY_EXISTS and leaves nothing behind. Applies `THEHIDEOUT_ADMINS`.
pub async fn create_user(state: &AppState, new: NewUser<'_>) -> Result<Account, ConnectError> {
    let username = normalize_username(new.username);
    check_username(&username)?;
    if let Some(password) = new.password {
        check_password(password)?;
    }
    let email = new.email.map(normalize_email);
    if let Some(email) = &email {
        check_email(email)?;
    }
    let password_hash = match new.password {
        Some(p) => Some(hash_password(p.to_owned()).await?),
        None => None,
    };
    let id = state.snowflake.next_id();
    let db_id = id_to_db(id);
    let created_at = snowflake_datetime(id);

    // Write the user row before claiming the username: a failure here leaves
    // only an unreachable orphan, never a claimed name without an account.
    state
        .session
        .execute_unpaged(
            "INSERT INTO users (id, username, password_hash, created_at, email) VALUES (?, ?, ?, ?, ?)",
            (db_id, &username, password_hash.as_deref().unwrap_or(""), created_at, &email),
        )
        .await
        .map_err(internal)?;

    // An LWT returns `[applied]` plus the existing row's columns when not applied.
    let (claimed, _, _) = state
        .session
        .execute_unpaged(
            "INSERT INTO users_by_username (username, user_id) VALUES (?, ?) IF NOT EXISTS",
            (&username, db_id),
        )
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .first_row::<(bool, Option<String>, Option<i64>)>()
        .map_err(internal)?;
    if !claimed {
        delete_orphan(state, id).await;
        return Err(ConnectError::already_exists("that username is taken"));
    }
    if let Some(email) = &email
        && !claim_email(state, email, id).await?
    {
        if let Err(err) = state
            .session
            .execute_unpaged("DELETE FROM users_by_username WHERE username = ? IF user_id = ?", (&username, db_id))
            .await
        {
            tracing::warn!(user_id = id, error = %err, "failed to release username");
        }
        delete_orphan(state, id).await;
        return Err(ConnectError::already_exists("an account with that email already exists"));
    }

    let is_admin = state.admins.contains(&username);
    if is_admin {
        crate::admin::set_flag(state, id, true).await?;
    }
    crate::stats::user_signed_up(state).await;
    tracing::info!(user_id = id, "account created");
    Ok(Account {
        id,
        username,
        password_hash,
        email,
        totp_secret: None,
        totp_last_step: 0,
        created_at,
        avatar: None,
        is_admin,
        deletion_state: None,
        suspended: false,
        suspended_until: None,
        suspended_reason: None,
        accepted_terms_version: None,
    })
}

async fn delete_orphan(state: &AppState, id: u64) {
    if let Err(err) = state.session.execute_unpaged("DELETE FROM users WHERE id = ?", (id_to_db(id),)).await {
        tracing::warn!(user_id = id, error = %err, "failed to delete orphaned user row");
    }
}

/// Whether the install has no accounts yet.
pub async fn none_exist(state: &AppState) -> Result<bool, ConnectError> {
    Ok(state
        .session
        .execute_unpaged("SELECT username FROM users_by_username LIMIT 1", ())
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .rows_num()
        == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_rules() {
        for ok in ["a@b.co", "first.last+tag@sub.example.org"] {
            assert!(valid_email(ok), "{ok}");
        }
        for bad in ["", "a", "a@b", "@b.co", "a@@b.co", "a b@c.de", "a@.co", "a@co.", "<a@b.co>"] {
            assert!(!valid_email(bad), "{bad}");
        }
        assert_eq!(normalize_email("  Alice@Example.COM "), "alice@example.com");
        assert_eq!(mask_email("alice@example.com"), "a***@example.com");
    }

    #[test]
    fn username_rules() {
        assert!(valid_username("al.ice_9"));
        assert!(!valid_username("a") && !valid_username("Alice") && !valid_username(&"a".repeat(33)));
    }
}
