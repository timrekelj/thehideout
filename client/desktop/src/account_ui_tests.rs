//! The account lifecycle's screens, driven the way a user does it (presses and typing on the
//! built tree, against the fake backend): the invite's terms and age boxes, the terms prompt, the
//! suspended card, the export button, the delete-account dialog and its variants, the admin's
//! suspend and delete dialogs, and a deleted user showing as "Deleted user".

use std::time::Duration;

use rune::Runtime;
use serde_json::Value;
use thehideout_design::components::avatar::DELETED_USER as DESIGN_DELETED_USER;

use crate::app::{fake_now, App};
use crate::backend::{Backend, Command};
use crate::fake::account::CODE;
use crate::fake::{FakeBackend, ANA, BRUNO, CVETKA, HIDEOUT, TIM};
use crate::forms::AuthStep;
use crate::lifecycle::DELETED_USER;
use crate::scenes::{booted, booted_as, SCENES};
use crate::server_settings::ServerPage;
use crate::state::{suspend_until, AdminUserDialog, Screen, SUSPEND_DURATIONS};
use crate::tests::{click, click_at, has, runtime, settle};

// ---- helpers ---------------------------------------------------------------------------------

fn scene_rt(name: &str) -> Runtime<App> {
    let (_, f) = SCENES
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("no scene {name}"));
    let (mut app, _fake) = booted(true);
    f(&mut app);
    let mut rt = runtime(app);
    settle(&mut rt, 30);
    rt
}

fn rt_of(app: App) -> Runtime<App> {
    let mut rt = runtime(app);
    settle(&mut rt, 30);
    rt
}

fn walk<'a>(n: &'a Value, out: &mut Vec<&'a Value>) {
    out.push(n);
    for c in n["children"].as_array().into_iter().flatten() {
        walk(c, out);
    }
}

fn find(rt: &Runtime<App>, pick: impl Fn(&Value) -> bool) -> Option<Value> {
    let tree = rt.accessibility_json();
    let mut all = Vec::new();
    walk(&tree, &mut all);
    all.into_iter().find(|n| pick(n)).cloned()
}

fn by_key(rt: &Runtime<App>, key: &str) -> Option<Value> {
    find(rt, |n| n["key"] == key)
}

fn named(rt: &Runtime<App>, role: &str, name: &str) -> Option<Value> {
    find(rt, |n| n["role"] == role && n["name"] == name)
}

fn has_text(rt: &Runtime<App>, needle: &str) -> bool {
    find(rt, |n| {
        n["name"].as_str().is_some_and(|s| s.contains(needle))
            || n["value"].as_str().is_some_and(|s| s.contains(needle))
    })
    .is_some()
}

fn disabled(n: &Value) -> bool {
    n["states"]
        .as_array()
        .is_some_and(|s| s.iter().any(|s| s == "disabled"))
}

fn checked(n: &Value) -> bool {
    n["states"]
        .as_array()
        .is_some_and(|s| s.iter().any(|s| s == "checked"))
}

/// Presses the control with this role and name (a checkbox has no key: its centre is clicked).
fn press_named(rt: &mut Runtime<App>, role: &str, name: &str) {
    let n = named(rt, role, name).unwrap_or_else(|| panic!("no {role} named {name}"));
    let b: Vec<f32> = n["bounds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap() as f32)
        .collect();
    click_at(rt, (b[0] + b[2] / 2.0, b[1] + b[3] / 2.0));
    settle(rt, 40);
}

fn press(rt: &mut Runtime<App>, key: &str) {
    click(rt, key);
    settle(rt, 40);
}

/// A press on something below the fold of the test window: the handler's own action instead.
fn act(rt: &mut Runtime<App>, f: impl FnOnce(&mut crate::state::AppState)) {
    rt.update(|a, _| a.act(f));
    settle(rt, 40);
}

/// Types into the focused field and lets the frame apply it.
fn type_text(rt: &mut Runtime<App>, text: &str) {
    crate::tests::type_text(rt, text);
    settle(rt, 3);
}

fn sent(fake: &FakeBackend, pick: impl Fn(&Command) -> bool) -> usize {
    fake.sent().iter().filter(|c| pick(c)).count()
}

const AGREE: &str = "I agree to the Terms and Privacy Policy";
const AGE: &str = "I'm 16 or older";

// ---- the invite's boxes ----------------------------------------------------------------------

#[test]
fn the_terms_and_age_boxes_gate_the_create_button() {
    let mut rt = scene_rt("auth-invite-accept");
    let submit = |rt: &Runtime<App>| by_key(rt, "auth-submit").expect("the button");
    assert!(disabled(&submit(&rt)), "nothing ticked");
    assert!(
        has_text(&rt, "Tick both boxes to create your account."),
        "the reason is said inline"
    );
    assert!(
        !checked(&named(&rt, "check-box", AGREE).unwrap())
            && !checked(&named(&rt, "check-box", AGE).unwrap())
    );
    // The age box carries the server's minimum.
    let AuthStep::InviteAccept { preview, .. } = &rt.state.state.auth.step else {
        panic!("the invite's account step")
    };
    assert_eq!(preview.minimum_age, 16);

    press_named(&mut rt, "check-box", AGREE);
    assert!(rt.state.state.auth.accept_terms && !rt.state.state.auth.confirm_age);
    assert!(disabled(&submit(&rt)), "one box is not enough");
    press_named(&mut rt, "check-box", AGE);
    assert!(rt.state.state.auth.confirm_age);
    assert!(!disabled(&submit(&rt)), "both ticked");
    assert!(
        !has_text(&rt, "Tick both boxes"),
        "the reason goes with the cause"
    );
    // Unticking one closes it again.
    press_named(&mut rt, "check-box", AGREE);
    assert!(disabled(&submit(&rt)));
}

#[test]
fn the_terms_and_privacy_names_are_links() {
    let rt = scene_rt("auth-invite-accept-terms");
    for (key, label) in [
        ("auth-agree-terms", "Terms"),
        ("auth-agree-privacy", "Privacy Policy"),
        ("auth-terms", "Terms"),
        ("auth-privacy", "Privacy"),
    ] {
        let n = by_key(&rt, key).unwrap_or_else(|| panic!("no link {key}"));
        assert_eq!(
            (n["role"].as_str(), n["name"].as_str()),
            (Some("link"), Some(label)),
            "{n}"
        );
    }
}

// ---- the terms prompt ------------------------------------------------------------------------

#[test]
fn the_terms_prompt_blocks_the_app_until_accepted() {
    let mut rt = scene_rt("terms-update");
    assert!(rt.state.state.terms_prompt_due());
    assert!(by_key(&rt, "terms-accept").is_some() && by_key(&rt, "terms-sign-out").is_some());
    assert!(has_text(&rt, "We updated our Terms and Privacy Policy"));
    assert!(
        !has(&rt, "rail-add") && !has(&rt, "home"),
        "no app behind the card"
    );
    press(&mut rt, "terms-accept");
    assert!(!rt.state.state.terms_prompt_due());
    assert!(has(&rt, "rail-add"), "the app is back");
}

#[test]
fn signing_out_of_the_terms_prompt_goes_to_the_auth_screen() {
    let mut rt = scene_rt("terms-update");
    press(&mut rt, "terms-sign-out");
    assert_eq!(rt.state.state.screen, Screen::Auth);
    assert!(!rt.state.state.terms_prompt_due());
    assert!(has(&rt, "auth-submit"));
}

// ---- suspended -------------------------------------------------------------------------------

#[test]
fn a_suspended_sign_in_shows_the_card_with_reason_and_end() {
    let mut rt = scene_rt("auth-suspended");
    assert!(has_text(&rt, "Your account is suspended"));
    assert!(has_text(&rt, "Spamming links in several servers"));
    assert!(has_text(&rt, "Your access is paused until"));
    assert!(has_text(&rt, "contact the server admin"));
    assert!(
        !has(&rt, "auth-username"),
        "the sign-in fields give way to the card"
    );
    press(&mut rt, "suspended-back");
    assert!(has(&rt, "auth-username") && !has(&rt, "suspended-title"));
}

#[test]
fn an_indefinite_suspension_says_so_and_a_live_session_ends_on_the_card() {
    let (mut app, fake) = booted(true);
    app.act(|s| s.session_suspended("".into(), None));
    app.pump();
    assert_eq!(app.state.screen, Screen::Auth);
    assert!(
        sent(&fake, |c| matches!(c, Command::SignOut)) >= 1,
        "the saved session is dropped"
    );
    let rt = rt_of(app);
    assert!(has_text(&rt, "Your account is suspended"));
    assert!(has_text(&rt, "indefinitely"));
    assert!(!has(&rt, "suspended-reason"), "no reason, no reason block");
}

// ---- export ----------------------------------------------------------------------------------

#[test]
fn the_export_button_goes_through_its_states() {
    let (mut app, fake) = booted(true);
    app.act(|s| s.open_security());
    app.pump();
    let mut rt = rt_of(app);
    let b = by_key(&rt, "account-export-button").expect("the button");
    assert_eq!(b["name"], "Export your data");
    assert!(!disabled(&b));
    act(&mut rt, |s| s.export_data());
    let b = by_key(&rt, "account-export-button").unwrap();
    assert_eq!(b["name"], "Export requested, check your email");
    assert!(disabled(&b), "one export a day");
    assert!(has_text(&rt, "***@"), "the masked address is named");
    assert_eq!(sent(&fake, |c| matches!(c, Command::ExportData)), 1);
}

#[test]
fn a_second_export_within_a_day_shows_the_limit() {
    let (mut app, _fake) = booted(true);
    app.act(|s| s.open_security());
    app.act(|s| s.export_data());
    app.pump();
    app.act(|s| s.export_data());
    app.pump();
    let rt = rt_of(app);
    assert!(has_text(
        &rt,
        "You can export your data once every 24 hours."
    ));
    assert_eq!(
        by_key(&rt, "account-export-button").unwrap()["name"],
        "Export your data"
    );
}

// ---- deleting your account -------------------------------------------------------------------

fn security_rt(who: &str) -> (Runtime<App>, FakeBackend) {
    let (mut app, fake) = booted_as(true, who);
    app.act(|s| s.open_security());
    app.pump();
    (rt_of(app), fake)
}

#[test]
fn deleting_with_a_password_needs_it_then_signs_out_with_a_notice() {
    let (mut rt, fake) = security_rt("cvetka");
    assert!(by_key(&rt, "account-delete").is_some_and(|b| b["name"] == "Delete account\u{2026}"));
    act(&mut rt, |s| s.open_delete_account());
    assert!(has(&rt, "delete-submit") && has_text(&rt, "Your password"));
    assert!(
        disabled(&by_key(&rt, "delete-submit").unwrap()),
        "nothing typed"
    );
    assert!(!has(&rt, "delete-totp") && !has(&rt, "delete-send-code"));
    type_text(&mut rt, "hunter22");
    settle(&mut rt, 3);
    assert!(!disabled(&by_key(&rt, "delete-submit").unwrap()));
    press(&mut rt, "delete-submit");
    assert_eq!(rt.state.state.screen, Screen::Auth);
    assert_eq!(
        rt.state.state.auth.notice.as_deref(),
        Some("Your account was deleted.")
    );
    assert!(has_text(&rt, "Your account was deleted."));
    assert_eq!(
        sent(
            &fake,
            |c| matches!(c, Command::DeleteAccount { password: Some(p), email_code: None, totp_code: None } if p == "hunter22")
        ),
        1
    );
}

#[test]
fn a_wrong_password_keeps_the_dialog_and_says_so() {
    let (mut rt, _fake) = security_rt("cvetka");
    assert!(by_key(&rt, "account-delete").is_some_and(|b| b["name"] == "Delete account\u{2026}"));
    act(&mut rt, |s| s.open_delete_account());
    type_text(&mut rt, "wrong");
    press(&mut rt, "delete-submit");
    assert_eq!(rt.state.state.screen, Screen::Main);
    assert!(has(&rt, "delete-submit"));
    assert!(has_text(&rt, "That password is wrong."));
}

#[test]
fn with_two_factor_on_the_authenticator_code_is_asked_too() {
    let (mut app, fake) = booted_as(true, "cvetka");
    fake.enable_totp("cvetka");
    app.act(|s| s.open_security());
    app.pump();
    let mut rt = rt_of(app);
    assert!(by_key(&rt, "account-delete").is_some_and(|b| b["name"] == "Delete account\u{2026}"));
    act(&mut rt, |s| s.open_delete_account());
    assert!(has(&rt, "delete-totp"));
    rt.update(|a, _| {
        a.state
            .ui
            .account
            .delete
            .as_mut()
            .unwrap()
            .password
            .replace_text("hunter22")
    });
    settle(&mut rt, 3);
    assert!(
        disabled(&by_key(&rt, "delete-submit").unwrap()),
        "the password alone is not enough"
    );
    rt.update(|a, _| {
        a.state
            .ui
            .account
            .delete
            .as_mut()
            .unwrap()
            .totp
            .replace_text(CODE)
    });
    settle(&mut rt, 3);
    assert!(!disabled(&by_key(&rt, "delete-submit").unwrap()));
    press(&mut rt, "delete-submit");
    assert_eq!(rt.state.state.screen, Screen::Auth);
    assert_eq!(
        sent(
            &fake,
            |c| matches!(c, Command::DeleteAccount { password: Some(_), totp_code: Some(t), .. } if t == CODE)
        ),
        1
    );
}

#[test]
fn an_account_without_a_password_confirms_with_an_emailed_code() {
    let (mut app, fake) = booted_as(true, "cvetka");
    app.act(|s| s.open_security());
    app.pump();
    // The fake's accounts all have a password; this one is shown as having none.
    app.state
        .ui
        .security
        .info
        .as_mut()
        .expect("loaded")
        .summary
        .has_password = false;
    let mut rt = rt_of(app);
    assert!(by_key(&rt, "account-delete").is_some_and(|b| b["name"] == "Delete account\u{2026}"));
    act(&mut rt, |s| s.open_delete_account());
    assert!(has(&rt, "delete-send-code") && !has_text(&rt, "Your password"));
    assert!(
        !has(&rt, "delete-email-code"),
        "the code cells come once a code was sent"
    );
    assert!(disabled(&by_key(&rt, "delete-submit").unwrap()));
    press(&mut rt, "delete-send-code");
    assert_eq!(
        sent(&fake, |c| matches!(c, Command::RequestAccountDeletionCode)),
        1
    );
    assert!(has(&rt, "delete-email-code"));
    rt.update(|a, _| {
        a.state
            .ui
            .account
            .delete
            .as_mut()
            .unwrap()
            .email_code
            .replace_text(CODE)
    });
    settle(&mut rt, 3);
    assert!(!disabled(&by_key(&rt, "delete-submit").unwrap()));
    press(&mut rt, "delete-submit");
    assert_eq!(rt.state.state.screen, Screen::Auth);
    assert_eq!(
        sent(
            &fake,
            |c| matches!(c, Command::DeleteAccount { password: None, email_code: Some(c), totp_code: None } if c == CODE)
        ),
        1
    );
}

#[test]
fn owning_a_server_lists_it_and_opens_its_overview() {
    let mut rt = scene_rt("delete-account-owns-guilds");
    assert!(has(&rt, "delete-owned") && has_text(&rt, "You still own 1 server"));
    assert!(has_text(&rt, "Transfer or delete these servers first."));
    assert_eq!(rt.state.state.screen, Screen::Main);
    press(&mut rt, &format!("delete-owned-open-{HIDEOUT}"));
    assert!(
        rt.state.state.ui.account.delete.is_none(),
        "the dialog gives way"
    );
    let ss = rt
        .state
        .state
        .ui
        .server_settings
        .as_ref()
        .expect("the server's settings");
    assert_eq!((ss.guild, ss.page), (HIDEOUT, ServerPage::Overview));
    assert!(has(&rt, "danger-zone"), "the Overview page's danger zone");
}

// ---- admin: suspend and delete ---------------------------------------------------------------

fn admin_rt() -> (Runtime<App>, FakeBackend) {
    let (mut app, fake) = booted_as(true, "cvetka");
    app.act(|s| s.open_admin());
    app.pump();
    app.act(|s| s.admin_page(crate::admin::AdminPage::Users));
    app.pump();
    (rt_of(app), fake)
}

#[test]
fn suspend_durations_map_to_the_right_end() {
    let now = fake_now();
    let day = Duration::from_secs(86_400);
    assert_eq!(
        SUSPEND_DURATIONS.map(|d| d.0),
        ["1 day", "7 days", "30 days", "Indefinitely"]
    );
    assert_eq!(suspend_until(now, 0), Some(now + day));
    assert_eq!(suspend_until(now, 1), Some(now + 7 * day));
    assert_eq!(suspend_until(now, 2), Some(now + 30 * day));
    assert_eq!(suspend_until(now, 3), None);
    assert_eq!(suspend_until(now, 9), None);
}

#[test]
fn the_suspend_dialog_sends_the_picked_duration_and_the_reason() {
    for (choice, days) in [(0usize, Some(1u64)), (1, Some(7)), (2, Some(30)), (3, None)] {
        let (mut rt, fake) = admin_rt();
        rt.update(|a, _| {
            a.act(|s| s.ask_admin_suspend(BRUNO));
            if let Some(AdminUserDialog::Suspend {
                duration, reason, ..
            }) = a.state.ui.account.admin_dialog.as_mut()
            {
                *duration = choice;
                reason.replace_text("  spamming ");
            }
        });
        settle(&mut rt, 6);
        assert!(has(&rt, "suspend-submit"));
        let before = (crate::app::fake_now(), crate::app::fake_now() + Duration::from_secs(5));
        press(&mut rt, "suspend-submit");
        let cmd = fake
            .sent()
            .into_iter()
            .find_map(|c| match c {
                Command::AdminSuspendUser {
                    user,
                    reason,
                    until,
                } if user == BRUNO => Some((reason, until)),
                _ => None,
            })
            .expect("the suspend command");
        assert_eq!(cmd.0, "spamming");
        match (days, cmd.1) {
            (None, None) => {}
            (Some(d), Some(at)) => {
                let want = Duration::from_secs(d * 86_400);
                assert!(
                    at >= before.0 + want - Duration::from_secs(5) && at <= before.1 + want,
                    "{choice}: {at:?}"
                );
            }
            other => panic!("{choice}: {other:?}"),
        }
        assert!(
            rt.state.state.ui.account.admin_dialog.is_none(),
            "{choice}: the dialog closed on success"
        );
        assert!(rt
            .state
            .state
            .admin()
            .unwrap()
            .users
            .items
            .iter()
            .any(|r| r.user.id == BRUNO && r.suspended));
    }
}

#[test]
fn the_row_menu_offers_suspend_delete_and_unsuspend() {
    let (mut rt, _fake) = admin_rt();
    press(&mut rt, &format!("user-actions-{ANA}"));
    assert!(
        named(&rt, "menu-item", "Suspend\u{2026}").is_some()
            && named(&rt, "menu-item", "Delete user\u{2026}").is_some()
    );
    assert!(named(&rt, "menu-item", "Unsuspend").is_none());
    press_named(&mut rt, "menu-item", "Suspend\u{2026}");
    assert!(has(&rt, "suspend-submit") && rt.state.state.ui.account.admin_menu.is_none());
    press(&mut rt, "suspend-submit");
    // Suspended: the menu says Unsuspend, which lifts it at once.
    press(&mut rt, &format!("user-actions-{ANA}"));
    assert!(named(&rt, "menu-item", "Suspend\u{2026}").is_none());
    press_named(&mut rt, "menu-item", "Unsuspend");
    assert!(
        !rt.state
            .state
            .admin()
            .unwrap()
            .users
            .items
            .iter()
            .find(|r| r.user.id == ANA)
            .unwrap()
            .suspended
    );
}

#[test]
fn admins_and_yourself_are_out_of_reach() {
    let (rt, _fake) = admin_rt();
    let me = by_key(&rt, &format!("user-actions-{CVETKA}")).expect("the button is there");
    assert!(disabled(&me), "not on yourself");
    let state = &rt.state.state;
    assert!(!state.admin_can_act_on(CVETKA) && state.admin_can_act_on(ANA));
    let other_admin = state
        .admin()
        .unwrap()
        .users
        .items
        .iter()
        .find(|r| r.user.is_admin && r.user.id != CVETKA)
        .map(|r| r.user.id);
    if let Some(id) = other_admin {
        assert!(!state.admin_can_act_on(id));
    }
    // And the data layer ignores the dialogs for them.
    let (mut app, _) = booted_as(true, "cvetka");
    app.act(|s| s.ask_admin_suspend(CVETKA));
    assert!(app.state.ui.account.admin_dialog.is_none());
}

#[test]
fn deleting_a_user_who_owns_servers_offers_the_transfer() {
    let (mut rt, fake) = admin_rt();
    rt.update(|a, _| a.act(|s| s.ask_admin_delete(TIM)));
    settle(&mut rt, 6);
    assert!(has(&rt, "delete-user-submit") && has_text(&rt, "Delete tim"));
    assert!(
        !has(&rt, "delete-user-transfer"),
        "asked only once the server says so"
    );
    press(&mut rt, "delete-user-submit");
    assert!(
        rt.state.state.ui.account.admin_dialog.is_some(),
        "refused: the dialog stays"
    );
    assert!(has(&rt, "delete-user-transfer") && has_text(&rt, "tim owns 2 servers"));
    assert!(has_text(&rt, "highest role"));
    assert!(
        disabled(&by_key(&rt, "delete-user-submit").unwrap()),
        "blocked until the transfer is chosen"
    );
    press_named(&mut rt, "check-box", "Transfer their servers automatically");
    assert!(!disabled(&by_key(&rt, "delete-user-submit").unwrap()));
    press(&mut rt, "delete-user-submit");
    assert!(
        sent(
            &fake,
            |c| matches!(c, Command::AdminDeleteUser { user, force_transfer: true, .. } if *user == TIM)
        ) == 1
    );
    assert!(rt.state.state.ui.account.admin_dialog.is_none());
    let row = rt
        .state
        .state
        .admin()
        .unwrap()
        .users
        .items
        .iter()
        .find(|r| r.user.id == TIM)
        .cloned()
        .unwrap();
    assert!(row.deleted);
    // The row is dimmed (a Deleted badge, no actions) and its name is the placeholder.
    assert!(by_key(&rt, &format!("user-deleted-{TIM}")).is_some());
    assert_eq!(by_key(&rt, &format!("user-actions-{TIM}")), None);
}

#[test]
fn the_users_scene_has_a_suspended_row_with_its_end_date() {
    let rt = scene_rt("admin-users-actions");
    let rows = &rt.state.state.admin().unwrap().users.items;
    let bruno = rows.iter().find(|r| r.user.id == BRUNO).unwrap();
    assert!(bruno.suspended && bruno.suspended_until.is_some() && bruno.suspended_reason == "Spamming links");
    assert!(!rows.iter().find(|r| r.user.id == ANA).unwrap().suspended);
}

// ---- deleted users ---------------------------------------------------------------------------

#[test]
fn a_deleted_user_is_named_and_pictured_neutrally_and_cannot_be_clicked_through() {
    assert_eq!(
        DELETED_USER, DESIGN_DELETED_USER,
        "the avatar and the data layer agree on the name"
    );
    let mut rt = scene_rt("message-deleted-user");
    rt.update(|a, _| a.state.ui.members_open = true);
    settle(&mut rt, 10);
    let s = &rt.state.state;
    assert_eq!(s.user_name(ANA), DELETED_USER);
    assert_eq!(
        s.name_color(HIDEOUT, ANA),
        thehideout_api::PaletteColor::None,
        "no role colour on a gone account"
    );
    // The avatar is a labelled picture without initials (the glyph is hidden from the tree).
    assert!(named(&rt, "image", DELETED_USER).is_some());
    assert!(find(&rt, |n| n["name"].as_str().is_some_and(|s| s == "ana" || s.starts_with("ana,"))).is_none(), "the username is nowhere");
    // Tim may kick and ban members, but not a deleted one: pressing the row opens nothing.
    assert!(s.has_member_menu(HIDEOUT, ANA));
    if has(&rt, &format!("member-{ANA}")) {
        press(&mut rt, &format!("member-{ANA}"));
        assert_eq!(rt.state.state.ui.menu, crate::state::Menu::None);
    }
    // The same in the voice list: the row is there, a press opens no menu.
    if has(&rt, &format!("voice-user-{ANA}")) {
        press(&mut rt, &format!("voice-user-{ANA}"));
        assert_eq!(rt.state.state.ui.menu, crate::state::Menu::None);
    }
    // Nobody can start a conversation with the placeholder by typing its name.
    let (mut app, _) = booted(true);
    let ana = app.state.data.users.get(&ANA).cloned().unwrap();
    app.state.data.users.insert(
        ANA,
        thehideout_api::User {
            deleted: true,
            username: DELETED_USER.into(),
            ..ana
        },
    );
    app.act(|s| s.open_dialog(crate::state::DialogKind::NewDm));
    app.state
        .ui
        .dialog
        .as_mut()
        .unwrap()
        .input
        .replace_text("Deleted user");
    app.act(|s| s.submit_dialog());
    assert!(
        app.state
            .ui
            .dialog
            .as_ref()
            .is_some_and(|d| d.error.is_some()),
        "no one called that"
    );
}




