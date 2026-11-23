//! Invites, sign-in methods and account security calls of [`Client`].

use std::time::SystemTime;

use crate::transport::Transport;

use crate::admin::{Paged, next_token};
use crate::client::{Client, required};
use crate::error::ApiError;
use crate::proto::thehideout::v1 as pb;
use crate::types::{User, time};

type Result<T> = std::result::Result<T, ApiError>;

/// Result of a first sign-in step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginOutcome {
    /// Signed in; the token is already stored on the client.
    Session { token: String, user: User },
    /// The account has two-factor authentication: answer `challenge` with
    /// [`Client::complete_second_factor`] within 5 minutes.
    SecondFactor { challenge: String, methods: Vec<SecondFactorMethod> },
}

impl LoginOutcome {
    pub(crate) fn from_challenge(c: pb::SecondFactorChallenge) -> Self {
        let methods = c
            .methods
            .iter()
            .filter_map(|m| match m.as_known() {
                Some(pb::SecondFactorMethod::SECOND_FACTOR_METHOD_TOTP) => Some(SecondFactorMethod::Totp),
                Some(pb::SecondFactorMethod::SECOND_FACTOR_METHOD_RECOVERY_CODE) => Some(SecondFactorMethod::RecoveryCode),
                Some(pb::SecondFactorMethod::SECOND_FACTOR_METHOD_PASSKEY) => Some(SecondFactorMethod::Passkey),
                _ => None,
            })
            .collect();
        Self::SecondFactor { challenge: c.challenge_id, methods }
    }
}

/// A way to answer a second-factor challenge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecondFactorMethod {
    /// A 6-digit code from an authenticator app.
    Totp,
    /// One of the single-use recovery codes.
    RecoveryCode,
    /// One of the account's passkeys ([`Client::begin_second_factor_passkey`]).
    Passkey,
}

/// The code answering a second-factor challenge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecondFactor<'a> {
    Totp(&'a str),
    RecoveryCode(&'a str),
}

/// Lifecycle of an invite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InviteStatus {
    Pending,
    Accepted,
    Expired,
    Revoked,
    /// A status this client does not know.
    Unknown,
}

impl From<buffa::EnumValue<pb::InviteStatus>> for InviteStatus {
    fn from(v: buffa::EnumValue<pb::InviteStatus>) -> Self {
        match v.as_known() {
            Some(pb::InviteStatus::INVITE_STATUS_PENDING) => Self::Pending,
            Some(pb::InviteStatus::INVITE_STATUS_ACCEPTED) => Self::Accepted,
            Some(pb::InviteStatus::INVITE_STATUS_EXPIRED) => Self::Expired,
            Some(pb::InviteStatus::INVITE_STATUS_REVOKED) => Self::Revoked,
            _ => Self::Unknown,
        }
    }
}

/// An invite as seen by admins and inviters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
    pub id: u64,
    pub email: String,
    /// `XXXX-XXXX`; only right after [`Client::create_invite`] / [`Client::resend_invite`].
    pub code: Option<String>,
    /// The invite link (`{server}/invite/{token}`); like `code`, only right after creating or
    /// resending.
    pub link: Option<String>,
    pub status: InviteStatus,
    /// `None` for invites made by the server (bootstrap, CLI).
    pub inviter: Option<User>,
    pub guild_ids: Vec<u64>,
    pub make_admin: bool,
    pub created_at: SystemTime,
    pub expires_at: SystemTime,
    pub accepted_user_id: Option<u64>,
}

impl From<pb::Invite> for Invite {
    fn from(i: pb::Invite) -> Self {
        Self {
            id: i.id,
            email: i.email,
            code: i.code,
            link: i.link,
            status: i.status.into(),
            inviter: i.inviter.into_option().map(Into::into),
            guild_ids: i.guild_ids,
            make_admin: i.make_admin,
            created_at: time(i.created_at.as_option()),
            expires_at: time(i.expires_at.as_option()),
            accepted_user_id: i.accepted_user_id,
        }
    }
}

/// What an invitee sees before accepting ([`Client::lookup_invite`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvitePreview {
    /// `a***@example.com`.
    pub masked_email: String,
    pub inviter: Option<User>,
    pub guild_names: Vec<String>,
    pub expires_at: SystemTime,
    /// Only [`InviteStatus::Pending`] invites can be accepted.
    pub status: InviteStatus,
    /// The terms version [`Client::accept_invite`] must be given ([`InviteTerms::version`]).
    pub terms_version: String,
    /// The minimum age the invitee confirms (16).
    pub minimum_age: u32,
}

/// What an invitee agreed to: the terms version they were shown and the age confirmation.
/// The server refuses an invite without both ([`ApiError::TermsUpdateRequired`] /
/// [`ApiError::InvalidArgument`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InviteTerms<'a> {
    /// [`InvitePreview::terms_version`].
    pub version: &'a str,
    /// The invitee confirmed they are at least [`InvitePreview::minimum_age`].
    pub age_confirmed: bool,
}

/// What to create with [`Client::create_invite`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewInvite {
    pub email: String,
    /// Invite into this guild (needs [`Permissions::INVITE_PEOPLE`](crate::Permissions::INVITE_PEOPLE)).
    pub guild_id: Option<u64>,
    /// Site admins only: more guilds to pre-join.
    pub guild_ids: Vec<u64>,
    /// Site admins only: make the new account a site admin.
    pub make_admin: bool,
}

/// How the caller's account is protected ([`Client::get_security`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SecuritySummary {
    pub totp_enabled: bool,
    /// `false` for passwordless accounts.
    pub has_password: bool,
    pub passkey_count: u32,
    /// Unused recovery codes; 0 when TOTP is off.
    pub recovery_codes_remaining: u32,
    /// `false` for legacy accounts without an email.
    pub email_verified: bool,
}

/// A new authenticator-app secret ([`Client::begin_totp_setup`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TotpSetup {
    /// `otpauth://` URI to render as a QR code.
    pub otpauth_uri: String,
    /// Base32 secret for manual entry.
    pub secret: String,
}

/// A running WebAuthn ceremony. Run `options_json` with WebAuthn, or open
/// `url` in the system browser and poll the matching `finish_*` call with
/// `None` until it returns `Some`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasskeyCeremony {
    pub id: String,
    pub url: String,
    pub options_json: String,
}

impl From<pb::PasskeyCeremony> for PasskeyCeremony {
    fn from(c: pb::PasskeyCeremony) -> Self {
        Self { id: c.ceremony_id, url: c.ceremony_url, options_json: c.options_json }
    }
}

/// One of the caller's passkeys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passkey {
    pub id: u64,
    pub name: String,
    pub created_at: SystemTime,
    pub last_used_at: Option<SystemTime>,
}

impl From<pb::Passkey> for Passkey {
    fn from(p: pb::Passkey) -> Self {
        Self {
            id: p.id,
            name: p.name,
            created_at: time(p.created_at.as_option()),
            last_used_at: p.last_used_at.as_option().map(|t| time(Some(t))),
        }
    }
}

/// A signed-in session (device) of the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceSession {
    pub id: u64,
    pub device_name: String,
    pub created_at: SystemTime,
    pub last_seen_at: Option<SystemTime>,
    /// Whether this is the session of this client.
    pub current: bool,
}

impl From<pb::Session> for DeviceSession {
    fn from(s: pb::Session) -> Self {
        Self {
            id: s.id,
            device_name: s.device_name,
            created_at: time(s.created_at.as_option()),
            last_seen_at: s.last_seen_at.as_option().map(|t| time(Some(t))),
            current: s.current,
        }
    }
}

impl Client {
    fn invites(&self) -> Result<pb::InviteServiceClient<Transport>> {
        Ok(pb::InviteServiceClient::new(self.http.clone(), self.config()?))
    }

    // --- Invites ---------------------------------------------------------

    /// Creates and emails an invite; the returned invite carries its code.
    pub async fn create_invite(&self, invite: &NewInvite) -> Result<Invite> {
        let res = self
            .invites()?
            .create_invite(pb::CreateInviteRequest {
                email: invite.email.clone(),
                guild_id: invite.guild_id,
                guild_ids: invite.guild_ids.clone(),
                make_admin: invite.make_admin,
                ..Default::default()
            })
            .await?
            .into_owned();
        required(res.invite.into_option())
    }

    /// A page of invites, newest first: all (site admins, `guild_id = None`)
    /// or those of one guild.
    pub async fn list_invites(&self, guild_id: Option<u64>, page_token: Option<&str>, limit: u32) -> Result<Paged<Invite>> {
        let res = self
            .invites()?
            .list_invites(pb::ListInvitesRequest {
                guild_id,
                page_token: page_token.unwrap_or_default().into(),
                limit,
                ..Default::default()
            })
            .await?
            .into_owned();
        Ok(Paged { items: res.invites.into_iter().map(Into::into).collect(), next_page_token: next_token(res.next_page_token) })
    }

    /// Revokes a pending invite.
    pub async fn revoke_invite(&self, invite_id: u64) -> Result<Invite> {
        let res = self
            .invites()?
            .revoke_invite(pb::RevokeInviteRequest { invite_id, ..Default::default() })
            .await?
            .into_owned();
        required(res.invite.into_option())
    }

    /// Emails a pending or expired invite again with a new code (returned) and expiry.
    pub async fn resend_invite(&self, invite_id: u64) -> Result<Invite> {
        let res = self
            .invites()?
            .resend_invite(pb::ResendInviteRequest { invite_id, ..Default::default() })
            .await?
            .into_owned();
        required(res.invite.into_option())
    }

    /// No session needed: what an invite (link token or code) is for.
    pub async fn lookup_invite(&self, token_or_code: &str) -> Result<InvitePreview> {
        let res = self
            .invites()?
            .lookup_invite(pb::LookupInviteRequest { token_or_code: token_or_code.into(), ..Default::default() })
            .await?
            .into_owned();
        Ok(InvitePreview {
            masked_email: res.masked_email,
            inviter: res.inviter.into_option().map(Into::into),
            guild_names: res.guild_names,
            expires_at: time(res.expires_at.as_option()),
            status: res.status.into(),
            terms_version: res.terms_version,
            minimum_age: res.minimum_age,
        })
    }

    /// Creates the account an invite is for and stores its session token.
    /// `password: None` makes a passwordless account (email code / passkey sign-in).
    /// A taken username is [`ApiError::AlreadyExists`]; a used, expired or
    /// revoked invite is [`ApiError::Precondition`]. `terms` must carry the current terms version
    /// and the age confirmation, else [`ApiError::TermsUpdateRequired`] / [`ApiError::InvalidArgument`].
    pub async fn accept_invite(
        &self,
        token_or_code: &str,
        username: &str,
        password: Option<&str>,
        device_name: &str,
        terms: InviteTerms<'_>,
    ) -> Result<(String, User)> {
        let res = self
            .invites()?
            .accept_invite(pb::AcceptInviteRequest {
                token_or_code: token_or_code.into(),
                username: username.into(),
                password: password.map(str::to_owned),
                device_name: device_name.into(),
                accepted_terms_version: terms.version.into(),
                age_confirmed: terms.age_confirmed,
                ..Default::default()
            })
            .await?
            .into_owned();
        self.finish_auth(res.token, res.user.into_option())
    }

    // --- Sign-in -----------------------------------------------------------

    /// Emails a sign-in code (succeeds whether or not the address has an account).
    pub async fn request_email_code(&self, email: &str) -> Result<()> {
        self.auth()?
            .request_email_code(pb::RequestEmailCodeRequest { email: email.into(), ..Default::default() })
            .await?;
        Ok(())
    }

    /// Signs in with an emailed code.
    pub async fn verify_email_code(&self, email: &str, code: &str, device_name: &str) -> Result<LoginOutcome> {
        let res = self
            .auth()?
            .verify_email_code(pb::VerifyEmailCodeRequest {
                email: email.into(),
                code: code.into(),
                device_name: device_name.into(),
                ..Default::default()
            })
            .await?
            .into_owned();
        self.outcome(res.token, res.user.into_option(), res.second_factor_required.into_option())
    }

    /// Answers a [`LoginOutcome::SecondFactor`] challenge; stores the session token.
    pub async fn complete_second_factor(&self, challenge: &str, answer: SecondFactor<'_>) -> Result<(String, User)> {
        let (totp_code, recovery_code) = match answer {
            SecondFactor::Totp(c) => (c.to_owned(), String::new()),
            SecondFactor::RecoveryCode(c) => (String::new(), c.to_owned()),
        };
        let res = self
            .auth()?
            .complete_second_factor(pb::CompleteSecondFactorRequest {
                challenge_id: challenge.into(),
                totp_code,
                recovery_code,
                ..Default::default()
            })
            .await?
            .into_owned();
        self.finish_auth(res.token, res.user.into_option())
    }

    /// Starts answering a [`LoginOutcome::SecondFactor`] challenge with a passkey (offered as
    /// [`SecondFactorMethod::Passkey`]); finish with [`finish_second_factor_passkey`](Self::finish_second_factor_passkey).
    pub async fn begin_second_factor_passkey(&self, challenge: &str, redirect_port: Option<u16>) -> Result<PasskeyCeremony> {
        let res = self
            .auth()?
            .begin_second_factor_passkey(pb::BeginSecondFactorPasskeyRequest {
                challenge_id: challenge.into(),
                redirect_port: redirect_port.map(u32::from),
                ..Default::default()
            })
            .await?
            .into_owned();
        required(res.ceremony.into_option())
    }

    /// Finishes a second-factor passkey answer and stores the token; `Ok(None)` while the
    /// ceremony page has no result yet. A refused passkey is [`ApiError::WrongCode`].
    pub async fn finish_second_factor_passkey(&self, ceremony_id: &str, credential_json: Option<&str>) -> Result<Option<(String, User)>> {
        let res = self
            .auth()?
            .finish_second_factor_passkey(pb::FinishSecondFactorPasskeyRequest {
                ceremony_id: ceremony_id.into(),
                credential_json: credential_json.unwrap_or_default().into(),
                ..Default::default()
            })
            .await?
            .into_owned();
        if res.pending {
            return Ok(None);
        }
        self.finish_auth(res.token, res.user.into_option()).map(Some)
    }

    /// Emails a password reset code (succeeds whether or not the address has an account).
    pub async fn request_password_reset(&self, email: &str) -> Result<()> {
        self.auth()?
            .request_password_reset(pb::RequestPasswordResetRequest { email: email.into(), ..Default::default() })
            .await?;
        Ok(())
    }

    /// Sets a new password with an emailed code; every session is signed out.
    pub async fn reset_password(&self, email: &str, code: &str, new_password: &str) -> Result<()> {
        self.auth()?
            .reset_password(pb::ResetPasswordRequest {
                email: email.into(),
                code: code.into(),
                new_password: new_password.into(),
                ..Default::default()
            })
            .await?;
        Ok(())
    }

    // --- Account security ----------------------------------------------------

    /// How the account is protected: TOTP, password, passkeys, recovery codes, email.
    pub async fn get_security(&self) -> Result<SecuritySummary> {
        let res = self.auth()?.get_security(pb::GetSecurityRequest::default()).await?.into_owned();
        Ok(SecuritySummary {
            totp_enabled: res.totp_enabled,
            has_password: res.has_password,
            passkey_count: res.passkey_count,
            recovery_codes_remaining: res.recovery_codes_remaining,
            email_verified: res.email_verified,
        })
    }

    /// Changes the password (`current_password` is "" for passwordless
    /// accounts); other sessions are signed out.
    pub async fn change_password(&self, current_password: &str, new_password: &str) -> Result<()> {
        self.auth()?
            .change_password(pb::ChangePasswordRequest {
                current_password: current_password.into(),
                new_password: new_password.into(),
                ..Default::default()
            })
            .await?;
        Ok(())
    }

    /// Starts changing (or adding) the email: a code is sent to `new_email`.
    pub async fn change_email(&self, new_email: &str, current_password: &str) -> Result<()> {
        self.auth()?
            .change_email(pb::ChangeEmailRequest {
                new_email: new_email.into(),
                current_password: current_password.into(),
                ..Default::default()
            })
            .await?;
        Ok(())
    }

    /// Finishes [`change_email`](Self::change_email); returns the updated user.
    pub async fn confirm_email_change(&self, code: &str) -> Result<User> {
        let res = self
            .auth()?
            .confirm_email_change(pb::ConfirmEmailChangeRequest { code: code.into(), ..Default::default() })
            .await?
            .into_owned();
        required(res.user.into_option())
    }

    /// Starts enabling an authenticator app.
    pub async fn begin_totp_setup(&self) -> Result<TotpSetup> {
        let res = self.auth()?.begin_totp_setup(pb::BeginTotpSetupRequest::default()).await?.into_owned();
        Ok(TotpSetup { otpauth_uri: res.otpauth_uri, secret: res.secret })
    }

    /// Enables the authenticator app; returns the 10 recovery codes (show them once).
    pub async fn confirm_totp(&self, code: &str) -> Result<Vec<String>> {
        let res = self
            .auth()?
            .confirm_totp(pb::ConfirmTotpRequest { code: code.into(), ..Default::default() })
            .await?
            .into_owned();
        Ok(res.recovery_codes)
    }

    /// Disables the authenticator app with a current or recovery code.
    pub async fn disable_totp(&self, code: &str) -> Result<()> {
        self.auth()?.disable_totp(pb::DisableTotpRequest { code: code.into(), ..Default::default() }).await?;
        Ok(())
    }

    /// New recovery codes (the old ones stop working).
    pub async fn regenerate_recovery_codes(&self, totp_code: &str) -> Result<Vec<String>> {
        let res = self
            .auth()?
            .regenerate_recovery_codes(pb::RegenerateRecoveryCodesRequest { code: totp_code.into(), ..Default::default() })
            .await?
            .into_owned();
        Ok(res.recovery_codes)
    }

    // --- Passkeys ------------------------------------------------------------

    /// Starts adding a passkey. `redirect_port`: the ceremony page then
    /// redirects to `http://127.0.0.1:<port>/done` when finished.
    pub async fn begin_passkey_registration(&self, name: &str, redirect_port: Option<u16>) -> Result<PasskeyCeremony> {
        let res = self
            .auth()?
            .begin_passkey_registration(pb::BeginPasskeyRegistrationRequest {
                name: name.into(),
                redirect_port: redirect_port.map(u32::from),
                ..Default::default()
            })
            .await?
            .into_owned();
        required(res.ceremony.into_option())
    }

    /// Finishes adding a passkey. `credential_json: None` uses the ceremony
    /// page's result; `Ok(None)` means it is not there yet (poll again).
    pub async fn finish_passkey_registration(&self, ceremony_id: &str, credential_json: Option<&str>) -> Result<Option<Passkey>> {
        let res = self
            .auth()?
            .finish_passkey_registration(pb::FinishPasskeyRegistrationRequest {
                ceremony_id: ceremony_id.into(),
                credential_json: credential_json.unwrap_or_default().into(),
                ..Default::default()
            })
            .await?
            .into_owned();
        if res.pending {
            return Ok(None);
        }
        required(res.passkey.into_option()).map(Some)
    }

    pub async fn list_passkeys(&self) -> Result<Vec<Passkey>> {
        let res = self.auth()?.list_passkeys(pb::ListPasskeysRequest::default()).await?.into_owned();
        Ok(res.passkeys.into_iter().map(Into::into).collect())
    }

    pub async fn rename_passkey(&self, passkey_id: u64, name: &str) -> Result<Passkey> {
        let res = self
            .auth()?
            .rename_passkey(pb::RenamePasskeyRequest { passkey_id, name: name.into(), ..Default::default() })
            .await?
            .into_owned();
        required(res.passkey.into_option())
    }

    pub async fn delete_passkey(&self, passkey_id: u64) -> Result<()> {
        self.auth()?.delete_passkey(pb::DeletePasskeyRequest { passkey_id, ..Default::default() }).await?;
        Ok(())
    }

    /// Starts signing in with a passkey of the account `username_or_email`.
    pub async fn begin_passkey_login(
        &self,
        username_or_email: &str,
        device_name: &str,
        redirect_port: Option<u16>,
    ) -> Result<PasskeyCeremony> {
        let res = self
            .auth()?
            .begin_passkey_login(pb::BeginPasskeyLoginRequest {
                username: username_or_email.into(),
                device_name: device_name.into(),
                redirect_port: redirect_port.map(u32::from),
                ..Default::default()
            })
            .await?
            .into_owned();
        required(res.ceremony.into_option())
    }

    /// Finishes a passkey sign-in (no second factor needed) and stores the
    /// token; `Ok(None)` while the ceremony page has no result yet.
    pub async fn finish_passkey_login(&self, ceremony_id: &str, credential_json: Option<&str>) -> Result<Option<(String, User)>> {
        let res = self
            .auth()?
            .finish_passkey_login(pb::FinishPasskeyLoginRequest {
                ceremony_id: ceremony_id.into(),
                credential_json: credential_json.unwrap_or_default().into(),
                ..Default::default()
            })
            .await?
            .into_owned();
        if res.pending {
            return Ok(None);
        }
        self.finish_auth(res.token, res.user.into_option()).map(Some)
    }

    // --- Sessions --------------------------------------------------------------

    /// The caller's sessions, newest first.
    pub async fn list_sessions(&self) -> Result<Vec<DeviceSession>> {
        let res = self.auth()?.list_sessions(pb::ListSessionsRequest::default()).await?.into_owned();
        Ok(res.sessions.into_iter().map(Into::into).collect())
    }

    /// Signs out one session (may be the current one).
    pub async fn revoke_session(&self, session_id: u64) -> Result<()> {
        self.auth()?.revoke_session(pb::RevokeSessionRequest { session_id, ..Default::default() }).await?;
        Ok(())
    }

    /// Signs out every other session; returns how many.
    pub async fn revoke_other_sessions(&self) -> Result<u32> {
        let res = self.auth()?.revoke_other_sessions(pb::RevokeOtherSessionsRequest::default()).await?.into_owned();
        Ok(res.revoked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_maps_methods() {
        let c = pb::SecondFactorChallenge {
            challenge_id: "c".into(),
            methods: vec![
                pb::SecondFactorMethod::SECOND_FACTOR_METHOD_TOTP.into(),
                pb::SecondFactorMethod::SECOND_FACTOR_METHOD_RECOVERY_CODE.into(),
                pb::SecondFactorMethod::SECOND_FACTOR_METHOD_PASSKEY.into(),
            ],
            ..Default::default()
        };
        assert_eq!(
            LoginOutcome::from_challenge(c),
            LoginOutcome::SecondFactor {
                challenge: "c".into(),
                methods: vec![SecondFactorMethod::Totp, SecondFactorMethod::RecoveryCode, SecondFactorMethod::Passkey]
            }
        );
    }

    #[test]
    fn invite_status_maps() {
        assert_eq!(InviteStatus::from(buffa::EnumValue::from(pb::InviteStatus::INVITE_STATUS_EXPIRED)), InviteStatus::Expired);
    }
}
