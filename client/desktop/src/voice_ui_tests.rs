//! The voice screens, driven through the real view with pointer input against the fake backend:
//! a voice row joins and never becomes the open channel, the voice bar and its Disconnect, the mute
//! and deafen buttons (state and accessible names), the participant menu and who is offered what,
//! and the Create channel dialog's Text / Voice choice. The state rules are in `voice_tests`.

use rune::Runtime;
use serde_json::Value;
use thehideout_api::ChannelKind;

use crate::app::App;
use crate::backend::{Backend, Command};
use crate::fake::{FakeBackend, ANA, BRUNO, GAME_NIGHT, GENERAL, LOUNGE};
use crate::forms::DialogKind;
use crate::scenes::voice_screen_as;
use crate::state::{AppState, Menu};
use crate::tests::{click, has, rect_of, runtime, settle};
use crate::voice::VoiceSession;

const G: u64 = 1;

fn start(who: &str) -> (Runtime<App>, FakeBackend) {
    let (app, fake) = voice_screen_as(true, who);
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

/// The accessibility node keyed `key`.
fn node(rt: &Runtime<App>, key: &str) -> Value {
    let tree = rt.accessibility_json();
    let mut all = Vec::new();
    walk(&tree, &mut all);
    all.into_iter()
        .find(|n| n["key"] == key)
        .unwrap_or_else(|| panic!("no accessible element keyed {key}"))
        .clone()
}

fn name_of(rt: &Runtime<App>, key: &str) -> String {
    node(rt, key)["name"].as_str().unwrap_or("").to_owned()
}

/// The names of the open menu's items.
fn menu_items(rt: &Runtime<App>) -> Vec<String> {
    let tree = rt.accessibility_json();
    let mut all = Vec::new();
    walk(&tree, &mut all);
    all.into_iter()
        .filter(|n| n["role"] == "menu-item")
        .filter_map(|n| n["name"].as_str().map(str::to_owned))
        .collect()
}

fn click_item(rt: &mut Runtime<App>, name: &str) {
    let tree = rt.accessibility_json();
    let mut all = Vec::new();
    walk(&tree, &mut all);
    let key = all
        .into_iter()
        .find(|n| n["role"] == "menu-item" && n["name"] == name)
        .and_then(|n| n["key"].as_str().map(str::to_owned))
        .unwrap_or_else(|| panic!("no menu item {name}"));
    click(rt, &key);
    settle(rt, 10);
}

/// A handler's way in: runs `f` on the app state, wakes the view and lets it settle.
fn act(rt: &mut Runtime<App>, f: impl FnOnce(&mut AppState)) {
    rt.update(|a, _| a.act(f));
    settle(rt, 6);
}

fn press(rt: &mut Runtime<App>, key: &str) {
    click(rt, key);
    settle(rt, 12);
}

fn join_lounge(rt: &mut Runtime<App>) {
    press(rt, "channels-1-row-lounge");
}

fn session(rt: &Runtime<App>) -> VoiceSession {
    rt.state.state.voice.session.clone()
}

// ---- the sidebar -----------------------------------------------------------------------------

#[test]
fn voice_channels_list_their_people_under_the_row() {
    let (rt, _fake) = start("tim");
    assert!(has(&rt, "channels-1-row-lounge"));
    // Ana and Bruno, in the order they joined, indented under it; game-night is empty.
    let (lounge, ana, bruno) = (
        rect_of(&rt, "channels-1-row-lounge"),
        rect_of(&rt, &format!("voice-user-{ANA}")),
        rect_of(&rt, &format!("voice-user-{BRUNO}")),
    );
    assert!(
        lounge[1] < ana[1] && ana[1] < bruno[1],
        "{lounge:?} {ana:?} {bruno:?}"
    );
    assert!(!has(&rt, &format!("voice-user-{}", crate::fake::TIM)));
    let game_night = rect_of(&rt, "channels-1-row-game-night");
    assert!(
        game_night[1] > bruno[1],
        "game-night sits below the lounge's people"
    );
    // Rows span the same width: the indent is inside them, so the hover wash lines up.
    assert_eq!((ana[0], ana[2]), (lounge[0], lounge[2]));
    // Bruno muted himself, and says so to assistive technology.
    assert_eq!(
        node(&rt, &format!("voice-user-{BRUNO}"))["description"],
        "muted"
    );
    assert!(node(&rt, &format!("voice-user-{ANA}"))["description"].is_null());
    // Nobody is shown as speaking until the audio engine says so.
    assert!(rt.state.state.voice.speaking.is_empty());
}

#[test]
fn clicking_a_voice_row_joins_and_keeps_the_text_channel() {
    let (mut rt, fake) = start("tim");
    assert_eq!(rt.state.state.current_channel(), Some(GENERAL));
    join_lounge(&mut rt);
    assert_eq!(rt.state.state.voice.session.channel(), Some(LOUNGE));
    assert!(matches!(session(&rt), VoiceSession::Connected { .. }));
    assert_eq!(
        rt.state.state.current_channel(),
        Some(GENERAL),
        "the text channel stays open"
    );
    // The lounge reads as connected, general is still the selected row.
    assert_eq!(node(&rt, "channels-1-row-lounge")["value"], "connected");
    assert_eq!(node(&rt, "channels-1-row-general")["value"], "selected");
    // Tim is listed among the people in it.
    assert!(has(&rt, &format!("voice-user-{}", crate::fake::TIM)));
    // Pressing it again, or double-clicking, changes nothing and sends nothing more.
    let joins = |f: &FakeBackend| {
        sent(f)
            .iter()
            .filter(|c| matches!(c, Command::JoinVoice { .. }))
            .count()
    };
    assert_eq!(joins(&fake), 1);
    press(&mut rt, "channels-1-row-lounge");
    press(&mut rt, "channels-1-row-lounge");
    assert_eq!(joins(&fake), 1);
    assert!(matches!(session(&rt), VoiceSession::Connected { .. }));
    assert_eq!(rt.state.state.current_channel(), Some(GENERAL));
}

#[test]
fn a_voice_row_is_never_the_selected_row() {
    let (mut rt, _fake) = start("tim");
    assert!(node(&rt, "channels-1-row-lounge")["value"].is_null());
    join_lounge(&mut rt);
    press(&mut rt, "channels-1-row-game-night");
    assert_eq!(
        rt.state.state.voice.session.channel(),
        Some(GAME_NIGHT),
        "the other voice channel switches calls"
    );
    for key in ["channels-1-row-lounge", "channels-1-row-game-night"] {
        assert_ne!(node(&rt, key)["value"], "selected", "{key}");
    }
    assert_eq!(node(&rt, "channels-1-row-general")["value"], "selected");
}

#[test]
fn a_voice_channels_menu_offers_join_and_disconnect_to_everyone() {
    // A plain member has no structure entries, only Join / Disconnect.
    let (mut rt, _fake) = start("ana");
    assert!(
        !has(&rt, &format!("channel-more-{LOUNGE}")),
        "no manager button"
    );
    let at = rt.find("channels-1-row-lounge").unwrap();
    rt.secondary_down(at.0, at.1);
    rt.secondary_up(at.0, at.1);
    settle(&mut rt, 6);
    assert_eq!(menu_items(&rt), ["Join"]);
    click_item(&mut rt, "Join");
    assert_eq!(rt.state.state.voice.session.channel(), Some(LOUNGE));
    let at = rt.find("channels-1-row-lounge").unwrap();
    rt.secondary_down(at.0, at.1);
    rt.secondary_up(at.0, at.1);
    settle(&mut rt, 6);
    assert_eq!(menu_items(&rt), ["Disconnect"]);
    click_item(&mut rt, "Disconnect");
    assert_eq!(session(&rt), VoiceSession::None);

    // The owner gets them first, then the usual edit entries.
    let (mut rt, _fake) = start("tim");
    let at = rt.find("channels-1-row-lounge").unwrap();
    rt.secondary_down(at.0, at.1);
    rt.secondary_up(at.0, at.1);
    settle(&mut rt, 6);
    let items = menu_items(&rt);
    assert_eq!(items.first().map(String::as_str), Some("Join"), "{items:?}");
    assert!(
        items.iter().any(|i| i == "Edit channel\u{2026}"),
        "{items:?}"
    );
}

// ---- the voice bar and the strip -------------------------------------------------------------

#[test]
fn the_voice_bar_shows_while_in_voice_and_disconnect_leaves() {
    let (mut rt, fake) = start("tim");
    assert!(!has(&rt, "voice-bar"), "no bar before joining");
    join_lounge(&mut rt);
    assert!(has(&rt, "voice-bar"));
    assert_eq!(name_of(&rt, "voice-disconnect"), "Disconnect");
    assert_eq!(node(&rt, "voice-disconnect")["role"], "button");
    assert!(
        name_of(&rt, "voice-bar").contains("lounge")
            && name_of(&rt, "voice-bar").contains("The Hideout"),
        "{}",
        name_of(&rt, "voice-bar")
    );
    // It sits just above the user strip, inside the sidebar's width.
    let (bar, strip) = (rect_of(&rt, "voice-bar"), rect_of(&rt, "user-strip"));
    assert!(bar[1] + bar[3] <= strip[1] + 0.5, "{bar:?} over {strip:?}");
    assert!(bar[0] >= 0.0 && bar[0] + bar[2] <= strip[0] + strip[2]);

    press(&mut rt, "voice-disconnect");
    assert_eq!(session(&rt), VoiceSession::None);
    assert!(!has(&rt, "voice-bar"));
    assert!(sent(&fake).iter().any(|c| matches!(c, Command::LeaveVoice)));
    assert!(
        !has(&rt, &format!("voice-user-{}", crate::fake::TIM)),
        "gone from the list"
    );
}

#[test]
fn the_voice_bar_says_connecting_while_the_join_is_out() {
    let (mut rt, fake) = start("tim");
    fake.set_latency(5.0);
    click(&mut rt, "channels-1-row-lounge");
    settle(&mut rt, 8);
    assert!(matches!(session(&rt), VoiceSession::Joining { .. }));
    assert!(has(&rt, "voice-bar"));
    assert_eq!(
        name_of(&rt, "voice-bar"),
        "Connecting to voice, lounge, The Hideout"
    );
}

#[test]
fn mute_and_deafen_toggle_and_rename_themselves() {
    let (mut rt, _fake) = start("tim");
    // Always there, in voice or not.
    assert_eq!(name_of(&rt, "voice-mute"), "Mute");
    assert_eq!(name_of(&rt, "voice-deafen"), "Deafen");
    join_lounge(&mut rt);

    press(&mut rt, "voice-mute");
    assert!(rt.state.state.voice.self_mute);
    assert_eq!(name_of(&rt, "voice-mute"), "Unmute");
    press(&mut rt, "voice-mute");
    assert!(!rt.state.state.voice.self_mute);
    assert_eq!(name_of(&rt, "voice-mute"), "Mute");

    press(&mut rt, "voice-deafen");
    let v = &rt.state.state.voice;
    assert!(v.self_deaf && v.self_mute, "deafening mutes too");
    assert_eq!(name_of(&rt, "voice-deafen"), "Undeafen");
    assert_eq!(name_of(&rt, "voice-mute"), "Unmute");
    // His own row shows both, to the screen reader as well.
    assert_eq!(
        node(&rt, &format!("voice-user-{}", crate::fake::TIM))["description"],
        "deafened"
    );
    press(&mut rt, "voice-deafen");
    let v = &rt.state.state.voice;
    assert!(!v.self_deaf && !v.self_mute);
    assert_eq!(name_of(&rt, "voice-deafen"), "Deafen");
}

#[test]
fn the_toggles_work_before_joining_and_apply_to_the_next_call() {
    let (mut rt, fake) = start("tim");
    press(&mut rt, "voice-mute");
    assert!(rt.state.state.voice.self_mute);
    assert!(
        !sent(&fake)
            .iter()
            .any(|c| matches!(c, Command::UpdateVoiceState { .. })),
        "nothing to tell the server while out of voice"
    );
    join_lounge(&mut rt);
    assert!(rt
        .state
        .state
        .voice_state_of(crate::fake::TIM)
        .is_some_and(|s| s.self_mute));
}

#[test]
fn a_moderators_mute_disables_ours_and_says_why() {
    let (mut rt, _fake) = start("tim");
    join_lounge(&mut rt);
    act(&mut rt, |s| {
        let me = s.my_id();
        s.data.voice.values_mut().for_each(|m| {
            if let Some(v) = m.get_mut(&me) {
                v.server_mute = true;
            }
        });
    });
    let mute = node(&rt, "voice-mute");
    assert_eq!(mute["description"], "Muted by a moderator");
    assert!(
        mute["disabled"] == true
            || !mute["actions"]
                .as_array()
                .is_some_and(|a| a.iter().any(|x| x == "click")),
        "{mute}"
    );
    // Pressing it does nothing.
    let before = rt.state.state.voice.self_mute;
    click(&mut rt, "voice-mute");
    settle(&mut rt, 6);
    assert_eq!(rt.state.state.voice.self_mute, before);
    assert_eq!(
        node(&rt, &format!("voice-user-{}", crate::fake::TIM))["description"],
        "muted by a moderator"
    );
}

// ---- the participant menu --------------------------------------------------------------------

#[test]
fn the_owner_is_offered_every_moderation_entry_for_someone_else() {
    let (mut rt, _fake) = start("tim");
    press(&mut rt, &format!("voice-user-{BRUNO}"));
    assert!(matches!(
        rt.state.state.ui.menu,
        Menu::Participant { user: BRUNO, .. }
    ));
    assert_eq!(
        menu_items(&rt),
        ["Mute for me", "Server mute", "Move to", "Disconnect"]
    );
}

#[test]
fn a_plain_member_only_gets_mute_for_me() {
    let (mut rt, fake) = start("ana");
    press(&mut rt, &format!("voice-user-{BRUNO}"));
    assert_eq!(menu_items(&rt), ["Mute for me"]);
    // For yourself there is nothing to open at all.
    click_item(&mut rt, "Mute for me");
    assert!(rt.state.state.is_muted_for_me(BRUNO));
    assert!(
        !sent(&fake).iter().any(|c| matches!(
            c,
            Command::ServerMuteMember { .. } | Command::UpdateVoiceState { .. }
        )),
        "a local flag: nothing is sent"
    );
    // The row says so, and the entry flips.
    assert!(node(&rt, &format!("voice-user-{BRUNO}"))["description"]
        .as_str()
        .is_some_and(|d| d.contains("muted for you")));
    press(&mut rt, &format!("voice-user-{BRUNO}"));
    assert_eq!(menu_items(&rt), ["Unmute for me"]);
}

#[test]
fn your_own_row_opens_nothing_without_the_right_to_move_yourself() {
    let (mut rt, _fake) = start("ana");
    join_lounge(&mut rt);
    let key = format!("voice-user-{ANA}");
    press(&mut rt, &key);
    assert_eq!(rt.state.state.ui.menu, Menu::None);
    assert!(menu_items(&rt).is_empty());
}

#[test]
fn a_moderator_server_mutes_and_disconnects_through_the_menu() {
    let (mut rt, fake) = start("tim");
    press(&mut rt, &format!("voice-user-{ANA}"));
    click_item(&mut rt, "Server mute");
    assert!(sent(&fake).iter().any(|c| matches!(
        c,
        Command::ServerMuteMember {
            user: ANA,
            muted: true,
            ..
        }
    )));
    assert!(rt
        .state
        .state
        .voice_state_of(ANA)
        .is_some_and(|s| s.server_mute));
    assert_eq!(rt.state.state.ui.menu, Menu::None, "the menu closes");
    // The row shows the danger glyph (and says so) and the entry now unmutes.
    assert_eq!(
        node(&rt, &format!("voice-user-{ANA}"))["description"],
        "muted by a moderator"
    );
    press(&mut rt, &format!("voice-user-{ANA}"));
    assert!(menu_items(&rt).contains(&"Server unmute".to_owned()));
    click_item(&mut rt, "Server unmute");
    assert!(rt
        .state
        .state
        .voice_state_of(ANA)
        .is_some_and(|s| !s.server_mute));

    press(&mut rt, &format!("voice-user-{ANA}"));
    click_item(&mut rt, "Disconnect");
    assert!(sent(&fake)
        .iter()
        .any(|c| matches!(c, Command::DisconnectMember { user: ANA, .. })));
    assert!(rt.state.state.voice_state_of(ANA).is_none());
    assert!(!has(&rt, &format!("voice-user-{ANA}")));
}

#[test]
fn a_moderator_moves_someone_to_another_voice_channel() {
    let (mut rt, fake) = start("tim");
    press(&mut rt, &format!("voice-user-{BRUNO}"));
    click_item(&mut rt, "Move to");
    // The submenu lists the other voice channels only.
    assert_eq!(
        menu_items(&rt)[..4],
        ["Mute for me", "Server mute", "Move to", "Disconnect"]
    );
    assert!(
        menu_items(&rt).iter().any(|i| i == "game-night"),
        "{:?}",
        menu_items(&rt)
    );
    assert!(!menu_items(&rt).iter().any(|i| i == "lounge"));
    click_item(&mut rt, "game-night");
    assert!(sent(&fake).iter().any(|c| matches!(
        c,
        Command::MoveMember {
            user: BRUNO,
            channel: GAME_NIGHT,
            ..
        }
    )));
    assert_eq!(
        rt.state.state.voice_state_of(BRUNO).map(|s| s.channel_id),
        Some(GAME_NIGHT)
    );
    assert!(has(&rt, &format!("voice-user-{BRUNO}")));
    let (game, user) = (
        rect_of(&rt, "channels-1-row-game-night"),
        rect_of(&rt, &format!("voice-user-{BRUNO}")),
    );
    assert!(user[1] > game[1], "now listed under game-night");
}

// ---- the Create channel dialog ---------------------------------------------------------------

#[test]
fn the_create_dialog_offers_text_or_voice_and_creates_a_voice_channel() {
    let (mut rt, fake) = start("tim");
    act(&mut rt, |s| {
        s.open_dialog(DialogKind::CreateChannel { guild: G })
    });
    settle(&mut rt, 8);
    // Text is the default; Voice is the second segment of the radio group.
    assert!(has(&rt, "kind-field") && has(&rt, "segment0") && has(&rt, "segment1"));
    assert_eq!(
        rt.state.state.channel_form().map(|f| f.kind),
        Some(ChannelKind::Text)
    );
    click(&mut rt, "segment1");
    settle(&mut rt, 6);
    assert_eq!(
        rt.state.state.channel_form().map(|f| f.kind),
        Some(ChannelKind::Voice)
    );
    act(&mut rt, |s| {
        s.ui.dialog
            .as_mut()
            .unwrap()
            .input
            .replace_text("Karaoke Night")
    });
    click(&mut rt, "dialog-submit");
    settle(&mut rt, 12);
    assert!(sent(&fake).iter().any(|c| matches!(
        c,
        Command::CreateChannel { name, kind: ChannelKind::Voice, .. } if name == "karaoke-night"
    )));
    let made = rt
        .state
        .state
        .channels_of(G)
        .iter()
        .find(|c| c.name == "karaoke-night")
        .cloned()
        .expect("the channel arrived");
    assert_eq!(made.kind, ChannelKind::Voice);
    assert!(rt.state.state.ui.dialog.is_none());
    assert_eq!(
        rt.state.state.current_channel(),
        Some(GENERAL),
        "not opened"
    );
    assert_eq!(session(&rt), VoiceSession::None, "nor joined");
}

#[test]
fn the_edit_dialog_has_no_type_choice_and_no_topic_for_voice() {
    let (mut rt, _fake) = start("tim");
    act(&mut rt, |s| {
        s.open_dialog(DialogKind::ChannelSettings {
            guild: G,
            channel: LOUNGE,
        })
    });
    assert!(!has(&rt, "kind-field"), "a channel's kind never changes");
    let tree = rt.accessibility_json().to_string();
    assert!(
        !tree.contains("What is this channel about?"),
        "no topic field for voice"
    );
}

// ---- the speaking ring ----------------------------------------------------------------------

#[test]
fn the_engine_feeds_who_is_speaking() {
    let (mut rt, _fake) = start("tim");
    act(&mut rt, |s| s.set_speaking([ANA]));
    assert!(rt.state.state.is_speaking(ANA) && !rt.state.state.is_speaking(BRUNO));
    assert_eq!(
        node(&rt, &format!("voice-user-{ANA}"))["description"],
        "speaking"
    );
    act(&mut rt, |s| s.set_speaking([]));
    assert!(node(&rt, &format!("voice-user-{ANA}"))["description"].is_null());
}
