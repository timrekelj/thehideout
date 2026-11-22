//! AuthService: sign-in (password, email code, passkey, second factor),
//! password and email changes, TOTP, passkeys and sessions.

use std::time::Duration;

use chrono::Utc;
use connectrpc::{ConnectError, RequestContext, Response, ServiceRequest, ServiceResult};
use webauthn_rs::prelude::{PublicKeyCredential, RegisterPublicKeyCredential};

use crate::AppState;
use crate::accounts::{self, Account};
use crate::auth::{
    DUMMY_PASSWORD_HASH, authenticate, bearer_token, create_session, generate_token, hash_password, hash_token, internal,
    lwt_applied, peer_key, verify_password,
};
use crate::codes;
use crate::convert::{datetime_to_proto, id_to_db};
use crate::email::templates;
use crate::passkeys::{self, Ceremony, CeremonyKind, StoredPasskey};
use crate::proto::thehideout::v1::*;
use crate::ttl::TtlMap;

/// Hard cap on login attempts per peer IP within [`LOGIN_WINDOW`].
pub const LOGIN_IP_MAX: usize = 30;
const LOGIN_WINDOW: Duration = Duration::from_secs(15 * 60);
/// Per-username login attempts within [`LOGIN_WINDOW`] after which each further
/// attempt is delayed instead of rejected, so an attacker cannot lock out a
/// victim's account.
pub const LOGIN_USER_SOFT: usize = 5;

/// Emailed codes (sign-in, reset) requested per peer IP / per address within [`CODE_WINDOW`].
pub const CODE_REQUEST_IP_MAX: usize = 10;
pub const CODE_REQUEST_EMAIL_MAX: usize = 5;
/// Code verifications per peer IP / per address within [`CODE_WINDOW`].
pub const CODE_VERIFY_IP_MAX: usize = 30;
pub const CODE_VERIFY_EMAIL_MAX: usize = 10;
const CODE_WINDOW: Duration = Duration::from_secs(15 * 60);
/// How long an emailed code is valid, and how many wrong guesses it survives.
const CODE_TTL_SECS: i32 = 10 * 60;
const CODE_ATTEMPTS: i32 = 5;

/// Second-factor answers per peer IP within [`LOGIN_WINDOW`], and per challenge.
pub const SECOND_FACTOR_IP_MAX: usize = 30;
const CHALLENGE_ATTEMPTS: u32 = 5;
const CHALLENGE_TTL: Duration = Duration::from_secs(5 * 60);
/// Code checks (TOTP, password) by a signed-in user within an hour.
const USER_CHECK_MAX: usize = 10;
const USER_CHECK_WINDOW: Duration = Duration::from_secs(60 * 60);

/// Passkey ceremonies started per peer IP within [`LOGIN_WINDOW`].
pub const PASSKEY_IP_MAX: usize = 30;
/// Passkey login attempts per login name within [`LOGIN_WINDOW`], so a distributed
/// caller cannot use many peer IPs to enumerate which accounts have passkeys.
pub const PASSKEY_USER_MAX: usize = 10;
const CEREMONY_TTL: Duration = Duration::from_secs(5 * 60);
const TOTP_SETUP_TTL: Duration = Duration::from_secs(10 * 60);
const RECOVERY_CODES: usize = 10;
const MAX_PASSKEY_NAME: usize = 64;

/// Bounds a login name used inside a rate-limit key so keys stay short.
fn key_part(name: &str) -> String {
    name.trim().to_lowercase().chars().take(64).collect()
}

/// A first factor succeeded and the account has TOTP: what the second step needs.
#[derive(Clone)]
pub struct Challenge {
    pub user_id: u64,
    pub device_name: String,
    /// The first factor, for the new-device email.
    pub method: &'static str,
    pub attempts: u32,
}

/// In-memory, short-lived auth state (see `ttl.rs` for the single-node caveat).
pub struct AuthMemory {
    pub challenges: TtlMap<String, Challenge>,
    /// Secret of a TOTP setup awaiting confirmation, per user.
    pub totp_setup: TtlMap<u64, String>,
    pub ceremonies: TtlMap<String, Ceremony>,
}

impl Default for AuthMemory {
    fn default() -> Self {
        Self {
            challenges: TtlMap::new(CHALLENGE_TTL),
            totp_setup: TtlMap::new(TOTP_SETUP_TTL),
            ceremonies: TtlMap::new(CEREMONY_TTL),
        }
    }
}

pub struct AuthSvc {
    state: AppState,
}

impl AuthSvc {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

fn invalid_credentials() -> ConnectError {
    ConnectError::unauthenticated("invalid username or password")
}

fn invalid_code() -> ConnectError {
    ConnectError::unauthenticated("invalid or expired code")
}

/// A rejected second-factor answer or signed-in code check, with its `CodeError` detail.
fn code_error(reason: CodeErrorReason, attempts_remaining: u32) -> ConnectError {
    let err = match reason {
        CodeErrorReason::CODE_ERROR_REASON_WRONG => ConnectError::invalid_argument("wrong code"),
        CodeErrorReason::CODE_ERROR_REASON_TOO_MANY_ATTEMPTS => {
            ConnectError::resource_exhausted("too many wrong attempts, start again")
        }
        _ => ConnectError::failed_precondition("expired, start again"),
    };
    let detail = CodeError { reason: reason.into(), attempts_remaining, ..Default::default() };
    err.with_detail(connectrpc::ErrorDetail::from_message("thehideout.v1.CodeError", &detail))
}

fn wrong_code(attempts_remaining: usize) -> ConnectError {
    code_error(CodeErrorReason::CODE_ERROR_REASON_WRONG, attempts_remaining as u32)
}

fn expired() -> ConnectError {
    code_error(CodeErrorReason::CODE_ERROR_REASON_EXPIRED, 0)
}

fn too_many_attempts() -> ConnectError {
    code_error(CodeErrorReason::CODE_ERROR_REASON_TOO_MANY_ATTEMPTS, 0)
}

/// Result of a first factor.
enum SignIn {
    Session { token: String, user: User },
    SecondFactor(SecondFactorChallenge),
}

fn now_secs() -> u64 {
    Utc::now().timestamp().max(0) as u64
}

/// Emailed one-time codes (`email_codes`).
pub mod email_codes {
    use super::*;

    /// Stores a fresh code for `(purpose, subject)` (replacing any previous one) and returns it.
    pub async fn issue(state: &AppState, purpose: &str, subject: &str, new_email: Option<&str>) -> Result<String, ConnectError> {
        let code = codes::six_digits();
        state
            .session
            .execute_unpaged(
                "INSERT INTO email_codes (purpose, subject, code_hash, attempts, new_email) VALUES (?, ?, ?, 0, ?) USING TTL ?",
                (purpose, subject, hash_token(&code), new_email, CODE_TTL_SECS),
            )
            .await
            .map_err(internal)?;
        Ok(code)
    }

    /// Why [`check`] refused a code.
    pub enum Rejected {
        /// No code (never sent, expired, used, or killed by wrong guesses).
        Missing,
        /// Wrong; it survives this many more wrong guesses.
        Wrong { remaining: usize },
        /// Wrong, and that was the last guess: the code is gone.
        Exhausted,
    }

    /// [`check`] for callers that must not reveal why (enumeration-safe sign-in and reset).
    pub async fn consume(state: &AppState, purpose: &str, subject: &str, code: &str) -> Result<Option<String>, ConnectError> {
        check(state, purpose, subject, code).await?.map_err(|_| invalid_code())
    }

    /// Consumes the code if it matches; returns the stored `new_email`.
    /// Wrong guesses count against the code, which dies after [`CODE_ATTEMPTS`].
    pub async fn check(
        state: &AppState,
        purpose: &str,
        subject: &str,
        code: &str,
    ) -> Result<Result<Option<String>, Rejected>, ConnectError> {
        let row = state
            .session
            .execute_unpaged(
                "SELECT code_hash, attempts, new_email FROM email_codes WHERE purpose = ? AND subject = ?",
                (purpose, subject),
            )
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .maybe_first_row::<(Option<String>, Option<i32>, Option<String>)>()
            .map_err(internal)?;
        let Some((Some(stored), attempts, new_email)) = row else {
            return Ok(Err(Rejected::Missing));
        };
        let attempts = attempts.unwrap_or(0);
        let code = codes::digits(code);
        if code.len() != 6 || hash_token(&code) != stored {
            if attempts + 1 >= CODE_ATTEMPTS {
                delete(state, purpose, subject).await?;
                return Ok(Err(Rejected::Exhausted));
            } else {
                state
                    .session
                    .execute_unpaged(
                        "UPDATE email_codes USING TTL ? SET attempts = ? WHERE purpose = ? AND subject = ?",
                        (CODE_TTL_SECS, attempts + 1, purpose, subject),
                    )
                    .await
                    .map_err(internal)?;
            }
            return Ok(Err(Rejected::Wrong { remaining: (CODE_ATTEMPTS - attempts - 1) as usize }));
        }
        // Single use, also against a concurrent verification of the same code.
        let applied = lwt_applied(
            state
                .session
                .execute_unpaged(
                    "DELETE FROM email_codes WHERE purpose = ? AND subject = ? IF code_hash = ?",
                    (purpose, subject, &stored),
                )
                .await
                .map_err(internal)?,
        )?;
        if !applied {
            return Ok(Err(Rejected::Missing));
        }
        Ok(Ok(new_email))
    }

    async fn delete(state: &AppState, purpose: &str, subject: &str) -> Result<(), ConnectError> {
        state
            .session
            .execute_unpaged("DELETE FROM email_codes WHERE purpose = ? AND subject = ?", (purpose, subject))
            .await
            .map_err(internal)?;
        Ok(())
    }
}

/// Sends a security notice to the account's address, if it has one.
fn notify(state: &AppState, account: &Account, what: &str) {
    if let Some(email) = &account.email {
        state.mailer.send(templates::security_change(email, format!("security-{}", generate_token()), what));
    }
}

async fn audit_self(state: &AppState, account: &Account, action: &str) -> Result<(), ConnectError> {
    crate::admin::audit(state, account.id, action, &account.id.to_string(), "").await.map(|_| ())
}

/// Creates a session and emails a notice when the device name is new for the account.
async fn start_session(state: &AppState, account: &Account, device_name: &str, method: &str) -> Result<String, ConnectError> {
    let device = crate::auth::device_label(device_name);
    let known = crate::auth::list_sessions(state, account.id).await?.iter().any(|s| s.device_name == device);
    let token = create_session(state, account.id, &device).await?;
    if !known && let Some(email) = &account.email {
        state.mailer.send(templates::new_device(email, format!("device-{}", generate_token()), &device, method));
    }
    tracing::info!(user_id = account.id, method, "user signed in");
    Ok(token)
}

impl AuthSvc {
    /// After a first factor: a session, or a second-factor challenge for TOTP accounts.
    async fn first_factor_done(&self, account: Account, device_name: &str, method: &'static str) -> Result<SignIn, ConnectError> {
        if account.has_totp() {
            let id = generate_token();
            self.state.auth.challenges.insert(
                id.clone(),
                Challenge { user_id: account.id, device_name: device_name.to_owned(), method, attempts: 0 },
            );
            let expires_at = Utc::now() + chrono::Duration::from_std(CHALLENGE_TTL).unwrap_or_default();
            let mut methods = vec![
                SecondFactorMethod::SECOND_FACTOR_METHOD_TOTP.into(),
                SecondFactorMethod::SECOND_FACTOR_METHOD_RECOVERY_CODE.into(),
            ];
            if self.state.webauthn.is_some() && !passkeys::list(&self.state, account.id).await?.is_empty() {
                methods.push(SecondFactorMethod::SECOND_FACTOR_METHOD_PASSKEY.into());
            }
            return Ok(SignIn::SecondFactor(SecondFactorChallenge {
                challenge_id: id,
                methods,
                expires_at: datetime_to_proto(expires_at).into(),
                ..Default::default()
            }));
        }
        let token = start_session(&self.state, &account, device_name, method).await?;
        Ok(SignIn::Session { token, user: account.to_proto() })
    }

    /// The signed-in caller's account and session token hash.
    async fn caller(&self, ctx: &RequestContext) -> Result<(Account, String), ConnectError> {
        let id = authenticate(&self.state, ctx).await?;
        let token_hash = bearer_token(ctx).map(hash_token).unwrap_or_default();
        let account = accounts::load(&self.state, id)
            .await?
            .ok_or_else(|| ConnectError::unauthenticated("user no longer exists"))?;
        Ok((account, token_hash))
    }

    /// Requires the account's password when it has one (`""` otherwise).
    async fn check_current_password(&self, account: &Account, password: &str) -> Result<(), ConnectError> {
        self.state
            .limiter
            .check(&format!("password-check:user:{}", account.id), USER_CHECK_MAX, USER_CHECK_WINDOW)?;
        match &account.password_hash {
            Some(hash) => {
                if !verify_password(password.to_owned(), hash.clone()).await? {
                    return Err(ConnectError::permission_denied("current password is wrong"));
                }
            }
            None if !password.is_empty() => return Err(ConnectError::permission_denied("current password is wrong")),
            None => {}
        }
        Ok(())
    }

    /// Accepts a TOTP code once (recording its step), for a signed-in or challenged user.
    async fn use_totp(&self, account: &Account, code: &str) -> Result<bool, ConnectError> {
        let Some(secret) = &account.totp_secret else {
            return Ok(false);
        };
        let Some(step) = codes::verify_totp(secret, code, now_secs(), account.totp_last_step) else {
            return Ok(false);
        };
        lwt_applied(
            self.state
                .session
                .execute_unpaged(
                    "UPDATE users SET totp_last_step = ? WHERE id = ? IF totp_last_step = ?",
                    (step, id_to_db(account.id), account.totp_last_step),
                )
                .await
                .map_err(internal)?,
        )
    }

    /// Consumes a recovery code.
    async fn use_recovery_code(&self, account: &Account, code: &str) -> Result<bool, ConnectError> {
        let normalized = codes::normalize_recovery(code);
        if normalized.len() != 10 {
            return Ok(false);
        }
        let applied = lwt_applied(
            self.state
                .session
                .execute_unpaged(
                    "DELETE FROM recovery_codes WHERE user_id = ? AND code_hash = ? IF EXISTS",
                    (id_to_db(account.id), hash_token(&normalized)),
                )
                .await
                .map_err(internal)?,
        )?;
        if applied {
            notify(&self.state, account, "a recovery code was used");
        }
        Ok(applied)
    }

    /// Replaces the account's recovery codes; returns the new plaintext codes.
    async fn new_recovery_codes(&self, user_id: u64) -> Result<Vec<String>, ConnectError> {
        self.state
            .session
            .execute_unpaged("DELETE FROM recovery_codes WHERE user_id = ?", (id_to_db(user_id),))
            .await
            .map_err(internal)?;
        let mut out = Vec::with_capacity(RECOVERY_CODES);
        for _ in 0..RECOVERY_CODES {
            let code = codes::recovery_code();
            self.state
                .session
                .execute_unpaged(
                    "INSERT INTO recovery_codes (user_id, code_hash) VALUES (?, ?)",
                    (id_to_db(user_id), hash_token(&codes::normalize_recovery(&code))),
                )
                .await
                .map_err(internal)?;
            out.push(code);
        }
        Ok(out)
    }

    /// Counts an answer to the challenge `id`: EXPIRED when it is gone.
    fn challenge_attempt(&self, id: &String) -> Result<Challenge, ConnectError> {
        let challenge = self
            .state
            .auth
            .challenges
            .update(id, |c| {
                c.attempts += 1;
                c.clone()
            })
            .ok_or_else(expired)?;
        if challenge.attempts > CHALLENGE_ATTEMPTS {
            self.state.auth.challenges.take(id);
            return Err(too_many_attempts());
        }
        Ok(challenge)
    }

    /// The error for a wrong answer (the `attempts`-th) to the challenge `id`; the last one
    /// allowed drops the challenge.
    fn challenge_wrong(&self, id: &String, attempts: u32) -> ConnectError {
        if attempts >= CHALLENGE_ATTEMPTS {
            self.state.auth.challenges.take(id);
            return too_many_attempts();
        }
        wrong_code((CHALLENGE_ATTEMPTS - attempts) as usize)
    }

    /// Checks a signed-in user's TOTP-style code against the hourly limit; the number of checks
    /// left goes into the wrong-code error.
    fn user_check(&self, account: &Account) -> Result<usize, ConnectError> {
        let used = self
            .state
            .limiter
            .hit(&format!("totp-check:user:{}", account.id), USER_CHECK_MAX, USER_CHECK_WINDOW)?;
        Ok(USER_CHECK_MAX.saturating_sub(used))
    }

    /// The ceremony `id` if it is of the expected kind (and user, for registrations).
    fn ceremony(&self, id: &str) -> Result<Ceremony, ConnectError> {
        self.state
            .auth
            .ceremonies
            .get(&id.to_owned())
            .ok_or_else(|| ConnectError::not_found("passkey ceremony not found or expired"))
    }
}

macro_rules! sign_in_response {
    ($ty:ident, $value:expr) => {
        match $value {
            SignIn::Session { token, user } => $ty { token, user: user.into(), ..Default::default() },
            SignIn::SecondFactor(challenge) => $ty { second_factor_required: challenge.into(), ..Default::default() },
        }
    };
}

impl AuthService for AuthSvc {
    async fn sign_up(
        &self,
        _ctx: RequestContext,
        _req: ServiceRequest<'_, SignUpRequest>,
    ) -> ServiceResult<SignUpResponse> {
        Err(ConnectError::failed_precondition("invite required"))
    }

    async fn login(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, LoginRequest>,
    ) -> ServiceResult<LoginResponse> {
        // Hard per-IP cap on floods. Per-account attempts are delayed past a soft
        // threshold, never rejected, so an attacker cannot lock out a victim's
        // account; the growing delay alone throttles distributed brute force.
        self.state
            .limiter
            .check(&format!("login:ip:{}", peer_key(&ctx)), LOGIN_IP_MAX, LOGIN_WINDOW)?;
        let attempts = self.state.limiter.record(&format!("login:user:{}", key_part(req.username)), LOGIN_WINDOW);
        if attempts > LOGIN_USER_SOFT {
            tokio::time::sleep(Duration::from_secs((attempts - LOGIN_USER_SOFT).min(10) as u64)).await;
        }
        let account = accounts::find_by_login(&self.state, req.username).await?;
        let Some((account, hash)) = account.and_then(|a| a.password_hash.clone().map(|h| (a, h))) else {
            // Burn the same CPU time as a real verification.
            verify_password(req.password.to_owned(), DUMMY_PASSWORD_HASH.to_owned()).await?;
            return Err(invalid_credentials());
        };
        if !verify_password(req.password.to_owned(), hash).await? {
            return Err(invalid_credentials());
        }
        let signed = self.first_factor_done(account, req.device_name, "a password").await?;
        Response::ok(sign_in_response!(LoginResponse, signed))
    }

    async fn get_me(
        &self,
        ctx: RequestContext,
        _req: ServiceRequest<'_, GetMeRequest>,
    ) -> ServiceResult<GetMeResponse> {
        let id = authenticate(&self.state, &ctx).await?;
        let user = crate::admin::load_user(&self.state, id)
            .await?
            .ok_or_else(|| ConnectError::unauthenticated("user no longer exists"))?;
        Response::ok(GetMeResponse {
            user: user.into(),
            ..Default::default()
        })
    }

    async fn get_security(
        &self,
        ctx: RequestContext,
        _req: ServiceRequest<'_, GetSecurityRequest>,
    ) -> ServiceResult<GetSecurityResponse> {
        let (account, _) = self.caller(&ctx).await?;
        let passkey_count = passkeys::list(&self.state, account.id).await?.len() as u32;
        let recovery_codes_remaining = if account.has_totp() {
            self.state
                .session
                .execute_unpaged("SELECT code_hash FROM recovery_codes WHERE user_id = ?", (id_to_db(account.id),))
                .await
                .map_err(internal)?
                .into_rows_result()
                .map_err(internal)?
                .rows_num() as u32
        } else {
            0
        };
        Response::ok(GetSecurityResponse {
            totp_enabled: account.has_totp(),
            has_password: account.password_hash.is_some(),
            passkey_count,
            recovery_codes_remaining,
            email_verified: account.email.is_some(),
            ..Default::default()
        })
    }

    async fn request_email_code(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, RequestEmailCodeRequest>,
    ) -> ServiceResult<RequestEmailCodeResponse> {
        let email = accounts::normalize_email(req.email);
        self.limit_code_request(&ctx, "signin", &email)?;
        // Look up, store and send in the background: the response (content and
        // timing) is the same whether or not the address has an account.
        let state = self.state.clone();
        tokio::spawn(async move {
            let result = async {
                if let Some(account) = accounts::find_by_email(&state, &email).await? {
                    let code = email_codes::issue(&state, "login", &email, None).await?;
                    state.mailer.send(templates::sign_in_code(&email, format!("login-{}", generate_token()), &code));
                    tracing::info!(user_id = account.id, "sign-in code sent");
                }
                Ok::<_, ConnectError>(())
            }
            .await;
            if let Err(e) = result {
                tracing::warn!(error = %e.message.as_deref().unwrap_or("?"), "issuing a sign-in code failed");
            }
        });
        Response::ok(RequestEmailCodeResponse::default())
    }

    async fn verify_email_code(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, VerifyEmailCodeRequest>,
    ) -> ServiceResult<VerifyEmailCodeResponse> {
        let email = accounts::normalize_email(req.email);
        self.limit_code_verify(&ctx, &email)?;
        email_codes::consume(&self.state, "login", &email, req.code).await?;
        let account = accounts::find_by_email(&self.state, &email).await?.ok_or_else(invalid_code)?;
        let signed = self.first_factor_done(account, req.device_name, "an email code").await?;
        Response::ok(sign_in_response!(VerifyEmailCodeResponse, signed))
    }

    async fn complete_second_factor(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, CompleteSecondFactorRequest>,
    ) -> ServiceResult<CompleteSecondFactorResponse> {
        self.state
            .limiter
            .check(&format!("2fa:ip:{}", peer_key(&ctx)), SECOND_FACTOR_IP_MAX, LOGIN_WINDOW)?;
        if req.totp_code.is_empty() == req.recovery_code.is_empty() {
            return Err(ConnectError::invalid_argument("set exactly one of totp_code and recovery_code"));
        }
        let id = req.challenge_id.to_owned();
        let challenge = self.challenge_attempt(&id)?;
        let account = accounts::load(&self.state, challenge.user_id).await?.ok_or_else(expired)?;
        let ok = if req.recovery_code.is_empty() {
            self.use_totp(&account, req.totp_code).await?
        } else {
            self.use_recovery_code(&account, req.recovery_code).await?
        };
        if !ok {
            return Err(self.challenge_wrong(&id, challenge.attempts));
        }
        // Only one session per challenge, even if two answers race.
        if self.state.auth.challenges.take(&id).is_none() {
            return Err(expired());
        }
        let token = start_session(&self.state, &account, &challenge.device_name, challenge.method).await?;
        Response::ok(CompleteSecondFactorResponse { token, user: account.to_proto().into(), ..Default::default() })
    }

    async fn request_password_reset(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, RequestPasswordResetRequest>,
    ) -> ServiceResult<RequestPasswordResetResponse> {
        let email = accounts::normalize_email(req.email);
        self.limit_code_request(&ctx, "reset", &email)?;
        let state = self.state.clone();
        tokio::spawn(async move {
            let result = async {
                if accounts::find_by_email(&state, &email).await?.is_some() {
                    let code = email_codes::issue(&state, "reset", &email, None).await?;
                    state.mailer.send(templates::password_reset(&email, format!("reset-{}", generate_token()), &code));
                }
                Ok::<_, ConnectError>(())
            }
            .await;
            if let Err(e) = result {
                tracing::warn!(error = %e.message.as_deref().unwrap_or("?"), "issuing a reset code failed");
            }
        });
        Response::ok(RequestPasswordResetResponse::default())
    }

    async fn reset_password(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, ResetPasswordRequest>,
    ) -> ServiceResult<ResetPasswordResponse> {
        let email = accounts::normalize_email(req.email);
        self.limit_code_verify(&ctx, &email)?;
        accounts::check_password(req.new_password)?;
        email_codes::consume(&self.state, "reset", &email, req.code).await?;
        let account = accounts::find_by_email(&self.state, &email).await?.ok_or_else(invalid_code)?;
        self.set_password(&account, req.new_password).await?;
        crate::auth::revoke_all_sessions(&self.state, account.id, None).await?;
        audit_self(&self.state, &account, "auth.password_reset").await?;
        notify(&self.state, &account, "your password was reset and every device was signed out");
        Response::ok(ResetPasswordResponse::default())
    }

    async fn change_password(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, ChangePasswordRequest>,
    ) -> ServiceResult<ChangePasswordResponse> {
        let (account, token_hash) = self.caller(&ctx).await?;
        accounts::check_password(req.new_password)?;
        self.check_current_password(&account, req.current_password).await?;
        self.set_password(&account, req.new_password).await?;
        crate::auth::revoke_all_sessions(&self.state, account.id, Some(&token_hash)).await?;
        audit_self(&self.state, &account, "auth.password_change").await?;
        notify(&self.state, &account, "your password was changed and your other devices were signed out");
        Response::ok(ChangePasswordResponse::default())
    }

    async fn change_email(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, ChangeEmailRequest>,
    ) -> ServiceResult<ChangeEmailResponse> {
        let (account, _) = self.caller(&ctx).await?;
        let new_email = accounts::normalize_email(req.new_email);
        accounts::check_email(&new_email)?;
        if account.email.as_deref() == Some(new_email.as_str()) {
            return Err(ConnectError::invalid_argument("that is already your email"));
        }
        self.check_current_password(&account, req.current_password).await?;
        self.state
            .limiter
            .check(&format!("email-change:user:{}", account.id), CODE_REQUEST_EMAIL_MAX, CODE_WINDOW)?;
        if accounts::id_by_email(&self.state, &new_email).await?.is_some() {
            return Err(ConnectError::already_exists("an account with that email already exists"));
        }
        let code = email_codes::issue(&self.state, "email_change", &account.id.to_string(), Some(&new_email)).await?;
        self.state
            .mailer
            .send(templates::email_change_code(&new_email, format!("email-change-{}", generate_token()), &code));
        Response::ok(ChangeEmailResponse::default())
    }

    async fn confirm_email_change(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, ConfirmEmailChangeRequest>,
    ) -> ServiceResult<ConfirmEmailChangeResponse> {
        let (account, _) = self.caller(&ctx).await?;
        self.state
            .limiter
            .check(&format!("email-change-verify:user:{}", account.id), CODE_VERIFY_EMAIL_MAX, CODE_WINDOW)?;
        let new_email = match email_codes::check(&self.state, "email_change", &account.id.to_string(), req.code).await? {
            Ok(Some(email)) => email,
            Ok(None) | Err(email_codes::Rejected::Missing) => return Err(expired()),
            Err(email_codes::Rejected::Wrong { remaining }) => return Err(wrong_code(remaining)),
            Err(email_codes::Rejected::Exhausted) => return Err(too_many_attempts()),
        };
        if !accounts::claim_email(&self.state, &new_email, account.id).await? {
            return Err(ConnectError::already_exists("an account with that email already exists"));
        }
        self.state
            .session
            .execute_unpaged("UPDATE users SET email = ? WHERE id = ?", (&new_email, id_to_db(account.id)))
            .await
            .map_err(internal)?;
        if let Some(old) = &account.email {
            accounts::release_email(&self.state, old, account.id).await?;
            self.state
                .mailer
                .send(templates::email_changed(old, format!("email-changed-{}", generate_token()), &new_email));
        }
        audit_self(&self.state, &account, "auth.email_change").await?;
        let user = crate::admin::load_user(&self.state, account.id)
            .await?
            .ok_or_else(|| ConnectError::unauthenticated("user no longer exists"))?;
        self.state.events.notify_user_profile_updated(account.id, user.clone());
        Response::ok(ConfirmEmailChangeResponse { user: user.into(), ..Default::default() })
    }

    async fn begin_totp_setup(
        &self,
        ctx: RequestContext,
        _req: ServiceRequest<'_, BeginTotpSetupRequest>,
    ) -> ServiceResult<BeginTotpSetupResponse> {
        let (account, _) = self.caller(&ctx).await?;
        if account.has_totp() {
            return Err(ConnectError::failed_precondition("an authenticator app is already set up"));
        }
        let secret = codes::new_totp_secret();
        let label = account.email.clone().unwrap_or_else(|| account.username.clone());
        self.state.auth.totp_setup.insert(account.id, secret.clone());
        Response::ok(BeginTotpSetupResponse {
            otpauth_uri: codes::otpauth_uri(&secret, &label),
            secret,
            ..Default::default()
        })
    }

    async fn confirm_totp(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, ConfirmTotpRequest>,
    ) -> ServiceResult<ConfirmTotpResponse> {
        let (account, _) = self.caller(&ctx).await?;
        let remaining = self.user_check(&account)?;
        if account.has_totp() {
            return Err(ConnectError::failed_precondition("an authenticator app is already set up"));
        }
        let secret = self.state.auth.totp_setup.get(&account.id).ok_or_else(expired)?;
        let step = codes::verify_totp(&secret, req.code, now_secs(), i64::MIN).ok_or_else(|| wrong_code(remaining))?;
        self.state.auth.totp_setup.take(&account.id);
        let recovery_codes = self.new_recovery_codes(account.id).await?;
        self.state
            .session
            .execute_unpaged(
                "UPDATE users SET totp_secret = ?, totp_last_step = ? WHERE id = ?",
                (&secret, step, id_to_db(account.id)),
            )
            .await
            .map_err(internal)?;
        audit_self(&self.state, &account, "auth.totp_enable").await?;
        notify(&self.state, &account, "two-factor authentication was turned on");
        Response::ok(ConfirmTotpResponse { recovery_codes, ..Default::default() })
    }

    async fn disable_totp(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, DisableTotpRequest>,
    ) -> ServiceResult<DisableTotpResponse> {
        let (account, _) = self.caller(&ctx).await?;
        if !account.has_totp() {
            return Err(ConnectError::failed_precondition("two-factor authentication is off"));
        }
        let remaining = self.user_check(&account)?;
        if !(self.use_totp(&account, req.code).await? || self.use_recovery_code(&account, req.code).await?) {
            return Err(wrong_code(remaining));
        }
        self.state
            .session
            .execute_unpaged("UPDATE users SET totp_secret = null, totp_last_step = null WHERE id = ?", (id_to_db(account.id),))
            .await
            .map_err(internal)?;
        self.state
            .session
            .execute_unpaged("DELETE FROM recovery_codes WHERE user_id = ?", (id_to_db(account.id),))
            .await
            .map_err(internal)?;
        audit_self(&self.state, &account, "auth.totp_disable").await?;
        notify(&self.state, &account, "two-factor authentication was turned off");
        Response::ok(DisableTotpResponse::default())
    }

    async fn regenerate_recovery_codes(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, RegenerateRecoveryCodesRequest>,
    ) -> ServiceResult<RegenerateRecoveryCodesResponse> {
        let (account, _) = self.caller(&ctx).await?;
        if !account.has_totp() {
            return Err(ConnectError::failed_precondition("two-factor authentication is off"));
        }
        let remaining = self.user_check(&account)?;
        if !self.use_totp(&account, req.code).await? {
            return Err(wrong_code(remaining));
        }
        let recovery_codes = self.new_recovery_codes(account.id).await?;
        audit_self(&self.state, &account, "auth.recovery_regenerate").await?;
        notify(&self.state, &account, "new recovery codes were generated; the old ones no longer work");
        Response::ok(RegenerateRecoveryCodesResponse { recovery_codes, ..Default::default() })
    }

    async fn begin_passkey_registration(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, BeginPasskeyRegistrationRequest>,
    ) -> ServiceResult<BeginPasskeyRegistrationResponse> {
        let (account, _) = self.caller(&ctx).await?;
        let webauthn = passkeys::webauthn(&self.state)?;
        let name = passkey_name(req.name)?;
        let existing = passkeys::list(&self.state, account.id).await?;
        let exclude = existing.iter().map(|p| p.credential.cred_id().clone()).collect::<Vec<_>>();
        let (options, reg) = webauthn
            .start_passkey_registration(
                passkeys::user_handle(account.id),
                &account.username,
                &account.username,
                (!exclude.is_empty()).then_some(exclude),
            )
            .map_err(internal)?;
        let options_json = serde_json::to_string(&options).map_err(internal)?;
        let id = generate_token();
        self.state.auth.ceremonies.insert(
            id.clone(),
            Ceremony {
                kind: CeremonyKind::Register { user_id: account.id, name, state: reg },
                options_json: options_json.clone(),
                result: None,
                redirect_port: redirect_port(req.redirect_port)?,
            },
        );
        Response::ok(BeginPasskeyRegistrationResponse {
            ceremony: passkeys::ceremony_proto(&self.state, &id, &options_json).into(),
            ..Default::default()
        })
    }

    async fn finish_passkey_registration(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, FinishPasskeyRegistrationRequest>,
    ) -> ServiceResult<FinishPasskeyRegistrationResponse> {
        let (account, _) = self.caller(&ctx).await?;
        let webauthn = passkeys::webauthn(&self.state)?;
        let ceremony = self.ceremony(req.ceremony_id)?;
        let CeremonyKind::Register { user_id, name, state } = ceremony.kind else {
            return Err(ConnectError::not_found("passkey ceremony not found or expired"));
        };
        if user_id != account.id {
            return Err(ConnectError::not_found("passkey ceremony not found or expired"));
        }
        let json = if req.credential_json.is_empty() { ceremony.result } else { Some(req.credential_json.to_owned()) };
        let Some(json) = json else {
            return Response::ok(FinishPasskeyRegistrationResponse { pending: true, ..Default::default() });
        };
        // One attempt per ceremony.
        self.state.auth.ceremonies.take(&req.ceremony_id.to_owned());
        let credential: RegisterPublicKeyCredential =
            serde_json::from_str(&json).map_err(|_| ConnectError::invalid_argument("invalid credential_json"))?;
        let credential = webauthn.finish_passkey_registration(&credential, &state).map_err(|e| {
            tracing::info!(error = %e, "passkey registration rejected");
            ConnectError::invalid_argument("passkey verification failed")
        })?;
        let passkey = StoredPasskey {
            id: self.state.snowflake.next_id(),
            name,
            credential,
            created_at: Some(Utc::now()),
            last_used_at: None,
        };
        passkeys::save(&self.state, account.id, &passkey).await?;
        audit_self(&self.state, &account, "auth.passkey_add").await?;
        notify(&self.state, &account, &format!("a passkey ({}) was added", passkey.name));
        Response::ok(FinishPasskeyRegistrationResponse { passkey: passkey.to_proto().into(), ..Default::default() })
    }

    async fn list_passkeys(
        &self,
        ctx: RequestContext,
        _req: ServiceRequest<'_, ListPasskeysRequest>,
    ) -> ServiceResult<ListPasskeysResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let passkeys = passkeys::list(&self.state, user_id).await?.iter().map(StoredPasskey::to_proto).collect();
        Response::ok(ListPasskeysResponse { passkeys, ..Default::default() })
    }

    async fn rename_passkey(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, RenamePasskeyRequest>,
    ) -> ServiceResult<RenamePasskeyResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let name = req.name.trim();
        if name.is_empty() {
            return Err(ConnectError::invalid_argument("name must not be empty"));
        }
        let name = passkey_name(name)?;
        let mut passkey = passkeys::list(&self.state, user_id)
            .await?
            .into_iter()
            .find(|p| p.id == req.passkey_id)
            .ok_or_else(|| ConnectError::not_found("passkey not found"))?;
        passkey.name = name;
        passkeys::save(&self.state, user_id, &passkey).await?;
        Response::ok(RenamePasskeyResponse { passkey: passkey.to_proto().into(), ..Default::default() })
    }

    async fn delete_passkey(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, DeletePasskeyRequest>,
    ) -> ServiceResult<DeletePasskeyResponse> {
        let (account, _) = self.caller(&ctx).await?;
        let passkey = passkeys::list(&self.state, account.id)
            .await?
            .into_iter()
            .find(|p| p.id == req.passkey_id)
            .ok_or_else(|| ConnectError::not_found("passkey not found"))?;
        passkeys::delete(&self.state, account.id, passkey.id).await?;
        audit_self(&self.state, &account, "auth.passkey_delete").await?;
        notify(&self.state, &account, &format!("a passkey ({}) was removed", passkey.name));
        Response::ok(DeletePasskeyResponse::default())
    }

    async fn begin_passkey_login(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, BeginPasskeyLoginRequest>,
    ) -> ServiceResult<BeginPasskeyLoginResponse> {
        self.state
            .limiter
            .check(&format!("passkey:ip:{}", peer_key(&ctx)), PASSKEY_IP_MAX, LOGIN_WINDOW)?;
        self.state
            .limiter
            .check(&format!("passkey:user:{}", key_part(req.username)), PASSKEY_USER_MAX, LOGIN_WINDOW)?;
        let webauthn = passkeys::webauthn(&self.state)?;
        // Username-first: this reveals whether an account has passkeys (rate limited
        // per peer IP and per login name).
        let no_passkey = || ConnectError::failed_precondition("no passkey for this account");
        let account = accounts::find_by_login(&self.state, req.username).await?.ok_or_else(no_passkey)?;
        let stored = passkeys::list(&self.state, account.id).await?;
        if stored.is_empty() {
            return Err(no_passkey());
        }
        let credentials: Vec<_> = stored.into_iter().map(|p| p.credential).collect();
        let (options, auth) = webauthn.start_passkey_authentication(&credentials).map_err(internal)?;
        let options_json = serde_json::to_string(&options).map_err(internal)?;
        let id = generate_token();
        self.state.auth.ceremonies.insert(
            id.clone(),
            Ceremony {
                kind: CeremonyKind::Login { user_id: account.id, device_name: req.device_name.to_owned(), state: auth },
                options_json: options_json.clone(),
                result: None,
                redirect_port: redirect_port(req.redirect_port)?,
            },
        );
        Response::ok(BeginPasskeyLoginResponse {
            ceremony: passkeys::ceremony_proto(&self.state, &id, &options_json).into(),
            ..Default::default()
        })
    }

    async fn finish_passkey_login(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, FinishPasskeyLoginRequest>,
    ) -> ServiceResult<FinishPasskeyLoginResponse> {
        self.state
            .limiter
            .check(&format!("passkey-finish:ip:{}", peer_key(&ctx)), PASSKEY_IP_MAX * 10, LOGIN_WINDOW)?;
        let webauthn = passkeys::webauthn(&self.state)?;
        let ceremony = self.ceremony(req.ceremony_id)?;
        let CeremonyKind::Login { user_id, device_name, state } = ceremony.kind else {
            return Err(ConnectError::not_found("passkey ceremony not found or expired"));
        };
        let json = if req.credential_json.is_empty() { ceremony.result } else { Some(req.credential_json.to_owned()) };
        let Some(json) = json else {
            return Response::ok(FinishPasskeyLoginResponse { pending: true, ..Default::default() });
        };
        self.state.auth.ceremonies.take(&req.ceremony_id.to_owned());
        let credential: PublicKeyCredential =
            serde_json::from_str(&json).map_err(|_| ConnectError::invalid_argument("invalid credential_json"))?;
        let result = webauthn.finish_passkey_authentication(&credential, &state).map_err(|e| {
            tracing::info!(error = %e, "passkey sign-in rejected");
            ConnectError::unauthenticated("passkey verification failed")
        })?;
        let account = accounts::load(&self.state, user_id)
            .await?
            .ok_or_else(|| ConnectError::unauthenticated("passkey verification failed"))?;
        passkeys::mark_used(&self.state, user_id, &result).await?;
        // A passkey is phishing-resistant and user-verified: it skips TOTP.
        let token = start_session(&self.state, &account, &device_name, "a passkey").await?;
        Response::ok(FinishPasskeyLoginResponse { token, user: account.to_proto().into(), ..Default::default() })
    }

    async fn begin_second_factor_passkey(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, BeginSecondFactorPasskeyRequest>,
    ) -> ServiceResult<BeginSecondFactorPasskeyResponse> {
        self.state
            .limiter
            .check(&format!("passkey:ip:{}", peer_key(&ctx)), PASSKEY_IP_MAX, LOGIN_WINDOW)?;
        let webauthn = passkeys::webauthn(&self.state)?;
        let challenge_id = req.challenge_id.to_owned();
        let challenge = self.state.auth.challenges.get(&challenge_id).ok_or_else(expired)?;
        let credentials: Vec<_> = passkeys::list(&self.state, challenge.user_id).await?.into_iter().map(|p| p.credential).collect();
        if credentials.is_empty() {
            return Err(ConnectError::failed_precondition("no passkey for this account"));
        }
        let (options, auth) = webauthn.start_passkey_authentication(&credentials).map_err(internal)?;
        let options_json = serde_json::to_string(&options).map_err(internal)?;
        let id = generate_token();
        self.state.auth.ceremonies.insert(
            id.clone(),
            Ceremony {
                kind: CeremonyKind::SecondFactor { challenge_id, user_id: challenge.user_id, state: auth },
                options_json: options_json.clone(),
                result: None,
                redirect_port: redirect_port(req.redirect_port)?,
            },
        );
        Response::ok(BeginSecondFactorPasskeyResponse {
            ceremony: passkeys::ceremony_proto(&self.state, &id, &options_json).into(),
            ..Default::default()
        })
    }

    async fn finish_second_factor_passkey(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, FinishSecondFactorPasskeyRequest>,
    ) -> ServiceResult<FinishSecondFactorPasskeyResponse> {
        self.state
            .limiter
            .check(&format!("passkey-finish:ip:{}", peer_key(&ctx)), PASSKEY_IP_MAX * 10, LOGIN_WINDOW)?;
        let webauthn = passkeys::webauthn(&self.state)?;
        let ceremony = self.ceremony(req.ceremony_id)?;
        let CeremonyKind::SecondFactor { challenge_id, user_id, state } = ceremony.kind else {
            return Err(ConnectError::not_found("passkey ceremony not found or expired"));
        };
        let json = if req.credential_json.is_empty() { ceremony.result } else { Some(req.credential_json.to_owned()) };
        let Some(json) = json else {
            return Response::ok(FinishSecondFactorPasskeyResponse { pending: true, ..Default::default() });
        };
        self.state.auth.ceremonies.take(&req.ceremony_id.to_owned());
        let challenge = self.challenge_attempt(&challenge_id)?;
        let verified = serde_json::from_str::<PublicKeyCredential>(&json)
            .ok()
            .and_then(|credential| match webauthn.finish_passkey_authentication(&credential, &state) {
                Ok(result) => Some(result),
                Err(e) => {
                    tracing::info!(error = %e, "second-factor passkey rejected");
                    None
                }
            });
        let Some(result) = verified else {
            return Err(self.challenge_wrong(&challenge_id, challenge.attempts));
        };
        let account = accounts::load(&self.state, user_id).await?.ok_or_else(expired)?;
        // Only one session per challenge, even if answers race.
        if self.state.auth.challenges.take(&challenge_id).is_none() {
            return Err(expired());
        }
        passkeys::mark_used(&self.state, user_id, &result).await?;
        let token = start_session(&self.state, &account, &challenge.device_name, challenge.method).await?;
        Response::ok(FinishSecondFactorPasskeyResponse { token, user: account.to_proto().into(), ..Default::default() })
    }

    async fn list_sessions(
        &self,
        ctx: RequestContext,
        _req: ServiceRequest<'_, ListSessionsRequest>,
    ) -> ServiceResult<ListSessionsResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let current = bearer_token(&ctx).map(hash_token).unwrap_or_default();
        let sessions = crate::auth::list_sessions(&self.state, user_id)
            .await?
            .into_iter()
            .map(|s| Session {
                id: s.id,
                device_name: s.device_name,
                created_at: s.created_at.map(datetime_to_proto).into(),
                last_seen_at: s.last_seen_at.map(datetime_to_proto).into(),
                current: s.token_hash == current,
                ..Default::default()
            })
            .collect();
        Response::ok(ListSessionsResponse { sessions, ..Default::default() })
    }

    async fn revoke_session(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, RevokeSessionRequest>,
    ) -> ServiceResult<RevokeSessionResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let session = crate::auth::list_sessions(&self.state, user_id)
            .await?
            .into_iter()
            .find(|s| s.id == req.session_id)
            .ok_or_else(|| ConnectError::not_found("session not found"))?;
        crate::auth::revoke_session(&self.state, user_id, session.id, &session.token_hash).await?;
        Response::ok(RevokeSessionResponse::default())
    }

    async fn revoke_other_sessions(
        &self,
        ctx: RequestContext,
        _req: ServiceRequest<'_, RevokeOtherSessionsRequest>,
    ) -> ServiceResult<RevokeOtherSessionsResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let current = bearer_token(&ctx).map(hash_token).unwrap_or_default();
        let revoked = crate::auth::revoke_all_sessions(&self.state, user_id, Some(&current)).await?;
        Response::ok(RevokeOtherSessionsResponse { revoked, ..Default::default() })
    }
}

impl AuthSvc {
    fn limit_code_request(&self, ctx: &RequestContext, kind: &str, email: &str) -> Result<(), ConnectError> {
        let limiter = &self.state.limiter;
        limiter.check(&format!("code-req:{kind}:ip:{}", peer_key(ctx)), CODE_REQUEST_IP_MAX, CODE_WINDOW)?;
        limiter.check(&format!("code-req:{kind}:email:{}", key_part(email)), CODE_REQUEST_EMAIL_MAX, CODE_WINDOW)
    }

    fn limit_code_verify(&self, ctx: &RequestContext, email: &str) -> Result<(), ConnectError> {
        let limiter = &self.state.limiter;
        limiter.check(&format!("code-verify:ip:{}", peer_key(ctx)), CODE_VERIFY_IP_MAX, CODE_WINDOW)?;
        limiter.check(&format!("code-verify:email:{}", key_part(email)), CODE_VERIFY_EMAIL_MAX, CODE_WINDOW)
    }

    async fn set_password(&self, account: &Account, password: &str) -> Result<(), ConnectError> {
        let hash = hash_password(password.to_owned()).await?;
        self.state
            .session
            .execute_unpaged("UPDATE users SET password_hash = ? WHERE id = ?", (hash, id_to_db(account.id)))
            .await
            .map_err(internal)?;
        Ok(())
    }
}

fn passkey_name(raw: &str) -> Result<String, ConnectError> {
    let name = raw.trim();
    if name.chars().count() > MAX_PASSKEY_NAME {
        return Err(ConnectError::invalid_argument("name must be at most 64 characters"));
    }
    Ok(if name.is_empty() { "Passkey".to_owned() } else { name.to_owned() })
}

fn redirect_port(port: Option<u32>) -> Result<Option<u16>, ConnectError> {
    match port {
        None => Ok(None),
        Some(p) if (1024..=65535).contains(&p) => Ok(Some(p as u16)),
        Some(_) => Err(ConnectError::invalid_argument("redirect_port must be 1024-65535")),
    }
}
