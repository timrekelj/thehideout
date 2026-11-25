//! Proofs of signing in, invites and account security against the fake backend: every way in
//! (password, emailed code, passkey, reset), the second factor (authenticator, recovery code,
//! passkey, the server's tries left, lockout), invites (lookup, accept, the errors), the legacy add-email prompt, the
//! Security page's actions (and recovery codes shown only once), the server's invite dialog and
//! who sees it, the admin Invites tab and the role editor's "Invite people" switch. Mostly
//! state-level; the keyboard paths drive the real view.

use rune::Key;
use thehideout_api::{InviteStatus, Permissions};

use crate::account::SecKind;
use crate::admin::AdminPage;
use crate::app::{fake_now, App};
use crate::backend::{Backend, Command};
use crate::fake::account::{CODE, DORA, INVITE_EXPIRED, INVITE_PENDING, INVITE_REVOKED, RECOVERY};
use crate::fake::{FakeBackend, HIDEOUT, HIDEOUT_MODS};
use crate::format::Tz;
use crate::forms::{AuthField, AuthMode, AuthStep, DialogKind, Phase};
use crate::scenes::{booted, booted_as};
use crate::server_settings::ServerPage;
use crate::state::{AppState, Screen};
use crate::tests::{click, has, key, runtime, settle, type_text};

fn signed_out() -> (App, FakeBackend) {
    let fake = FakeBackend::new();
    let app = App::new(
        Box::new(fake.clone()),
        AppState::new(fake_now, Tz::Fixed(0)),
    );
    (app, fake)
}

fn sent(fake: &FakeBackend, pick: impl Fn(&Command) -> bool) -> usize {
    fake.sent().iter().filter(|c| pick(c)).count()
}

fn error(app: &App, field: AuthField) -> Option<String> {
    app.state.auth.error_for(field).map(str::to_owned)
}

/// Signs in as `who` with a password, up to whatever the fake answers.
fn password(app: &mut App, who: &str, pass: &str) {
    app.state.auth.username.replace_text(who);
    app.state.auth.password.replace_text(pass);
    app.act(|s| s.submit_auth());
    app.pump();
}

// ---- password ----------------------------------------------------------------------------------

#[test]
fn a_username_or_an_email_signs_in_with_the_password() {
    let (mut app, fake) = signed_out();
    password(&mut app, "tim@example.com", "hunter22");
    assert_eq!(app.state.screen, Screen::Main);
    assert_eq!(app.state.me().unwrap().username, "tim");
    assert_eq!(
        sent(
            &fake,
            |c| matches!(c, Command::Login { username, .. } if username == "tim@example.com")
        ),
        1
    );
}

#[test]
fn a_wrong_password_keeps_the_login_and_names_no_account() {
    let (mut app, _) = signed_out();
    password(&mut app, "tim", "wrong");
    assert_eq!(app.state.screen, Screen::Auth);
    assert_eq!(app.state.auth.username.text, "tim", "the login stays typed");
    assert!(app.state.auth.password.text.is_empty(), "the secret goes");
    let msg = error(&app, AuthField::Form).unwrap();
    assert_eq!(msg, "Wrong username, email or password.");
    // An unknown email reads exactly the same.
    password(&mut app, "nobody@example.com", "x");
    assert_eq!(error(&app, AuthField::Form).unwrap(), msg);
}

#[test]
fn empty_fields_are_caught_before_anything_is_sent() {
    let (mut app, fake) = signed_out();
    app.act(|s| s.submit_auth());
    assert!(error(&app, AuthField::Username).is_some());
    assert!(fake.sent().is_empty());
}

// ---- emailed code ------------------------------------------------------------------------------

#[test]
fn an_emailed_code_signs_in_once_six_digits_are_typed() {
    let (app, fake) = signed_out();
    let mut rt = runtime(app);
    click(&mut rt, "auth-link-code");
    settle(&mut rt, 2);
    assert_eq!(
        rt.state.state.auth.step,
        AuthStep::EmailCode { sent: false }
    );
    click(&mut rt, "auth-email/value/input");
    type_text(&mut rt, "tim@example.com");
    key(&mut rt, Key::Enter);
    // The card grows to the code step; let it settle before pointing at the cells.
    settle(&mut rt, 40);
    assert_eq!(rt.state.state.auth.step, AuthStep::EmailCode { sent: true });
    assert!(
        rt.state
            .state
            .auth
            .notice
            .as_deref()
            .unwrap()
            .contains("If an account uses tim@example.com"),
        "said for any address"
    );
    assert!(has(&rt, "auth-resend"));
    // Pasted with a dash and spaces: only the digits count, and a full code goes at once.
    // The cells are what is pressed; the field over them takes the keys.
    click(&mut rt, "auth-code/cells/cell0");
    type_text(&mut rt, "123 -456");
    settle(&mut rt, 6);
    assert_eq!(
        sent(
            &fake,
            |c| matches!(c, Command::VerifyEmailCode { code, .. } if code == CODE)
        ),
        1
    );
    assert_eq!(rt.state.state.screen, Screen::Main);
}

#[test]
fn a_wrong_emailed_code_says_so_and_clears_the_cells() {
    let (mut app, fake) = signed_out();
    app.act(|s| s.auth_go(AuthStep::EmailCode { sent: false }));
    app.state.auth.email.replace_text("tim@example.com");
    app.act(|s| s.submit_auth());
    app.pump();
    app.state.auth.code.replace_text("999999");
    app.act(|s| s.auth_code_changed());
    app.pump();
    assert_eq!(
        sent(&fake, |c| matches!(c, Command::VerifyEmailCode { .. })),
        1
    );
    assert!(error(&app, AuthField::Code)
        .unwrap()
        .starts_with("That code didn't work"));
    assert!(app.state.auth.code.text.is_empty());
    assert_eq!(app.state.screen, Screen::Auth);
}

#[test]
fn resending_a_code_waits_thirty_seconds() {
    let (mut app, fake) = signed_out();
    app.act(|s| s.auth_go(AuthStep::EmailCode { sent: false }));
    app.state.auth.email.replace_text("tim@example.com");
    app.act(|s| s.submit_auth());
    app.pump();
    app.state.tick(10.0);
    assert_eq!(app.state.resend_wait(), 30);
    app.act(|s| s.resend_code());
    assert_eq!(
        sent(&fake, |c| matches!(c, Command::RequestEmailCode { .. })),
        1,
        "too early"
    );
    app.state.tick(25.0);
    assert_eq!(app.state.resend_wait(), 15);
    app.state.tick(40.5);
    assert_eq!(app.state.resend_wait(), 0);
    app.act(|s| s.resend_code());
    app.pump();
    assert_eq!(
        sent(&fake, |c| matches!(c, Command::RequestEmailCode { .. })),
        2
    );
}

// ---- passkey -----------------------------------------------------------------------------------

#[test]
fn a_passkey_sign_in_waits_for_the_browser_then_signs_in() {
    let (app, fake) = signed_out();
    let mut rt = runtime(app);
    click(&mut rt, "auth-link-passkey");
    settle(&mut rt, 2);
    click(&mut rt, "auth-username/value/input");
    type_text(&mut rt, "tim");
    key(&mut rt, Key::Enter);
    settle(&mut rt, 4);
    assert_eq!(
        rt.state.state.auth.step,
        AuthStep::Passkey { waiting: true }
    );
    assert!(has(&rt, "auth-passkey-waiting"));
    assert!(has(&rt, "auth-passkey-cancel"));
    // The fake ceremony finishes two seconds later.
    settle(&mut rt, 150);
    assert_eq!(rt.state.state.screen, Screen::Main);
    assert_eq!(
        sent(
            &fake,
            |c| matches!(c, Command::BeginPasskeyLogin { username } if username == "tim")
        ),
        1
    );
}

#[test]
fn cancelling_a_passkey_sign_in_stops_waiting() {
    let (app, fake) = signed_out();
    let mut rt = runtime(app);
    rt.update(|a, _| {
        a.act(|s| s.auth_go(AuthStep::Passkey { waiting: false }));
        a.state.auth.username.replace_text("tim");
        a.act(|s| s.submit_auth());
    });
    settle(&mut rt, 4);
    click(&mut rt, "auth-passkey-cancel");
    settle(&mut rt, 180);
    assert_eq!(
        rt.state.state.screen,
        Screen::Auth,
        "nothing lands after a cancel"
    );
    assert_eq!(
        rt.state.state.auth.step,
        AuthStep::Passkey { waiting: false }
    );
    assert_eq!(sent(&fake, |c| matches!(c, Command::CancelPasskey)), 1);
}

#[test]
fn an_account_without_a_passkey_is_pointed_at_the_other_ways_in() {
    let (mut app, _) = signed_out();
    app.act(|s| s.auth_go(AuthStep::Passkey { waiting: false }));
    app.state.auth.username.replace_text("ana");
    app.act(|s| s.submit_auth());
    app.pump();
    assert_eq!(app.state.auth.step, AuthStep::Passkey { waiting: false });
    assert!(error(&app, AuthField::Form)
        .unwrap()
        .contains("Use your password or an emailed code"));
}

// ---- second factor -----------------------------------------------------------------------------

fn at_second_factor() -> (App, FakeBackend) {
    let (mut app, fake) = signed_out();
    password(&mut app, DORA, "hunter22");
    assert!(
        matches!(app.state.auth.step, AuthStep::SecondFactor { .. }),
        "dora has an authenticator app"
    );
    assert_eq!(app.state.screen, Screen::Auth);
    (app, fake)
}

#[test]
fn an_authenticator_code_completes_the_sign_in() {
    let (mut app, fake) = at_second_factor();
    app.state.auth.code.replace_text(CODE);
    app.act(|s| s.auth_code_changed());
    app.pump();
    assert_eq!(app.state.screen, Screen::Main);
    assert_eq!(
        sent(
            &fake,
            |c| matches!(c, Command::CompleteSecondFactor { recovery: false, code, .. } if code == CODE)
        ),
        1
    );
    assert_eq!(
        app.state.ui.security.totp,
        Some(true),
        "the Security page knows it is on"
    );
}

#[test]
fn a_recovery_code_completes_the_sign_in_once() {
    let (mut app, fake) = at_second_factor();
    app.act(|s| s.toggle_recovery());
    app.state
        .auth
        .recovery
        .replace_text(RECOVERY[0].to_lowercase());
    app.act(|s| s.submit_auth());
    app.pump();
    assert_eq!(app.state.screen, Screen::Main);
    assert_eq!(
        sent(&fake, |c| matches!(
            c,
            Command::CompleteSecondFactor { recovery: true, .. }
        )),
        1
    );
    // The same code a second time is refused.
    let mut again = App::new(
        Box::new(fake.clone()),
        AppState::new(fake_now, Tz::Fixed(0)),
    );
    password(&mut again, DORA, "hunter22");
    again.act(|s| s.toggle_recovery());
    again.state.auth.recovery.replace_text(RECOVERY[0]);
    again.act(|s| s.submit_auth());
    again.pump();
    assert_eq!(again.state.screen, Screen::Auth);
    assert_eq!(
        error(&again, AuthField::Code).unwrap(),
        "That recovery code didn't work. 4 tries left."
    );
}

#[test]
fn wrong_codes_show_the_servers_tries_left_and_the_fifth_stops_the_sign_in() {
    let (mut app, fake) = at_second_factor();
    for n in 1..=5u32 {
        app.state.auth.code.replace_text("111111");
        app.act(|s| s.auth_code_changed());
        app.pump();
        let AuthStep::SecondFactor { remaining, locked, .. } = &app.state.auth.step else {
            panic!()
        };
        // The count is the server's (no client-side counter): 4, 3, 2, 1, then it is over.
        if n < 5 {
            assert_eq!(*remaining, Some(5 - n));
            assert!(!*locked);
            let left = if n == 4 { "1 try left.".to_owned() } else { format!("{} tries left.", 5 - n) };
            assert_eq!(error(&app, AuthField::Code).unwrap(), format!("That code didn't work. {left}"));
        } else {
            assert!(*locked);
        }
    }
    assert!(error(&app, AuthField::Form)
        .unwrap()
        .starts_with("Too many wrong attempts"));
    // Locked: nothing more is sent, and "Start again" goes back to the password.
    app.state.auth.code.replace_text(CODE);
    app.act(|s| s.submit_auth());
    assert_eq!(
        sent(&fake, |c| matches!(c, Command::CompleteSecondFactor { .. })),
        5
    );
    app.act(|s| s.auth_go(AuthStep::Password));
    assert_eq!(app.state.auth.step, AuthStep::Password);
}

#[test]
fn an_expired_challenge_stops_the_sign_in() {
    let (mut app, _) = at_second_factor();
    // The server forgot it (restart, five minutes): the form says so and offers a restart.
    if let AuthStep::SecondFactor { challenge, .. } = &mut app.state.auth.step {
        *challenge = "gone".into();
    }
    app.state.auth.code.replace_text(CODE);
    app.act(|s| s.auth_code_changed());
    app.pump();
    assert!(matches!(app.state.auth.step, AuthStep::SecondFactor { locked: true, .. }));
    assert_eq!(error(&app, AuthField::Form).unwrap(), "This sign-in expired. Start again.");
}

#[test]
fn a_passkey_answers_the_second_factor_through_the_browser() {
    let (app, fake) = at_second_factor();
    let AuthStep::SecondFactor { methods, challenge, .. } = &app.state.auth.step else { panic!() };
    assert!(methods.contains(&thehideout_api::SecondFactorMethod::Passkey), "dora has a passkey");
    let challenge = challenge.clone();
    let mut rt = runtime(app);
    settle(&mut rt, 4);
    click(&mut rt, "auth-2fa-passkey");
    settle(&mut rt, 4);
    assert!(matches!(rt.state.state.auth.step, AuthStep::SecondFactor { passkey: true, .. }));
    assert!(has(&rt, "auth-2fa-passkey-waiting"));
    // The challenge's own ceremony, not a fresh passkey sign-in.
    assert_eq!(
        sent(&fake, |c| matches!(c, Command::BeginSecondFactorPasskey { challenge: id } if *id == challenge)),
        1
    );
    assert_eq!(sent(&fake, |c| matches!(c, Command::BeginPasskeyLogin { .. })), 0);
    settle(&mut rt, 150);
    assert_eq!(rt.state.state.screen, Screen::Main);
    assert_eq!(rt.state.state.me().unwrap().username, DORA);
}

#[test]
fn cancelling_the_second_factor_passkey_returns_to_the_code() {
    let (mut app, fake) = at_second_factor();
    app.act(|s| s.second_factor_passkey());
    app.pump();
    assert!(matches!(app.state.auth.step, AuthStep::SecondFactor { passkey: true, .. }));
    app.act(|s| s.cancel_passkey_login());
    assert!(matches!(app.state.auth.step, AuthStep::SecondFactor { passkey: false, locked: false, .. }));
    assert_eq!(app.state.auth.phase, Phase::Idle);
    assert_eq!(sent(&fake, |c| matches!(c, Command::CancelPasskey)), 1);
}

#[test]
fn without_a_passkey_the_second_factor_offers_none() {
    let (mut app, fake) = signed_out();
    // Turn tim's authenticator on in the fake (no second-factor passkey for him).
    password(&mut app, "tim", "hunter22");
    app.act(|s| s.open_sec(SecKind::TotpSetup { setup: None }));
    app.pump();
    app.state.ui.security.dialog.as_mut().unwrap().code.replace_text(CODE);
    app.act(|s| s.submit_sec());
    app.pump();
    let mut other = App::new(Box::new(fake.clone()), AppState::new(fake_now, Tz::Fixed(0)));
    password(&mut other, "tim", "hunter22");
    let AuthStep::SecondFactor { methods, .. } = &other.state.auth.step else { panic!("{:?}", other.state.auth.step) };
    assert!(!methods.contains(&thehideout_api::SecondFactorMethod::Passkey));
    let mut rt = runtime(other);
    settle(&mut rt, 4);
    assert!(has(&rt, "auth-toggle-recovery"));
    assert!(!has(&rt, "auth-2fa-passkey"));
}

// ---- password reset ------------------------------------------------------------------------------

#[test]
fn forgot_password_resets_with_a_code_and_returns_to_sign_in() {
    let (mut app, fake) = signed_out();
    app.state.auth.username.replace_text("tim@example.com");
    app.act(|s| s.auth_go(AuthStep::Reset { sent: false }));
    assert_eq!(
        app.state.auth.email.text, "tim@example.com",
        "an email typed as the login comes along"
    );
    app.act(|s| s.submit_auth());
    app.pump();
    assert_eq!(app.state.auth.step, AuthStep::Reset { sent: true });
    app.state.auth.code.replace_text(CODE);
    app.state.auth.password.replace_text("short");
    app.act(|s| s.submit_auth());
    assert_eq!(
        error(&app, AuthField::Password).unwrap(),
        "Use at least 8 characters."
    );
    app.state.auth.password.replace_text("a new long password");
    app.act(|s| s.submit_auth());
    app.pump();
    assert_eq!(app.state.auth.step, AuthStep::Password);
    assert!(app
        .state
        .auth
        .notice
        .as_deref()
        .unwrap()
        .contains("Sign in with the new one"));
    assert_eq!(app.state.auth.username.text, "tim@example.com");
    assert_eq!(
        sent(
            &fake,
            |c| matches!(c, Command::ResetPassword { new_password, .. } if new_password == "a new long password")
        ),
        1
    );
}

// ---- invites -----------------------------------------------------------------------------------

fn looked_up(code: &str) -> (App, FakeBackend) {
    let (mut app, fake) = signed_out();
    app.act(|s| s.set_auth_mode(AuthMode::Invite));
    app.state.auth.invite.replace_text(code);
    app.act(|s| s.submit_auth());
    app.pump();
    // The invitee ticks the terms and age boxes (the screen's checkboxes set these).
    app.state.auth.accept_terms = true;
    app.state.auth.confirm_age = true;
    (app, fake)
}

#[test]
fn an_invite_is_looked_up_then_accepted_into_its_server() {
    let (mut app, fake) = looked_up(INVITE_PENDING);
    let AuthStep::InviteAccept { preview, code } = &app.state.auth.step else {
        panic!("{:?}", app.state.auth.step)
    };
    assert_eq!(code, INVITE_PENDING);
    assert_eq!(preview.masked_email, "e***@example.com");
    assert_eq!(preview.inviter.as_ref().unwrap().username, "tim");
    assert_eq!(preview.guild_names, vec!["The Hideout".to_owned()]);
    app.state.auth.username.replace_text("erin");
    app.state.auth.password.replace_text("correct horse");
    app.act(|s| s.submit_auth());
    app.pump();
    assert_eq!(app.state.screen, Screen::Main);
    assert_eq!(
        app.state.me().unwrap().email.as_deref(),
        Some("erin@example.com")
    );
    assert!(
        app.state.guild(HIDEOUT).is_some(),
        "joined the invite's server"
    );
    assert_eq!(
        sent(
            &fake,
            |c| matches!(c, Command::AcceptInvite { username, password: Some(_), .. } if username == "erin")
        ),
        1
    );
    // Signed up: protection is offered, and can be skipped.
    assert_eq!(
        app.state
            .ui
            .security
            .dialog
            .as_ref()
            .map(|d| d.kind.clone()),
        Some(SecKind::Welcome)
    );
    app.act(|s| s.close_sec());
    assert!(app.state.ui.security.dialog.is_none());
}

#[test]
fn signing_up_with_a_passkey_asks_for_it_first_then_offers_two_factor() {
    let (mut app, fake) = looked_up(INVITE_PENDING);
    app.state.auth.username.replace_text("erin");
    app.state.auth.password.replace_text("correct horse");
    app.state.auth.add_passkey = true;
    app.act(|s| s.submit_auth());
    app.pump();
    assert!(matches!(
        app.state.ui.security.dialog.as_ref().map(|d| &d.kind),
        Some(SecKind::AddPasskey { waiting: false })
    ));
    app.act(|s| s.close_sec());
    assert_eq!(
        app.state
            .ui
            .security
            .dialog
            .as_ref()
            .map(|d| d.kind.clone()),
        Some(SecKind::Welcome)
    );
    app.act(|s| s.submit_sec());
    app.pump();
    assert!(matches!(
        app.state.ui.security.dialog.as_ref().map(|d| &d.kind),
        Some(SecKind::TotpSetup { setup: Some(_) })
    ));
    assert_eq!(sent(&fake, |c| matches!(c, Command::BeginTotpSetup)), 1);
}

#[test]
fn the_invite_field_takes_the_code_as_typed_without_padding() {
    let (app, fake) = looked_up(&format!("  {INVITE_PENDING} "));
    assert!(matches!(app.state.auth.step, AuthStep::InviteAccept { .. }));
    assert_eq!(
        sent(
            &fake,
            |c| matches!(c, Command::LookupInvite { code } if code == INVITE_PENDING)
        ),
        1
    );
}

#[test]
fn a_short_code_says_so_and_asks_the_server_nothing() {
    for code in ["", "WXYZ-234", "WXYZ"] {
        let (app, fake) = looked_up(code);
        assert_eq!(app.state.auth.step, AuthStep::InviteCode, "{code}");
        let says = if code.is_empty() { "Enter the code from your invite email." } else { "Invite codes have 8 characters." };
        assert_eq!(error(&app, AuthField::Invite).unwrap(), says, "{code}");
        assert_eq!(sent(&fake, |c| matches!(c, Command::LookupInvite { .. })), 0, "{code}");
        assert_eq!(app.state.auth.phase, Phase::Idle);
    }
}

#[test]
fn a_pasted_link_becomes_its_code() {
    for link in [
        "https://hideout.example/invite/wxyz-2345",
        "thehideout://invite/WXYZ2345",
        "hideout.example/invite/wxyz-2345",
    ] {
        let (app, fake) = looked_up(link);
        assert!(matches!(app.state.auth.step, AuthStep::InviteAccept { .. }), "{link}");
        assert_eq!(
            sent(&fake, |c| matches!(c, Command::LookupInvite { code } if code == INVITE_PENDING)),
            1,
            "{link}"
        );
    }
}

#[test]
fn the_eighth_character_looks_the_code_up_by_itself() {
    let (app, _) = signed_out();
    let mut rt = runtime(app);
    rt.update(|a, _| a.act(|s| s.set_auth_mode(AuthMode::Invite)));
    settle(&mut rt, 3);
    click(&mut rt, "auth-invite/value/input");
    type_text(&mut rt, "WXYZ234");
    settle(&mut rt, 4);
    assert!(!has(&rt, "auth-invite-summary"), "seven characters wait");
    assert_eq!(rt.state.state.auth.step, AuthStep::InviteCode);
    type_text(&mut rt, "5");
    settle(&mut rt, 6);
    assert!(has(&rt, "auth-invite-summary"), "no Enter needed");
    // A pasted whole code does the same, and a wrong one is looked up once, not over and over.
    let (app, fake) = signed_out();
    let mut rt = runtime(app);
    rt.update(|a, _| a.act(|s| s.set_auth_mode(AuthMode::Invite)));
    settle(&mut rt, 3);
    rt.update(|a, _| a.state.auth.invite.replace_text("NOPE-0000"));
    settle(&mut rt, 8);
    assert_eq!(rt.state.state.auth.step, AuthStep::InviteCode);
    assert_eq!(sent(&fake, |c| matches!(c, Command::LookupInvite { .. })), 1);
    assert!(error(&rt.state, AuthField::Invite).unwrap().starts_with("We couldn't find that invite"));
}

#[test]
fn unknown_expired_and_withdrawn_invites_are_refused_at_lookup() {
    for (code, says) in [
        ("NOPE-0000", "We couldn't find that invite"),
        (INVITE_EXPIRED, "This invite has expired"),
        (INVITE_REVOKED, "This invite was withdrawn"),
    ] {
        let (app, _) = looked_up(code);
        assert_eq!(app.state.auth.step, AuthStep::InviteCode, "{code}");
        assert!(
            error(&app, AuthField::Invite).unwrap().starts_with(says),
            "{code}: {:?}",
            app.state.auth.error
        );
    }
}

#[test]
fn a_taken_name_keeps_the_invite_open_and_says_so_under_the_name() {
    let (mut app, _) = looked_up(INVITE_PENDING);
    app.state.auth.username.replace_text("ana");
    app.state.auth.password.replace_text("correct horse");
    app.act(|s| s.submit_auth());
    app.pump();
    assert!(
        matches!(app.state.auth.step, AuthStep::InviteAccept { .. }),
        "the invite stays usable"
    );
    assert_eq!(
        error(&app, AuthField::Username).unwrap(),
        "That username is taken. Try another."
    );
    assert_eq!(app.state.auth.phase, Phase::Idle);
}

#[test]
fn an_invite_that_lapsed_meanwhile_is_refused_at_accept() {
    let (mut app, _) = looked_up(INVITE_PENDING);
    if let AuthStep::InviteAccept { code, .. } = &mut app.state.auth.step {
        *code = INVITE_REVOKED.into();
    }
    app.state.auth.username.replace_text("erin");
    app.state.auth.password.replace_text("correct horse");
    app.act(|s| s.submit_auth());
    app.pump();
    assert!(error(&app, AuthField::Form)
        .unwrap()
        .starts_with("This invite can't be used any more"));
    assert_eq!(app.state.screen, Screen::Auth);
}

#[test]
fn the_invite_tab_is_reached_with_the_mouse() {
    let (app, _) = signed_out();
    let mut rt = runtime(app);
    click(&mut rt, "auth-link-invite");
    settle(&mut rt, 3);
    assert_eq!(rt.state.state.auth.mode, AuthMode::Invite);
    assert!(has(&rt, "auth-invite"));
    click(&mut rt, "auth-invite/value/input");
    type_text(&mut rt, INVITE_PENDING);
    key(&mut rt, Key::Enter);
    settle(&mut rt, 4);
    assert!(has(&rt, "auth-invite-summary"));
}

// ---- legacy accounts -----------------------------------------------------------------------------

#[test]
fn a_legacy_account_is_asked_for_an_email_and_adds_one() {
    let (app, fake) = booted_as(true, "bruno");
    let mut rt = runtime(app);
    assert!(rt.state.state.wants_email());
    assert!(has(&rt, "email-banner"));
    click(&mut rt, "email-banner-add");
    settle(&mut rt, 4);
    rt.update(|a, _| {
        let d = a.state.ui.security.dialog.as_mut().unwrap();
        d.first.replace_text("bruno@example.com");
        a.act(|s| s.submit_sec());
    });
    settle(&mut rt, 4);
    assert!(matches!(
        rt.state.state.ui.security.dialog.as_ref().map(|d| &d.kind),
        Some(SecKind::Email { sent: true })
    ));
    rt.update(|a, _| {
        a.state
            .ui
            .security
            .dialog
            .as_mut()
            .unwrap()
            .code
            .replace_text(CODE);
        a.act(|s| s.sec_code_changed());
    });
    settle(&mut rt, 4);
    assert_eq!(
        rt.state.state.me().unwrap().email.as_deref(),
        Some("bruno@example.com")
    );
    assert!(!rt.state.state.wants_email());
    assert!(!has(&rt, "email-banner"));
    assert_eq!(
        sent(
            &fake,
            |c| matches!(c, Command::ChangeEmail { email, password } if email == "bruno@example.com" && password.is_empty())
        ),
        1
    );
}

#[test]
fn the_email_banner_can_be_dismissed_and_others_never_see_it() {
    let (mut app, _) = booted_as(true, "bruno");
    app.act(|s| s.dismiss_email_banner());
    assert!(!app.state.wants_email());
    let (app, _) = booted(true);
    assert!(!app.state.wants_email(), "tim has an email");
}

// ---- the Security page ---------------------------------------------------------------------------

fn security() -> (App, FakeBackend) {
    let (mut app, fake) = booted(true);
    app.act(|s| s.open_security());
    app.pump();
    (app, fake)
}

#[test]
fn the_security_page_lists_passkeys_and_devices_with_this_one_marked() {
    let (app, fake) = security();
    let mut rt = runtime(app);
    settle(&mut rt, 4);
    assert_eq!(sent(&fake, |c| matches!(c, Command::LoadSecurity)), 1);
    assert!(has(&rt, "page-security"));
    assert!(has(&rt, "passkey-1"));
    assert!(has(&rt, "session-1"));
    assert!(has(&rt, "session-current"));
    assert!(
        !has(&rt, "session-revoke-1"),
        "this device has no sign-out button"
    );
    assert!(has(&rt, "session-revoke-2"));
    assert_eq!(rt.state.state.ui.security.totp, Some(false));
    // Whether an authenticator is on comes from the server's summary: nothing is probed.
    assert_eq!(sent(&fake, |c| matches!(c, Command::BeginTotpSetup)), 0);
    let summary = rt.state.state.ui.security.info.as_ref().unwrap().summary;
    assert!(summary.has_password && summary.email_verified && !summary.totp_enabled);
    assert_eq!(summary.passkey_count, 1);
}

#[test]
fn turning_on_two_factor_shows_the_recovery_codes_only_once() {
    let (mut app, fake) = security();
    app.act(|s| s.open_sec(SecKind::TotpSetup { setup: None }));
    app.pump();
    let Some(SecKind::TotpSetup { setup: Some(setup) }) = app
        .state
        .ui
        .security
        .dialog
        .as_ref()
        .map(|d| d.kind.clone())
    else {
        panic!()
    };
    assert!(setup.otpauth_uri.starts_with("otpauth://totp/"));
    // A wrong code first.
    app.state
        .ui
        .security
        .dialog
        .as_mut()
        .unwrap()
        .code
        .replace_text("000000");
    app.act(|s| s.sec_code_changed());
    app.pump();
    assert!(app
        .state
        .ui
        .security
        .dialog
        .as_ref()
        .unwrap()
        .error
        .as_deref()
        .unwrap()
        .starts_with("That code didn't work"));
    assert!(
        app.state.ui.security.dialog.as_ref().unwrap().error.as_deref().unwrap().ends_with("9 tries left."),
        "the server's count of checks left"
    );
    assert!(app.state.session.is_some(), "a wrong code never ends the session");
    app.state
        .ui
        .security
        .dialog
        .as_mut()
        .unwrap()
        .code
        .replace_text(CODE);
    app.act(|s| s.sec_code_changed());
    app.pump();
    assert_eq!(sent(&fake, |c| matches!(c, Command::ConfirmTotp { .. })), 2);
    let Some(SecKind::RecoveryCodes { codes }) = app
        .state
        .ui
        .security
        .dialog
        .as_ref()
        .map(|d| d.kind.clone())
    else {
        panic!()
    };
    assert_eq!(codes.len(), 10);
    assert_eq!(app.state.ui.security.totp, Some(true));
    // It doesn't close until "I saved them" is ticked.
    app.act(|s| s.close_sec());
    app.act(|s| s.submit_sec());
    assert!(app.state.ui.security.dialog.is_some());
    app.act(|s| s.toggle_saved());
    app.act(|s| s.submit_sec());
    assert!(app.state.ui.security.dialog.is_none());
    // Once closed the codes are nowhere in the app any more.
    let mut rt = runtime(app);
    settle(&mut rt, 4);
    assert!(!rt
        .elements()
        .iter()
        .any(|e| e.label.as_deref().is_some_and(|l| l.contains(&codes[0]))));
    assert!(!has(&rt, "recovery-codes"));
}

#[test]
fn every_security_action_sends_its_command() {
    let (mut app, fake) = security();
    // Password: checked first, then sent.
    app.act(|s| s.open_sec(SecKind::Password));
    {
        let d = app.state.ui.security.dialog.as_mut().unwrap();
        d.first.replace_text("hunter22");
        d.second.replace_text("a better password");
        d.third.replace_text("a different one");
    }
    app.act(|s| s.submit_sec());
    assert_eq!(
        app.state
            .ui
            .security
            .dialog
            .as_ref()
            .unwrap()
            .error
            .as_deref(),
        Some("The new passwords don't match.")
    );
    app.state
        .ui
        .security
        .dialog
        .as_mut()
        .unwrap()
        .third
        .replace_text("a better password");
    app.act(|s| s.submit_sec());
    app.pump();
    assert!(app.state.ui.security.dialog.is_none());
    assert_eq!(
        app.state.other_sessions().len(),
        0,
        "the other devices were signed out"
    );
    // A wrong current password.
    app.act(|s| s.open_sec(SecKind::Password));
    {
        let d = app.state.ui.security.dialog.as_mut().unwrap();
        d.first.replace_text("wrong");
        d.second.replace_text("a better password");
        d.third.replace_text("a better password");
    }
    app.act(|s| s.submit_sec());
    app.pump();
    assert_eq!(
        app.state
            .ui
            .security
            .dialog
            .as_ref()
            .unwrap()
            .error
            .as_deref(),
        Some("That password isn't right.")
    );
    app.act(|s| s.close_sec());
    // Email change: an address someone else has is refused.
    app.act(|s| s.open_sec(SecKind::Email { sent: false }));
    app.state
        .ui
        .security
        .dialog
        .as_mut()
        .unwrap()
        .first
        .replace_text("ana@example.com");
    app.act(|s| s.submit_sec());
    app.pump();
    assert_eq!(
        app.state
            .ui
            .security
            .dialog
            .as_ref()
            .unwrap()
            .error
            .as_deref(),
        Some("Another account uses that email.")
    );
    app.state
        .ui
        .security
        .dialog
        .as_mut()
        .unwrap()
        .first
        .replace_text("tim@new.example");
    app.act(|s| s.submit_sec());
    app.pump();
    app.state
        .ui
        .security
        .dialog
        .as_mut()
        .unwrap()
        .code
        .replace_text(CODE);
    app.act(|s| s.submit_sec());
    app.pump();
    assert_eq!(
        app.state.me().unwrap().email.as_deref(),
        Some("tim@new.example")
    );
    // Passkeys: rename, delete, add (the browser ceremony).
    app.act(|s| s.open_sec(SecKind::RenamePasskey { id: 1 }));
    assert_eq!(
        app.state.ui.security.dialog.as_ref().unwrap().first.text,
        "MacBook Touch ID"
    );
    app.state
        .ui
        .security
        .dialog
        .as_mut()
        .unwrap()
        .first
        .replace_text("Work laptop");
    app.act(|s| s.submit_sec());
    app.pump();
    assert_eq!(app.state.passkey(1).unwrap().name, "Work laptop");
    app.act(|s| s.open_sec(SecKind::DeletePasskey { id: 1 }));
    app.act(|s| s.submit_sec());
    app.pump();
    assert!(app.state.passkey(1).is_none());
    app.act(|s| s.open_sec(SecKind::AddPasskey { waiting: false }));
    app.state
        .ui
        .security
        .dialog
        .as_mut()
        .unwrap()
        .first
        .replace_text("Phone");
    app.act(|s| s.submit_sec());
    app.pump();
    assert!(matches!(
        app.state.ui.security.dialog.as_ref().map(|d| &d.kind),
        Some(SecKind::AddPasskey { waiting: true })
    ));
    app.state.clock = 10.0;
    app.pump();
    assert!(app.state.ui.security.dialog.is_none());
    assert_eq!(
        app.state
            .ui
            .security
            .info
            .as_ref()
            .unwrap()
            .passkeys
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Phone"]
    );
    // Two-factor: on (already proven), then new codes and off.
    app.act(|s| s.open_sec(SecKind::TotpSetup { setup: None }));
    app.pump();
    app.state
        .ui
        .security
        .dialog
        .as_mut()
        .unwrap()
        .code
        .replace_text(CODE);
    app.act(|s| s.submit_sec());
    app.pump();
    app.state.ui.security.dialog.as_mut().unwrap().saved = true;
    app.act(|s| s.close_sec());
    app.act(|s| s.open_sec(SecKind::Regenerate));
    app.state
        .ui
        .security
        .dialog
        .as_mut()
        .unwrap()
        .code
        .replace_text(CODE);
    app.act(|s| s.submit_sec());
    app.pump();
    assert!(matches!(
        app.state.ui.security.dialog.as_ref().map(|d| &d.kind),
        Some(SecKind::RecoveryCodes { .. })
    ));
    app.state.ui.security.dialog.as_mut().unwrap().saved = true;
    app.act(|s| s.close_sec());
    app.act(|s| s.open_sec(SecKind::DisableTotp));
    app.state
        .ui
        .security
        .dialog
        .as_mut()
        .unwrap()
        .code
        .replace_text(CODE);
    app.act(|s| s.submit_sec());
    app.pump();
    assert_eq!(app.state.ui.security.totp, Some(false));
    for (what, n) in [
        (
            "ChangePassword",
            sent(&fake, |c| matches!(c, Command::ChangePassword { .. })),
        ),
        (
            "ChangeEmail",
            sent(&fake, |c| matches!(c, Command::ChangeEmail { .. })),
        ),
        (
            "ConfirmEmailChange",
            sent(&fake, |c| matches!(c, Command::ConfirmEmailChange { .. })),
        ),
        (
            "RenamePasskey",
            sent(&fake, |c| matches!(c, Command::RenamePasskey { id: 1, .. })),
        ),
        (
            "DeletePasskey",
            sent(&fake, |c| matches!(c, Command::DeletePasskey { id: 1 })),
        ),
        (
            "BeginPasskeyRegistration",
            sent(
                &fake,
                |c| matches!(c, Command::BeginPasskeyRegistration { name } if name == "Phone"),
            ),
        ),
        (
            "RegenerateRecoveryCodes",
            sent(&fake, |c| {
                matches!(c, Command::RegenerateRecoveryCodes { .. })
            }),
        ),
        (
            "DisableTotp",
            sent(&fake, |c| matches!(c, Command::DisableTotp { .. })),
        ),
    ] {
        assert!(n >= 1, "{what} was sent");
    }
}

#[test]
fn devices_are_signed_out_one_by_one_or_all_at_once() {
    let (app, fake) = security();
    let mut rt = runtime(app);
    settle(&mut rt, 4);
    click(&mut rt, "session-revoke-2");
    settle(&mut rt, 4);
    assert!(!has(&rt, "session-2"));
    assert_eq!(rt.state.state.other_sessions().len(), 1);
    click(&mut rt, "sec-sessions-revoke-others");
    settle(&mut rt, 4);
    assert!(rt.state.state.other_sessions().is_empty());
    assert!(has(&rt, "session-1"), "this device stays");
    assert_eq!(
        sent(&fake, |c| matches!(c, Command::RevokeSession { id: 2 })),
        1
    );
    assert_eq!(
        sent(&fake, |c| matches!(c, Command::RevokeOtherSessions)),
        1
    );
}

// ---- inviting by email ---------------------------------------------------------------------------

#[test]
fn invite_by_email_is_offered_only_to_those_who_may_invite() {
    // Tim owns The Hideout.
    let (app, _) = booted(true);
    assert!(app.state.can_invite(HIDEOUT));
    let mut rt = runtime(app);
    click(&mut rt, "guild-header");
    settle(&mut rt, 6);
    assert!(rt
        .elements()
        .iter()
        .any(|e| e.label.as_deref() == Some("Invite people by email\u{2026}")));
    // Ana is a plain member there: no item...
    let (mut app, _) = booted_as(true, "ana");
    assert!(!app.state.can_invite(HIDEOUT));
    let mut rt = runtime(booted_as(true, "ana").0);
    click(&mut rt, "guild-header");
    settle(&mut rt, 6);
    assert!(!rt
        .elements()
        .iter()
        .any(|e| e.label.as_deref() == Some("Invite people by email\u{2026}")));
    // ...until one of her roles has INVITE_PEOPLE.
    let roles = app.state.roles_of(HIDEOUT).to_vec();
    let mods = roles.iter().find(|r| r.id == HIDEOUT_MODS).unwrap().clone();
    let me = app.state.my_id();
    app.state
        .data
        .member_roles
        .entry(HIDEOUT)
        .or_default()
        .insert(me, vec![HIDEOUT_MODS]);
    let with = roles.into_iter().map(|mut r| {
        if r.id == mods.id {
            r.permissions = Permissions(r.permissions.bits() | Permissions::INVITE_PEOPLE.bits());
        }
        r
    });
    app.state.set_roles(HIDEOUT, with.collect(), None);
    assert!(app.state.can_invite(HIDEOUT));
}

#[test]
fn inviting_by_email_shows_the_code_link_and_the_pending_invites() {
    let (mut app, fake) = booted(true);
    app.act(|s| s.open_dialog(DialogKind::InviteByEmail { guild: HIDEOUT }));
    app.pump();
    assert_eq!(app.state.ui.invites.list.len(), 4, "this server's invites");
    // Not an email.
    app.state
        .ui
        .dialog
        .as_mut()
        .unwrap()
        .input
        .replace_text("dana");
    app.act(|s| s.submit_dialog());
    assert_eq!(
        app.state.ui.dialog.as_ref().unwrap().error.as_deref(),
        Some("Enter their email address")
    );
    // Someone who already has an account.
    app.state
        .ui
        .dialog
        .as_mut()
        .unwrap()
        .input
        .replace_text("ana@example.com");
    app.act(|s| s.submit_dialog());
    app.pump();
    assert_eq!(
        app.state.ui.dialog.as_ref().unwrap().error.as_deref(),
        Some("Someone with that email already has an account")
    );
    app.state
        .ui
        .dialog
        .as_mut()
        .unwrap()
        .input
        .replace_text("dana@example.com");
    app.act(|s| s.submit_dialog());
    app.pump();
    let created = app
        .state
        .ui
        .invites
        .created
        .clone()
        .expect("the new invite");
    assert_eq!(created.email, "dana@example.com");
    assert!(created.code.is_some(), "its code is shown");
    assert!(created.link.as_deref().is_some_and(|l| l.contains("/invite/")), "and its link");
    assert!(
        app.state.ui.invites.list[0].code.is_none() && app.state.ui.invites.list[0].link.is_none(),
        "the list never keeps codes or links"
    );

    assert_eq!(
        sent(
            &fake,
            |c| matches!(c, Command::CreateInvite { invite } if invite.guild_id == Some(HIDEOUT) && invite.guild_ids.is_empty() && !invite.make_admin)
        ),
        2
    );
    // Resend and withdraw from the list.
    let id = created.id;
    app.act(|s| s.resend_invite(id));
    app.pump();
    assert!(app
        .state
        .ui
        .invites
        .created
        .as_ref()
        .unwrap()
        .code
        .as_deref()
        .unwrap()
        .starts_with("RSND"));
    assert!(
        app.state.ui.invites.created.as_ref().unwrap().link.as_deref().is_some_and(|l| l.ends_with("/invite/RSND-9005")),
        "a resend brings the new link"
    );
    app.act(|s| s.revoke_invite(id));
    app.pump();
    assert_eq!(
        app.state
            .ui
            .invites
            .list
            .iter()
            .find(|i| i.id == id)
            .unwrap()
            .status,
        InviteStatus::Revoked
    );
    // The last invite made or sent again stays on show: its link and code, each with Copy.
    let link = app.state.ui.invites.created.as_ref().unwrap().link.clone().unwrap();
    let mut rt = runtime(app);
    settle(&mut rt, 6);
    assert!(has(&rt, "invite-created-link"));
    assert!(rt.elements().iter().any(|e| e.label.as_deref() == Some(format!("Invite link {link}").as_str())));
    assert!(has(&rt, "invite-created-code-copy"));
    click(&mut rt, "invite-created-link-copy");
    settle(&mut rt, 2);
    assert!(rt.state.state.ui.toasts.iter().any(|t| t.message == "Invite link copied"));
}

#[test]
fn an_invite_accepted_elsewhere_leaves_the_open_dialogs_pending_list() {
    let (mut app, fake) = booted(true);
    app.act(|s| s.open_dialog(DialogKind::InviteByEmail { guild: HIDEOUT }));
    app.pump();
    let erin = |app: &App| app.state.ui.invites.list.iter().find(|i| i.email == "erin@example.com").map(|i| i.status);
    assert_eq!(erin(&app), Some(InviteStatus::Pending));
    let lists = |fake: &FakeBackend| sent(fake, |c| matches!(c, Command::ListInvites { guild: Some(HIDEOUT), .. }));
    assert_eq!(lists(&fake), 1, "asked once on open");
    // Erin signs up from the email: the gateway says she joined, and the list asks again.
    fake.invite_accepted_elsewhere("erin@example.com", 9100, "erin");
    app.pump();
    assert_eq!(lists(&fake), 2);
    assert_eq!(erin(&app), Some(InviteStatus::Accepted));
    assert!(app.state.data.users.contains_key(&9100), "the join itself still applies");
    // With the dialog closed nothing asks; opening it again does.
    app.act(|s| s.close_dialog());
    fake.gateway(thehideout_api::GatewayUpdate::Event(thehideout_api::Event::GuildMemberJoined {
        guild_id: HIDEOUT,
        user: fake.user(9100).unwrap(),
    }));
    app.pump();
    assert_eq!(lists(&fake), 2);
    app.act(|s| s.open_dialog(DialogKind::InviteByEmail { guild: HIDEOUT }));
    app.pump();
    assert_eq!(lists(&fake), 3);
    assert_eq!(erin(&app), Some(InviteStatus::Accepted));
}

#[test]
fn the_admin_invites_tab_asks_again_on_open_and_when_someone_joins() {
    let (mut app, fake) = booted_as(true, "cvetka");
    app.act(|s| s.open_admin());
    app.pump();
    app.act(|s| s.admin_page(AdminPage::Invites));
    app.pump();
    let erin = |app: &App| app.state.admin_invite_rows().iter().find(|i| i.email == "erin@example.com").map(|i| i.status);
    let lists = |fake: &FakeBackend| sent(fake, |c| matches!(c, Command::ListInvites { guild: None, .. }));
    assert_eq!(erin(&app), Some(InviteStatus::Pending));
    assert_eq!(lists(&fake), 1);
    // Away on another tab, the invite is accepted; coming back asks again.
    app.act(|s| s.admin_page(AdminPage::Users));
    app.pump();
    fake.invite_accepted_elsewhere("erin@example.com", 9100, "erin");
    app.pump();
    app.act(|s| s.admin_page(AdminPage::Invites));
    app.pump();
    assert_eq!(lists(&fake), 2);
    assert_eq!(erin(&app), Some(InviteStatus::Accepted));
    // Opening the tab again with nothing new still asks (invites change in other inboxes).
    app.act(|s| s.admin_page(AdminPage::Overview));
    app.pump();
    app.act(|s| s.admin_page(AdminPage::Invites));
    app.pump();
    assert_eq!(lists(&fake), 3);
    // While it is open, a join asks at once.
    fake.gateway(thehideout_api::GatewayUpdate::Event(thehideout_api::Event::GuildMemberJoined {
        guild_id: HIDEOUT,
        user: fake.user(9100).unwrap(),
    }));
    app.pump();
    assert_eq!(lists(&fake), 4);
    assert_eq!(app.state.admin_invite_rows().len(), 4, "the list is whole again");
}

// ---- the admin Invites tab -----------------------------------------------------------------------

#[test]
fn the_admin_invites_tab_lists_filters_creates_and_withdraws() {
    let (mut app, fake) = booted_as(true, "cvetka");
    app.act(|s| s.open_admin());
    app.pump();
    app.act(|s| s.admin_page(AdminPage::Invites));
    app.pump();
    assert_eq!(app.state.admin_invite_rows().len(), 4);
    assert_eq!(
        app.state.admin().unwrap().guild_name(HIDEOUT),
        Some("The Hideout"),
        "server names for the rows"
    );
    app.act(|s| s.filter_invites(Some(InviteStatus::Expired)));
    assert_eq!(app.state.admin_invite_rows().len(), 1);
    app.act(|s| s.filter_invites(None));
    // Create one for two servers, as a site admin.
    app.act(|s| s.open_invite_form());
    app.state
        .ui
        .admin
        .as_mut()
        .unwrap()
        .invite_form
        .as_mut()
        .unwrap()
        .email
        .replace_text("hana@example.com");
    app.act(|s| {
        s.toggle_invite_guild(HIDEOUT);
        s.toggle_invite_guild(2);
        s.toggle_invite_admin();
        s.submit_invite_form();
    });
    app.pump();
    let ad = app.state.admin().unwrap();
    assert!(ad.invite_form.is_none());
    assert_eq!(
        ad.invite_created.as_ref().unwrap().email,
        "hana@example.com"
    );
    assert!(ad.invite_created.as_ref().unwrap().link.is_some());
    assert_eq!(app.state.admin_invite_rows()[0].email, "hana@example.com");
    assert_eq!(
        sent(
            &fake,
            |c| matches!(c, Command::CreateInvite { invite } if invite.guild_id.is_none() && invite.guild_ids == vec![HIDEOUT, 2] && invite.make_admin)
        ),
        1
    );
    let id = app.state.admin_invite_rows()[0].id;
    app.act(|s| s.admin_revoke_invite(id));
    app.pump();
    assert_eq!(
        app.state.admin_invite_rows()[0].status,
        InviteStatus::Revoked
    );
    // Through the view: the tab, a row and the create button.
    let mut rt = runtime(app);
    settle(&mut rt, 4);
    assert!(has(&rt, "admin-nav-Invites"));
    assert!(has(&rt, &format!("invite-row-{id}")));
    assert!(has(&rt, "admin-invite-copy-link"), "the new invite's link can be copied");
    click(&mut rt, "admin-invite-create");
    settle(&mut rt, 6);
    assert!(has(&rt, "admin-invite-form"));
}

#[test]
fn a_member_may_not_list_every_invite() {
    let (mut app, fake) = booted(true);
    app.act(|s| {
        s.outbox.push(Command::ListInvites {
            epoch: 1,
            guild: None,
            page_token: None,
            limit: 10,
        })
    });
    app.pump();
    assert_eq!(
        sent(&fake, |c| matches!(
            c,
            Command::ListInvites { guild: None, .. }
        )),
        1
    );
    // Answered PermissionDenied with no admin page open: nothing changes and nothing breaks.
    assert!(app.state.ui.admin.is_none());
}

// ---- the role editor -----------------------------------------------------------------------------

#[test]
fn the_role_editor_has_an_invite_people_switch() {
    let (mut app, fake) = booted(true);
    app.act(|s| {
        s.open_server_settings(HIDEOUT);
        s.server_page(ServerPage::Roles);
        s.select_role(HIDEOUT_MODS);
    });
    app.pump();
    let mut rt = runtime(app);
    settle(&mut rt, 4);
    assert!(has(&rt, "perm-invite-people"));
    click(&mut rt, "perm-invite-people");
    settle(&mut rt, 2);
    rt.update(|a, _| a.act(|s| s.save_role()));
    settle(&mut rt, 4);
    assert!(fake.sent().iter().any(|c| matches!(c, Command::UpdateRole { role, permissions: Some(p), .. } if *role == HIDEOUT_MODS && p.contains(Permissions::INVITE_PEOPLE))));
}

#[test]
fn a_link_the_system_opens_the_app_with_goes_to_the_invite() {
    for link in ["thehideout://invite/WXYZ-2345", "https://hideout.example/invite/wxyz-2345"] {
        let (mut app, _) = signed_out();
        app.act(|s| s.open_link(link));
        app.pump();
        assert!(matches!(app.state.auth.step, AuthStep::InviteAccept { .. }), "{link}");
        assert_eq!(app.state.auth.invite.text, "WXYZ2345", "{link}");
    }
    // A link without a whole code is ignored.
    for link in ["thehideout://invite/WXYZ-23", "thehideout://invite/", "https://hideout.example/other/WXYZ-2345"] {
        let (mut app, fake) = signed_out();
        app.act(|s| s.open_link(link));
        app.pump();
        assert_eq!(app.state.auth.step, AuthStep::Password, "{link}");
        assert_eq!(sent(&fake, |c| matches!(c, Command::LookupInvite { .. })), 0, "{link}");
    }
    // Signed in, a link changes nothing.
    let (mut app, fake) = booted(true);
    app.act(|s| s.open_link("thehideout://invite/WXYZ-2345"));
    assert_eq!(
        sent(&fake, |c| matches!(c, Command::LookupInvite { .. })),
        0
    );
}
