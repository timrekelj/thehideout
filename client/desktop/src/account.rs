//! Signing in, invites and account security on `AppState`: the auth card's steps (password,
//! emailed code, passkey, reset, second factor, invite), the Security settings page and its
//! dialogs, the "add an email" prompt of legacy accounts and the server's invite-by-email
//! dialog. Pure state and commands, like the rest of the model; `apply.rs` hands the replies in.
//!
//! Errors never say whether an account exists: a wrong password, an unknown email and a wrong
//! code read the same, and "we sent a code" is said for any address.

use std::collections::HashSet;

use rune::{TextEdit, Tx};
use thehideout_api::{
    ApiError, DeviceSession, Invite, InvitePreview, InviteStatus, NewInvite, Passkey, Permissions,
    SecondFactorMethod, TotpSetup, User,
};
use thehideout_design::components::invite_code_input::CODE_LEN;
use thehideout_design::components::{ToastKind, digits_only, normalize_code};

use crate::backend::{Command, InviteAction, SecurityInfo, Session};
use crate::forms::{
    AuthError, AuthField, AuthMode, AuthStep, DialogKind, Phase, RESEND_SECS, auth_error, sentence,
};
use crate::server_settings::error_text;
use crate::state::{AppState, Screen, Section};

/// Shortest password the server takes.
pub const MIN_PASSWORD: usize = 8;
/// Invites listed in the server's invite dialog.
pub const INVITE_PAGE: u32 = 50;

/// A dialog of the Security page (also opened after sign-up and from the add-email prompt).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SecKind {
    /// Change (or add) the email: the address and current password, then (`sent`) the code.
    Email { sent: bool },
    Password,
    /// The authenticator setup: the QR code and secret once `setup` arrives, then the code.
    TotpSetup { setup: Option<TotpSetup> },
    /// Fresh recovery codes, shown this once.
    RecoveryCodes { codes: Vec<String> },
    DisableTotp,
    Regenerate,
    /// A new passkey: its name, then (`waiting`) the browser ceremony.
    AddPasskey { waiting: bool },
    RenamePasskey { id: u64 },
    DeletePasskey { id: u64 },
    /// Right after sign-up: protect the account (skippable).
    Welcome,
}

pub struct SecDialog {
    pub kind: SecKind,
    /// Email: the new address; Password: the current one; passkeys: the name.
    pub first: TextEdit,
    /// Email: the current password; Password: the new one.
    pub second: TextEdit,
    /// Password: the new one again.
    pub third: TextEdit,
    /// A 6-digit (or recovery) code.
    pub code: TextEdit,
    pub error: Option<String>,
    pub busy: bool,
    /// Recovery codes: "I saved them" is ticked.
    pub saved: bool,
}

impl SecDialog {
    fn new(kind: SecKind) -> SecDialog {
        let mut d = SecDialog {
            kind,
            first: TextEdit::new(""),
            second: TextEdit::new("").secure(),
            third: TextEdit::new("").secure(),
            code: TextEdit::new(""),
            error: None,
            busy: false,
            saved: false,
        };
        match &d.kind {
            SecKind::Password => d.first = TextEdit::new("").secure(),
            SecKind::TotpSetup { .. } | SecKind::DisableTotp | SecKind::Regenerate => d.code.focused = true,
            _ => {}
        }
        d.first.focused = !d.code.focused;
        d
    }
}

/// The Security page and its dialogs. Lives as long as the session.
#[derive(Default)]
pub struct SecurityUi {
    pub info: Option<SecurityInfo>,
    pub loading: bool,
    pub error: Option<String>,
    /// Whether an authenticator app is on, as far as this app knows.
    pub totp: Option<bool>,
    pub dialog: Option<SecDialog>,
    /// The dialog to open when this one closes (sign-up: the passkey, then the 2FA offer).
    pub next: Option<SecKind>,
    /// Sessions being signed out and passkeys being removed.
    pub busy: HashSet<u64>,
    pub others_busy: bool,
    /// The legacy account's "add an email" banner was dismissed.
    pub email_banner_hidden: bool,
    /// Signed up a moment ago: offer protection once the shell is up (and first a passkey).
    pub welcome: Option<bool>,
}

/// The server invite dialog's list and the invite it just made.
#[derive(Default)]
pub struct InvitesUi {
    pub list: Vec<Invite>,
    pub loaded: bool,
    pub error: Option<String>,
    pub epoch: u64,
    /// The invite just made (or sent again), with its code.
    pub created: Option<Invite>,
    pub busy: HashSet<u64>,
}

/// The invite code as typed or pasted, cleaned up (a pasted link gives its code) and written
/// `XXXX-XXXX` once it is whole; shorter codes come back as the bare characters.
pub fn invite_code(raw: &str) -> String {
    let code = normalize_code(raw);
    if code.len() == CODE_LEN {
        format!("{}-{}", &code[..CODE_LEN / 2], &code[CODE_LEN / 2..])
    } else {
        code
    }
}

/// The whole code in an invite link (`thehideout://invite/XXXX-XXXX`, `https://<host>/invite/...`),
/// `None` when the text has no `/invite/` or no full 8-character code after it.
pub fn link_code(url: &str) -> Option<String> {
    let url = url.trim();
    let rest = &url[url.rfind("/invite/")? + "/invite/".len()..];
    exact_code(rest.split(['?', '#', '/']).next().unwrap_or(""))
}

/// The code of the invite link the web client was opened at (`/app/?invite=XXXX-XXXX`; the path
/// may lack its trailing slash); `None` for any other page or a code that isn't complete.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn invite_query_code(pathname: &str, search: &str) -> Option<String> {
    if !matches!(pathname, "/app" | "/app/") {
        return None;
    }
    let query = search.strip_prefix('?').unwrap_or(search);
    let value = query.split('&').find_map(|pair| pair.strip_prefix("invite="))?;
    // The browser may send the dash percent-encoded.
    exact_code(&value.replace("%2D", "-").replace("%2d", "-"))
}

/// The code characters of `segment` (as the code field reads them) when there are exactly
/// [`CODE_LEN`] of them; a longer or shorter segment is no code, it is not cut to length.
fn exact_code(segment: &str) -> Option<String> {
    let code: String = segment
        .chars()
        .filter_map(|c| normalize_code(c.encode_utf8(&mut [0; 4])).chars().next())
        .collect();
    (code.len() == CODE_LEN).then_some(code)
}

/// A loose email check: something@something.something, no spaces.
pub fn looks_like_email(s: &str) -> bool {
    let s = s.trim();
    match s.split_once('@') {
        Some((a, b)) => !a.is_empty() && b.contains('.') && !b.starts_with('.') && !b.ends_with('.') && !s.contains(char::is_whitespace),
        None => false,
    }
}

fn status_problem(status: InviteStatus) -> Option<&'static str> {
    match status {
        InviteStatus::Pending => None,
        InviteStatus::Accepted => Some("This invite was already used. Sign in instead."),
        InviteStatus::Expired => Some("This invite has expired. Ask for a new one."),
        InviteStatus::Revoked => Some("This invite was withdrawn. Ask for a new one."),
        InviteStatus::Unknown => Some("This invite can't be used."),
    }
}

impl AppState {
    // ---- the auth card ---------------------------------------------------------------------------

    pub fn set_auth_mode(&mut self, mode: AuthMode) {
        if self.auth.phase == Phase::Busy {
            return;
        }
        self.cancel_auth_passkey();
        self.auth.mode = mode;
        self.auth.go(match mode {
            AuthMode::Login => AuthStep::Password,
            AuthMode::Invite => AuthStep::InviteCode,
        });
    }

    /// The links under the form: "Email me a code instead", "Use a passkey", "Forgot password?",
    /// "Use your password" and "Start again". What was typed as the login comes along.
    pub fn auth_go(&mut self, step: AuthStep) {
        if self.auth.phase == Phase::Busy && !self.auth_passkey_waiting() {
            return;
        }
        self.cancel_auth_passkey();
        let login = self.auth.username.text.trim().to_owned();
        if matches!(step, AuthStep::EmailCode { sent: false } | AuthStep::Reset { sent: false })
            && self.auth.email.text.is_empty()
            && login.contains('@')
        {
            self.auth.email.replace_text(&login);
        }
        self.auth.password.clear();
        self.auth.go(step);
    }

    /// Whether the form waits for a passkey in the browser (a sign-in or a second factor).
    fn auth_passkey_waiting(&self) -> bool {
        matches!(self.auth.step, AuthStep::Passkey { waiting: true } | AuthStep::SecondFactor { passkey: true, .. })
    }

    fn cancel_auth_passkey(&mut self) {
        if !self.auth_passkey_waiting() {
            return;
        }
        self.outbox.push(Command::CancelPasskey);
        match &mut self.auth.step {
            AuthStep::SecondFactor { passkey, .. } => *passkey = false,
            step => *step = AuthStep::Passkey { waiting: false },
        }
        self.auth.phase = Phase::Idle;
    }

    /// The waiting passkey's Cancel (a sign-in or a second factor).
    pub fn cancel_passkey_login(&mut self) {
        self.cancel_auth_passkey();
    }

    /// "Use a passkey" on the second-factor step: the browser ceremony answers this challenge.
    pub fn second_factor_passkey(&mut self) {
        if self.auth.phase == Phase::Busy {
            return;
        }
        let AuthStep::SecondFactor { challenge, locked: false, passkey: false, .. } = &self.auth.step else { return };
        let cmd = Command::BeginSecondFactorPasskey { challenge: challenge.clone() };
        self.auth.error = None;
        self.auth.notice = None;
        self.auth.phase = Phase::Busy;
        self.outbox.push(cmd);
    }

    fn auth_invalid(&mut self, field: AuthField, message: &str) {
        self.auth.error = Some(AuthError { field, message: message.into(), suspension: None });
    }

    /// Enter or the primary button, whatever the step.
    pub fn submit_auth(&mut self) {
        if self.auth.phase == Phase::Busy {
            return;
        }
        let a = &self.auth;
        let login = a.username.text.trim().to_owned();
        let password = a.password.text.clone();
        let email = a.email.text.trim().to_owned();
        let code = a.code.text.trim().to_owned();
        let cmd = match a.step.clone() {
            AuthStep::Password => {
                if login.is_empty() {
                    return self.auth_invalid(AuthField::Username, "Enter your username or email.");
                }
                if password.is_empty() {
                    return self.auth_invalid(AuthField::Password, "Enter your password.");
                }
                Command::Login { username: login, password }
            }
            AuthStep::EmailCode { sent: false } | AuthStep::Reset { sent: false } => {
                if !looks_like_email(&email) {
                    return self.auth_invalid(AuthField::Email, "Enter the email of your account.");
                }
                if matches!(a.step, AuthStep::Reset { .. }) {
                    Command::RequestPasswordReset { email }
                } else {
                    Command::RequestEmailCode { email }
                }
            }
            AuthStep::EmailCode { sent: true } => {
                if code.len() != 6 {
                    return self.auth_invalid(AuthField::Code, "Enter the 6-digit code from the email.");
                }
                Command::VerifyEmailCode { email, code }
            }
            AuthStep::Reset { sent: true } => {
                if code.len() != 6 {
                    return self.auth_invalid(AuthField::Code, "Enter the 6-digit code from the email.");
                }
                if password.chars().count() < MIN_PASSWORD {
                    return self.auth_invalid(AuthField::Password, "Use at least 8 characters.");
                }
                Command::ResetPassword { email, code, new_password: password }
            }
            AuthStep::Passkey { waiting: true } => return,
            AuthStep::Passkey { waiting: false } => {
                if login.is_empty() {
                    return self.auth_invalid(AuthField::Username, "Enter your username.");
                }
                Command::BeginPasskeyLogin { username: login }
            }
            AuthStep::SecondFactor { locked: true, .. } | AuthStep::SecondFactor { passkey: true, .. } => return,
            AuthStep::SecondFactor { challenge, recovery, .. } => {
                let code = if recovery { a.recovery.text.trim().to_owned() } else { code };
                if recovery && code.is_empty() {
                    return self.auth_invalid(AuthField::Code, "Enter one of your recovery codes.");
                }
                if !recovery && code.len() != 6 {
                    return self.auth_invalid(AuthField::Code, "Enter the 6-digit code from your authenticator app.");
                }
                Command::CompleteSecondFactor { challenge, code, recovery }
            }
            AuthStep::InviteCode => {
                let code = invite_code(&a.invite.text);
                if code.is_empty() {
                    return self.auth_invalid(AuthField::Invite, "Enter the code from your invite email.");
                }
                if normalize_code(&code).len() != CODE_LEN {
                    return self.auth_invalid(AuthField::Invite, "Invite codes have 8 characters.");
                }
                self.auth.invite_tried = Some(code.clone());
                Command::LookupInvite { code }
            }
            AuthStep::InviteAccept { code, .. } => {
                let n = login.chars().count();
                if !(3..=32).contains(&n) {
                    return self.auth_invalid(AuthField::Username, "Choose a username of 3 to 32 characters.");
                }
                if password.chars().count() < MIN_PASSWORD {
                    return self.auth_invalid(AuthField::Password, "Use at least 8 characters.");
                }
                if !self.auth.accept_terms {
                    return self.auth_invalid(AuthField::Form, "Accept the terms and privacy notice to continue.");
                }
                if !self.auth.confirm_age {
                    return self.auth_invalid(AuthField::Form, "Confirm that you are at least 16 years old.");
                }
                let terms_version = match &self.auth.step {
                    AuthStep::InviteAccept { preview, .. } => preview.terms_version.clone(),
                    _ => String::new(),
                };
                Command::AcceptInvite {
                    code,
                    username: login,
                    password: Some(password),
                    terms_version,
                    age_confirmed: self.auth.confirm_age,
                }
            }
        };
        self.auth.error = None;
        self.auth.notice = None;
        self.auth.phase = Phase::Busy;
        self.outbox.push(cmd);
    }

    /// The code field changed: keep digits only, and a full code goes at once.
    pub fn auth_code_changed(&mut self) {
        let clean = digits_only(&self.auth.code.text, 6);
        if clean != self.auth.code.text {
            self.auth.code.replace_text(&clean);
        }
        if self.auth.error.as_ref().is_some_and(|e| e.field == AuthField::Code) {
            self.auth.error = None;
        }
        let auto = matches!(self.auth.step, AuthStep::EmailCode { sent: true } | AuthStep::SecondFactor { recovery: false, locked: false, passkey: false, .. });
        if clean.len() == 6 && auto {
            self.submit_auth();
        }
    }

    /// "Use a recovery code" / "Use your authenticator app".
    pub fn toggle_recovery(&mut self) {
        if let AuthStep::SecondFactor { recovery, locked: false, passkey: false, .. } = &mut self.auth.step {
            *recovery = !*recovery;
            let r = *recovery;
            self.auth.error = None;
            self.auth.code.clear();
            self.auth.recovery.clear();
            self.auth.code.focused = !r;
            self.auth.recovery.focused = r;
        }
    }

    /// Seconds until "Resend code" works again (0: now).
    pub fn resend_wait(&self) -> u32 {
        match self.auth.resend_at {
            Some(t) if t > self.clock => (t - self.clock).ceil() as u32,
            Some(_) => 0,
            None if self.auth.resend_due => RESEND_SECS as u32,
            None => 0,
        }
    }

    pub fn resend_code(&mut self) {
        if self.auth.phase == Phase::Busy || self.resend_wait() > 0 {
            return;
        }
        let email = self.auth.email.text.trim().to_owned();
        let cmd = match self.auth.step {
            AuthStep::EmailCode { sent: true } => Command::RequestEmailCode { email },
            AuthStep::Reset { sent: true } => Command::RequestPasswordReset { email },
            _ => return,
        };
        self.auth.code.clear();
        self.auth.error = None;
        self.auth.phase = Phase::Busy;
        self.outbox.push(cmd);
    }

    /// The resend countdown, from `tick`: whether it changed what is shown, and when to come back.
    pub(crate) fn tick_resend(&mut self, clock: f64) -> (bool, Option<f64>) {
        if std::mem::take(&mut self.auth.resend_due) {
            self.auth.resend_at = Some(clock + RESEND_SECS);
        }
        match self.auth.resend_at {
            Some(t) if t > clock => (true, Some(clock + (t - clock).min(1.0))),
            Some(_) => {
                self.auth.resend_at = None;
                (true, None)
            }
            None => (false, None),
        }
    }

    // ---- sign-in replies ----------------------------------------------------------------------------

    /// A sign-in was refused: the error goes where the user is looking, and what they typed stays
    /// (only the secret goes).
    pub(crate) fn auth_refused(&mut self, e: &ApiError) {
        let wrong_code = "That code didn't work. Check it and try again.";
        self.auth.phase = Phase::Idle;
        self.auth.password.clear();
        match &mut self.auth.step {
            // The server says what happened: a wrong answer (and how many are left), or a
            // challenge that is used up or expired.
            AuthStep::SecondFactor { remaining, locked, recovery, passkey, .. } => {
                let (r, was_passkey) = (*recovery, std::mem::take(passkey));
                match e {
                    ApiError::WrongCode { attempts_remaining } => *remaining = Some(*attempts_remaining),
                    ApiError::TooManyAttempts | ApiError::CodeExpired => *locked = true,
                    _ => {}
                }
                self.auth.code.clear();
                self.auth.recovery.clear();
                match e {
                    ApiError::TooManyAttempts => self.auth.fail(AuthField::Form, "Too many wrong attempts. Start signing in again."),
                    ApiError::CodeExpired => self.auth.fail(AuthField::Form, "This sign-in expired. Start again."),
                    ApiError::WrongCode { attempts_remaining } => {
                        let (field, what) = match (was_passkey, r) {
                            (true, _) => (AuthField::Form, "That passkey wasn't accepted."),
                            (false, true) => (AuthField::Code, "That recovery code didn't work."),
                            (false, false) => (AuthField::Code, "That code didn't work."),
                        };
                        self.auth.fail(field, format!("{what} {}", tries_left(*attempts_remaining)));
                    }
                    ApiError::Precondition(_) if was_passkey => {
                        self.auth.fail(AuthField::Form, "A passkey can't be used for this sign-in. Use a code instead.")
                    }
                    _ => self.auth.error = Some(auth_error(e)),
                }
            }
            AuthStep::EmailCode { sent: true } if *e == ApiError::Unauthenticated => {
                self.auth.code.clear();
                self.auth.fail(AuthField::Code, wrong_code);
            }
            AuthStep::Passkey { waiting } => {
                *waiting = false;
                let message = match e {
                    ApiError::Precondition(_) | ApiError::NotFound => "Passkey sign-in isn't available for that account. Use your password or an emailed code.".to_owned(),
                    ApiError::Unauthenticated => "The passkey wasn't accepted. Try again.".to_owned(),
                    other => auth_error(other).message,
                };
                self.auth.fail(AuthField::Form, message);
            }
            AuthStep::InviteAccept { .. } => {
                let (field, message) = match e {
                    ApiError::AlreadyExists => (AuthField::Username, "That username is taken. Try another.".to_owned()),
                    ApiError::Precondition(m) => (AuthField::Form, format!("This invite can't be used any more ({m}). Ask for a new one.")),
                    ApiError::NotFound => (AuthField::Form, "This invite can't be used any more. Ask for a new one.".to_owned()),
                    ApiError::InvalidArgument(m) => (AuthField::Form, sentence(m)),
                    other => (AuthField::Form, auth_error(other).message),
                };
                self.auth.fail(field, message);
            }
            AuthStep::Password if *e == ApiError::Unauthenticated => {
                self.auth.fail(AuthField::Form, "Wrong username, email or password.");
            }
            _ => self.auth.error = Some(auth_error(e)),
        }
    }

    pub(crate) fn second_factor(&mut self, challenge: String, methods: Vec<SecondFactorMethod>) {
        self.auth.mode = AuthMode::Login;
        self.auth.password.clear();
        self.auth.go(AuthStep::SecondFactor { challenge, methods, recovery: false, remaining: None, locked: false, passkey: false });
    }

    /// A sign-in landed. Remembers what it says about the account for the Security page.
    pub(crate) fn auth_succeeded(&mut self, session: &Session) {
        if matches!(self.auth.step, AuthStep::SecondFactor { .. }) {
            self.ui.security.totp = Some(true);
        }
        if matches!(self.auth.step, AuthStep::InviteAccept { .. }) {
            self.ui.security.totp = Some(false);
            self.ui.security.welcome = Some(self.auth.add_passkey);
        }
        let _ = session;
    }

    pub(crate) fn code_sent(&mut self, result: Result<(), ApiError>) {
        self.auth.phase = Phase::Idle;
        if let Err(e) = result {
            self.auth.error = Some(auth_error(&e));
            return;
        }
        let email = self.auth.email.text.trim().to_owned();
        let step = match self.auth.step {
            AuthStep::Reset { .. } => AuthStep::Reset { sent: true },
            _ => AuthStep::EmailCode { sent: true },
        };
        if self.auth.step != step {
            self.auth.go(step);
        }
        self.auth.notice = Some(format!("If an account uses {email}, a code is on its way. It works for 10 minutes."));
        self.auth.resend_at = None;
        self.auth.resend_due = true;
    }

    pub(crate) fn password_reset(&mut self, result: Result<(), ApiError>) {
        match result {
            Ok(()) => {
                let email = self.auth.email.text.trim().to_owned();
                self.auth.go(AuthStep::Password);
                self.auth.username.replace_text(&email);
                self.auth.username.focused = false;
                self.auth.password.focused = true;
                self.auth.notice = Some("Your password was changed and every device was signed out. Sign in with the new one.".into());
            }
            Err(ApiError::Unauthenticated) => {
                self.auth.code.clear();
                self.auth.fail(AuthField::Code, "That code didn't work. Check it, or send a new one.");
            }
            Err(e) => {
                self.auth.phase = Phase::Idle;
                self.auth.error = Some(auth_error(&e));
            }
        }
    }

    pub(crate) fn invite_looked_up(&mut self, result: Result<InvitePreview, ApiError>) {
        self.auth.phase = Phase::Idle;
        match result {
            Ok(preview) => match status_problem(preview.status) {
                Some(problem) => self.auth.fail(AuthField::Invite, problem),
                None => {
                    let code = invite_code(&self.auth.invite.text);
                    self.auth.go(AuthStep::InviteAccept { preview, code });
                }
            },
            Err(ApiError::NotFound) | Err(ApiError::InvalidArgument(_)) => {
                self.auth.fail(AuthField::Invite, "We couldn't find that invite. Check the code and try again.")
            }
            Err(e) => self.auth.error = Some(auth_error(&e)),
        }
    }

    /// A link the system opened the app with (`thehideout://invite/XXXX-XXXX`, or the emailed
    /// `https://<host>/invite/XXXX-XXXX`): straight to the invite, when signed out. Only a full
    /// code counts.
    pub fn open_link(&mut self, url: &str) {
        let Some(code) = link_code(url) else { return };
        if self.session.is_some() || self.auth.phase == Phase::Busy {
            return;
        }
        self.set_auth_mode(AuthMode::Invite);
        self.auth.invite.replace_text(&code);
        self.submit_auth();
    }

    /// The 8th character of an invite code was entered (typed or pasted): look it up, once per
    /// code. Enter and "Continue" still work; so does a second go at a code that failed, after it
    /// is edited. True when the lookup started.
    pub fn auto_submit_invite(&mut self) -> bool {
        if self.auth.step != AuthStep::InviteCode || self.auth.phase == Phase::Busy {
            return false;
        }
        let code = invite_code(&self.auth.invite.text);
        if normalize_code(&code).len() != CODE_LEN {
            self.auth.invite_tried = None;
            return false;
        }
        if self.auth.invite_tried.as_deref() == Some(code.as_str()) {
            return false;
        }
        self.submit_auth();
        true
    }

    /// The link the page was opened at, once the saved session has been checked.
    /// True when it did something.
    pub fn apply_pending_link(&mut self) -> bool {
        match self.screen {
            Screen::Loading => false,
            Screen::Main => self.pending_link.take().is_some(),
            Screen::Auth => match self.pending_link.take() {
                Some(url) => {
                    self.open_link(&url);
                    true
                }
                None => false,
            },
        }
    }

    /// A passkey ceremony page is ready: the browser opens it and the form (or dialog) waits.
    pub(crate) fn passkey_ceremony(&mut self, result: Result<String, ApiError>, tx: &mut Tx) {
        let signing_in = self.session.is_none();
        match result {
            Ok(url) => {
                // `open_url` refuses anything but http(s); the server's page is one.
                let _ = tx.open_url(&url);
                if signing_in {
                    match &mut self.auth.step {
                        AuthStep::Passkey { .. } => self.auth.step = AuthStep::Passkey { waiting: true },
                        AuthStep::SecondFactor { passkey, .. } => *passkey = true,
                        _ => {}
                    }
                } else if let Some(d) = self.ui.security.dialog.as_mut()
                    && matches!(d.kind, SecKind::AddPasskey { .. })
                {
                    d.kind = SecKind::AddPasskey { waiting: true };
                    d.busy = false;
                }
            }
            Err(e) if signing_in => self.auth_refused(&e),
            Err(e) => self.sec_failed(&e),
        }
    }

    // ---- the add-email prompt -----------------------------------------------------------------------

    /// A legacy account (no email) sees the prompt until it is dismissed for this session.
    pub fn wants_email(&self) -> bool {
        self.me().is_some_and(|u| u.email.is_none()) && !self.ui.security.email_banner_hidden
    }

    pub fn dismiss_email_banner(&mut self) {
        self.ui.security.email_banner_hidden = true;
    }

    /// After the first load of a session: the sign-up offer, if one is due.
    pub(crate) fn after_initial(&mut self) {
        self.check_terms();
        if let Some(passkey) = self.ui.security.welcome.take() {
            if passkey {
                self.ui.security.next = Some(SecKind::Welcome);
                self.open_sec(SecKind::AddPasskey { waiting: false });
            } else {
                self.open_sec(SecKind::Welcome);
            }
        }
    }

    // ---- the Security page --------------------------------------------------------------------------

    pub fn open_security(&mut self) {
        self.open_settings();
        self.ui.settings = Some(Section::Security);
        self.load_security();
    }

    pub fn load_security(&mut self) {
        if !self.ui.security.loading {
            self.ui.security.loading = true;
            self.ui.security.error = None;
            self.outbox.push(Command::LoadSecurity);
        }
    }

    pub(crate) fn security_loaded(&mut self, result: Result<SecurityInfo, ApiError>) {
        let sec = &mut self.ui.security;
        sec.loading = false;
        match result {
            Ok(info) => {
                sec.totp = Some(info.summary.totp_enabled);
                sec.info = Some(info);
                sec.error = None;
            }
            Err(e) => sec.error = Some(error_text(&e)),
        }
    }

    /// Opens a Security dialog; one that needs the server first asks for it.
    pub fn open_sec(&mut self, kind: SecKind) {
        self.ui.menu = crate::state::Menu::None;
        let mut d = SecDialog::new(kind);
        match &d.kind {
            SecKind::TotpSetup { setup: None } => {
                d.busy = true;
                self.outbox.push(Command::BeginTotpSetup);
            }
            SecKind::AddPasskey { .. } => d.first.replace_text(crate::backend::device_label()),
            SecKind::RenamePasskey { id } => {
                let name = self.passkey(*id).map_or_else(String::new, |p| p.name.clone());
                d.first.replace_text(&name);
            }
            SecKind::Email { .. } => {}
            _ => {}
        }
        self.ui.security.dialog = Some(d);
    }

    pub fn passkey(&self, id: u64) -> Option<&Passkey> {
        self.ui.security.info.as_ref()?.passkeys.iter().find(|p| p.id == id)
    }

    /// Cancel, Esc, Skip, Done. Recovery codes only go once "I saved them" is ticked; a waiting
    /// passkey ceremony is stopped.
    pub fn close_sec(&mut self) {
        let Some(d) = &self.ui.security.dialog else { return };
        match &d.kind {
            SecKind::RecoveryCodes { .. } if !d.saved => return,
            SecKind::AddPasskey { waiting: true } => self.outbox.push(Command::CancelPasskey),
            _ => {}
        }
        // The codes are dropped with the dialog: they are never shown again.
        self.ui.security.dialog = None;
        if let Some(next) = self.ui.security.next.take() {
            self.open_sec(next);
        }
    }

    pub fn sec_code_changed(&mut self) {
        if let Some(d) = self.ui.security.dialog.as_mut() {
            let recovery_ok = matches!(d.kind, SecKind::DisableTotp);
            if !recovery_ok {
                let clean = digits_only(&d.code.text, 6);
                if clean != d.code.text {
                    d.code.replace_text(&clean);
                }
            }
            d.error = None;
            let full = d.code.text.len() == 6 && d.code.text.chars().all(|c| c.is_ascii_digit());
            let auto = matches!(d.kind, SecKind::TotpSetup { setup: Some(_) } | SecKind::Regenerate | SecKind::Email { sent: true });
            if full && auto && !d.busy {
                self.submit_sec();
            }
        }
    }

    pub fn toggle_saved(&mut self) {
        if let Some(d) = self.ui.security.dialog.as_mut() {
            d.saved = !d.saved;
        }
    }

    /// The dialog's primary button (or Enter).
    pub fn submit_sec(&mut self) {
        let Some(d) = self.ui.security.dialog.as_mut() else { return };
        if d.busy {
            return;
        }
        let first = d.first.text.trim().to_owned();
        let code = d.code.text.trim().to_owned();
        let fail = |d: &mut SecDialog, m: &str| d.error = Some(m.into());
        let cmd = match &d.kind {
            SecKind::Email { sent: false } => {
                if !looks_like_email(&first) {
                    return fail(d, "Enter an email address.");
                }
                Command::ChangeEmail { email: first, password: d.second.text.clone() }
            }
            SecKind::Email { sent: true } | SecKind::TotpSetup { setup: Some(_) } | SecKind::Regenerate => {
                if code.len() != 6 {
                    return fail(d, "Enter the 6-digit code.");
                }
                match d.kind {
                    SecKind::Email { .. } => Command::ConfirmEmailChange { code },
                    SecKind::Regenerate => Command::RegenerateRecoveryCodes { code },
                    _ => Command::ConfirmTotp { code },
                }
            }
            SecKind::TotpSetup { setup: None } => return,
            SecKind::Password => {
                let new = d.second.text.clone();
                if new.chars().count() < MIN_PASSWORD {
                    return fail(d, "Use at least 8 characters for the new password.");
                }
                if new != d.third.text {
                    return fail(d, "The new passwords don't match.");
                }
                Command::ChangePassword { current: d.first.text.clone(), new }
            }
            SecKind::RecoveryCodes { .. } => {
                if !d.saved {
                    return fail(d, "Tick the box once the codes are somewhere safe.");
                }
                self.close_sec();
                return;
            }
            SecKind::DisableTotp => {
                if code.is_empty() {
                    return fail(d, "Enter a code from your app, or a recovery code.");
                }
                Command::DisableTotp { code }
            }
            SecKind::AddPasskey { waiting: true } => return,
            SecKind::AddPasskey { waiting: false } | SecKind::RenamePasskey { .. } => {
                if first.is_empty() || first.chars().count() > 64 {
                    return fail(d, "Give the passkey a name of up to 64 characters.");
                }
                match d.kind {
                    SecKind::RenamePasskey { id } => Command::RenamePasskey { id, name: first },
                    _ => Command::BeginPasskeyRegistration { name: first },
                }
            }
            SecKind::DeletePasskey { id } => Command::DeletePasskey { id: *id },
            SecKind::Welcome => {
                self.ui.security.dialog = None;
                self.open_sec(SecKind::TotpSetup { setup: None });
                return;
            }
        };
        d.error = None;
        d.busy = true;
        self.outbox.push(cmd);
    }

    pub fn revoke_session(&mut self, id: u64) {
        if self.ui.security.busy.insert(id) {
            self.outbox.push(Command::RevokeSession { id });
        }
    }

    pub fn revoke_other_sessions(&mut self) {
        if !self.ui.security.others_busy {
            self.ui.security.others_busy = true;
            self.outbox.push(Command::RevokeOtherSessions);
        }
    }

    /// The other devices (all but this one).
    pub fn other_sessions(&self) -> Vec<&DeviceSession> {
        self.ui.security.info.as_ref().map_or_else(Vec::new, |i| i.sessions.iter().filter(|s| !s.current).collect())
    }

    fn sec_failed(&mut self, e: &ApiError) {
        let text = match (e, self.ui.security.dialog.as_ref().map(|d| &d.kind)) {
            (ApiError::PermissionDenied, Some(SecKind::Email { .. } | SecKind::Password)) => "That password isn't right.".to_owned(),
            (ApiError::WrongCode { attempts_remaining }, Some(SecKind::DisableTotp)) => {
                format!("That code didn't work. Use a current code or an unused recovery code. {}", tries_left(*attempts_remaining))
            }
            (ApiError::WrongCode { attempts_remaining }, _) => {
                format!("That code didn't work. Check it and try again. {}", tries_left(*attempts_remaining))
            }
            (ApiError::CodeExpired, Some(SecKind::Email { .. })) => "That code expired. Start the change again.".to_owned(),
            (ApiError::CodeExpired, Some(SecKind::TotpSetup { .. })) => "The setup timed out. Close this and start again.".to_owned(),
            (ApiError::CodeExpired, _) => "That expired. Start again.".to_owned(),
            (ApiError::TooManyAttempts, Some(SecKind::Email { .. })) => "Too many wrong codes. Start the change again.".to_owned(),
            (ApiError::TooManyAttempts | ApiError::RateLimited, _) => "Too many wrong codes. Try again later.".to_owned(),
            (ApiError::AlreadyExists, Some(SecKind::Email { .. })) => "Another account uses that email.".to_owned(),
            (ApiError::Precondition(m), Some(SecKind::AddPasskey { .. })) => format!("Passkeys aren't available on this server ({m})."),
            _ => error_text(e),
        };
        match self.ui.security.dialog.as_mut() {
            Some(d) => {
                d.busy = false;
                d.error = Some(text);
                if let SecKind::AddPasskey { waiting } = &mut d.kind {
                    *waiting = false;
                }
                d.code.clear();
            }
            None => self.toast(ToastKind::Danger, text),
        }
    }

    /// Every Security reply.
    pub(crate) fn sec_replied(&mut self, reply: SecReply) {
        match reply {
            SecReply::PasswordChanged(r) => match r {
                Ok(()) => {
                    self.ui.security.dialog = None;
                    self.toast(ToastKind::Success, "Password changed. Your other devices were signed out");
                    self.refresh_security();
                }
                Err(e) => self.sec_failed(&e),
            },
            SecReply::EmailChangeStarted(r) => match r {
                Ok(()) => {
                    if let Some(d) = self.ui.security.dialog.as_mut() {
                        d.kind = SecKind::Email { sent: true };
                        d.busy = false;
                        d.first.focused = false;
                        d.second.focused = false;
                        d.code.focused = true;
                    }
                }
                Err(e) => self.sec_failed(&e),
            },
            SecReply::EmailChanged(r) => match r {
                Ok(user) => {
                    let email = user.email.clone().unwrap_or_default();
                    self.me_updated(user);
                    self.ui.security.dialog = None;
                    self.toast(ToastKind::Success, format!("Your email is now {email}"));
                }
                Err(e) => self.sec_failed(&e),
            },
            SecReply::TotpSetup(r) => match r {
                Ok(setup) => {
                    if let Some(d) = self.ui.security.dialog.as_mut()
                        && matches!(d.kind, SecKind::TotpSetup { .. })
                    {
                        d.kind = SecKind::TotpSetup { setup: Some(setup) };
                        d.busy = false;
                        d.code.focused = true;
                    }
                }
                Err(ApiError::Precondition(_)) => {
                    self.ui.security.totp = Some(true);
                    self.ui.security.dialog = None;
                    self.toast(ToastKind::Info, "Two-factor authentication is already on");
                }
                Err(e) => self.sec_failed(&e),
            },
            SecReply::RecoveryCodes { regenerated, result } => match result {
                Ok(codes) => {
                    self.ui.security.totp = Some(true);
                    let mut d = SecDialog::new(SecKind::RecoveryCodes { codes });
                    d.first.focused = false;
                    self.ui.security.dialog = Some(d);
                    if !regenerated {
                        self.toast(ToastKind::Success, "Two-factor authentication is on");
                    }
                }
                Err(e) => self.sec_failed(&e),
            },
            SecReply::TotpDisabled(r) => match r {
                Ok(()) => {
                    self.ui.security.totp = Some(false);
                    self.ui.security.dialog = None;
                    self.toast(ToastKind::Success, "Two-factor authentication is off");
                }
                Err(e) => self.sec_failed(&e),
            },
            SecReply::PasskeyAdded(r) => match r {
                Ok(p) => {
                    let name = p.name.clone();
                    if let Some(info) = self.ui.security.info.as_mut() {
                        info.passkeys.retain(|x| x.id != p.id);
                        info.passkeys.push(p);
                    }
                    if self.ui.security.dialog.as_ref().is_some_and(|d| matches!(d.kind, SecKind::AddPasskey { .. })) {
                        self.ui.security.dialog = None;
                        if let Some(next) = self.ui.security.next.take() {
                            self.open_sec(next);
                        }
                    }
                    self.toast(ToastKind::Success, format!("Added the passkey \u{201c}{name}\u{201d}"));
                }
                Err(e) => self.sec_failed(&e),
            },
            SecReply::PasskeyRenamed(r) => match r {
                Ok(p) => {
                    if let Some(slot) = self.ui.security.info.as_mut().and_then(|i| i.passkeys.iter_mut().find(|x| x.id == p.id)) {
                        *slot = p;
                    }
                    self.ui.security.dialog = None;
                }
                Err(e) => self.sec_failed(&e),
            },
            SecReply::PasskeyDeleted { id, result } => match result {
                Ok(()) => {
                    if let Some(info) = self.ui.security.info.as_mut() {
                        info.passkeys.retain(|p| p.id != id);
                    }
                    self.ui.security.dialog = None;
                    self.toast(ToastKind::Success, "Passkey removed");
                }
                Err(e) => self.sec_failed(&e),
            },
            SecReply::SessionRevoked { id, result } => {
                self.ui.security.busy.remove(&id);
                match result {
                    Ok(()) => {
                        if let Some(info) = self.ui.security.info.as_mut() {
                            info.sessions.retain(|s| s.id != id);
                        }
                        self.toast(ToastKind::Success, "Signed out of that device");
                    }
                    Err(e) => self.toast(ToastKind::Danger, format!("Couldn't sign that device out. {}", error_text(&e))),
                }
            }
            SecReply::OthersRevoked(r) => {
                self.ui.security.others_busy = false;
                match r {
                    Ok(n) => {
                        if let Some(info) = self.ui.security.info.as_mut() {
                            info.sessions.retain(|s| s.current);
                        }
                        self.toast(ToastKind::Success, match n {
                            1 => "Signed out of 1 other device".to_owned(),
                            n => format!("Signed out of {n} other devices"),
                        });
                    }
                    Err(e) => self.toast(ToastKind::Danger, format!("Couldn't sign the other devices out. {}", error_text(&e))),
                }
            }
        }
    }

    fn refresh_security(&mut self) {
        if self.ui.settings == Some(Section::Security) {
            self.load_security();
        }
    }

    /// Your own user changed (a new email).
    fn me_updated(&mut self, user: User) {
        if let Some(s) = self.session.as_mut().filter(|s| s.me.id == user.id) {
            s.me = user.clone();
        }
        self.data.users.insert(user.id, user);
    }

    // ---- inviting by email --------------------------------------------------------------------------

    /// Whether "Invite people by email…" is offered in `guild`.
    pub fn can_invite(&self, guild: u64) -> bool {
        self.is_admin() || self.perms(guild).contains(Permissions::INVITE_PEOPLE)
    }

    /// The dialog opened: its list of this server's invites is fetched.
    pub(crate) fn invite_dialog_opened(&mut self, guild: u64) {
        let epoch = self.ui.invites.epoch + 1;
        self.ui.invites = InvitesUi { epoch, ..InvitesUi::default() };
        self.outbox.push(Command::ListInvites { epoch, guild: Some(guild), page_token: None, limit: INVITE_PAGE });
    }

    /// Someone joined `guild` or their membership changed: maybe an invite was just accepted, so
    /// the invite lists on screen ask again (the dialog keeps its rows meanwhile).
    pub(crate) fn membership_changed(&mut self, guild: u64) {
        if matches!(self.ui.dialog.as_ref().map(|d| &d.kind), Some(DialogKind::InviteByEmail { guild: g }) if *g == guild) {
            let ui = &mut self.ui.invites;
            ui.epoch += 1;
            let epoch = ui.epoch;
            self.outbox.push(Command::ListInvites { epoch, guild: Some(guild), page_token: None, limit: INVITE_PAGE });
        }
        self.admin_invites_changed();
    }

    pub fn invite_command(&mut self, guild: u64, email: String) -> Result<Command, String> {
        if !looks_like_email(&email) {
            return Err("Enter their email address".into());
        }
        self.ui.invites.created = None;
        Ok(Command::CreateInvite { invite: NewInvite { email: email.trim().to_owned(), guild_id: Some(guild), ..NewInvite::default() } })
    }

    pub fn revoke_invite(&mut self, id: u64) {
        if self.ui.invites.busy.insert(id) {
            self.outbox.push(Command::RevokeInvite { id });
        }
    }

    pub fn resend_invite(&mut self, id: u64) {
        if self.ui.invites.busy.insert(id) {
            self.outbox.push(Command::ResendInvite { id });
        }
    }

    pub(crate) fn invite_created(&mut self, result: Result<Invite, ApiError>) {
        if self.admin_invite_busy() {
            return self.admin_invite_created(result);
        }
        let Some(d) = self.ui.dialog.as_mut().filter(|d| matches!(d.kind, DialogKind::InviteByEmail { .. })) else { return };
        d.phase = Phase::Idle;
        match result {
            Ok(invite) => {
                d.input.clear();
                d.error = None;
                self.ui.invites.list.retain(|i| i.id != invite.id);
                self.ui.invites.list.insert(0, Invite { code: None, link: None, ..invite.clone() });
                self.ui.invites.created = Some(invite);
            }
            Err(e) => {
                d.error = Some(match e {
                    ApiError::AlreadyExists => "Someone with that email already has an account".into(),
                    ApiError::PermissionDenied => "You can't invite people to this server".into(),
                    other => error_text(&other),
                })
            }
        }
    }

    pub(crate) fn invites_listed(&mut self, epoch: u64, guild: Option<u64>, result: Result<thehideout_api::Paged<Invite>, ApiError>) {
        if guild.is_none() {
            return self.admin_invites_replied(epoch, result);
        }
        let ui = &mut self.ui.invites;
        if ui.epoch != epoch {
            return;
        }
        ui.loaded = true;
        match result {
            Ok(page) => ui.list = page.items,
            Err(e) => ui.error = Some(error_text(&e)),
        }
    }

    pub(crate) fn invite_saved(&mut self, id: u64, action: InviteAction, result: Result<Invite, ApiError>) {
        self.ui.invites.busy.remove(&id);
        if let Some(ad) = self.ui.admin.as_mut() {
            ad.invite_busy.remove(&id);
        }
        match result {
            Ok(invite) => {
                let plain = Invite { code: None, link: None, ..invite.clone() };
                for list in [Some(&mut self.ui.invites.list), self.ui.admin.as_mut().map(|ad| &mut ad.invites.items)].into_iter().flatten() {
                    if let Some(slot) = list.iter_mut().find(|i| i.id == id) {
                        *slot = plain.clone();
                    }
                }
                match action {
                    InviteAction::Revoke => self.toast(ToastKind::Success, format!("Invite for {} withdrawn", invite.email)),
                    InviteAction::Resend => {
                        self.toast(ToastKind::Success, format!("Sent the invite to {} again", invite.email));
                        if let Some(ad) = self.ui.admin.as_mut() {
                            ad.invite_created = Some(invite.clone());
                        }
                        self.ui.invites.created = Some(invite);
                    }
                }
            }
            Err(e) => self.toast(ToastKind::Danger, format!("Couldn't change the invite. {}", error_text(&e))),
        }
    }
}

/// The Security replies, gathered so `apply` hands them over in one place.
pub enum SecReply {
    PasswordChanged(Result<(), ApiError>),
    EmailChangeStarted(Result<(), ApiError>),
    EmailChanged(Result<User, ApiError>),
    TotpSetup(Result<TotpSetup, ApiError>),
    RecoveryCodes { regenerated: bool, result: Result<Vec<String>, ApiError> },
    TotpDisabled(Result<(), ApiError>),
    PasskeyAdded(Result<Passkey, ApiError>),
    PasskeyRenamed(Result<Passkey, ApiError>),
    PasskeyDeleted { id: u64, result: Result<(), ApiError> },
    SessionRevoked { id: u64, result: Result<(), ApiError> },
    OthersRevoked(Result<u32, ApiError>),
}

/// "1 try left." / "3 tries left." / "No tries left."
pub fn tries_left(n: u32) -> String {
    match n {
        0 => "No tries left.".to_owned(),
        1 => "1 try left.".to_owned(),
        n => format!("{n} tries left."),
    }
}

/// When an invite expires or expired, in words.
pub fn invite_status_label(status: InviteStatus) -> &'static str {
    match status {
        InviteStatus::Pending => "Pending",
        InviteStatus::Accepted => "Accepted",
        InviteStatus::Expired => "Expired",
        InviteStatus::Revoked => "Withdrawn",
        InviteStatus::Unknown => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_codes_are_taken_as_typed_and_links_are_spotted() {
        assert_eq!(invite_code(" wxyz-2345 "), "WXYZ-2345");
        assert_eq!(invite_code("WXYZ2"), "WXYZ2");
        assert_eq!(invite_code("https://hideout.example/invite/wxyz-2345"), "WXYZ-2345");
        assert_eq!(invite_code(""), "");
    }

    #[test]
    fn deep_links_carry_their_code() {
        assert_eq!(link_code("https://hideout.example/invite/wxyz-2345").as_deref(), Some("WXYZ2345"));
        assert_eq!(link_code("thehideout://invite/WXYZ2345?x=1").as_deref(), Some("WXYZ2345"));
        assert_eq!(link_code("thehideout://invite/WXYZ-23"), None);
        assert_eq!(link_code("https://hideout.example/invite/ABCDEFGHJK"), None);
        assert_eq!(link_code("thehideout://invite/"), None);
        assert_eq!(link_code("thehideout://other"), None);
        assert_eq!(link_code("WXYZ-2345"), None);
    }

    #[test]
    fn the_web_query_of_an_invite_is_parsed() {
        assert_eq!(invite_query_code("/app/", "?invite=wxyz-2345").as_deref(), Some("WXYZ2345"));
        assert_eq!(invite_query_code("/app", "?invite=WXYZ-2345").as_deref(), Some("WXYZ2345"));
        assert_eq!(invite_query_code("/app/", "?x=1&invite=WXYZ2345").as_deref(), Some("WXYZ2345"));
        assert_eq!(invite_query_code("/app/", "?invite=WXYZ%2D2345").as_deref(), Some("WXYZ2345"));
        assert_eq!(invite_query_code("/app/", "?invite=WXYZ-23"), None);
        assert_eq!(invite_query_code("/app/", "?invite=ABCD-EFGH-XYZ"), None);
        assert_eq!(invite_query_code("/app/", "?invite="), None);
        assert_eq!(invite_query_code("/app/", "?x=1"), None);
        assert_eq!(invite_query_code("/app/", ""), None);
        assert_eq!(invite_query_code("/app/invite/WXYZ-2345", ""), None);
        assert_eq!(invite_query_code("/other", "?invite=WXYZ-2345"), None);
    }

    #[test]
    fn emails_are_checked_loosely() {
        assert!(looks_like_email("a@b.co"));
        assert!(!looks_like_email("a@b"));
        assert!(!looks_like_email("a b@c.de"));
        assert!(!looks_like_email("@c.de"));
    }
}
