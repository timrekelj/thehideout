//! Form state shared by the auth screen and the dialogs: which mode, what is typed, what went
//! wrong. Pure data; `AppState` owns the instances and the transitions that submit them.

use std::rc::Rc;

use rune::TextEdit;
use thehideout_api::{ApiError, Channel, ChannelIconRef, ChannelKind, InvitePreview, SecondFactorMethod};
use thehideout_design::components::EmojiPickerState;

use crate::emoji::IconEmoji;
use thehideout_design::icons::catalog::{self, CatalogIcon};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Idle,
    Busy,
}

/// The auth card's two tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthMode {
    /// "Sign in": an existing account.
    Login,
    /// "I have an invite": accounts are invite-only.
    Invite,
}

/// Where the auth card is within its tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthStep {
    /// Username or email, and the password.
    Password,
    /// "Email me a code instead": the email, then (`sent`) the 6-digit code.
    EmailCode { sent: bool },
    /// "Use a passkey": the username, then (`waiting`) the browser ceremony.
    Passkey { waiting: bool },
    /// "Forgot password?": the email, then (`sent`) the code and a new password.
    Reset { sent: bool },
    /// The account has an authenticator app: a 6-digit code, (`recovery`) a recovery code, or
    /// (`passkey`: waiting for the browser) one of its passkeys. `remaining`: the tries the server
    /// said are left after a wrong answer. `locked`: too many wrong answers, or the challenge
    /// expired; the sign-in has to start again.
    SecondFactor {
        challenge: String,
        methods: Vec<SecondFactorMethod>,
        recovery: bool,
        remaining: Option<u32>,
        locked: bool,
        passkey: bool,
    },
    /// "I have an invite": the code or the link from the email.
    InviteCode,
    /// The invite is valid: choose a username and a password.
    InviteAccept { preview: InvitePreview, code: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthField {
    Username,
    Password,
    /// The email field (code, reset).
    Email,
    /// The 6-digit or recovery code.
    Code,
    /// The invite code or link.
    Invite,
    /// Under the button: the problem isn't one field's.
    Form,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthError {
    pub field: AuthField,
    pub message: String,
    /// Set when the account is suspended: the sign-in screen shows its own card, not a line.
    pub suspension: Option<Suspension>,
}

/// Why and until when a suspended account is locked out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suspension {
    pub reason: String,
    /// `None`: until an admin lifts it.
    pub until: Option<std::time::SystemTime>,
}

/// Seconds before "Resend code" may be pressed again.
pub const RESEND_SECS: f64 = 30.0;

pub struct Auth {
    pub mode: AuthMode,
    pub step: AuthStep,
    /// Username or email (sign-in), the username (passkey), or the new username (invite).
    pub username: TextEdit,
    /// The password, or the new account's password (invite) or new password (reset).
    pub password: TextEdit,
    pub email: TextEdit,
    pub code: TextEdit,
    pub recovery: TextEdit,
    pub invite: TextEdit,
    /// Invite: add a passkey right after the account is made.
    pub add_passkey: bool,
    /// Invite: the invitee accepted the terms and privacy notice (sent with the accept call;
    /// the screen's checkbox sets it). Starts false, so an accept without it fails clearly.
    pub accept_terms: bool,
    /// Invite: the invitee confirmed being at least 16 (same).
    pub confirm_age: bool,
    pub phase: Phase,
    pub error: Option<AuthError>,
    /// A note that isn't an error ("We sent a code to ...").
    pub notice: Option<String>,
    /// When "Resend code" may be pressed again (the app clock); stamped by the next tick.
    pub resend_at: Option<f64>,
    pub resend_due: bool,
    /// Invite: the code last sent for lookup, so a code that failed isn't sent again until it is edited.
    pub invite_tried: Option<String>,
}

fn focused(mut e: TextEdit) -> TextEdit {
    e.focused = true;
    e
}

impl Auth {
    pub fn new() -> Auth {
        Auth {
            mode: AuthMode::Login,
            step: AuthStep::Password,
            username: focused(TextEdit::new("")),
            password: TextEdit::new("").secure(),
            email: TextEdit::new(""),
            code: TextEdit::new(""),
            recovery: TextEdit::new(""),
            invite: TextEdit::new(""),
            add_passkey: false,
            accept_terms: false,
            confirm_age: false,
            phase: Phase::Idle,
            error: None,
            notice: None,
            resend_at: None,
            resend_due: false,
            invite_tried: None,
        }
    }

    /// Moves to `step` with nothing typed for it yet and the first field focused.
    pub fn go(&mut self, step: AuthStep) {
        self.error = None;
        self.notice = None;
        self.phase = Phase::Idle;
        self.code.clear();
        self.recovery.clear();
        for e in [&mut self.username, &mut self.password, &mut self.email, &mut self.code, &mut self.recovery, &mut self.invite] {
            e.focused = false;
        }
        match &step {
            AuthStep::Password | AuthStep::Passkey { .. } => self.username.focused = true,
            AuthStep::EmailCode { sent: false } | AuthStep::Reset { sent: false } => self.email.focused = true,
            AuthStep::EmailCode { sent: true } | AuthStep::Reset { sent: true } => self.code.focused = true,
            AuthStep::SecondFactor { recovery: true, .. } => self.recovery.focused = true,
            AuthStep::SecondFactor { .. } => self.code.focused = true,
            AuthStep::InviteCode => self.invite.focused = true,
            AuthStep::InviteAccept { .. } => {
                self.accept_terms = false;
                self.confirm_age = false;
                self.username.clear();
                self.password.clear();
                self.username.focused = true;
            }
        }
        self.step = step;
    }

    pub fn fail(&mut self, field: AuthField, message: impl Into<String>) {
        self.phase = Phase::Idle;
        self.error = Some(AuthError { field, message: message.into(), suspension: None });
    }

    pub fn error_for(&self, field: AuthField) -> Option<&str> {
        self.error.as_ref().filter(|e| e.field == field).map(|e| e.message.as_str())
    }
}

/// What an `ApiError` means on the auth form. Wrong credentials never say which part was wrong,
/// so they reveal nothing about whether an account exists.
pub fn auth_error(e: &ApiError) -> AuthError {
    let (field, message) = match e {
        ApiError::Unauthenticated => (AuthField::Form, "Those details didn't work. Check them and try again.".to_owned()),
        ApiError::AlreadyExists => (AuthField::Username, "That username is taken.".to_owned()),
        ApiError::InviteRequired => (AuthField::Form, "You need an invite to make an account.".to_owned()),
        ApiError::InvalidArgument(m) => (AuthField::Form, sentence(m)),
        ApiError::RateLimited => (AuthField::Form, "Too many attempts. Try again in a moment.".to_owned()),
        ApiError::Suspended { reason, until } => (AuthField::Form, suspended_text(reason, *until)),
        ApiError::TermsUpdateRequired => (AuthField::Form, "Accept the terms and privacy notice to continue.".to_owned()),
        ApiError::Network(_) => (AuthField::Form, "Can't reach the server.".to_owned()),
        other => (AuthField::Form, sentence(&other.to_string())),
    };
    let suspension = match e {
        ApiError::Suspended { reason, until } => Some(Suspension { reason: reason.clone(), until: *until }),
        _ => None,
    };
    AuthError { field, message, suspension }
}

/// "Your account is suspended. Reason: ... It ends on 2026-12-01." for the sign-in form.
pub fn suspended_text(reason: &str, until: Option<std::time::SystemTime>) -> String {
    let mut text = "Your account is suspended.".to_owned();
    if !reason.trim().is_empty() {
        text.push_str(&format!(" Reason: {}", sentence(reason)));
    }
    match until {
        Some(t) => {
            let secs = t.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
            let (y, m, d) = civil(secs.div_euclid(86_400));
            text.push_str(&format!(" It ends on {y:04}-{m:02}-{d:02}."));
        }
        None => text.push_str(" It has no end date; contact an admin."),
    }
    text
}

/// (year, month, day) of a day count since 1970-01-01 (proleptic Gregorian).
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Capitalised, ending in a full stop.
pub fn sentence(m: &str) -> String {
    let m = m.trim();
    let mut c = m.chars();
    let mut s: String = c.next().into_iter().flat_map(char::to_uppercase).chain(c).collect();
    if !s.is_empty() && !s.ends_with(['.', '!', '?']) {
        s.push('.');
    }
    s
}


#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogKind {
    CreateGuild,
    JoinGuild,
    CreateChannel { guild: u64 },
    Invite { guild: u64 },
    NewDm,
    CreateSection { guild: u64 },
    RenameSection { guild: u64, section: u64 },
    /// A confirmation: no field, only "Delete section".
    DeleteSection { guild: u64, section: u64 },
    ChannelSettings { guild: u64, channel: u64 },
    /// Your picture and name.
    Profile,
    /// A confirmation: no field, only "Delete role".
    DeleteRole { guild: u64, role: u64 },
    /// A confirmation: no field, only "Delete emoji".
    DeleteEmoji { guild: u64, emoji: u64 },
    /// Invite someone to the server by email (needs INVITE_PEOPLE).
    InviteByEmail { guild: u64 },
    /// A confirmation with a preview of the message.
    DeleteMessage { channel: u64, message: u64 },
    /// A confirmation: no field, only "Kick".
    KickMember { guild: u64, user: u64 },
    /// A reason and what to do with their recent messages.
    BanMember { guild: u64, user: u64 },
    /// A confirmation: no field, only "Leave server".
    LeaveGuild { guild: u64 },
    /// A confirmation naming the channel.
    DeleteChannel { guild: u64, channel: u64 },
    /// Type the server's name to delete it.
    DeleteGuild { guild: u64 },
    /// Pick a member and type the server's name to hand it over.
    TransferOwnership { guild: u64 },
}

/// The look and place fields of the channel dialogs (create and settings): where the channel
/// goes, its icon and colour, topic, and the icon picker's search.
pub struct ChannelForm {
    /// Text or voice (the create dialog offers the choice; a channel's kind never changes).
    pub kind: ChannelKind,
    /// `None` = top level. Only the create dialog offers the choice.
    pub section: Option<u64>,
    pub section_open: bool,
    /// Phosphor name, `""` = the default `#` (unless `emoji` is set).
    pub icon: String,
    /// An emoji icon (Unicode, or one of the server's): it replaces `icon`.
    pub emoji: Option<IconEmoji>,
    /// The Emoji and Server emoji tabs' search and highlight.
    pub emoji_pick: EmojiPickerState,
    /// Palette key, `""` = none.
    pub color: &'static str,
    pub topic: TextEdit,
    pub picker_open: bool,
    /// The picker tab: 0 Icons, 1 Emoji, 2 Server emoji.
    pub tab: usize,
    pub search: TextEdit,
    /// The keyboard-chosen result in `results`.
    pub highlight: Option<usize>,
    pub results: Rc<Vec<&'static CatalogIcon>>,
    /// What the channel had when the dialog opened (settings), to send only what changed.
    pub original: Option<Channel>,
}

impl ChannelForm {
    pub fn new(section: Option<u64>) -> ChannelForm {
        ChannelForm {
            kind: ChannelKind::Text,
            section,
            section_open: false,
            icon: String::new(),
            emoji: None,
            emoji_pick: EmojiPickerState::new(),
            color: "",
            topic: TextEdit::new(""),
            picker_open: false,
            tab: 0,
            search: TextEdit::new(""),
            highlight: None,
            results: Rc::new(catalog::search("")),
            original: None,
        }
    }

    pub fn of(c: &Channel) -> ChannelForm {
        let mut f = ChannelForm::new(c.section_id);
        f.kind = c.kind;
        match &c.icon_ref {
            ChannelIconRef::Unicode(g) => f.emoji = Some(IconEmoji::Unicode(g.clone())),
            ChannelIconRef::Custom(id) => f.emoji = Some(IconEmoji::Custom(*id)),
            ChannelIconRef::Phosphor(name) => f.icon = name.clone(),
            ChannelIconRef::Default => f.icon = c.icon.0.clone(),
        }
        f.color = thehideout_design::PALETTE_KEYS.iter().copied().find(|k| *k == c.color.as_key()).unwrap_or("");
        f.topic = TextEdit::new(&c.topic);
        f.original = Some(c.clone());
        f
    }

    /// The icon the form describes, as the server stores it.
    pub fn icon_ref(&self) -> ChannelIconRef {
        match (&self.emoji, self.icon.is_empty()) {
            (Some(e), _) => e.to_ref(),
            (None, true) => ChannelIconRef::Default,
            (None, false) => ChannelIconRef::Phosphor(self.icon.clone()),
        }
    }

    /// Recomputes `results` after the search text changed, and puts the highlight on the best one.
    pub fn research(&mut self) {
        self.results = Rc::new(catalog::search(&self.search.text));
        self.highlight = (!self.results.is_empty()).then_some(0);
    }

    /// Arrow keys in the grid: `dx` columns, `dy` rows. Starts at the first result.
    pub fn step_highlight(&mut self, dx: i32, dy: i32) {
        use thehideout_design::components::chat::pickers::GRID_COLS;
        let n = self.results.len() as i32;
        if n == 0 {
            return;
        }
        let at = self.highlight.map_or(-dx - dy * GRID_COLS as i32, |h| h as i32) + dx + dy * GRID_COLS as i32;
        self.highlight = Some(at.clamp(0, n - 1) as usize);
    }
}

pub struct DialogState {
    pub kind: DialogKind,
    pub input: TextEdit,
    /// Present for the channel dialogs.
    pub channel: Option<ChannelForm>,
    pub error: Option<String>,
    pub phase: Phase,
    /// The Profile dialog's picked picture.
    pub pick: crate::server_settings::ImagePick,
    /// What the moderation dialogs add (ban clean-up, hand-over target, invite reset).
    pub form: crate::moderation_ui::ModForm,
}

pub fn check_name(name: &str, min: usize, max: usize, empty: &str) -> Result<(), String> {
    let n = name.chars().count();
    if n == 0 {
        Err(empty.into())
    } else if n < min || n > max {
        Err(format!("Use {min} to {max} characters"))
    } else {
        Ok(())
    }
}

