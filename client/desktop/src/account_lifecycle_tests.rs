//! The account lifecycle as state, against the fake backend: the terms prompt and the invite's
//! terms and age fields, deleting your account (and why it can be refused), exporting your data,
//! the admin's suspend and delete, and a deleted user showing as "Deleted user" everywhere.

use std::time::{Duration, SystemTime};

use thehideout_api::{ApiError, Event, GatewayUpdate, OwnedGuild, User};
use thehideout_design::components::ToastKind;

use crate::admin::AdminPage;
use crate::app::App;
use crate::backend::{Backend, Command};
use crate::fake::account::{CODE, INVITE_PENDING, TERMS_VERSION};
use crate::fake::{ANA, BRUNO, CVETKA, DM_ANA, FakeBackend, TIM};
use crate::forms::{AuthField, AuthStep, auth_error, suspended_text};
use crate::lifecycle::{DELETED_USER, lifecycle_error_text};
use crate::scenes::{booted, booted_as};
use crate::state::{AppState, AuthMode, Screen};

use crate::app::fake_now;
use crate::format::Tz;

fn sent(fake: &FakeBackend, pick: impl Fn(&Command) -> bool) -> usize {
    fake.sent().iter().filter(|c| pick(c)).count()
}

fn toast(app: &App) -> Option<(ToastKind, String)> {
    app.state.ui.toasts.back().map(|t| (t.kind, t.message.clone()))
}

// ---- terms -----------------------------------------------------------------------------------

#[test]
fn the_terms_prompt_is_due_only_after_the_server_says_so_and_accepting_clears_it() {
    let (mut app, fake) = booted(true);
    app.act(|s| s.check_terms());
    app.pump();
    assert!(!app.state.terms_prompt_due(), "accepted terms: no prompt");
    assert_eq!(app.state.ui.lifecycle.terms_current.as_deref(), Some(TERMS_VERSION));

    fake.outdate_terms();
    app.act(|s| s.check_terms());
    app.pump();
    assert!(app.state.terms_prompt_due());
    assert_eq!(app.state.ui.lifecycle.terms_update.as_deref(), Some(TERMS_VERSION));

    app.act(|s| s.accept_terms());
    assert!(app.state.ui.lifecycle.terms_busy);
    // A second press while busy sends nothing more.
    app.act(|s| s.accept_terms());
    app.pump();
    assert_eq!(sent(&fake, |c| matches!(c, Command::AcceptTerms { version } if version == TERMS_VERSION)), 1);
    assert!(!app.state.terms_prompt_due() && !app.state.ui.lifecycle.terms_busy);
    // Nothing to accept: the action does nothing.
    app.act(|s| s.accept_terms());
    assert_eq!(sent(&fake, |c| matches!(c, Command::AcceptTerms { .. })), 1);
}

#[test]
fn a_stale_terms_version_looks_again() {
    let (mut app, fake) = booted(true);
    app.state.ui.lifecycle.terms_update = Some("1999-01-01".into());
    app.act(|s| s.accept_terms());
    app.pump();
    assert!(app.state.ui.lifecycle.terms_error.as_deref().unwrap().contains(TERMS_VERSION));
    assert_eq!(sent(&fake, |c| matches!(c, Command::CheckTerms)), 1, "asked the server again");
}

fn invite_form() -> (App, FakeBackend) {
    let fake = FakeBackend::new();
    let mut app = App::new(Box::new(fake.clone()), AppState::new(fake_now, Tz::Fixed(0)));
    app.act(|s| s.set_auth_mode(AuthMode::Invite));
    app.state.auth.invite.replace_text(INVITE_PENDING);
    app.act(|s| s.submit_auth());
    app.pump();
    assert!(matches!(app.state.auth.step, AuthStep::InviteAccept { .. }));
    app.state.auth.username.replace_text("erin");
    app.state.auth.password.replace_text("correct horse");
    (app, fake)
}

#[test]
fn accepting_an_invite_sends_the_terms_version_and_the_age_confirmation() {
    let (mut app, fake) = invite_form();
    let AuthStep::InviteAccept { preview, .. } = &app.state.auth.step else { unreachable!() };
    assert_eq!((preview.terms_version.as_str(), preview.minimum_age), (TERMS_VERSION, 16));
    // The boxes start unticked: nothing is sent, and the form says what is missing.
    assert!(!app.state.auth.accept_terms && !app.state.auth.confirm_age);
    app.act(|s| s.submit_auth());
    assert_eq!(sent(&fake, |c| matches!(c, Command::AcceptInvite { .. })), 0);
    assert_eq!(app.state.auth.error_for(AuthField::Form), Some("Accept the terms and privacy notice to continue."));
    app.state.auth.accept_terms = true;
    app.act(|s| s.submit_auth());
    assert_eq!(app.state.auth.error_for(AuthField::Form), Some("Confirm that you are at least 16 years old."));
    assert_eq!(sent(&fake, |c| matches!(c, Command::AcceptInvite { .. })), 0);

    app.state.auth.confirm_age = true;
    app.act(|s| s.submit_auth());
    app.pump();
    assert_eq!(
        sent(&fake, |c| matches!(c, Command::AcceptInvite { terms_version, age_confirmed: true, .. } if terms_version == TERMS_VERSION)),
        1
    );
    assert_eq!(app.state.screen, Screen::Main);
}

#[test]
fn the_fake_server_refuses_an_out_of_date_terms_version() {
    let (mut app, _) = invite_form();
    app.state.auth.accept_terms = true;
    app.state.auth.confirm_age = true;
    if let AuthStep::InviteAccept { preview, .. } = &mut app.state.auth.step {
        preview.terms_version = "1999-01-01".into();
    }
    app.act(|s| s.submit_auth());
    app.pump();
    assert_eq!(app.state.screen, Screen::Auth);
    assert_eq!(app.state.auth.error_for(AuthField::Form), Some("Accept the terms and privacy notice to continue."));
}

// ---- suspension on the sign-in form ----------------------------------------------------------

#[test]
fn a_suspended_sign_in_says_why_and_until_when() {
    let until = SystemTime::UNIX_EPOCH + Duration::from_secs(1_798_761_600); // 2027-01-01
    let e = ApiError::Suspended { reason: "spamming".into(), until: Some(until) };
    let text = auth_error(&e).message;
    assert_eq!(text, "Your account is suspended. Reason: Spamming. It ends on 2027-01-01.");
    assert_eq!(suspended_text("", None), "Your account is suspended. It has no end date; contact an admin.");
    assert_eq!(auth_error(&e).field, AuthField::Form);
}

// ---- deleting your account -------------------------------------------------------------------

#[test]
fn owning_a_server_blocks_deletion_and_names_it() {
    let (mut app, fake) = booted(true); // tim owns The Hideout
    app.act(|s| s.delete_account("hunter22", "", ""));
    assert!(app.state.ui.lifecycle.delete_busy);
    app.pump();
    let lc = &app.state.ui.lifecycle;
    assert!(!lc.delete_busy);
    assert!(lc.owned_guilds.iter().any(|g| g.name == "The Hideout"), "{:?}", lc.owned_guilds);
    assert!(lc.delete_error.as_deref().unwrap().contains("The Hideout"));
    assert_eq!(app.state.screen, Screen::Main, "still signed in");
    assert_eq!(sent(&fake, |c| matches!(c, Command::DeleteAccount { password: Some(p), email_code: None, totp_code: None } if p == "hunter22")), 1);
}

#[test]
fn deleting_checks_the_password_then_signs_out() {
    let (mut app, fake) = booted_as(true, "cvetka"); // owns no server
    app.act(|s| s.delete_account("wrong", "", ""));
    app.pump();
    assert_eq!(app.state.ui.lifecycle.delete_error.as_deref(), Some("That password is wrong."));
    assert_eq!(app.state.screen, Screen::Main);
    // A second press while one is pending sends nothing.
    app.act(|s| s.delete_account("hunter22", "", ""));
    app.act(|s| s.delete_account("hunter22", "", ""));
    app.pump();
    assert_eq!(sent(&fake, |c| matches!(c, Command::DeleteAccount { .. })), 2);
    assert_eq!(app.state.screen, Screen::Auth);
    assert_eq!(app.state.auth.notice.as_deref(), Some("Your account was deleted."));
    assert!(sent(&fake, |c| matches!(c, Command::SignOut)) >= 1, "the saved session is dropped");
    assert!(app.state.me().is_none());
}

#[test]
fn the_emailed_code_and_the_authenticator_code_are_sent_when_given() {
    let (mut app, fake) = booted_as(true, "cvetka");
    fake.enable_totp("cvetka");
    app.act(|s| s.request_deletion_code());
    app.pump();
    assert!(app.state.ui.lifecycle.deletion_code_sent);
    // With an authenticator app on: without its code the server refuses.
    app.act(|s| s.delete_account("", CODE, ""));
    app.pump();
    assert!(app.state.ui.lifecycle.delete_error.is_some());
    assert_eq!(app.state.screen, Screen::Main);
    app.act(|s| s.delete_account("", CODE, "000000"));
    app.pump();
    assert_eq!(app.state.ui.lifecycle.delete_error.as_deref(), Some("That code is wrong. 9 tries left."));
    app.act(|s| s.delete_account("", CODE, CODE));
    app.pump();
    assert_eq!(app.state.screen, Screen::Auth);
    assert_eq!(
        sent(&fake, |c| matches!(c, Command::DeleteAccount { password: None, email_code: Some(_), totp_code: Some(t) } if t == CODE)),
        1
    );
}

#[test]
fn error_texts_read_well() {
    assert_eq!(lifecycle_error_text(&ApiError::WrongCode { attempts_remaining: 1 }), "That code is wrong. 1 try left.");
    assert_eq!(lifecycle_error_text(&ApiError::CodeExpired), "That code expired. Ask for a new one.");
    let owns = ApiError::OwnsGuilds(vec![
        OwnedGuild { id: 1, name: "den".into(), member_count: 2 },
        OwnedGuild { id: 2, name: "lab".into(), member_count: 1 },
    ]);
    assert_eq!(lifecycle_error_text(&owns), "You still own den, lab. Transfer or delete them first.");
}

// ---- exporting your data ---------------------------------------------------------------------

#[test]
fn export_is_queued_once_a_day() {
    let (mut app, fake) = booted(true);
    app.act(|s| s.export_data());
    assert!(app.state.ui.lifecycle.export_busy);
    app.act(|s| s.export_data());
    app.pump();
    assert_eq!(sent(&fake, |c| matches!(c, Command::ExportData)), 1);
    assert!(app.state.ui.lifecycle.export_sent_to.as_deref().unwrap().contains("***@"));
    app.act(|s| s.export_data());
    app.pump();
    assert_eq!(app.state.ui.lifecycle.export_sent_to, None);
    assert_eq!(app.state.ui.lifecycle.export_error.as_deref(), Some("You can export your data once every 24 hours."));
}

#[test]
fn export_needs_an_email_address() {
    let (mut app, _) = booted_as(true, "bruno"); // a legacy account without an email
    app.act(|s| s.export_data());
    app.pump();
    assert!(app.state.ui.lifecycle.export_error.as_deref().unwrap().contains("email"));
}

// ---- admin -----------------------------------------------------------------------------------

fn admin_users() -> (App, FakeBackend) {
    let (mut app, fake) = booted_as(true, "cvetka");
    app.act(|s| s.open_admin());
    app.act(|s| s.admin_page(AdminPage::Users));
    app.pump();
    assert!(app.state.admin().unwrap().users.items.iter().any(|r| r.user.id == ANA), "users loaded");
    (app, fake)
}

fn row(app: &App, id: u64) -> thehideout_api::AdminUserRow {
    app.state.admin().unwrap().users.items.iter().find(|r| r.user.id == id).cloned().expect("row")
}

#[test]
fn an_admin_suspends_and_unsuspends_a_user() {
    let (mut app, fake) = admin_users();
    let until = SystemTime::now() + Duration::from_secs(3600);
    app.act(|s| s.admin_suspend(BRUNO, "  spamming ", Some(until)));
    assert!(app.state.admin().unwrap().busy_users.contains(&BRUNO));
    // Busy: a second call is ignored.
    app.act(|s| s.admin_suspend(BRUNO, "again", None));
    app.pump();
    assert_eq!(sent(&fake, |c| matches!(c, Command::AdminSuspendUser { user, reason, .. } if *user == BRUNO && reason == "spamming")), 1);
    let r = row(&app, BRUNO);
    assert!(r.suspended && r.suspended_reason == "spamming" && r.suspended_until.is_some());
    assert_eq!(toast(&app).map(|t| t.0), Some(ToastKind::Success));
    assert!(fake.admin_audit().iter().any(|(a, t, _)| a == "user.suspend" && *t == format!("user:{BRUNO}")));

    app.act(|s| s.admin_unsuspend(BRUNO));
    app.pump();
    assert!(!row(&app, BRUNO).suspended);
    assert!(fake.admin_audit().iter().any(|(a, ..)| a == "user.unsuspend"));
    // Not yourself, not admins.
    app.act(|s| s.admin_suspend(CVETKA, "x", None));
    assert_eq!(sent(&fake, |c| matches!(c, Command::AdminSuspendUser { .. })), 1);
}

#[test]
fn suspending_an_admin_is_refused_and_says_so() {
    let (mut app, _) = admin_users();
    let other_admin = app
        .state
        .admin()
        .unwrap()
        .users
        .items
        .iter()
        .find(|r| r.user.is_admin && r.user.id != CVETKA)
        .map(|r| r.user.id)
        .expect("another admin");
    app.act(|s| s.admin_suspend(other_admin, "x", None));
    app.pump();
    let msg = app.state.admin().unwrap().user_errors.get(&other_admin).cloned().unwrap();
    assert!(msg.to_lowercase().contains("site admins cannot be suspended"), "{msg}");
    assert!(!row(&app, other_admin).suspended);
}

#[test]
fn an_admin_deletes_a_user_and_everyone_sees_deleted_user() {
    let (mut app, fake) = admin_users();
    // tim owns The Hideout: refused without force_transfer, and the guilds are named.
    app.act(|s| s.admin_delete_user(TIM, false, "abuse"));
    app.pump();
    let msg = app.state.admin().unwrap().user_errors.get(&TIM).cloned().unwrap();
    assert!(msg.contains("The Hideout"), "{msg}");
    assert!(!row(&app, TIM).deleted);

    app.act(|s| s.admin_delete_user(TIM, true, "abuse"));
    app.pump();
    let r = row(&app, TIM);
    assert!(r.deleted && r.user.deleted && r.user.username == DELETED_USER);
    assert!(app.state.admin().unwrap().user_errors.get(&TIM).is_none());
    assert!(fake.admin_audit().iter().any(|(a, t, reason)| a == "account.delete" && *t == format!("user:{TIM}") && reason == "abuse"));
    // The gateway event reached the rest of the app.
    assert_eq!(app.state.user_name(TIM), DELETED_USER);
    assert!(app.state.data.users.get(&TIM).is_some_and(|u| u.deleted));
}

#[test]
fn a_deleted_users_update_replaces_every_copy() {
    let (mut app, fake) = booted(true);
    let deleted = User { deleted: true, username: DELETED_USER.into(), avatar: None, email: None, is_admin: false, ..app.state.data.users.get(&ANA).cloned().expect("ana is known") };
    fake.gateway(GatewayUpdate::Event(Event::UserUpdated(deleted)));
    app.pump();
    assert_eq!(app.state.user_name(ANA), DELETED_USER);
    let dm = app.state.data.dms.iter().find(|d| d.id == DM_ANA).expect("the DM stays");
    assert!(dm.participants.iter().any(|p| p.id == ANA && p.deleted && p.username == DELETED_USER));
    // Messages keep their author id; the name resolves to the placeholder.
    assert_eq!(app.state.user_name(ANA), "Deleted user");
}
