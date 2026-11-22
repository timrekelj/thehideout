//! The moderation controls, driven through the real view with pointer and key input against the
//! fake backend: editing a message (the hover button, Up in an empty composer, save and cancel),
//! deleting one (confirmed, and Shift past the question), who is offered which menu entry, the
//! Ban dialog's clean-up choice, unbanning from the Bans page, leaving and deleting a server,
//! resetting the invite code and deleting a channel. The state rules are in `moderation_tests`.

use rune::{Key, Runtime};
use serde_json::Value;
use thehideout_api::MemberRemovalReason;

use crate::app::App;
use crate::backend::{Backend, Command};
use crate::fake::{FakeBackend, ANA, BRUNO, GENERAL, HIDEOUT, LOUNGE, PLANS, TIM};
use crate::forms::DialogKind;
use crate::scenes::booted_as;
use crate::server_settings::ServerPage;
use crate::state::{chat_id_of, AppState, Menu, Place};
use crate::tests::{click, has, key, runtime, settle};

const GARAGE_BAND: u64 = 2;
const COOKING_CLUB: u64 = 3;

fn start(who: &str) -> (Runtime<App>, FakeBackend) {
    let (app, fake) = booted_as(true, who);
    (runtime(app), fake)
}

fn sent(fake: &FakeBackend) -> Vec<Command> {
    Backend::sent(&fake.clone())
}

fn walk<'a>(n: &'a Value, out: &mut Vec<&'a Value>) {
    out.push(n);
    for c in n["children"].as_array().into_iter().flatten() {
        walk(c, out);
    }
}

fn all_nodes(rt: &Runtime<App>) -> Vec<Value> {
    let tree = rt.accessibility_json();
    let mut all = Vec::new();
    walk(&tree, &mut all);
    all.into_iter().cloned().collect()
}

/// The names of the open menu's items.
fn menu_items(rt: &Runtime<App>) -> Vec<String> {
    all_nodes(rt)
        .iter()
        .filter(|n| n["role"] == "menu-item")
        .filter_map(|n| n["name"].as_str().map(str::to_owned))
        .collect()
}

fn click_item(rt: &mut Runtime<App>, name: &str) {
    let key = all_nodes(rt)
        .iter()
        .find(|n| n["role"] == "menu-item" && n["name"] == name)
        .and_then(|n| n["key"].as_str().map(str::to_owned))
        .unwrap_or_else(|| panic!("no menu item {name}"));
    click(rt, &key);
    settle(rt, 10);
}

/// Types `text` into the focused field and lets the frame apply it.
fn type_text(rt: &mut Runtime<App>, text: &str) {
    crate::tests::type_text(rt, text);
    settle(rt, 3);
}

fn act(rt: &mut Runtime<App>, f: impl FnOnce(&mut AppState)) {
    rt.update(|a, _| a.act(f));
    settle(rt, 6);
}

fn press(rt: &mut Runtime<App>, key: &str) {
    click(rt, key);
    settle(rt, 12);
}

fn right_click(rt: &mut Runtime<App>, key: &str) {
    let at = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    rt.secondary_down(at.0, at.1);
    rt.secondary_up(at.0, at.1);
    settle(rt, 8);
}

fn state(rt: &Runtime<App>) -> &AppState {
    &rt.state.state
}

/// The newest message of `channel` by `author`: its server id and its transcript id.
fn last_by(rt: &Runtime<App>, channel: u64, author: u64) -> (u64, String) {
    let s = state(rt);
    let m = s
        .log(channel)
        .and_then(|l| l.messages.iter().rev().find(|m| m.author_id == author))
        .expect("a message by that author");
    (m.id, chat_id_of(m, s.my_id()))
}

/// Moves the pointer over a message so its hover bar shows.
fn hover(rt: &mut Runtime<App>, chat: &str) {
    let at = rt
        .find(&format!("msg-{chat}"))
        .unwrap_or_else(|| panic!("no row for {chat}"));
    rt.pointer_move(at.0 + 200.0, at.1);
    settle(rt, 6);
}

fn content_of(rt: &Runtime<App>, channel: u64, message: u64) -> Option<String> {
    state(rt)
        .log(channel)?
        .messages
        .iter()
        .find(|m| m.id == message)
        .map(|m| m.content.clone())
}

fn count(fake: &FakeBackend, f: impl Fn(&Command) -> bool) -> usize {
    sent(fake).iter().filter(|c| f(c)).count()
}

fn dialog_kind(rt: &Runtime<App>) -> Option<DialogKind> {
    state(rt).ui.dialog.as_ref().map(|d| d.kind)
}

fn toast(rt: &Runtime<App>) -> Option<String> {
    state(rt).ui.toasts.back().map(|t| t.message.clone())
}

// ---- editing ---------------------------------------------------------------------------------

#[test]
fn the_edit_button_opens_an_editor_and_enter_saves() {
    let (mut rt, fake) = start("tim");
    let (id, chat) = last_by(&rt, GENERAL, TIM);
    let before = content_of(&rt, GENERAL, id).unwrap();
    hover(&mut rt, &chat);
    press(&mut rt, &format!("edit-bar-{chat}"));
    assert!(has(&rt, "edit-input"), "the body became an editor");
    assert!(has(&rt, "edit-hint"), "with its hint");
    assert!(
        !has(&rt, &format!("edit-bar-{chat}")),
        "no hover bar over the editor"
    );
    assert_eq!(
        state(&rt).ui.editing.as_ref().map(|e| e.input.text.clone()),
        Some(before.clone())
    );

    type_text(&mut rt, " (fixed)");
    key(&mut rt, Key::Enter);
    settle(&mut rt, 20);
    let want = format!("{before} (fixed)");
    assert_eq!(
        count(
            &fake,
            |c| matches!(c, Command::EditMessage { message, content, .. } if *message == id && *content == want)
        ),
        1
    );
    assert!(!has(&rt, "edit-input"), "the editor closed");
    assert_eq!(content_of(&rt, GENERAL, id).as_deref(), Some(want.as_str()));
    // The faint mark, with the edit time for a tooltip.
    assert!(has(&rt, "edited"));
    let tip = state(&rt)
        .log(GENERAL)
        .unwrap()
        .messages
        .iter()
        .find(|m| m.id == id)
        .and_then(|m| m.edited_at)
        .map(|t| state(&rt).edited_tip(t));
    assert!(
        tip.as_deref()
            .is_some_and(|t| t.starts_with("Edited ") && t.contains(" at ")),
        "{tip:?}"
    );
}

#[test]
fn escape_cancels_an_edit_and_sends_nothing() {
    let (mut rt, fake) = start("tim");
    let (id, chat) = last_by(&rt, GENERAL, TIM);
    let before = content_of(&rt, GENERAL, id).unwrap();
    hover(&mut rt, &chat);
    press(&mut rt, &format!("edit-bar-{chat}"));
    type_text(&mut rt, " nope");
    key(&mut rt, Key::Escape);
    settle(&mut rt, 10);
    assert!(!has(&rt, "edit-input"));
    assert!(state(&rt).ui.editing.is_none());
    assert_eq!(
        count(&fake, |c| matches!(c, Command::EditMessage { .. })),
        0
    );
    assert_eq!(content_of(&rt, GENERAL, id), Some(before));
}

#[test]
fn saving_unchanged_text_just_closes_the_editor() {
    let (mut rt, fake) = start("tim");
    let (_, chat) = last_by(&rt, GENERAL, TIM);
    hover(&mut rt, &chat);
    press(&mut rt, &format!("edit-bar-{chat}"));
    key(&mut rt, Key::Enter);
    settle(&mut rt, 10);
    assert!(!has(&rt, "edit-input"));
    assert_eq!(
        count(&fake, |c| matches!(c, Command::EditMessage { .. })),
        0
    );
}

#[test]
fn up_in_an_empty_composer_edits_your_last_message() {
    let (mut rt, fake) = start("tim");
    let (id, _) = last_by(&rt, GENERAL, TIM);
    // Someone else spoke last: it is still your own last message that opens.
    fake.incoming(GENERAL, BRUNO, "late reply");
    settle(&mut rt, 10);
    press(&mut rt, "input");
    key(&mut rt, Key::Up);
    settle(&mut rt, 8);
    assert_eq!(state(&rt).ui.editing.as_ref().map(|e| e.message), Some(id));
    assert!(has(&rt, "edit-input"));
    key(&mut rt, Key::Escape);
    settle(&mut rt, 6);
    assert!(state(&rt).ui.editing.is_none());

    // With text in the box, Up is the box's own.
    press(&mut rt, "input");
    type_text(&mut rt, "half a thought");
    key(&mut rt, Key::Up);
    settle(&mut rt, 6);
    assert!(
        state(&rt).ui.editing.is_none(),
        "Up moves the caret, nothing opens"
    );
}

#[test]
fn up_does_nothing_where_you_have_said_nothing() {
    let (mut rt, _) = start("tim");
    // A text channel where Tim never spoke.
    let channels: Vec<u64> = state(&rt).channels_of(HIDEOUT).iter().filter(|c| c.kind == thehideout_api::ChannelKind::Text).map(|c| c.id).collect();
    let mut quiet = None;
    for c in channels {
        act(&mut rt, |s| s.select_channel(c));
        settle(&mut rt, 10);
        if state(&rt).log(c).is_some_and(|l| l.loaded && !l.messages.iter().any(|m| m.author_id == TIM)) {
            quiet = Some(c);
            break;
        }
    }
    assert!(quiet.is_some(), "the fixture has a channel Tim never spoke in");
    press(&mut rt, "input");
    key(&mut rt, Key::Up);
    settle(&mut rt, 6);
    assert!(state(&rt).ui.editing.is_none());
}

#[test]
fn an_edit_ends_when_the_message_is_deleted_under_it() {
    let (mut rt, fake) = start("tim");
    let (id, chat) = last_by(&rt, GENERAL, TIM);
    hover(&mut rt, &chat);
    press(&mut rt, &format!("edit-bar-{chat}"));
    assert!(has(&rt, "edit-input"));
    fake.delete_message_as_moderator(GENERAL, id);
    settle(&mut rt, 20);
    assert!(!has(&rt, "edit-input"));
    assert!(state(&rt).ui.editing.is_none());
    assert_eq!(
        toast(&rt).as_deref(),
        Some("The message you were editing was deleted.")
    );
}

// ---- deleting a message ----------------------------------------------------------------------

#[test]
fn delete_asks_with_a_preview_and_the_button_deletes() {
    let (mut rt, fake) = start("tim");
    let (id, chat) = last_by(&rt, GENERAL, TIM);
    hover(&mut rt, &chat);
    press(&mut rt, &format!("delete-bar-{chat}"));
    assert!(
        matches!(dialog_kind(&rt), Some(DialogKind::DeleteMessage { message, .. }) if message == id)
    );
    assert!(has(&rt, "delete-preview"), "the dialog shows the message");
    assert_eq!(
        count(&fake, |c| matches!(c, Command::DeleteMessage { .. })),
        0,
        "nothing yet"
    );
    press(&mut rt, "dialog-cancel");
    assert!(state(&rt).ui.dialog.is_none());
    assert_eq!(
        count(&fake, |c| matches!(c, Command::DeleteMessage { .. })),
        0
    );

    hover(&mut rt, &chat);
    press(&mut rt, &format!("delete-bar-{chat}"));
    press(&mut rt, "dialog-submit");
    settle(&mut rt, 10);
    assert_eq!(
        count(
            &fake,
            |c| matches!(c, Command::DeleteMessage { message, .. } if *message == id)
        ),
        1
    );
    assert!(
        content_of(&rt, GENERAL, id).is_none(),
        "gone from the transcript"
    );
    assert!(state(&rt).ui.dialog.is_none());
}

#[test]
fn shift_skips_the_question_on_your_own_message_only() {
    let (mut rt, fake) = start("tim");
    let (mine, _) = last_by(&rt, GENERAL, TIM);
    act(&mut rt, |s| s.delete_pressed(GENERAL, mine, true));
    assert!(state(&rt).ui.dialog.is_none(), "no dialog");
    assert_eq!(
        count(
            &fake,
            |c| matches!(c, Command::DeleteMessage { message, .. } if *message == mine)
        ),
        1
    );
    settle(&mut rt, 10);
    assert!(content_of(&rt, GENERAL, mine).is_none());

    // Someone else's message always asks, Shift or not (Tim moderates The Hideout).
    let (theirs, _) = last_by(&rt, GENERAL, BRUNO);
    act(&mut rt, |s| s.delete_pressed(GENERAL, theirs, true));
    assert!(
        matches!(dialog_kind(&rt), Some(DialogKind::DeleteMessage { message, .. }) if message == theirs)
    );
    assert_eq!(
        count(
            &fake,
            |c| matches!(c, Command::DeleteMessage { message, .. } if *message == theirs)
        ),
        0
    );
    press(&mut rt, "dialog-submit");
    settle(&mut rt, 10);
    assert!(content_of(&rt, GENERAL, theirs).is_none());
}

#[test]
fn message_menus_offer_only_what_you_may() {
    // The owner: Edit only on their own, Delete on everyone's.
    let (mut rt, _) = start("tim");
    let (_, mine) = last_by(&rt, GENERAL, TIM);
    let (_, theirs) = last_by(&rt, GENERAL, BRUNO);
    right_click(&mut rt, &format!("msg-{mine}"));
    assert_eq!(
        menu_items(&rt),
        ["Reply", "Copy text", "Edit message", "Delete message"]
    );
    act(&mut rt, |s| s.ui.menu = Menu::None);
    right_click(&mut rt, &format!("msg-{theirs}"));
    assert_eq!(menu_items(&rt), ["Reply", "Copy text", "Delete message"]);
    click_item(&mut rt, "Delete message");
    assert!(matches!(
        dialog_kind(&rt),
        Some(DialogKind::DeleteMessage { .. })
    ));

    // A plain member: their own, but never someone else's delete.
    let (mut rt, _) = start("ana");
    let (_, mine) = last_by(&rt, GENERAL, ANA);
    let (_, theirs) = last_by(&rt, GENERAL, BRUNO);
    right_click(&mut rt, &format!("msg-{mine}"));
    assert_eq!(
        menu_items(&rt),
        ["Reply", "Copy text", "Edit message", "Delete message"]
    );
    act(&mut rt, |s| s.ui.menu = Menu::None);
    right_click(&mut rt, &format!("msg-{theirs}"));
    assert_eq!(menu_items(&rt), ["Reply", "Copy text"]);
    act(&mut rt, |s| s.ui.menu = Menu::None);
    hover(&mut rt, &theirs);
    assert!(!has(&rt, &format!("edit-bar-{theirs}")) && !has(&rt, &format!("delete-bar-{theirs}")));
    hover(&mut rt, &mine);
    assert!(has(&rt, &format!("edit-bar-{mine}")) && has(&rt, &format!("delete-bar-{mine}")));
}

#[test]
fn the_more_button_opens_the_same_menu_under_itself() {
    let (mut rt, _) = start("tim");
    let (_, mine) = last_by(&rt, GENERAL, TIM);
    hover(&mut rt, &mine);
    press(&mut rt, &format!("more-bar-{mine}"));
    assert_eq!(
        menu_items(&rt),
        ["Reply", "Copy text", "Edit message", "Delete message"]
    );
    click_item(&mut rt, "Edit message");
    assert!(has(&rt, "edit-input"));
}

// ---- members ---------------------------------------------------------------------------------

#[test]
fn the_member_menu_has_kick_and_ban_for_those_who_may() {
    let (mut rt, fake) = start("tim");
    act(&mut rt, |s| s.ui.members_open = true);
    settle(&mut rt, 40);
    right_click(&mut rt, &format!("member-{BRUNO}"));
    assert_eq!(menu_items(&rt), ["Kick", "Ban\u{2026}"]);
    click_item(&mut rt, "Kick");
    assert!(matches!(dialog_kind(&rt), Some(DialogKind::KickMember { user, .. }) if user == BRUNO));
    assert_eq!(count(&fake, |c| matches!(c, Command::KickMember { .. })), 0);
    press(&mut rt, "dialog-submit");
    settle(&mut rt, 10);
    assert_eq!(
        count(
            &fake,
            |c| matches!(c, Command::KickMember { user, .. } if *user == BRUNO)
        ),
        1
    );
    assert!(!has(&rt, &format!("member-{BRUNO}")), "gone from the panel");
    // Never offered on yourself: pressing your own row opens nothing.
    press(&mut rt, &format!("member-{TIM}"));
    assert!(menu_items(&rt).is_empty());

    // A plain member has no menu on anyone.
    let (mut rt, _) = start("ana");
    act(&mut rt, |s| s.ui.members_open = true);
    settle(&mut rt, 40);
    right_click(&mut rt, &format!("member-{BRUNO}"));
    assert!(menu_items(&rt).is_empty());
    press(&mut rt, &format!("member-{BRUNO}"));
    assert!(menu_items(&rt).is_empty());
}

#[test]
fn the_ban_dialog_sends_the_chosen_seconds_and_reason() {
    for (choice, want) in [
        (0, None),
        (1, Some(3_600)),
        (2, Some(86_400)),
        (3, Some(604_800)),
    ] {
        let (mut rt, fake) = start("tim");
        act(&mut rt, |s| s.ask_ban(HIDEOUT, BRUNO));
        assert!(has(&rt, "ban-cleanup-trigger"));
        type_text(&mut rt, "spam links");
        act(&mut rt, |s| {
            if let Some(d) = s.ui.dialog.as_mut() {
                d.form.cleanup = choice;
            }
        });
        press(&mut rt, "dialog-submit");
        settle(&mut rt, 10);
        let bans: Vec<_> = sent(&fake)
            .into_iter()
            .filter_map(|c| match c {
                Command::BanMember {
                    user,
                    reason,
                    delete_seconds,
                    ..
                } => Some((user, reason, delete_seconds)),
                _ => None,
            })
            .collect();
        assert_eq!(
            bans,
            [(BRUNO, "spam links".to_owned(), want)],
            "choice {choice}"
        );
        assert!(state(&rt).ui.dialog.is_none());
    }
}

#[test]
fn the_ban_select_picks_with_the_pointer() {
    let (mut rt, fake) = start("tim");
    act(&mut rt, |s| s.ask_ban(HIDEOUT, BRUNO));
    press(&mut rt, "ban-cleanup-trigger");
    let names: Vec<String> = all_nodes(&rt)
        .iter()
        .filter_map(|n| n["name"].as_str().map(str::to_owned))
        .collect();
    for label in ["Don't delete", "Last hour", "Last 24 hours", "Last 7 days"] {
        assert!(
            names.iter().any(|n| n == label),
            "{label} offered: {names:?}"
        );
    }
    let key = all_nodes(&rt)
        .iter()
        .find(|n| n["name"] == "Last 7 days" && n["key"].is_string() && n["key"] != "ban-cleanup-trigger")
        .and_then(|n| n["key"].as_str().map(str::to_owned))
        .expect("an option");
    press(&mut rt, &key);
    press(&mut rt, "dialog-submit");
    settle(&mut rt, 10);
    assert_eq!(
        count(&fake, |c| matches!(
            c,
            Command::BanMember {
                delete_seconds: Some(604_800),
                ..
            }
        )),
        1
    );
}

#[test]
fn unban_from_the_bans_page() {
    let (mut rt, fake) = start("tim");
    act(&mut rt, |s| {
        s.ban(HIDEOUT, BRUNO, "spam links", None);
    });
    settle(&mut rt, 10);
    act(&mut rt, |s| s.open_server_settings(HIDEOUT));
    press(&mut rt, "server-nav-Bans");
    assert_eq!(
        state(&rt).ui.server_settings.as_ref().map(|s| s.page),
        Some(ServerPage::Bans)
    );
    settle(&mut rt, 10);
    assert!(has(&rt, &format!("ban-{BRUNO}")), "listed");
    press(&mut rt, &format!("unban-{BRUNO}"));
    settle(&mut rt, 10);
    assert_eq!(
        count(
            &fake,
            |c| matches!(c, Command::UnbanMember { user, .. } if *user == BRUNO)
        ),
        1
    );
    assert!(!has(&rt, &format!("ban-{BRUNO}")));
    assert!(has(&rt, "bans-empty"));
    assert!(fake.bans(HIDEOUT).is_empty());
}

#[test]
fn the_members_page_has_kick_and_ban_and_a_plain_member_has_no_settings() {
    let (mut rt, fake) = start("tim");
    act(&mut rt, |s| s.open_server_settings(HIDEOUT));
    press(&mut rt, "server-nav-Members");
    for user in [ANA, BRUNO] {
        assert!(
            has(&rt, &format!("member-kick-{user}")) && has(&rt, &format!("member-ban-{user}")),
            "{user}"
        );
    }
    assert!(
        !has(&rt, &format!("member-kick-{TIM}")),
        "never on yourself"
    );
    press(&mut rt, &format!("member-ban-{BRUNO}"));
    assert!(matches!(dialog_kind(&rt), Some(DialogKind::BanMember { user, .. }) if user == BRUNO));
    press(&mut rt, "dialog-cancel");
    press(&mut rt, &format!("member-kick-{BRUNO}"));
    press(&mut rt, "dialog-submit");
    settle(&mut rt, 10);
    assert_eq!(
        count(
            &fake,
            |c| matches!(c, Command::KickMember { user, .. } if *user == BRUNO)
        ),
        1
    );

    let (rt, _) = start("ana");
    assert!(!state(&rt).can_open_server_settings(HIDEOUT));
    assert!(!state(&rt).settings_page_allowed(HIDEOUT, ServerPage::Bans));
}

// ---- the server ------------------------------------------------------------------------------

#[test]
fn leaving_a_server_asks_then_goes_home() {
    let (mut rt, fake) = start("tim");
    // Tim owns The Hideout: no Leave there.
    press(&mut rt, "guild-menu-trigger");
    assert!(
        !menu_items(&rt)
            .iter()
            .any(|i| i.starts_with("Leave server")),
        "{:?}",
        menu_items(&rt)
    );
    act(&mut rt, |s| s.ui.menu = Menu::None);

    act(&mut rt, |s| s.go_guild(GARAGE_BAND));
    press(&mut rt, "guild-menu-trigger");
    click_item(&mut rt, "Leave server\u{2026}");
    assert!(
        matches!(dialog_kind(&rt), Some(DialogKind::LeaveGuild { guild }) if guild == GARAGE_BAND)
    );
    assert_eq!(count(&fake, |c| matches!(c, Command::LeaveGuild { .. })), 0);
    press(&mut rt, "dialog-submit");
    settle(&mut rt, 10);
    assert_eq!(
        count(
            &fake,
            |c| matches!(c, Command::LeaveGuild { guild } if *guild == GARAGE_BAND)
        ),
        1
    );
    assert_eq!(state(&rt).ui.place, Place::Home);
    assert!(state(&rt).guild(GARAGE_BAND).is_none());
    assert!(
        !has(&rt, &format!("guild-{GARAGE_BAND}")),
        "gone from the rail"
    );
}

#[test]
fn deleting_a_server_needs_its_name_and_goes_home() {
    let (mut rt, fake) = start("tim");
    act(&mut rt, |s| s.open_server_settings(HIDEOUT));
    assert!(has(&rt, "danger-zone"));
    press(&mut rt, "server-delete");
    assert!(matches!(
        dialog_kind(&rt),
        Some(DialogKind::DeleteGuild { .. })
    ));
    // Wrong name: nothing is sent, and the dialog says why.
    type_text(&mut rt, "The Hideo");
    act(&mut rt, |s| s.submit_dialog());
    assert_eq!(
        count(&fake, |c| matches!(c, Command::DeleteGuild { .. })),
        0
    );
    assert!(state(&rt)
        .ui
        .dialog
        .as_ref()
        .and_then(|d| d.error.clone())
        .is_some());
    type_text(&mut rt, "ut");
    press(&mut rt, "dialog-submit");
    settle(&mut rt, 10);
    assert_eq!(
        count(
            &fake,
            |c| matches!(c, Command::DeleteGuild { confirm_name, .. } if confirm_name == "The Hideout")
        ),
        1
    );
    assert_eq!(state(&rt).ui.place, Place::Home);
    assert!(state(&rt).guild(HIDEOUT).is_none());
    assert!(
        state(&rt).ui.server_settings.is_none(),
        "the settings page closed with it"
    );
}

#[test]
fn transferring_needs_a_member_and_the_name_and_is_for_the_owner() {
    let (mut rt, fake) = start("tim");
    act(&mut rt, |s| s.open_server_settings(HIDEOUT));
    press(&mut rt, "server-transfer");
    act(&mut rt, |s| s.submit_dialog());
    assert!(
        state(&rt)
            .ui
            .dialog
            .as_ref()
            .and_then(|d| d.error.clone())
            .is_some(),
        "no one picked"
    );
    act(&mut rt, |s| {
        if let Some(d) = s.ui.dialog.as_mut() {
            d.form.target = Some(ANA);
        }
    });
    type_text(&mut rt, "The Hideout");
    press(&mut rt, "dialog-submit");
    settle(&mut rt, 10);
    assert_eq!(
        count(
            &fake,
            |c| matches!(c, Command::TransferOwnership { user, confirm_name, .. } if *user == ANA && confirm_name == "The Hideout")
        ),
        1
    );
    assert!(
        !state(&rt).can_delete_guild(HIDEOUT),
        "Tim is no longer the owner"
    );

    // A moderator sees no Danger zone.
    let (mut rt, _) = start("bruno");
    act(&mut rt, |s| s.open_server_settings(HIDEOUT));
    assert!(!has(&rt, "danger-zone"));
}

#[test]
fn resetting_the_invite_code_asks_and_shows_the_new_one() {
    let (mut rt, fake) = start("tim");
    let label = |rt: &Runtime<App>| {
        all_nodes(rt)
            .iter()
            .find(|n| n["key"] == "invite-code")
            .and_then(|n| n["name"].as_str().map(str::to_owned))
    };
    act(&mut rt, |s| {
        s.open_dialog(DialogKind::Invite { guild: HIDEOUT })
    });
    let old = label(&rt).expect("the code is shown");
    assert_eq!(old, "Invite code HIDEOUT1");
    press(&mut rt, "invite-reset");
    assert!(!has(&rt, "invite-code"), "the question replaces the code");
    // Back keeps the code.
    press(&mut rt, "dialog-cancel");
    assert_eq!(label(&rt).as_deref(), Some(old.as_str()));
    assert_eq!(
        count(&fake, |c| matches!(c, Command::RotateInviteCode { .. })),
        0
    );

    press(&mut rt, "invite-reset");
    press(&mut rt, "dialog-submit");
    settle(&mut rt, 10);
    assert_eq!(
        count(
            &fake,
            |c| matches!(c, Command::RotateInviteCode { guild } if *guild == HIDEOUT)
        ),
        1
    );
    let new = label(&rt).expect("the dialog is back on the code");
    assert_ne!(new, old, "a new code shows");
    assert!(new.starts_with("Invite code "));

    // Only those who manage the server see the button.
    let (mut rt, _) = start("ana");
    act(&mut rt, |s| {
        s.open_dialog(DialogKind::Invite { guild: HIDEOUT })
    });
    assert!(has(&rt, "invite-code") && !has(&rt, "invite-reset"));
}

// ---- channels --------------------------------------------------------------------------------

#[test]
fn deleting_the_open_channel_selects_the_first_text_channel() {
    let (mut rt, fake) = start("tim");
    assert_eq!(state(&rt).current_channel(), Some(GENERAL));
    right_click(&mut rt, "channels-1-row-general");
    click_item(&mut rt, "Delete channel\u{2026}");
    assert!(
        matches!(dialog_kind(&rt), Some(DialogKind::DeleteChannel { channel, .. }) if channel == GENERAL)
    );
    assert_eq!(
        count(&fake, |c| matches!(c, Command::DeleteChannel { .. })),
        0
    );
    press(&mut rt, "dialog-submit");
    settle(&mut rt, 10);
    assert_eq!(
        count(
            &fake,
            |c| matches!(c, Command::DeleteChannel { channel, .. } if *channel == GENERAL)
        ),
        1
    );
    let now = state(&rt).current_channel();
    assert!(now.is_some() && now != Some(GENERAL), "{now:?}");
    assert_eq!(now, state(&rt).first_text_channel(HIDEOUT));
    assert!(!has(&rt, "channels-1-row-general"));
}

#[test]
fn a_voice_channel_and_the_edit_dialog_delete_too() {
    let (mut rt, fake) = start("tim");
    right_click(&mut rt, "channels-1-row-lounge");
    click_item(&mut rt, "Delete channel\u{2026}");
    assert!(
        matches!(dialog_kind(&rt), Some(DialogKind::DeleteChannel { channel, .. }) if channel == LOUNGE)
    );
    press(&mut rt, "dialog-submit");
    settle(&mut rt, 10);
    assert_eq!(
        count(
            &fake,
            |c| matches!(c, Command::DeleteChannel { channel, .. } if *channel == LOUNGE)
        ),
        1
    );
    assert!(state(&rt).channel(HIDEOUT, LOUNGE).is_none());
    assert_eq!(
        state(&rt).current_channel(),
        Some(GENERAL),
        "an unrelated channel stays open"
    );

    // The Edit channel dialog has the same exit, then the confirmation takes its place.
    act(&mut rt, |s| {
        s.open_dialog(DialogKind::ChannelSettings {
            guild: HIDEOUT,
            channel: PLANS,
        })
    });
    press(&mut rt, "channel-delete");
    assert!(
        matches!(dialog_kind(&rt), Some(DialogKind::DeleteChannel { channel, .. }) if channel == PLANS)
    );
    press(&mut rt, "dialog-submit");
    settle(&mut rt, 10);
    assert!(state(&rt).channel(HIDEOUT, PLANS).is_none());

    // A member who may not manage channels is offered neither.
    let (rt, _) = start("ana");
    assert!(!has(&rt, &format!("channel-more-{PLANS}")));
}

// ---- what the server does while you look -------------------------------------------------------

#[test]
fn being_kicked_banned_or_losing_the_server_says_so_and_goes_home() {
    // Kicked from Garage Band while it is open.
    let (mut rt, fake) = start("tim");
    act(&mut rt, |s| s.go_guild(GARAGE_BAND));
    fake.remove_member_as_moderator(GARAGE_BAND, TIM, MemberRemovalReason::Kicked);
    settle(&mut rt, 20);
    assert_eq!(state(&rt).ui.place, Place::Home);
    assert!(!has(&rt, &format!("guild-{GARAGE_BAND}")));
    assert_eq!(
        toast(&rt).as_deref(),
        Some("You were removed from Garage Band")
    );

    // Banned from Cooking Club.
    act(&mut rt, |s| s.go_guild(COOKING_CLUB));
    fake.remove_member_as_moderator(COOKING_CLUB, TIM, MemberRemovalReason::Banned);
    settle(&mut rt, 20);
    assert_eq!(state(&rt).ui.place, Place::Home);
    assert_eq!(
        toast(&rt).as_deref(),
        Some("You were banned from Cooking Club")
    );

    // The server deleted while open.
    let (mut rt, fake) = start("tim");
    act(&mut rt, |s| s.go_guild(COOKING_CLUB));
    fake.delete_guild_as_owner(COOKING_CLUB);
    settle(&mut rt, 20);
    assert_eq!(state(&rt).ui.place, Place::Home);
    assert_eq!(toast(&rt).as_deref(), Some("Cooking Club was deleted"));
}

#[test]
fn a_channel_deleted_by_someone_else_reselects_the_first_text_channel() {
    let (mut rt, fake) = start("tim");
    assert_eq!(state(&rt).current_channel(), Some(GENERAL));
    fake.delete_channel_as_moderator(HIDEOUT, GENERAL);
    settle(&mut rt, 20);
    let now = state(&rt).current_channel();
    assert!(now.is_some() && now != Some(GENERAL));
    assert_eq!(now, state(&rt).first_text_channel(HIDEOUT));
}
