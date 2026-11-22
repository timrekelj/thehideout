//! The fake world's accounts: email codes, the second factor, passkeys, password resets, invites,
//! sessions and the Security page's lists. A child module of `fake` (see the `#[path]` there), so
//! it reaches the world's private fields. Its rules are the server's (`proto/README.md`,
//! "Authentication & invites"), with fixed secrets so scenes and tests are deterministic:
//!
//! - every emailed code, and every authenticator code, is `123456`;
//! - `dora` has an authenticator app (signing in as her asks for the second factor); her
//!   recovery codes are [`RECOVERY`]; she also has a passkey, offered as a second factor;
//! - a second-factor challenge takes five wrong answers (each says how many are left), and
//!   signed-in code checks ten per session;
//! - `tim` has a passkey; a passkey ceremony finishes [`PASSKEY_SECS`] after it starts;
//! - `bruno` is a legacy account without an email;
//! - invites `WXYZ-2345` (pending, The Hideout), `EXPD-0001` (expired), `RVKD-0001` (revoked)
//!   and `USED-0001` (accepted); a link `.../invite/tok-wxyz` is the first one's.

#![cfg_attr(not(test), allow(dead_code))]

use std::collections::HashSet;

use super::*;
use thehideout_api::{
    DeviceSession, Invite, InvitePreview, InviteStatus, NewInvite, Paged, Passkey,
    SecondFactorMethod, SecuritySummary, TermsStatus, TotpSetup,
};

use crate::backend::{InviteAction, SecurityInfo};

/// The terms version the fake server requires.
pub const TERMS_VERSION: &str = "2026-11-22";
/// The one valid emailed / authenticator code.
pub const CODE: &str = "123456";
/// The fake authenticator secret.
pub const TOTP_SECRET: &str = "JBSWY3DPEHPK3PXP";
/// Dora's recovery codes (each works once).
pub const RECOVERY: [&str; 10] = [
    "7KQ2-M9XD",
    "4HTW-PZ3N",
    "C8RB-V6YE",
    "J2LF-Q7KA",
    "N5XG-D4MS",
    "R9PC-W2HT",
    "T3YE-K8VB",
    "V6MA-F5QR",
    "X2ND-H9LC",
    "Z7GK-B3WP",
];
/// Seconds until a passkey ceremony "finishes" in the browser.
pub const PASSKEY_SECS: f64 = 2.0;
/// The account with an authenticator app.
pub const DORA: &str = "dora";
/// Invite codes the fake world knows.
pub const INVITE_PENDING: &str = "WXYZ-2345";
pub const INVITE_EXPIRED: &str = "EXPD-0001";
pub const INVITE_REVOKED: &str = "RVKD-0001";
pub const INVITE_USED: &str = "USED-0001";
const SECOND_FACTOR_TRIES: u32 = 5;
/// Signed-in code checks (authenticator, email change) the fake allows.
const USER_CHECKS: u32 = 10;
const INVITE_DAYS: i64 = 7;

struct FakeInvite {
    invite: Invite,
    code: String,
    token: String,
}

pub(super) struct Accounts {
    /// Usernames with an authenticator app on, and their unused recovery codes.
    totp: HashMap<String, Vec<String>>,
    /// A TOTP setup begun and not confirmed, by user id.
    setups: HashSet<u64>,
    /// Second-factor challenges: id, username, answers so far.
    challenges: Vec<(String, String, u32)>,
    /// Usernames whose passkey answers a second-factor challenge.
    second_factor_passkeys: HashSet<String>,
    /// Signed-in code checks so far (the server limits them per hour).
    checks: u32,
    next_challenge: u64,
    passkeys: HashMap<u64, Vec<Passkey>>,
    next_passkey: u64,
    sessions: HashMap<u64, Vec<DeviceSession>>,
    invites: Vec<FakeInvite>,
    next_invite: u64,
    /// An email change waiting for its code, by user id.
    email_change: HashMap<u64, String>,
    /// Bumped by every set of recovery codes, so each set differs.
    generation: u64,
    /// The passkey ceremony under way: when it finishes, and what it answers then.
    pub(super) ceremony: Option<(f64, Reply)>,
    /// The signed-in user has not accepted the current terms.
    pub(super) terms_outdated: bool,
    /// The user already exported their data in the last 24 hours.
    exported: bool,
}

fn at(unix: i64) -> std::time::SystemTime {
    time(unix)
}

/// `XXXX-XXXX` without case, dashes or spaces, as the server forgives them.
fn plain(code: &str) -> String {
    code.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase()
}

impl Accounts {
    pub(super) fn new() -> Accounts {
        let now = now_unix();
        let session =
            |id: u64, name: &str, created: i64, seen: Option<i64>, current: bool| DeviceSession {
                id,
                device_name: name.into(),
                created_at: at(created),
                last_seen_at: seen.map(at),
                current,
            };
        let sessions = HashMap::from([(
            TIM,
            vec![
                session(
                    1,
                    "thehideout desktop on macOS",
                    now - 3 * DAY,
                    Some(now - 60),
                    true,
                ),
                session(
                    2,
                    "Firefox on Linux",
                    now - 20 * DAY,
                    Some(now - 2 * DAY),
                    false,
                ),
                session(
                    3,
                    "Tim's iPhone",
                    now - 40 * DAY,
                    Some(now - 9 * DAY),
                    false,
                ),
            ],
        )]);
        let passkeys = HashMap::from([(
            TIM,
            vec![Passkey {
                id: 1,
                name: "MacBook Touch ID".into(),
                created_at: at(now - 30 * DAY),
                last_used_at: Some(at(now - 3 * DAY)),
            }],
        )]);
        let invite = |id: u64,
                      email: &str,
                      code: &str,
                      token: &str,
                      status: InviteStatus,
                      inviter: Option<User>,
                      guilds: Vec<u64>,
                      created: i64| FakeInvite {
            invite: Invite {
                id,
                email: email.into(),
                code: None,
                link: None,
                status,
                inviter,
                guild_ids: guilds,
                make_admin: false,
                created_at: at(created),
                expires_at: at(created + INVITE_DAYS * DAY),
                accepted_user_id: (status == InviteStatus::Accepted).then_some(ANA),
            },
            code: code.into(),
            token: token.into(),
        };
        let tim = Some(user(TIM, "tim"));
        Accounts {
            totp: HashMap::from([(
                DORA.to_owned(),
                RECOVERY.iter().map(|c| c.to_string()).collect(),
            )]),
            setups: HashSet::new(),
            challenges: Vec::new(),
            second_factor_passkeys: HashSet::from([DORA.to_owned()]),
            checks: 0,
            next_challenge: 0,
            passkeys,
            next_passkey: 2,
            sessions,
            invites: vec![
                invite(
                    9001,
                    "erin@example.com",
                    INVITE_PENDING,
                    "tok-wxyz",
                    InviteStatus::Pending,
                    tim.clone(),
                    vec![HIDEOUT],
                    now - DAY,
                ),
                invite(
                    9002,
                    "finn@example.com",
                    INVITE_EXPIRED,
                    "tok-expd",
                    InviteStatus::Expired,
                    tim.clone(),
                    vec![HIDEOUT],
                    now - 9 * DAY,
                ),
                invite(
                    9003,
                    "gus@example.com",
                    INVITE_REVOKED,
                    "tok-rvkd",
                    InviteStatus::Revoked,
                    tim.clone(),
                    vec![HIDEOUT],
                    now - 3 * DAY,
                ),
                invite(
                    9004,
                    "ana@example.com",
                    INVITE_USED,
                    "tok-used",
                    InviteStatus::Accepted,
                    None,
                    vec![HIDEOUT],
                    now - 60 * DAY,
                ),
            ],
            next_invite: 9005,
            email_change: HashMap::new(),
            generation: 0,
            ceremony: None,
            terms_outdated: false,
            exported: false,
        }
    }

    fn find_invite(&self, code_or_link: &str) -> Option<usize> {
        let raw = code_or_link.trim();
        let token = raw.rsplit('/').next().unwrap_or(raw);
        self.invites
            .iter()
            .position(|i| i.token == token || plain(&i.code) == plain(raw))
    }
}

/// The masked form of an email: `e***@example.com`.
pub fn mask(email: &str) -> String {
    match email.split_once('@') {
        Some((name, domain)) => format!("{}***@{domain}", name.chars().next().unwrap_or('*')),
        None => "***".into(),
    }
}

impl World {
    fn by_email(&self, email: &str) -> Option<&User> {
        let email = email.trim().to_lowercase();
        self.users
            .iter()
            .find(|u| u.email.as_deref() == Some(email.as_str()))
    }

    /// A username or an email, as `Login` takes it.
    fn by_login(&self, login: &str) -> Option<User> {
        let login = login.trim();
        if login.contains('@') {
            self.by_email(login).cloned()
        } else if login.eq_ignore_ascii_case(DORA) {
            Some(
                self.find_user(DORA)
                    .cloned()
                    .unwrap_or_else(|| user(5, DORA)),
            )
        } else {
            Some(
                self.find_user(login)
                    .cloned()
                    .unwrap_or_else(|| user(0, login)),
            )
        }
    }

    /// A first factor passed: a session, or the second factor when the account has an app.
    fn first_factor(&mut self, username: &str) -> Reply {
        if self.account.totp.contains_key(&username.to_lowercase()) {
            self.account.next_challenge += 1;
            let id = format!("fake-challenge-{}", self.account.next_challenge);
            self.account
                .challenges
                .push((id.clone(), username.to_lowercase(), 0));
            let mut methods = vec![SecondFactorMethod::Totp, SecondFactorMethod::RecoveryCode];
            if self.account.second_factor_passkeys.contains(&username.to_lowercase()) {
                methods.push(SecondFactorMethod::Passkey);
            }
            return Reply::SecondFactor { challenge: id, methods };
        }
        Reply::Auth(Ok(self.session_for(username)))
    }

    pub(super) fn enable_totp(&mut self, username: &str) {
        self.account.totp.insert(username.to_lowercase(), RECOVERY.iter().map(|c| (*c).to_owned()).collect());
    }

    fn me_has_totp(&self) -> bool {
        self.account
            .totp
            .contains_key(&self.me.username.to_lowercase())
    }

    /// Whether `code` answers the signed-in user's authenticator (or, with `recovery`, uses up
    /// one of their recovery codes).
    fn check_code(&mut self, username: &str, code: &str, recovery: bool) -> bool {
        if code.trim() == CODE && !recovery {
            return true;
        }
        let Some(left) = self.account.totp.get_mut(&username.to_lowercase()) else {
            return false;
        };
        match left.iter().position(|c| plain(c) == plain(code)) {
            Some(i) if recovery || code.contains('-') || code.len() > 6 => {
                left.remove(i);
                true
            }
            _ => false,
        }
    }

    fn security(&self) -> SecurityInfo {
        SecurityInfo {
            passkeys: self
                .account
                .passkeys
                .get(&self.me.id)
                .cloned()
                .unwrap_or_default(),
            sessions: self
                .account
                .sessions
                .get(&self.me.id)
                .cloned()
                .unwrap_or_else(|| {
                    vec![DeviceSession {
                        id: 1,
                        device_name: "thehideout desktop on macOS".into(),
                        created_at: at(now_unix()),
                        last_seen_at: Some(at(now_unix())),
                        current: true,
                    }]
                }),
            summary: SecuritySummary {
                totp_enabled: self.me_has_totp(),
                has_password: true,
                passkey_count: self.account.passkeys.get(&self.me.id).map_or(0, |p| p.len() as u32),
                recovery_codes_remaining: self
                    .account
                    .totp
                    .get(&self.me.username.to_lowercase())
                    .map_or(0, |c| c.len() as u32),
                email_verified: self.me.email.is_some(),
            },
        }
    }

    /// A wrong signed-in code: how many checks are left.
    fn wrong_check(&mut self) -> ApiError {
        self.account.checks += 1;
        ApiError::WrongCode { attempts_remaining: USER_CHECKS.saturating_sub(self.account.checks) }
    }

    /// Ten fresh recovery codes, `XXXX-XXXX` in the server's alphabet (each set differs).
    fn new_codes(&mut self) -> Vec<String> {
        const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTVWXYZ23456789";
        self.account.generation += 1;
        let mut rng = Lcg(0xc0de + self.account.generation);
        let mut pick = || ALPHABET[rng.next(ALPHABET.len() as u64) as usize] as char;
        let codes: Vec<String> = (0..10)
            .map(|_| {
                let a: String = (0..4).map(|_| pick()).collect();
                let b: String = (0..4).map(|_| pick()).collect();
                format!("{a}-{b}")
            })
            .collect();
        self.account
            .totp
            .insert(self.me.username.to_lowercase(), codes.clone());
        codes
    }

    fn may_invite(&self, guild: u64) -> bool {
        self.me.is_admin || self.has_perm(guild, self.me.id, Permissions::INVITE_PEOPLE)
    }

    /// Whether the signed-in user may revoke or resend `i` (others get `NotFound`).
    fn may_manage_invite(&self, i: &Invite) -> bool {
        self.me.is_admin
            || i.inviter.as_ref().is_some_and(|u| u.id == self.me.id)
            || i.guild_ids
                .first()
                .is_some_and(|g| self.has_perm(*g, self.me.id, Permissions::INVITE_PEOPLE))
    }

    fn create_invite(&mut self, new: NewInvite) -> Result<Invite, ApiError> {
        let email = new.email.trim().to_lowercase();
        let admin = self.me.is_admin;
        match new.guild_id {
            _ if !admin && (!new.guild_ids.is_empty() || new.make_admin) => {
                return Err(ApiError::PermissionDenied)
            }
            Some(g) if !self.may_invite(g) => return Err(ApiError::PermissionDenied),
            None if !admin => return Err(ApiError::PermissionDenied),
            _ => {}
        }
        if !email.contains('@')
            || email.starts_with('@')
            || email.ends_with('@')
            || !email.contains('.')
        {
            return Err(ApiError::InvalidArgument(
                "that is not an email address".into(),
            ));
        }
        if self.by_email(&email).is_some() {
            return Err(ApiError::AlreadyExists);
        }
        let id = self.account.next_invite;
        self.account.next_invite += 1;
        let code = format!("NEW{}-{:04}", id % 10, id);
        let mut guilds: Vec<u64> = new.guild_id.into_iter().collect();
        guilds.extend(new.guild_ids.iter().filter(|g| Some(**g) != new.guild_id));
        let now = now_unix();
        let invite = Invite {
            id,
            email,
            code: Some(code.clone()),
            link: Some(format!("http://127.0.0.1:8080/invite/tok-{id}")),
            status: InviteStatus::Pending,
            inviter: Some(self.me.clone()),
            guild_ids: guilds,
            make_admin: new.make_admin,
            created_at: at(now),
            expires_at: at(now + INVITE_DAYS * DAY),
            accepted_user_id: None,
        };
        self.account.invites.insert(
            0,
            FakeInvite {
                invite: Invite {
                    code: None,
                    link: None,
                    ..invite.clone()
                },
                code,
                token: format!("tok-{id}"),
            },
        );
        Ok(invite)
    }

    /// The server's deletion rules: no owned guilds; the password (or the emailed code); the
    /// authenticator or a recovery code when it is on. The fake takes either proof first.
    fn delete_account(
        &mut self,
        password: Option<String>,
        email_code: Option<String>,
        totp_code: Option<String>,
    ) -> Result<(), ApiError> {
        let me = self.me.id;
        let owned: Vec<thehideout_api::OwnedGuild> = self
            .guilds
            .iter()
            .filter(|g| g.owner_id == me)
            .map(|g| thehideout_api::OwnedGuild {
                id: g.id,
                name: g.name.clone(),
                member_count: self.members.get(&g.id).map_or(1, |m| m.len() as u32),
            })
            .collect();
        if !owned.is_empty() {
            return Err(ApiError::OwnsGuilds(owned));
        }
        match (password, email_code) {
            (Some(p), _) if p == "wrong" => return Err(ApiError::PermissionDenied),
            (Some(_), _) => {}
            (None, Some(c)) if c.trim() == CODE => {}
            (None, Some(_)) => return Err(ApiError::WrongCode { attempts_remaining: 4 }),
            (None, None) => return Err(ApiError::InvalidArgument("password required".into())),
        }
        if self.me_has_totp() {
            let code = totp_code.ok_or_else(|| ApiError::InvalidArgument("totp_code required".into()))?;
            if code.trim() != CODE && !RECOVERY.contains(&code.trim()) {
                return Err(ApiError::WrongCode { attempts_remaining: 9 });
            }
        }
        Ok(())
    }

    fn accept_invite(
        &mut self,
        code: &str,
        username: &str,
        password: Option<&str>,
        terms_version: &str,
        age_confirmed: bool,
    ) -> Result<Session, ApiError> {
        let i = self.account.find_invite(code).ok_or(ApiError::NotFound)?;
        match self.account.invites[i].invite.status {
            InviteStatus::Pending => {}
            InviteStatus::Accepted => {
                return Err(ApiError::Precondition(
                    "this invite was already used".into(),
                ))
            }
            InviteStatus::Expired => {
                return Err(ApiError::Precondition("this invite has expired".into()))
            }
            _ => return Err(ApiError::Precondition("this invite was revoked".into())),
        }
        if terms_version != TERMS_VERSION {
            return Err(ApiError::TermsUpdateRequired);
        }
        if !age_confirmed {
            return Err(ApiError::InvalidArgument("you must confirm that you are at least 16 years old".into()));
        }
        let name = username.trim();
        if name.chars().count() < 3
            || name.chars().count() > 32
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        {
            return Err(ApiError::InvalidArgument(
                "usernames are 3 to 32 letters, digits, dots or underscores".into(),
            ));
        }
        if password.is_some_and(|p| p.chars().count() < 8) {
            return Err(ApiError::InvalidArgument(
                "password must be at least 8 characters".into(),
            ));
        }
        if name.eq_ignore_ascii_case("taken")
            || self.find_user(name).is_some()
            || name.eq_ignore_ascii_case(DORA)
        {
            return Err(ApiError::AlreadyExists);
        }
        let session = self.session_for(name);
        let id = session.me.id;
        let email = self.account.invites[i].invite.email.clone();
        if let Some(u) = self.users.iter_mut().find(|u| u.id == id) {
            u.email = Some(email.clone());
        }
        self.me.email = Some(email);
        for g in self.account.invites[i].invite.guild_ids.clone() {
            self.members.entry(g).or_default().push(id);
        }
        let inv = &mut self.account.invites[i].invite;
        inv.status = InviteStatus::Accepted;
        inv.accepted_user_id = Some(id);
        Ok(Session {
            me: self.me.clone(),
            ..session
        })
    }

    fn invite_saved(&mut self, id: u64, action: InviteAction) -> Result<Invite, ApiError> {
        let i = self
            .account
            .invites
            .iter()
            .position(|f| f.invite.id == id)
            .ok_or(ApiError::NotFound)?;
        if !self.may_manage_invite(&self.account.invites[i].invite) {
            return Err(ApiError::NotFound);
        }
        let f = &mut self.account.invites[i];
        match action {
            InviteAction::Revoke => {
                if f.invite.status != InviteStatus::Pending {
                    return Err(ApiError::Precondition(
                        "only a pending invite can be revoked".into(),
                    ));
                }
                f.invite.status = InviteStatus::Revoked;
                Ok(f.invite.clone())
            }
            InviteAction::Resend => {
                if !matches!(
                    f.invite.status,
                    InviteStatus::Pending | InviteStatus::Expired
                ) {
                    return Err(ApiError::Precondition(
                        "only a pending or expired invite can be sent again".into(),
                    ));
                }
                f.code = format!("RSND-{:04}", id % 10_000);
                f.invite.status = InviteStatus::Pending;
                f.invite.expires_at = at(now_unix() + INVITE_DAYS * DAY);
                f.token = format!("tok-rsnd-{id}");
                Ok(Invite {
                    code: Some(f.code.clone()),
                    link: Some(format!("http://127.0.0.1:8080/invite/{}", f.token)),
                    ..f.invite.clone()
                })
            }
        }
    }

    /// The commands of this module; anything else comes back.
    pub(super) fn account_command(&mut self, cmd: Command) -> Result<Vec<Reply>, Command> {
        Ok(match cmd {
            Command::Login { username, password } => vec![match self.by_login(&username) {
                Some(u) if password != "wrong" => self.first_factor(&u.username),
                _ => Reply::Auth(Err(ApiError::Unauthenticated)),
            }],
            Command::RequestEmailCode { .. } | Command::RequestPasswordReset { .. } => {
                vec![Reply::CodeSent(Ok(()))]
            }
            Command::VerifyEmailCode { email, code } => {
                vec![match self.by_email(&email).cloned() {
                    Some(u) if code.trim() == CODE => self.first_factor(&u.username),
                    _ => Reply::Auth(Err(ApiError::Unauthenticated)),
                }]
            }
            Command::ResetPassword {
                email,
                code,
                new_password,
            } => vec![Reply::PasswordReset(
                if self.by_email(&email).is_none() || code.trim() != CODE {
                    Err(ApiError::Unauthenticated)
                } else if new_password.chars().count() < 8 {
                    Err(ApiError::InvalidArgument(
                        "password must be at least 8 characters".into(),
                    ))
                } else {
                    Ok(())
                },
            )],
            Command::CompleteSecondFactor {
                challenge,
                code,
                recovery,
            } => {
                let Some(i) = self
                    .account
                    .challenges
                    .iter()
                    .position(|c| c.0 == challenge)
                else {
                    return Ok(vec![Reply::Auth(Err(ApiError::CodeExpired))]);
                };
                self.account.challenges[i].2 += 1;
                let (_, name, tries) = self.account.challenges[i].clone();
                if !self.check_code(&name, &code, recovery) {
                    if tries >= SECOND_FACTOR_TRIES {
                        self.account.challenges.remove(i);
                        return Ok(vec![Reply::Auth(Err(ApiError::TooManyAttempts))]);
                    }
                    return Ok(vec![Reply::Auth(Err(ApiError::WrongCode {
                        attempts_remaining: SECOND_FACTOR_TRIES - tries,
                    }))]);
                }
                self.account.challenges.remove(i);
                vec![Reply::Auth(Ok(self.session_for(&name)))]
            }
            Command::BeginPasskeyLogin { username } => {
                let has = self.find_user(&username).is_some_and(|u| {
                    self.account
                        .passkeys
                        .get(&u.id)
                        .is_some_and(|p| !p.is_empty())
                });
                if !has {
                    return Ok(vec![Reply::PasskeyCeremony(Err(ApiError::Precondition(
                        "this account has no passkey".into(),
                    )))]);
                }
                let session = self.session_for(&username);
                self.account.ceremony = Some((
                    self.now + self.latency + PASSKEY_SECS,
                    Reply::Auth(Ok(session)),
                ));
                vec![Reply::PasskeyCeremony(Ok(format!(
                    "http://127.0.0.1:8080/passkey/fake-{}",
                    self.account.next_passkey
                )))]
            }
            Command::BeginSecondFactorPasskey { challenge } => {
                let Some(i) = self.account.challenges.iter().position(|c| c.0 == challenge) else {
                    return Ok(vec![Reply::PasskeyCeremony(Err(ApiError::CodeExpired))]);
                };
                let name = self.account.challenges[i].1.clone();
                if !self.account.second_factor_passkeys.contains(&name) {
                    return Ok(vec![Reply::PasskeyCeremony(Err(ApiError::Precondition(
                        "no passkey for this account".into(),
                    )))]);
                }
                self.account.challenges.remove(i);
                let session = self.session_for(&name);
                self.account.ceremony = Some((
                    self.now + self.latency + PASSKEY_SECS,
                    Reply::Auth(Ok(session)),
                ));
                vec![Reply::PasskeyCeremony(Ok(format!(
                    "http://127.0.0.1:8080/passkey/fake-2fa-{}",
                    self.account.next_challenge
                )))]
            }
            Command::CancelPasskey => {
                self.account.ceremony = None;
                Vec::new()
            }
            Command::LookupInvite { code } => {
                vec![Reply::InviteLookup(match self.account.find_invite(&code) {
                    None => Err(ApiError::NotFound),
                    Some(i) => {
                        let inv = &self.account.invites[i].invite;
                        let names = inv
                            .guild_ids
                            .iter()
                            .filter_map(|g| self.guilds.iter().find(|x| x.id == *g))
                            .map(|g| g.name.clone())
                            .collect();
                        Ok(InvitePreview {
                            masked_email: mask(&inv.email),
                            inviter: inv.inviter.clone(),
                            guild_names: names,
                            expires_at: inv.expires_at,
                            status: inv.status,
                            terms_version: TERMS_VERSION.to_owned(),
                            minimum_age: 16,
                        })
                    }
                })]
            }
            Command::AcceptInvite {
                code,
                username,
                password,
                terms_version,
                age_confirmed,
            } => {
                vec![Reply::Auth(self.accept_invite(
                    &code,
                    &username,
                    password.as_deref(),
                    &terms_version,
                    age_confirmed,
                ))]
            }
            Command::CheckTerms => vec![Reply::Terms(Ok(TermsStatus {
                current: TERMS_VERSION.to_owned(),
                update_required: self.account.terms_outdated.then(|| TERMS_VERSION.to_owned()),
            }))],
            Command::AcceptTerms { version } => {
                let result = if version == TERMS_VERSION {
                    self.account.terms_outdated = false;
                    Ok(())
                } else {
                    Err(ApiError::Precondition(format!("the current terms version is {TERMS_VERSION}")))
                };
                vec![Reply::TermsAccepted { version, result }]
            }
            Command::RequestAccountDeletionCode => vec![Reply::DeletionCodeSent(if self.me.email.is_some() {
                Ok(())
            } else {
                Err(ApiError::Precondition("your account has no email address".into()))
            })],
            Command::DeleteAccount { password, email_code, totp_code } => {
                vec![Reply::AccountDeleted(self.delete_account(password, email_code, totp_code))]
            }
            Command::ExportData => vec![Reply::DataExportQueued(match self.me.email.clone() {
                None => Err(ApiError::Precondition("your account has no email address to send the export to".into())),
                Some(_) if self.account.exported => Err(ApiError::RateLimited),
                Some(email) => {
                    self.account.exported = true;
                    Ok(mask(&email))
                }
            })],
            Command::LoadSecurity => vec![Reply::Security(Ok(self.security()))],
            Command::ChangePassword { current, new } => {
                vec![Reply::PasswordChanged(if current == "wrong" {
                    Err(ApiError::PermissionDenied)
                } else if new.chars().count() < 8 {
                    Err(ApiError::InvalidArgument(
                        "password must be at least 8 characters".into(),
                    ))
                } else {
                    if let Some(list) = self.account.sessions.get_mut(&self.me.id) {
                        list.retain(|s| s.current);
                    }
                    Ok(())
                })]
            }
            Command::ChangeEmail { email, password } => {
                let email = email.trim().to_lowercase();
                vec![Reply::EmailChangeStarted(
                    if !email.contains('@') || !email.contains('.') {
                        Err(ApiError::InvalidArgument(
                            "that is not an email address".into(),
                        ))
                    } else if password == "wrong" {
                        Err(ApiError::PermissionDenied)
                    } else if self.by_email(&email).is_some_and(|u| u.id != self.me.id) {
                        Err(ApiError::AlreadyExists)
                    } else {
                        self.account.email_change.insert(self.me.id, email);
                        Ok(())
                    },
                )]
            }
            Command::ConfirmEmailChange { code } => vec![Reply::EmailChanged(
                match self.account.email_change.get(&self.me.id).cloned() {
                    Some(email) if code.trim() == CODE => {
                        self.account.email_change.remove(&self.me.id);
                        self.me.email = Some(email.clone());
                        let id = self.me.id;
                        if let Some(u) = self.users.iter_mut().find(|u| u.id == id) {
                            u.email = Some(email);
                        }
                        Ok(self.me.clone())
                    }
                    Some(_) => Err(self.wrong_check()),
                    None => Err(ApiError::CodeExpired),
                },
            )],
            Command::BeginTotpSetup => vec![Reply::TotpSetup(if self.me_has_totp() {
                Err(ApiError::Precondition(
                    "an authenticator app is already set up".into(),
                ))
            } else {
                self.account.setups.insert(self.me.id);
                let label = self
                    .me
                    .email
                    .clone()
                    .unwrap_or_else(|| self.me.username.clone());
                Ok(TotpSetup {
                    otpauth_uri: format!("otpauth://totp/thehideout:{label}?secret={TOTP_SECRET}&issuer=thehideout&algorithm=SHA1&digits=6&period=30"),
                    secret: TOTP_SECRET.into(),
                })
            })],
            Command::ConfirmTotp { code } => vec![Reply::RecoveryCodes {
                regenerated: false,
                result: if !self.account.setups.contains(&self.me.id) {
                    Err(ApiError::CodeExpired)
                } else if code.trim() != CODE {
                    Err(self.wrong_check())
                } else {
                    self.account.setups.remove(&self.me.id);
                    Ok(self.new_codes())
                },
            }],
            Command::DisableTotp { code } => {
                let name = self.me.username.clone();
                vec![Reply::TotpDisabled(if !self.me_has_totp() {
                    Err(ApiError::Precondition(
                        "two-factor authentication is off".into(),
                    ))
                } else if !self.check_code(&name, &code, false) {
                    Err(self.wrong_check())
                } else {
                    self.account.totp.remove(&name.to_lowercase());
                    Ok(())
                })]
            }
            Command::RegenerateRecoveryCodes { code } => vec![Reply::RecoveryCodes {
                regenerated: true,
                result: if !self.me_has_totp() {
                    Err(ApiError::Precondition(
                        "two-factor authentication is off".into(),
                    ))
                } else if code.trim() != CODE {
                    Err(self.wrong_check())
                } else {
                    Ok(self.new_codes())
                },
            }],
            Command::BeginPasskeyRegistration { name } => {
                let name = if name.trim().is_empty() {
                    "Passkey".to_owned()
                } else {
                    name.trim().to_owned()
                };
                let id = self.account.next_passkey;
                self.account.next_passkey += 1;
                let key = Passkey {
                    id,
                    name,
                    created_at: at(now_unix()),
                    last_used_at: None,
                };
                self.account
                    .passkeys
                    .entry(self.me.id)
                    .or_default()
                    .push(key.clone());
                self.account.ceremony = Some((
                    self.now + self.latency + PASSKEY_SECS,
                    Reply::PasskeyAdded(Ok(key)),
                ));
                vec![Reply::PasskeyCeremony(Ok(format!(
                    "http://127.0.0.1:8080/passkey/fake-{id}"
                )))]
            }
            Command::RenamePasskey { id, name } => {
                let list = self.account.passkeys.entry(self.me.id).or_default();
                vec![Reply::PasskeyRenamed(
                    match list.iter_mut().find(|p| p.id == id) {
                        _ if name.trim().is_empty() || name.chars().count() > 64 => {
                            Err(ApiError::InvalidArgument("use 1 to 64 characters".into()))
                        }
                        Some(p) => {
                            p.name = name.trim().to_owned();
                            Ok(p.clone())
                        }
                        None => Err(ApiError::NotFound),
                    },
                )]
            }
            Command::DeletePasskey { id } => {
                let list = self.account.passkeys.entry(self.me.id).or_default();
                let before = list.len();
                list.retain(|p| p.id != id);
                vec![Reply::PasskeyDeleted {
                    id,
                    result: if list.len() < before {
                        Ok(())
                    } else {
                        Err(ApiError::NotFound)
                    },
                }]
            }
            Command::RevokeSession { id } => {
                let list = self.account.sessions.entry(self.me.id).or_default();
                let before = list.len();
                list.retain(|s| s.id != id);
                vec![Reply::SessionRevoked {
                    id,
                    result: if list.len() < before {
                        Ok(())
                    } else {
                        Err(ApiError::NotFound)
                    },
                }]
            }
            Command::RevokeOtherSessions => {
                let list = self.account.sessions.entry(self.me.id).or_default();
                let before = list.len();
                list.retain(|s| s.current);
                vec![Reply::OthersRevoked(Ok((before - list.len()) as u32))]
            }
            Command::CreateInvite { invite } => {
                vec![Reply::InviteCreated(self.create_invite(invite))]
            }
            Command::ListInvites {
                epoch,
                guild,
                page_token,
                limit,
            } => {
                let result = match guild {
                    None if !self.me.is_admin => Err(ApiError::PermissionDenied),
                    Some(g) if !self.may_invite(g) => Err(ApiError::PermissionDenied),
                    _ => {
                        let all: Vec<Invite> = self
                            .account
                            .invites
                            .iter()
                            .map(|f| f.invite.clone())
                            .filter(|i| guild.is_none_or(|g| i.guild_ids.contains(&g)))
                            .collect();
                        let from = page_token
                            .and_then(|t| t.parse::<usize>().ok())
                            .unwrap_or(0)
                            .min(all.len());
                        let to = (from + limit.max(1) as usize).min(all.len());
                        Ok(Paged {
                            items: all[from..to].to_vec(),
                            next_page_token: (to < all.len()).then(|| to.to_string()),
                        })
                    }
                };
                vec![Reply::Invites {
                    epoch,
                    guild,
                    result,
                }]
            }
            Command::RevokeInvite { id } => {
                vec![Reply::InviteSaved {
                    id,
                    action: InviteAction::Revoke,
                    result: self.invite_saved(id, InviteAction::Revoke),
                }]
            }
            Command::ResendInvite { id } => {
                vec![Reply::InviteSaved {
                    id,
                    action: InviteAction::Resend,
                    result: self.invite_saved(id, InviteAction::Resend),
                }]
            }
            other => return Err(other),
        })
    }
}

impl FakeBackend {
    /// The invite to `email` is accepted on another device by a new user `name`: it turns
    /// Accepted and the gateway says they joined each of its servers.
    #[cfg(test)]
    pub fn invite_accepted_elsewhere(&self, email: &str, id: u64, name: &str) {
        let mut w = self.0.borrow_mut();
        let Some(i) = w.account.invites.iter().position(|f| f.invite.email == email) else { return };
        let joined = user(id, name);
        w.users.push(joined.clone());
        let inv = &mut w.account.invites[i].invite;
        inv.status = InviteStatus::Accepted;
        inv.accepted_user_id = Some(id);
        for g in inv.guild_ids.clone() {
            w.members.entry(g).or_default().push(id);
            w.enqueue(Reply::Gateway(GatewayUpdate::Event(Event::GuildMemberJoined { guild_id: g, user: joined.clone() })));
        }
    }
}
