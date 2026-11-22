//! Moderation and structure as state, against the fake backend: the selectors follow the server's
//! rules, every action sends the right command (and nothing when the rules refuse), replies and
//! gateway events (which both arrive) change the lists once, and a removed guild or channel
//! leaves the app on a place that still exists.

use thehideout_api::{ApiError, MemberRemovalReason, Message, Permissions};
use thehideout_design::components::ToastKind;

use crate::app::App;
use crate::backend::{Backend, Command};
use crate::fake::{
    FakeBackend, ANA, BRUNO, COOK_MODS, CVETKA, DEV, DM_ANA, GAME_NIGHT, GENERAL, HIDEOUT,
    HIDEOUT_MODS, LOUNGE, PLANS, TIM,
};
use crate::moderation::moderation_error_text;
use crate::scenes::{booted, booted_as};
use crate::state::Place;

const GARAGE_BAND: u64 = 2;
const COOKING_CLUB: u64 = 3;

fn tim() -> (App, FakeBackend) {
    booted(true)
}

fn sent(fake: &FakeBackend) -> Vec<Command> {
    fake.clone().sent()
}

fn toast(app: &App) -> Option<(ToastKind, String)> {
    app.state
        .ui
        .toasts
        .back()
        .map(|t| (t.kind, t.message.clone()))
}

fn messages(app: &App, channel: u64) -> Vec<Message> {
    app.state
        .log(channel)
        .map(|l| l.messages.clone())
        .unwrap_or_default()
}

/// The newest message of `channel` written by `author`.
fn by(app: &App, channel: u64, author: u64) -> Message {
    messages(app, channel)
        .into_iter()
        .rev()
        .find(|m| m.author_id == author)
        .expect("a message by that author")
}

/// Opens `channel` and lets its history arrive.
fn open(app: &mut App, channel: u64) {
    app.act(|s| s.select_channel(channel));
    app.pump();
    assert!(
        app.state.log(channel).is_some_and(|l| l.loaded),
        "history of {channel} loaded"
    );
}

/// Runs `f`, delivers whatever it sent and applies the answers.
fn run(app: &mut App, f: impl FnOnce(&mut crate::state::AppState) -> bool) -> bool {
    let mut result = false;
    app.act(|s| result = f(s));
    app.pump();
    result
}

#[test]
fn the_selectors_follow_the_server_hierarchy() {
    let (app, _) = tim();
    // Tim owns The Hideout: he may act on everyone else there, never on himself.
    for user in [ANA, BRUNO, CVETKA] {
        assert!(
            app.state.can_kick(HIDEOUT, user) && app.state.can_ban(HIDEOUT, user),
            "owner outranks {user}"
        );
    }
    assert!(!app.state.can_kick(HIDEOUT, TIM) && !app.state.can_ban(HIDEOUT, TIM));
    // He is a plain member of Garage Band: none of the powers.
    assert!(!app.state.can_kick(GARAGE_BAND, ANA) && !app.state.can_ban(GARAGE_BAND, ANA));
    assert!(!app.state.can_view_bans(GARAGE_BAND) && app.state.can_view_bans(HIDEOUT));
    assert!(!app.state.can_leave_guild(HIDEOUT) && app.state.can_leave_guild(GARAGE_BAND));
    assert!(app.state.can_delete_guild(HIDEOUT) && !app.state.can_delete_guild(GARAGE_BAND));

    // Bruno's Moderator role (position 2) gets the bits: above Ana (1), below Cvetka's Admin (3), not the owner.
    let (mut bruno, bfake) = booted_as(true, "bruno");
    bfake.set_role_permissions(
        HIDEOUT,
        HIDEOUT_MODS,
        Permissions::MANAGE_CHANNELS
            | Permissions::KICK_MEMBERS
            | Permissions::BAN_MEMBERS
            | Permissions::MANAGE_MESSAGES,
    );
    bruno.pump();
    assert!(bruno.state.can_kick(HIDEOUT, ANA) && bruno.state.can_ban(HIDEOUT, ANA));
    assert!(
        !bruno.state.can_kick(HIDEOUT, CVETKA),
        "a higher role is protected"
    );
    assert!(
        !bruno.state.can_kick(HIDEOUT, TIM),
        "nobody but the owner acts on the owner"
    );
    assert!(!bruno.state.can_kick(HIDEOUT, BRUNO), "not yourself");
    assert!(
        !bruno.state.can_delete_guild(HIDEOUT) && !bruno.state.can_leave_guild(COOKING_CLUB),
        "bruno owns Cooking Club"
    );

    // Ana has no moderation bits at all.
    let (ana, _) = booted_as(true, "ana");
    assert!(!ana.state.can_kick(HIDEOUT, BRUNO) && !ana.state.can_ban(HIDEOUT, BRUNO));
}

#[test]
fn deleting_and_editing_messages_obey_authorship_and_the_manage_messages_bit() {
    let (mut app, fake) = tim();
    open(&mut app, GENERAL);
    let (mine, anas) = (by(&app, GENERAL, TIM), by(&app, GENERAL, ANA));
    // The owner may delete anyone's, but only edit his own.
    assert!(app.state.can_delete_message(&mine) && app.state.can_delete_message(&anas));
    assert!(app.state.can_edit_message(&mine) && !app.state.can_edit_message(&anas));
    assert!(!run(&mut app, |s| s.edit_message(GENERAL, anas.id, "mine now")));
    assert!(!run(&mut app, |s| s.edit_message(GENERAL, mine.id, "   ")));
    assert!(!run(&mut app, |s| s.edit_message(
        GENERAL,
        mine.id,
        &"x".repeat(4001)
    )));
    assert!(
        !run(&mut app, |s| s.edit_message(
            GENERAL,
            mine.id,
            &mine.content
        )),
        "nothing changed, nothing sent"
    );
    assert!(!run(&mut app, |s| s.edit_message(GENERAL, 99_999_999, "x")));
    assert!(sent(&fake)
        .iter()
        .all(|c| !matches!(c, Command::EditMessage { .. })));

    // An edit lands once from the reply and the echo.
    assert!(run(&mut app, |s| s.edit_message(
        GENERAL,
        mine.id,
        "  edited text "
    )));
    let edited = messages(&app, GENERAL)
        .into_iter()
        .find(|m| m.id == mine.id)
        .unwrap();
    assert_eq!(edited.content, "edited text");
    assert!(edited.edited_at.is_some());
    assert_eq!(
        fake.message(GENERAL, mine.id).unwrap().content,
        "edited text"
    );
    assert_eq!(
        messages(&app, GENERAL)
            .iter()
            .filter(|m| m.id == mine.id)
            .count(),
        1
    );

    // Delete: gone from the transcript and the world, however many echoes arrive.
    let before = messages(&app, GENERAL).len();
    assert!(run(&mut app, |s| s.delete_message(GENERAL, anas.id)));
    assert_eq!(messages(&app, GENERAL).len(), before - 1);
    assert!(fake.message(GENERAL, anas.id).is_none());
    fake.delete_message_as_moderator(GENERAL, mine.id);
    app.pump();
    assert!(messages(&app, GENERAL).iter().all(|m| m.id != mine.id));
    assert!(
        !run(&mut app, |s| s.delete_message(GENERAL, anas.id)),
        "already gone"
    );
}

#[test]
fn a_member_without_manage_messages_only_deletes_their_own_and_never_dm_messages_of_others() {
    let (mut app, _) = booted_as(true, "ana");
    open(&mut app, GENERAL);
    let (mine, others) = (by(&app, GENERAL, ANA), by(&app, GENERAL, TIM));
    assert!(app.state.can_delete_message(&mine) && !app.state.can_delete_message(&others));
    assert!(!run(&mut app, |s| s.delete_message(GENERAL, others.id)));
    assert!(run(&mut app, |s| s.delete_message(GENERAL, mine.id)));

    // The owner has no say over a DM.
    let (mut tim, _) = booted(true);
    app_dm(&mut tim);
    let theirs = by(&tim, DM_ANA, ANA);
    assert!(!tim.state.can_delete_message(&theirs));
    assert!(!run(&mut tim, |s| s.delete_message(DM_ANA, theirs.id)));
    assert!(tim.state.can_delete_message(&by(&tim, DM_ANA, TIM)));
}

fn app_dm(app: &mut App) {
    app.act(|s| {
        s.go_home();
        s.select_channel(DM_ANA);
    });
    app.pump();
}

#[test]
fn a_refused_command_toasts_why() {
    let (mut app, _) = tim();
    open(&mut app, GENERAL);
    let anas = by(&app, GENERAL, ANA);
    // Bypass the guards, as a stale control might: the server's refusal is what the user sees.
    app.act(|s| {
        s.outbox.push(Command::EditMessage {
            channel: GENERAL,
            message: anas.id,
            content: "x".into(),
        })
    });
    app.pump();
    let (kind, text) = toast(&app).expect("a toast");
    assert_eq!(kind, ToastKind::Danger);
    assert!(
        text.contains("Couldn't edit") && text.contains("permission"),
        "{text}"
    );
    app.act(|s| {
        s.outbox.push(Command::KickMember {
            guild: GARAGE_BAND,
            user: ANA,
        })
    });
    app.pump();
    assert!(toast(&app).unwrap().1.contains("permission"));
    assert!(moderation_error_text(&ApiError::RateLimited).contains("too fast"));
    assert!(
        moderation_error_text(&ApiError::Precondition("the owner cannot leave: x".into()))
            .contains("owner can't leave")
    );
}

#[test]
fn kicking_removes_the_member_from_the_lists_once() {
    let (mut app, fake) = tim();
    app.state.want_members(HIDEOUT);
    app.pump();
    assert!(app.state.data.members[&HIDEOUT].contains(&ANA));
    assert!(!run(&mut app, |s| s.kick(HIDEOUT, TIM)), "not yourself");
    assert!(
        !run(&mut app, |s| s.kick(GARAGE_BAND, ANA)),
        "no permission there"
    );
    assert!(run(&mut app, |s| s.kick(HIDEOUT, ANA)));
    assert!(!app.state.data.members[&HIDEOUT].contains(&ANA));
    assert!(!fake.member_ids(HIDEOUT).contains(&ANA));
    assert_eq!(toast(&app).unwrap().1, "Removed ana");
    // Others being removed by someone else arrive as events.
    fake.remove_member_as_moderator(HIDEOUT, BRUNO, MemberRemovalReason::Kicked);
    app.pump();
    assert!(!app.state.data.members[&HIDEOUT].contains(&BRUNO));
    assert!(app
        .state
        .data
        .member_roles
        .get(&HIDEOUT)
        .is_none_or(|m| !m.contains_key(&BRUNO)));
    // Still in the guild ourselves.
    assert!(app.state.guild(HIDEOUT).is_some());
}

#[test]
fn bans_list_unban_and_delete_recent_messages() {
    let (mut app, fake) = tim();
    open(&mut app, GENERAL);
    app.state.want_members(HIDEOUT);
    app.pump();
    let brunos = messages(&app, GENERAL)
        .iter()
        .filter(|m| m.author_id == BRUNO)
        .count();
    assert!(brunos > 0);
    assert!(!run(&mut app, |s| s.ban(HIDEOUT, TIM, "", None)));
    assert!(run(&mut app, |s| s.load_bans(HIDEOUT)));
    assert!(app.state.bans_of(HIDEOUT).is_empty());

    assert!(run(&mut app, |s| s.ban(
        HIDEOUT,
        BRUNO,
        "  spam  ",
        Some(7 * 24 * 3600)
    )));
    assert!(!app.state.data.members[&HIDEOUT].contains(&BRUNO));
    assert_eq!(
        messages(&app, GENERAL)
            .iter()
            .filter(|m| m.author_id == BRUNO)
            .count(),
        0,
        "their messages go too"
    );
    assert_eq!(toast(&app).unwrap().1, "Banned bruno");
    let bans = fake.bans(HIDEOUT);
    assert_eq!(
        (bans[0].user.id, bans[0].banned_by, bans[0].reason.as_str()),
        (BRUNO, TIM, "spam")
    );

    // The list on screen refreshes by itself after the ban; unbanning takes the row away.
    assert_eq!(app.state.bans_of(HIDEOUT).len(), 1);
    assert!(run(&mut app, |s| s.unban(HIDEOUT, BRUNO)));
    assert!(app.state.bans_of(HIDEOUT).is_empty() && fake.bans(HIDEOUT).is_empty());
    // Nothing to unban twice: the refusal is shown.
    app.act(|s| {
        s.outbox.push(Command::UnbanMember {
            guild: HIDEOUT,
            user: BRUNO,
        })
    });
    app.pump();
    assert!(toast(&app).unwrap().1.starts_with("Couldn't unban"));
    // An over-long span is refused by the server.
    app.act(|s| {
        s.outbox.push(Command::BanMember {
            guild: HIDEOUT,
            user: ANA,
            reason: String::new(),
            delete_seconds: Some(8 * 24 * 3600),
        })
    });
    app.pump();
    assert!(toast(&app).unwrap().1.starts_with("Couldn't ban"));
    assert!(fake.member_ids(HIDEOUT).contains(&ANA));
}

#[test]
fn a_banned_user_cannot_join_by_code() {
    let (mut app, fake) = tim();
    fake.set_role_permissions(
        COOKING_CLUB,
        COOK_MODS,
        Permissions::BAN_MEMBERS | Permissions::MANAGE_CHANNELS,
    );
    fake.add_member(COOKING_CLUB, ANA);
    // Visiting the guild brings its roles and who holds them.
    app.act(|s| s.go_guild(COOKING_CLUB));
    app.pump();
    assert!(run(&mut app, |s| s.ban(COOKING_CLUB, ANA, "", None)));
    assert!(fake.bans(COOKING_CLUB).iter().any(|b| b.user.id == ANA));
    assert!(!fake.member_ids(COOKING_CLUB).contains(&ANA));
    assert!(fake.is_banned(COOKING_CLUB, ANA));
}

#[test]
fn leaving_a_guild_drops_it_and_goes_home() {
    let (mut app, _) = tim();
    assert!(
        !run(&mut app, |s| s.leave_guild(HIDEOUT)),
        "the owner cannot leave"
    );
    app.act(|s| s.go_guild(GARAGE_BAND));
    app.pump();
    assert_eq!(app.state.ui.place, Place::Guild(GARAGE_BAND));
    assert!(run(&mut app, |s| s.leave_guild(GARAGE_BAND)));
    assert!(app.state.guild(GARAGE_BAND).is_none());
    assert!(app.state.channels_of(GARAGE_BAND).is_empty());
    assert_eq!(app.state.ui.place, Place::Home);
    assert!(app.state.data.exiting.is_empty());
    let toasts: Vec<_> = app
        .state
        .ui
        .toasts
        .iter()
        .map(|t| t.message.clone())
        .collect();
    assert_eq!(
        toasts,
        vec!["Left Garage Band".to_owned()],
        "the echo says nothing more"
    );
    // The other guilds are untouched.
    assert!(app.state.guild(HIDEOUT).is_some());
}

#[test]
fn being_removed_by_someone_else_drops_the_guild_with_a_note() {
    let (mut app, fake) = tim();
    app.act(|s| s.go_guild(GARAGE_BAND));
    app.pump();
    fake.remove_member_as_moderator(GARAGE_BAND, TIM, MemberRemovalReason::Banned);
    app.pump();
    assert!(app.state.guild(GARAGE_BAND).is_none());
    assert_eq!(
        app.state.ui.place,
        Place::Home,
        "the open guild is gone, so home"
    );
    let toasts: Vec<_> = app
        .state
        .ui
        .toasts
        .iter()
        .map(|t| (t.kind, t.message.clone()))
        .collect();
    assert_eq!(
        toasts,
        vec![(
            ToastKind::Info,
            "You were banned from Garage Band".to_owned()
        )]
    );

    // A guild removed from under us without a reason (deleted by its owner).
    fake.delete_guild_as_owner(COOKING_CLUB);
    app.pump();
    assert!(app.state.guild(COOKING_CLUB).is_none());
    assert_eq!(
        app.state.ui.toasts.back().unwrap().message,
        "Cooking Club was deleted"
    );
}

#[test]
fn deleting_a_channel_reselects_and_clears_voice() {
    let (mut app, fake) = tim();
    open(&mut app, DEV);
    assert_eq!(app.state.current_channel(), Some(DEV));
    assert!(
        !run(&mut app, |s| s.delete_channel(GARAGE_BAND, 20)),
        "no MANAGE_CHANNELS there... Tim is a plain member"
    );
    let version = app.state.data.layouts[&HIDEOUT].version;
    assert!(run(&mut app, |s| s.delete_channel(HIDEOUT, DEV)));
    assert!(app.state.channel(HIDEOUT, DEV).is_none() && fake.channel(DEV).is_none());
    assert!(app.state.log(DEV).is_none());
    assert!(app.state.data.layouts[&HIDEOUT].version > version);
    assert!(app
        .state
        .ordered_channels(HIDEOUT)
        .iter()
        .all(|c| c.id != DEV));
    let now = app
        .state
        .current_channel()
        .expect("a channel is open again");
    assert_ne!(now, DEV);
    assert_eq!(Some(now), app.state.first_text_channel(HIDEOUT));
    assert_eq!(toast(&app).unwrap().1, "Deleted #dev");

    // Everyone in a deleted voice channel leaves it.
    assert!(app.state.data.voice[&HIDEOUT].contains_key(&ANA));
    fake.delete_channel_as_moderator(HIDEOUT, LOUNGE);
    app.pump();
    assert!(app.state.channel(HIDEOUT, LOUNGE).is_none());
    assert!(app
        .state
        .data
        .voice
        .get(&HIDEOUT)
        .is_none_or(|v| v.is_empty()));
    // A channel somebody else deleted while we were reading it.
    open(&mut app, PLANS);
    fake.delete_channel_as_moderator(HIDEOUT, PLANS);
    app.pump();
    assert_ne!(app.state.current_channel(), Some(PLANS));
    assert!(app.state.channel(HIDEOUT, GAME_NIGHT).is_some());
}

#[test]
fn deleting_a_guild_needs_its_name_and_goes_home() {
    let (mut app, fake) = tim();
    assert!(
        !run(&mut app, |s| s.delete_guild(GARAGE_BAND, "Garage Band")),
        "not the owner"
    );
    assert!(!run(&mut app, |s| s.delete_guild(HIDEOUT, "the hideout")));
    assert!(!run(&mut app, |s| s.delete_guild(HIDEOUT, "")));
    assert!(sent(&fake)
        .iter()
        .all(|c| !matches!(c, Command::DeleteGuild { .. })));
    assert!(run(&mut app, |s| s.delete_guild(HIDEOUT, " The Hideout ")));
    assert!(app.state.guild(HIDEOUT).is_none() && fake.guild(HIDEOUT).is_none());
    assert!(app.state.channels_of(HIDEOUT).is_empty());
    assert_eq!(app.state.ui.place, Place::Home);
    assert!(app.state.data.exiting.is_empty());
    let toasts: Vec<_> = app
        .state
        .ui
        .toasts
        .iter()
        .map(|t| t.message.clone())
        .collect();
    assert_eq!(toasts, vec!["Deleted The Hideout".to_owned()]);
    // Another guild is still there and opens.
    app.act(|s| s.go_guild(GARAGE_BAND));
    assert_eq!(app.state.ui.place, Place::Guild(GARAGE_BAND));
}

#[test]
fn rotating_the_invite_code_replaces_it() {
    let (mut app, fake) = tim();
    let old = app.state.guild(HIDEOUT).unwrap().invite_code.clone();
    assert!(
        !run(&mut app, |s| s.rotate_invite(GARAGE_BAND)),
        "no MANAGE_SERVER there"
    );
    assert!(run(&mut app, |s| s.rotate_invite(HIDEOUT)));
    let new = app.state.guild(HIDEOUT).unwrap().invite_code.clone();
    assert_ne!(new, old);
    assert_eq!(fake.guild(HIDEOUT).unwrap().invite_code, new);
    assert!(toast(&app).unwrap().1.contains("old one no longer works"));
}

#[test]
fn transferring_ownership_moves_the_powers() {
    let (mut app, fake) = tim();
    app.state.want_members(HIDEOUT);
    app.pump();
    assert!(
        !run(&mut app, |s| s.transfer_ownership(
            HIDEOUT,
            TIM,
            "The Hideout"
        )),
        "not to yourself"
    );
    assert!(
        !run(&mut app, |s| s.transfer_ownership(
            HIDEOUT,
            424_242,
            "The Hideout"
        )),
        "not to a stranger"
    );
    assert!(
        !run(&mut app, |s| s.transfer_ownership(HIDEOUT, ANA, "Hideout")),
        "the name must match"
    );
    assert!(!run(&mut app, |s| s.transfer_ownership(
        GARAGE_BAND,
        TIM,
        "Garage Band"
    )));
    assert!(run(&mut app, |s| s.transfer_ownership(
        HIDEOUT,
        ANA,
        "The Hideout"
    )));
    assert_eq!(app.state.guild(HIDEOUT).unwrap().owner_id, ANA);
    assert_eq!(fake.guild(HIDEOUT).unwrap().owner_id, ANA);
    assert_eq!(toast(&app).unwrap().1, "ana now owns The Hideout");
    // Tim is an ordinary member now: no powers, and he may leave.
    assert!(!app.state.can_delete_guild(HIDEOUT) && !app.state.can_kick(HIDEOUT, BRUNO));
    assert!(app.state.can_leave_guild(HIDEOUT));
    assert!(!run(&mut app, |s| s.rotate_invite(HIDEOUT)));
}
