//! The error type of every API call.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

use connectrpc::{ConnectError, ErrorCode};

use crate::proto::thehideout::v1 as pb;

/// Why an API call failed, mapped from the Connect error code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    /// Missing, invalid or expired session token, or wrong username/password (and, so they don't
    /// reveal whether an account exists, a wrong emailed sign-in or reset code).
    Unauthenticated,
    /// A second-factor answer or a signed-in code check (authenticator code, recovery code,
    /// email-change code, passkey) was wrong; that many more tries are accepted.
    WrongCode { attempts_remaining: u32 },
    /// The sign-in challenge, code or setup expired, was used, or never existed: start again.
    CodeExpired,
    /// Too many wrong answers: the challenge or code is dead, start again.
    TooManyAttempts,
    /// The request was rejected as malformed (empty or over-long message, bad username, ...).
    InvalidArgument(String),
    /// The guild, channel, user or invite does not exist (or is not visible to the caller).
    NotFound,
    /// The caller is not allowed to do this (e.g. creating a channel in a guild they don't own).
    PermissionDenied,
    /// The change was based on a stale version (layout or role order); refetch and retry.
    Conflict,
    /// The server does not implement this call.
    Unimplemented,
    /// Too many attempts; try again later.
    RateLimited,
    /// The resource already exists. Sign-up reports a taken username this way.
    AlreadyExists,
    /// Accounts are invite-only: sign up by accepting an invite
    /// ([`Client::accept_invite`](crate::Client::accept_invite)).
    InviteRequired,
    /// The operation is not allowed in the current state (e.g. demoting yourself or the last admin).
    Precondition(String),
    /// The upload is larger than the server (or the local limit) allows.
    TooLarge,
    /// The server refuses this app version (FAILED_PRECONDITION, message starting
    /// "client too old"): the user must install a newer build.
    ClientTooOld,
    /// Sign-in refused because an admin suspended the account (PERMISSION_DENIED with an
    /// `AccountSuspended` detail).
    Suspended { reason: String, until: Option<std::time::SystemTime> },
    /// The invitee did not accept the current terms (or the version sent is out of date): fetch
    /// the version from the invite preview and ask again.
    TermsUpdateRequired,
    /// The user still owns these guilds: transfer or delete them first (deleting the account).
    OwnsGuilds(Vec<OwnedGuild>),
    /// The server could not be reached or the connection broke.
    Network(String),
    /// Anything else, with the server's message.
    Other(String),
}

impl ApiError {
    /// Whether retrying the same call later might succeed.
    pub fn is_transient(&self) -> bool {
        matches!(self, Self::Network(_) | Self::RateLimited)
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthenticated => f.write_str("not signed in or invalid credentials"),
            Self::WrongCode { attempts_remaining: 1 } => f.write_str("wrong code, 1 try left"),
            Self::WrongCode { attempts_remaining } => write!(f, "wrong code, {attempts_remaining} tries left"),
            Self::CodeExpired => f.write_str("expired, start again"),
            Self::TooManyAttempts => f.write_str("too many wrong attempts, start again"),
            Self::InvalidArgument(msg) => write!(f, "invalid request: {msg}"),
            Self::NotFound => f.write_str("not found"),
            Self::PermissionDenied => f.write_str("permission denied"),
            Self::Conflict => f.write_str("changed by someone else, refresh and try again"),
            Self::Unimplemented => f.write_str("not supported by this server"),
            Self::RateLimited => f.write_str("too many attempts, try again later"),
            Self::AlreadyExists => f.write_str("already exists"),
            Self::InviteRequired => f.write_str("an invite is required to create an account"),
            Self::Precondition(msg) if msg.is_empty() => f.write_str("not allowed right now"),
            Self::Precondition(msg) => write!(f, "not allowed: {msg}"),
            Self::TooLarge => f.write_str("file is too large"),
            Self::ClientTooOld => f.write_str("this version of thehideout is too old, update required"),
            Self::Suspended { reason, .. } if reason.is_empty() => f.write_str("your account is suspended"),
            Self::Suspended { reason, .. } => write!(f, "your account is suspended: {reason}"),
            Self::TermsUpdateRequired => f.write_str("the terms must be accepted first"),
            Self::OwnsGuilds(guilds) => {
                let names: Vec<&str> = guilds.iter().map(|g| g.name.as_str()).collect();
                write!(f, "transfer or delete the servers you own first: {}", names.join(", "))
            }
            Self::Network(msg) => write!(f, "network error: {msg}"),
            Self::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for ApiError {}

impl ApiError {
    /// Maps the HTTP status of a plain (non-Connect) request, e.g. `POST /attachments`.
    pub(crate) fn from_status(status: u16, body: &str) -> Self {
        let msg = body.trim().to_owned();
        match status {
            400 => Self::InvalidArgument(msg),
            401 => Self::Unauthenticated,
            403 => Self::PermissionDenied,
            404 => Self::NotFound,
            413 => Self::TooLarge,
            429 => Self::RateLimited,
            503 | 504 => Self::Network(msg),
            s => Self::Other(if msg.is_empty() { format!("request failed with status {s}") } else { msg }),
        }
    }
}

/// The server's message for a sign-up without an invite.
const INVITE_REQUIRED: &str = "invite required";
/// Prefix of the FAILED_PRECONDITION message a server sends to a client it no longer supports.
const CLIENT_TOO_OLD: &str = "client too old";

static UPDATE_REQUIRED: AtomicBool = AtomicBool::new(false);

/// Whether any call so far was refused with [`ApiError::ClientTooOld`]. Sticky for the process:
/// the app polls it to show its blocking "Update required" screen, wherever the call was made
/// (a request or the gateway stream).
pub fn update_required() -> bool {
    UPDATE_REQUIRED.load(Ordering::Relaxed)
}
/// A guild that blocks deleting its owner's account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedGuild {
    pub id: u64,
    pub name: String,
    /// 1 = only the owner is in it.
    pub member_count: u32,
}

const ACCOUNT_SUSPENDED: &str = "thehideout.v1.AccountSuspended";
const OWNED_GUILDS: &str = "thehideout.v1.OwnedGuilds";
/// Start of the INVALID_ARGUMENT message of a sign-up that did not accept the terms.
const TERMS_REQUIRED: &str = "you must accept the terms";

fn detail_bytes(err: &ConnectError, type_name: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    let detail = err.details.iter().find(|d| d.type_url.trim_start_matches("type.googleapis.com/") == type_name)?;
    let raw = detail.value.as_deref()?.trim_end_matches('=');
    base64::engine::general_purpose::STANDARD_NO_PAD.decode(raw).ok()
}

/// The suspension or owned-guilds detail of an error, if it has one.
fn lifecycle_error(err: &ConnectError) -> Option<ApiError> {
    use buffa::Message as _;
    if let Some(bytes) = detail_bytes(err, ACCOUNT_SUSPENDED) {
        let d = pb::AccountSuspended::decode_from_slice(&bytes).ok()?;
        let until = d.until.as_option().map(|ts| crate::types::time(Some(ts)));
        return Some(ApiError::Suspended { reason: d.reason, until });
    }
    if let Some(bytes) = detail_bytes(err, OWNED_GUILDS) {
        let d = pb::OwnedGuilds::decode_from_slice(&bytes).ok()?;
        let guilds = d.guilds.into_iter().map(|g| OwnedGuild { id: g.id, name: g.name, member_count: g.member_count }).collect();
        return Some(ApiError::OwnsGuilds(guilds));
    }
    None
}

/// Type of the error detail on rejected codes.
const CODE_ERROR: &str = "thehideout.v1.CodeError";

/// The `CodeError` detail of a rejected code, if the error has one.
fn code_error(err: &ConnectError) -> Option<ApiError> {
    use base64::Engine as _;
    use buffa::Message as _;
    let detail = err.details.iter().find(|d| d.type_url.trim_start_matches("type.googleapis.com/") == CODE_ERROR)?;
    let raw = detail.value.as_deref()?.trim_end_matches('=');
    let bytes = base64::engine::general_purpose::STANDARD_NO_PAD.decode(raw).ok()?;
    let detail = pb::CodeError::decode_from_slice(&bytes).ok()?;
    match detail.reason.as_known()? {
        pb::CodeErrorReason::CODE_ERROR_REASON_WRONG => Some(ApiError::WrongCode { attempts_remaining: detail.attempts_remaining }),
        pb::CodeErrorReason::CODE_ERROR_REASON_EXPIRED => Some(ApiError::CodeExpired),
        pb::CodeErrorReason::CODE_ERROR_REASON_TOO_MANY_ATTEMPTS => Some(ApiError::TooManyAttempts),
        _ => None,
    }
}

impl From<ConnectError> for ApiError {
    fn from(err: ConnectError) -> Self {
        if let Some(e) = code_error(&err) {
            return e;
        }
        if let Some(e) = lifecycle_error(&err) {
            return e;
        }
        let msg = err.message.clone().unwrap_or_default();
        match err.code {
            ErrorCode::Unauthenticated => Self::Unauthenticated,
            ErrorCode::InvalidArgument if msg.starts_with(TERMS_REQUIRED) => Self::TermsUpdateRequired,
            ErrorCode::InvalidArgument | ErrorCode::OutOfRange => Self::InvalidArgument(msg),
            ErrorCode::NotFound => Self::NotFound,
            ErrorCode::PermissionDenied => Self::PermissionDenied,
            ErrorCode::Aborted => Self::Conflict,
            ErrorCode::Unimplemented => Self::Unimplemented,
            ErrorCode::ResourceExhausted => Self::RateLimited,
            ErrorCode::AlreadyExists => Self::AlreadyExists,
            ErrorCode::FailedPrecondition if msg == INVITE_REQUIRED => Self::InviteRequired,
            ErrorCode::FailedPrecondition if msg.to_ascii_lowercase().starts_with(CLIENT_TOO_OLD) => {
                UPDATE_REQUIRED.store(true, Ordering::Relaxed);
                Self::ClientTooOld
            }
            ErrorCode::FailedPrecondition => Self::Precondition(msg),
            ErrorCode::Unavailable | ErrorCode::DeadlineExceeded => Self::Network(msg),
            _ => Self::Other(msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(code: ErrorCode) -> ApiError {
        ConnectError::new(code, "boom").into()
    }

    #[test]
    fn maps_connect_codes() {
        assert_eq!(map(ErrorCode::Unauthenticated), ApiError::Unauthenticated);
        assert_eq!(map(ErrorCode::InvalidArgument), ApiError::InvalidArgument("boom".into()));
        assert_eq!(map(ErrorCode::NotFound), ApiError::NotFound);
        assert_eq!(map(ErrorCode::PermissionDenied), ApiError::PermissionDenied);
        assert_eq!(map(ErrorCode::ResourceExhausted), ApiError::RateLimited);
        assert_eq!(map(ErrorCode::AlreadyExists), ApiError::AlreadyExists);
        assert_eq!(map(ErrorCode::FailedPrecondition), ApiError::Precondition("boom".into()));
        assert_eq!(ApiError::from(ConnectError::new(ErrorCode::FailedPrecondition, "invite required")), ApiError::InviteRequired);
        assert_eq!(ApiError::from(ConnectError::new(ErrorCode::FailedPrecondition, "client too old: need 0.2.0")), ApiError::ClientTooOld);
        assert!(update_required());
        // Only FAILED_PRECONDITION, and only that prefix.
        assert_eq!(map(ErrorCode::Internal), ApiError::Other("boom".into()));
        assert_eq!(ApiError::from(ConnectError::new(ErrorCode::NotFound, "client too old")), ApiError::NotFound);
        assert_eq!(map(ErrorCode::Unavailable), ApiError::Network("boom".into()));
        assert_eq!(map(ErrorCode::Aborted), ApiError::Conflict);
        assert_eq!(map(ErrorCode::Unimplemented), ApiError::Unimplemented);
        assert_eq!(map(ErrorCode::Internal), ApiError::Other("boom".into()));
    }

    fn with_detail(code: ErrorCode, reason: pb::CodeErrorReason, attempts_remaining: u32) -> ApiError {
        let detail = pb::CodeError { reason: reason.into(), attempts_remaining, ..Default::default() };
        ConnectError::new(code, "boom")
            .with_detail(connectrpc::ErrorDetail::from_message(CODE_ERROR, &detail))
            .into()
    }

    #[test]
    fn maps_code_error_details() {
        use pb::CodeErrorReason::*;
        assert_eq!(with_detail(ErrorCode::InvalidArgument, CODE_ERROR_REASON_WRONG, 3), ApiError::WrongCode { attempts_remaining: 3 });
        assert_eq!(with_detail(ErrorCode::FailedPrecondition, CODE_ERROR_REASON_EXPIRED, 0), ApiError::CodeExpired);
        assert_eq!(with_detail(ErrorCode::ResourceExhausted, CODE_ERROR_REASON_TOO_MANY_ATTEMPTS, 0), ApiError::TooManyAttempts);
        // An unknown reason falls back to the code.
        assert_eq!(with_detail(ErrorCode::InvalidArgument, CODE_ERROR_REASON_UNSPECIFIED, 0), ApiError::InvalidArgument("boom".into()));
        assert!(!ApiError::TooManyAttempts.is_transient());
        assert_eq!(ApiError::WrongCode { attempts_remaining: 1 }.to_string(), "wrong code, 1 try left");
    }

    #[test]
    fn maps_lifecycle_details() {
        let until = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000);
        let suspended = pb::AccountSuspended { reason: "spam".into(), until: Some(buffa_types::google::protobuf::Timestamp::from(until)).into(), ..Default::default() };
        let err: ApiError = ConnectError::new(ErrorCode::PermissionDenied, "Your account is suspended")
            .with_detail(connectrpc::ErrorDetail::from_message(ACCOUNT_SUSPENDED, &suspended))
            .into();
        assert_eq!(err, ApiError::Suspended { reason: "spam".into(), until: Some(until) });
        assert_eq!(err.to_string(), "your account is suspended: spam");
        // Without an `until` the suspension is indefinite.
        let indefinite = pb::AccountSuspended { reason: String::new(), ..Default::default() };
        let err: ApiError = ConnectError::new(ErrorCode::PermissionDenied, "x")
            .with_detail(connectrpc::ErrorDetail::from_message(ACCOUNT_SUSPENDED, &indefinite))
            .into();
        assert_eq!(err, ApiError::Suspended { reason: String::new(), until: None });
        // A bare PERMISSION_DENIED stays one.
        assert_eq!(map(ErrorCode::PermissionDenied), ApiError::PermissionDenied);

        let owned = pb::OwnedGuilds { guilds: vec![pb::OwnedGuild { id: 7, name: "den".into(), member_count: 3, ..Default::default() }], ..Default::default() };
        let err: ApiError = ConnectError::new(ErrorCode::FailedPrecondition, "transfer")
            .with_detail(connectrpc::ErrorDetail::from_message(OWNED_GUILDS, &owned))
            .into();
        assert_eq!(err, ApiError::OwnsGuilds(vec![OwnedGuild { id: 7, name: "den".into(), member_count: 3 }]));
        assert!(err.to_string().contains("den"));

        let terms: ApiError = ConnectError::new(ErrorCode::InvalidArgument, "you must accept the terms and privacy notice (version 1)").into();
        assert_eq!(terms, ApiError::TermsUpdateRequired);
        assert_eq!(map(ErrorCode::InvalidArgument), ApiError::InvalidArgument("boom".into()));
    }

    #[test]
    fn maps_http_statuses() {
        assert_eq!(ApiError::from_status(401, ""), ApiError::Unauthenticated);
        assert_eq!(ApiError::from_status(403, ""), ApiError::PermissionDenied);
        assert_eq!(ApiError::from_status(404, ""), ApiError::NotFound);
        assert_eq!(ApiError::from_status(413, ""), ApiError::TooLarge);
        assert_eq!(ApiError::from_status(429, ""), ApiError::RateLimited);
        assert_eq!(ApiError::from_status(400, " bad\n"), ApiError::InvalidArgument("bad".into()));
        assert_eq!(ApiError::from_status(500, ""), ApiError::Other("request failed with status 500".into()));
    }

    #[test]
    fn transient_errors() {
        assert!(ApiError::Network(String::new()).is_transient());
        assert!(ApiError::RateLimited.is_transient());
        assert!(!ApiError::NotFound.is_transient());
    }
}
