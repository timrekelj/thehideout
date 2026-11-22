//! Account lifecycle as state: the terms prompt, deleting your account, exporting your data, and
//! the admin actions that suspend or delete other users. No view here (a later change adds the
//! screens): `AppState` actions push the commands, `*_replied` methods take the answers. Every
//! transition is idempotent and ignores answers nobody is waiting for.

#![cfg_attr(not(test), allow(dead_code))]

use std::time::SystemTime;

use thehideout_api::{AdminUserRow, ApiError, OwnedGuild, TermsStatus};
use thehideout_design::components::ToastKind;

use crate::backend::Command;
use crate::server_settings::error_text;
use crate::state::{AppState, sentence};

/// What messages of a deleted account show as the author.
pub const DELETED_USER: &str = "Deleted user";

/// The pending state of the lifecycle screens.
#[derive(Debug, Default, Clone)]
pub struct Lifecycle {
    /// Set when the user has not accepted the current terms: the version to accept.
    pub terms_update: Option<String>,
    /// The server's current terms version, once known.
    pub terms_current: Option<String>,
    pub terms_busy: bool,
    pub terms_error: Option<String>,
    /// The deletion code was emailed (accounts without a password).
    pub deletion_code_sent: bool,
    pub deletion_code_busy: bool,
    pub delete_busy: bool,
    pub delete_error: Option<String>,
    /// The guilds that block the deletion (the server named them).
    pub owned_guilds: Vec<OwnedGuild>,
    pub export_busy: bool,
    /// The masked address the export link was sent to.
    pub export_sent_to: Option<String>,
    pub export_error: Option<String>,
}

/// What a refused lifecycle call says to the user.
pub fn lifecycle_error_text(e: &ApiError) -> String {
    match e {
        ApiError::PermissionDenied => "That password is wrong.".to_owned(),
        ApiError::WrongCode { attempts_remaining: 1 } => "That code is wrong. 1 try left.".to_owned(),
        ApiError::WrongCode { attempts_remaining } => format!("That code is wrong. {attempts_remaining} tries left."),
        ApiError::CodeExpired => "That code expired. Ask for a new one.".to_owned(),
        ApiError::TooManyAttempts => "Too many wrong codes. Ask for a new one.".to_owned(),
        ApiError::RateLimited => "You're doing that too fast. Try again later.".to_owned(),
        ApiError::OwnsGuilds(guilds) => {
            let names: Vec<&str> = guilds.iter().map(|g| g.name.as_str()).collect();
            format!("You still own {}. Transfer or delete {} first.", names.join(", "), if guilds.len() == 1 { "it" } else { "them" })
        }
        ApiError::Precondition(m) => sentence(m),
        other => error_text(other),
    }
}

impl AppState {
    // ---- terms -------------------------------------------------------------------------------

    /// Asks whether the terms changed since the user accepted them (at launch).
    pub fn check_terms(&mut self) {
        self.outbox.push(Command::CheckTerms);
    }

    /// Whether the re-accept prompt should be up.
    pub fn terms_prompt_due(&self) -> bool {
        self.ui.lifecycle.terms_update.is_some()
    }

    /// "Accept" on the terms prompt.
    pub fn accept_terms(&mut self) {
        let lc = &mut self.ui.lifecycle;
        let Some(version) = lc.terms_update.clone() else { return };
        if lc.terms_busy {
            return;
        }
        lc.terms_busy = true;
        lc.terms_error = None;
        self.outbox.push(Command::AcceptTerms { version });
    }

    pub fn terms_replied(&mut self, result: Result<TermsStatus, ApiError>) {
        if let Ok(status) = result {
            let lc = &mut self.ui.lifecycle;
            lc.terms_current = Some(status.current);
            lc.terms_update = status.update_required;
        }
    }

    pub fn terms_accepted_replied(&mut self, version: &str, result: Result<(), ApiError>) {
        let lc = &mut self.ui.lifecycle;
        lc.terms_busy = false;
        match result {
            Ok(()) => {
                if lc.terms_update.as_deref() == Some(version) {
                    lc.terms_update = None;
                }
                lc.terms_error = None;
            }
            Err(e @ ApiError::Precondition(_)) => {
                // The terms moved on while the prompt was up: look again.
                lc.terms_error = Some(lifecycle_error_text(&e));
                self.outbox.push(Command::CheckTerms);
            }
            Err(e) => lc.terms_error = Some(lifecycle_error_text(&e)),
        }
    }

    // ---- deleting your account ---------------------------------------------------------------

    /// "Email me a code" (accounts without a password).
    pub fn request_deletion_code(&mut self) {
        let lc = &mut self.ui.lifecycle;
        if lc.deletion_code_busy {
            return;
        }
        lc.deletion_code_busy = true;
        lc.delete_error = None;
        self.outbox.push(Command::RequestAccountDeletionCode);
    }

    pub fn deletion_code_replied(&mut self, result: Result<(), ApiError>) {
        let lc = &mut self.ui.lifecycle;
        lc.deletion_code_busy = false;
        match result {
            Ok(()) => lc.deletion_code_sent = true,
            Err(e) => lc.delete_error = Some(lifecycle_error_text(&e)),
        }
    }

    /// Deletes the signed-in account. Empty strings count as "not given".
    pub fn delete_account(&mut self, password: &str, email_code: &str, totp_code: &str) {
        let lc = &mut self.ui.lifecycle;
        if lc.delete_busy {
            return;
        }
        let given = |s: &str| (!s.trim().is_empty()).then(|| s.trim().to_owned());
        lc.delete_busy = true;
        lc.delete_error = None;
        lc.owned_guilds.clear();
        self.outbox.push(Command::DeleteAccount {
            password: given(password),
            email_code: given(email_code),
            totp_code: given(totp_code),
        });
    }

    pub fn account_deleted_replied(&mut self, result: Result<(), ApiError>) {
        match result {
            Ok(()) => {
                // The session is dead: start over from the sign-in form.
                self.sign_out();
                self.auth.notice = Some("Your account was deleted.".to_owned());
            }
            Err(e) => {
                let lc = &mut self.ui.lifecycle;
                lc.delete_busy = false;
                if let ApiError::OwnsGuilds(guilds) = &e {
                    lc.owned_guilds = guilds.clone();
                }
                lc.delete_error = Some(lifecycle_error_text(&e));
            }
        }
    }

    // ---- exporting your data -----------------------------------------------------------------

    pub fn export_data(&mut self) {
        let lc = &mut self.ui.lifecycle;
        if lc.export_busy {
            return;
        }
        lc.export_busy = true;
        lc.export_error = None;
        lc.export_sent_to = None;
        self.outbox.push(Command::ExportData);
    }

    pub fn export_replied(&mut self, result: Result<String, ApiError>) {
        let lc = &mut self.ui.lifecycle;
        lc.export_busy = false;
        match result {
            Ok(address) => lc.export_sent_to = Some(address),
            Err(ApiError::RateLimited) => {
                lc.export_error = Some("You can export your data once every 24 hours.".to_owned())
            }
            Err(e) => lc.export_error = Some(lifecycle_error_text(&e)),
        }
    }

    // ---- admin: suspending and deleting users ------------------------------------------------

    fn admin_user_idle(&self, user: u64) -> bool {
        self.ui.admin.as_ref().is_some_and(|ad| !ad.busy_users.contains(&user) && ad.users.items.iter().any(|r| r.user.id == user))
    }

    /// Suspends a user (their sessions end, sign-in is refused); `until: None` = until lifted.
    pub fn admin_suspend(&mut self, user: u64, reason: &str, until: Option<SystemTime>) {
        if !self.admin_user_idle(user) || user == self.my_id() {
            return;
        }
        let ad = self.ui.admin.as_mut().expect("checked");
        ad.busy_users.insert(user);
        ad.user_errors.remove(&user);
        self.outbox.push(Command::AdminSuspendUser { user, reason: reason.trim().to_owned(), until });
    }

    pub fn admin_unsuspend(&mut self, user: u64) {
        if !self.admin_user_idle(user) {
            return;
        }
        let ad = self.ui.admin.as_mut().expect("checked");
        ad.busy_users.insert(user);
        ad.user_errors.remove(&user);
        self.outbox.push(Command::AdminUnsuspendUser { user });
    }

    /// Deletes (anonymizes) a user; `force_transfer` hands over or deletes the guilds they own.
    pub fn admin_delete_user(&mut self, user: u64, force_transfer: bool, reason: &str) {
        if !self.admin_user_idle(user) || user == self.my_id() {
            return;
        }
        let ad = self.ui.admin.as_mut().expect("checked");
        ad.busy_users.insert(user);
        ad.user_errors.remove(&user);
        self.outbox.push(Command::AdminDeleteUser { user, force_transfer, reason: reason.trim().to_owned() });
    }

    /// Takes the suspension fields of `fresh` into the listed row (the usage numbers stay).
    fn merge_admin_row(&mut self, user: u64, fresh: AdminUserRow) {
        let Some(ad) = self.ui.admin.as_mut() else { return };
        if let Some(row) = ad.users.items.iter_mut().find(|r| r.user.id == user) {
            row.suspended = fresh.suspended;
            row.suspended_until = fresh.suspended_until;
            row.suspended_reason = fresh.suspended_reason;
            row.deleted = fresh.deleted;
            row.user = fresh.user;
        }
        ad.stale.insert(crate::admin::AdminPage::Audit);
    }

    fn admin_user_failed(&mut self, user: u64, e: &ApiError) {
        let Some(ad) = self.ui.admin.as_mut() else { return };
        if matches!(e, ApiError::Unimplemented) {
            ad.unavailable = true;
            return;
        }
        ad.user_errors.insert(user, lifecycle_error_text(e));
    }

    pub fn admin_suspend_replied(&mut self, user: u64, result: Result<AdminUserRow, ApiError>) {
        if let Some(ad) = self.ui.admin.as_mut() {
            ad.busy_users.remove(&user);
        }
        match result {
            Ok(fresh) => {
                let name = fresh.user.username.clone();
                self.merge_admin_row(user, fresh);
                self.toast(ToastKind::Success, format!("{name} is suspended"));
            }
            Err(e) => self.admin_user_failed(user, &e),
        }
    }

    pub fn admin_unsuspend_replied(&mut self, user: u64, result: Result<AdminUserRow, ApiError>) {
        if let Some(ad) = self.ui.admin.as_mut() {
            ad.busy_users.remove(&user);
        }
        match result {
            Ok(fresh) => {
                let name = fresh.user.username.clone();
                self.merge_admin_row(user, fresh);
                self.toast(ToastKind::Success, format!("{name} can sign in again"));
            }
            Err(e) => self.admin_user_failed(user, &e),
        }
    }

    pub fn admin_delete_replied(&mut self, user: u64, result: Result<(), ApiError>) {
        if let Some(ad) = self.ui.admin.as_mut() {
            ad.busy_users.remove(&user);
        }
        match result {
            Ok(()) => {
                let name = self.user_name(user);
                if let Some(ad) = self.ui.admin.as_mut() {
                    if let Some(row) = ad.users.items.iter_mut().find(|r| r.user.id == user) {
                        row.deleted = true;
                        row.suspended = false;
                        row.user.username = DELETED_USER.to_owned();
                        row.user.avatar = None;
                        row.user.email = None;
                        row.user.deleted = true;
                    }
                    ad.stale.insert(crate::admin::AdminPage::Audit);
                }
                self.toast(ToastKind::Success, format!("{name} was deleted"));
            }
            Err(e) => self.admin_user_failed(user, &e),
        }
    }
}
