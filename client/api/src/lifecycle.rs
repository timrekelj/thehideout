//! Account lifecycle calls of [`Client`]: deleting your account, exporting your data, accepting
//! the terms, and the admin calls that suspend or delete other users.

use std::time::SystemTime;

use buffa_types::google::protobuf::Timestamp;

use crate::admin::AdminUserRow;
use crate::client::{Client, required};
use crate::error::ApiError;
use crate::proto::thehideout::v1 as pb;
use crate::types::User;

type Result<T> = std::result::Result<T, ApiError>;

/// Whether the signed-in user has accepted the current terms ([`Client::terms_status`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermsStatus {
    /// The server's current terms version.
    pub current: String,
    /// `Some(version)` while the user still has to accept it: show a prompt, then
    /// [`Client::accept_terms`].
    pub update_required: Option<String>,
}

/// What proves a deletion request. Which fields are needed depends on the account: the password
/// if it has one, else the code emailed by [`Client::request_account_deletion_code`]; plus an
/// authenticator or recovery code when two-factor authentication is on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeleteCredentials {
    pub password: Option<String>,
    pub email_code: Option<String>,
    pub totp_code: Option<String>,
}

fn timestamp(t: SystemTime) -> Timestamp {
    Timestamp::from(t)
}

impl Client {
    /// The signed-in user plus their terms status; the call to make at launch.
    pub async fn get_me_with_terms(&self) -> Result<(User, TermsStatus)> {
        let res = self.auth()?.get_me(pb::GetMeRequest::default()).await?.into_owned();
        let user = required(res.user.into_option())?;
        Ok((user, TermsStatus { current: res.terms_version, update_required: res.terms_update_required }))
    }

    /// Accepts the current terms (`version` from [`TermsStatus::current`]); an outdated version is
    /// [`ApiError::Precondition`].
    pub async fn accept_terms(&self, version: &str) -> Result<()> {
        self.auth()?.accept_terms(pb::AcceptTermsRequest { version: version.into(), ..Default::default() }).await?;
        Ok(())
    }

    /// Emails a 6-digit code that [`delete_account`](Self::delete_account) accepts instead of a
    /// password (for accounts without one).
    pub async fn request_account_deletion_code(&self) -> Result<()> {
        self.auth()?.request_account_deletion_code(pb::RequestAccountDeletionCodeRequest::default()).await?;
        Ok(())
    }

    /// Deletes your account (messages stay, shown as "Deleted user"). Owning guilds is
    /// [`ApiError::OwnsGuilds`]; a wrong password is [`ApiError::PermissionDenied`], a wrong code
    /// [`ApiError::WrongCode`]. On success the session is dead; sign out locally.
    pub async fn delete_account(&self, credentials: DeleteCredentials) -> Result<()> {
        self.auth()?
            .delete_account(pb::DeleteAccountRequest {
                password: credentials.password,
                email_code: credentials.email_code,
                totp_code: credentials.totp_code,
                ..Default::default()
            })
            .await?;
        Ok(())
    }

    /// Queues an export of your data; the link is emailed when ready (valid 7 days). Returns the
    /// masked address it goes to. Once per 24 hours ([`ApiError::RateLimited`]).
    pub async fn export_data(&self) -> Result<String> {
        let res = self.auth()?.export_data(pb::ExportDataRequest::default()).await?.into_owned();
        Ok(res.masked_email)
    }

    /// Site admins: suspends a user (sessions revoked, sign-in refused with
    /// [`ApiError::Suspended`]) until `until`, or until lifted.
    pub async fn admin_suspend_user(&self, user_id: u64, reason: &str, until: Option<SystemTime>) -> Result<AdminUserRow> {
        let res = self
            .admin()?
            .suspend_user(pb::SuspendUserRequest {
                user_id,
                reason: reason.into(),
                until: until.map(timestamp).into(),
                ..Default::default()
            })
            .await?
            .into_owned();
        row(res.user.into_option())
    }

    /// Site admins: lifts a suspension.
    pub async fn admin_unsuspend_user(&self, user_id: u64) -> Result<AdminUserRow> {
        let res = self.admin()?.unsuspend_user(pb::UnsuspendUserRequest { user_id, ..Default::default() }).await?.into_owned();
        row(res.user.into_option())
    }

    /// Site admins: deletes (anonymizes) a user. A user who owns guilds is [`ApiError::OwnsGuilds`]
    /// unless `force_transfer`, which hands each guild to its highest-ranked member and deletes
    /// guilds where the user is alone. Site admins cannot be deleted ([`ApiError::Precondition`]).
    pub async fn admin_delete_user(&self, user_id: u64, force_transfer: bool, reason: &str) -> Result<()> {
        self.admin()?
            .admin_delete_user(pb::AdminDeleteUserRequest { user_id, force_transfer, reason: reason.into(), ..Default::default() })
            .await?;
        Ok(())
    }
}

fn row(r: Option<pb::AdminUserRow>) -> Result<AdminUserRow> {
    r.and_then(AdminUserRow::from_proto).ok_or_else(|| ApiError::Other("the server sent an incomplete response".into()))
}
