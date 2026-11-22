//! Proofs of voice against the fake backend: the fake world's rules (the server's), how gateway
//! events become state, join / switch / leave and the intents the audio engine gets, the mute and
//! deafen flags, who may moderate whom, and that a voice channel is joined, never opened as text.
//! All state-level: no view is drawn (the UI comes later).

use std::cell::RefCell;
use std::rc::Rc;

use thehideout_api::{
    ApiError, ChannelIconRef, ChannelKind, Event, GatewayUpdate, PaletteColor, Permissions,
    VoiceJoin, VoiceState,
};
use thehideout_design::components::ToastKind;

use crate::app::App;
use crate::backend::{Backend, Command, Reply};
use crate::fake::{
    FakeBackend, ANA, BRUNO, CVETKA, GAME_NIGHT, GENERAL, HIDEOUT_MODS, LOUNGE, RANDOM, TIM,
};
use crate::forms::DialogKind;
use crate::scenes::{booted, booted_as};
use crate::voice::{VoiceIntent, VoiceSession, VoiceSink};

const G: u64 = 1;
const COOKING: u64 = 3;
/// A text channel of Cooking Club, which Tim is not the owner of.
const RECIPES: u64 = 30;

// ---- helpers ---------------------------------------------------------------------------------

/// A sink that records what the audio engine would have been told.
#[derive(Clone, Default)]
struct Engine(Rc<RefCell<Vec<VoiceIntent>>>);

impl VoiceSink for Engine {
    fn intent(&mut self, intent: VoiceIntent) {
        self.0.borrow_mut().push(intent);
    }
}

impl Engine {
    /// What arrived since the last call.
    fn take(&self) -> Vec<VoiceIntent> {
        std::mem::take(&mut self.0.borrow_mut())
    }
}

fn wired(who: &str) -> (App, FakeBackend, Engine) {
    let (mut app, fake) = booted_as(true, who);
    let engine = Engine::default();
    app.set_voice_sink(Box::new(engine.clone()));
    (app, fake, engine)
}

fn tim() -> (App, FakeBackend, Engine) {
    wired("tim")
}

/// Sends a command straight to the fake world and returns its replies.
fn ask(fake: &FakeBackend, cmd: Command) -> Vec<Reply> {
    let mut f = fake.clone();
    f.submit(cmd);
    f.poll(0.0)
}

fn join(fake: &FakeBackend, channel: u64) -> Result<VoiceJoin, ApiError> {
    match ask(fake, Command::JoinVoice { channel }).into_iter().next() {
        Some(Reply::VoiceJoined { result, .. }) => result,
        other => panic!("not a join answer: {other:?}"),
    }
}

fn moderate(fake: &FakeBackend, cmd: Command) -> Result<(), ApiError> {
    match ask(fake, cmd).into_iter().next() {
        Some(Reply::VoiceModerated { result, .. }) => result,
        other => panic!("not a moderation answer: {other:?}"),
    }
}

fn mute(fake: &FakeBackend, user: u64, muted: bool) -> Result<(), ApiError> {
    moderate(
        fake,
        Command::ServerMuteMember {
            guild: G,
            user,
            muted,
        },
    )
}

fn move_to(fake: &FakeBackend, user: u64, channel: u64) -> Result<(), ApiError> {
    moderate(
        fake,
        Command::MoveMember {
            guild: G,
            user,
            channel,
        },
    )
}

fn ids(states: &[VoiceState]) -> Vec<u64> {
    states.iter().map(|s| s.user_id).collect()
}

/// Who the app shows in `channel`, in order.
fn who(app: &App, channel: u64) -> Vec<u64> {
    app.state
        .voice_participants(channel)
        .iter()
        .map(|s| s.user_id)
        .collect()
}

fn sent(fake: &FakeBackend) -> Vec<Command> {
    Backend::sent(&fake.clone())
}

fn sent_voice(fake: &FakeBackend) -> Vec<Command> {
    sent(fake)
        .into_iter()
        .filter(|c| {
            matches!(
                c,
                Command::JoinVoice { .. }
                    | Command::LeaveVoice
                    | Command::UpdateVoiceState { .. }
                    | Command::ServerMuteMember { .. }
                    | Command::MoveMember { .. }
                    | Command::DisconnectMember { .. }
            )
        })
        .collect()
}

fn connect() -> VoiceIntent {
    VoiceIntent::Connect {
        url: "ws://fake".into(),
        token: "fake".into(),
    }
}

/// A new voice channel in Cooking Club (Tim moderates it), by the fake world's own rules.
fn kitchen_call(fake: &FakeBackend) -> u64 {
    let replies = ask(
        fake,
        Command::CreateChannel {
            guild: COOKING,
            name: "kitchen-call".into(),
            kind: ChannelKind::Voice,
            section: None,
            icon: ChannelIconRef::Default,
            color: PaletteColor::None,
        },
    );
    match replies.into_iter().next() {
        Some(Reply::ChannelCreated(Ok(c))) => {
            assert_eq!(c.kind, ChannelKind::Voice);
            c.id
        }
        other => panic!("channel not created: {other:?}"),
    }
}

/// Bruno, the Hideout moderator (above Ana, below Cvetka), who may also mute and move.
fn bruno_the_moderator() -> (App, FakeBackend, Engine) {
    let (mut app, fake, engine) = wired("bruno");
    fake.set_role_permissions(
        G,
        HIDEOUT_MODS,
        Permissions::MANAGE_CHANNELS | Permissions::MUTE_MEMBERS | Permissions::MOVE_MEMBERS,
    );
    app.pump();
    (app, fake, engine)
}

// ---- the fake world's rules ------------------------------------------------------------------

#[test]
fn the_scene_has_a_lounge_with_ana_and_a_muted_bruno_and_an_empty_game_night() {
    let (app, fake) = booted(true);
    assert_eq!(who(&app, LOUNGE), [ANA, BRUNO], "in the order they joined");
    assert!(who(&app, GAME_NIGHT).is_empty());
    let bruno = app.state.voice_state_of(BRUNO).unwrap();
    assert!(bruno.self_mute && !bruno.self_deaf && !bruno.server_mute);
    assert!(!app.state.voice_state_of(ANA).unwrap().self_mute);
    assert_eq!(fake.channel(LOUNGE).unwrap().kind, ChannelKind::Voice);
    assert_eq!(fake.channel(GENERAL).unwrap().kind, ChannelKind::Text);
}

#[test]
fn joining_follows_the_servers_rules() {
    let (_app, fake) = booted(true);
    assert_eq!(
        join(&fake, GENERAL),
        Err(ApiError::Precondition("not a voice channel".into()))
    );
    assert_eq!(join(&fake, 9999), Err(ApiError::NotFound));
    let joined = join(&fake, LOUNGE).unwrap();
    assert_eq!(
        joined,
        VoiceJoin {
            url: "ws://fake".into(),
            token: "fake".into(),
            channel_id: LOUNGE
        }
    );
    assert_eq!(ids(&fake.voice_states(G)), [ANA, BRUNO, TIM]);
    // Joining where you already are keeps your place in the list.
    join(&fake, LOUNGE).unwrap();
    assert_eq!(ids(&fake.voice_states(G)), [ANA, BRUNO, TIM]);

    // Someone who isn't in the guild finds neither the channel nor a way in.
    let (_app, fake) = booted_as(true, "ana");
    assert_eq!(join(&fake, RECIPES), Err(ApiError::NotFound));
    fake.voice_unavailable(true);
    assert_eq!(
        join(&fake, LOUNGE),
        Err(ApiError::Network(
            "voice is not configured on this server".into()
        ))
    );
}

#[test]
fn a_user_is_in_one_voice_channel_and_the_mute_flags_survive_a_move_within_the_guild() {
    let (_app, fake) = booted(true);
    join(&fake, LOUNGE).unwrap();
    ask(
        &fake,
        Command::UpdateVoiceState {
            self_mute: true,
            self_deaf: false,
        },
    );
    mute(&fake, ANA, true).unwrap();
    let before = fake.voice_states(G);
    assert!(before.iter().find(|s| s.user_id == TIM).unwrap().self_mute);

    // Another channel of the guild: one state, flags kept (the server mute too).
    join(&fake, GAME_NIGHT).unwrap();
    let after = fake.voice_states(G);
    assert_eq!(after.len(), 3);
    let me = after.iter().find(|s| s.user_id == TIM).unwrap();
    assert_eq!((me.channel_id, me.self_mute), (GAME_NIGHT, true));
    move_to(&fake, ANA, GAME_NIGHT).unwrap();
    assert!(
        fake.voice_states(G)
            .iter()
            .find(|s| s.user_id == ANA)
            .unwrap()
            .server_mute
    );

    // Another guild: out of the old one (announced), fresh flags there.
    let kitchen = kitchen_call(&fake);
    let replies = ask(&fake, Command::JoinVoice { channel: kitchen });
    assert!(matches!(
        &replies[0],
        Reply::VoiceJoined { result: Ok(_), .. }
    ));
    assert!(replies.iter().any(|r| matches!(
        r,
        Reply::Gateway(GatewayUpdate::Event(Event::VoiceStateUpdate {
            guild_id: G,
            user_id: TIM,
            state: None
        }))
    )));
    assert!(!ids(&fake.voice_states(G)).contains(&TIM));
    let there = fake.voice_states(COOKING);
    assert_eq!((ids(&there), there[0].self_mute), (vec![TIM], false));
}

#[test]
fn leaving_flags_and_messages_follow_the_servers_rules() {
    let (_app, fake) = booted(true);
    // Not in voice: no flags to change.
    let r = ask(
        &fake,
        Command::UpdateVoiceState {
            self_mute: true,
            self_deaf: false,
        },
    );
    assert!(matches!(
        &r[0],
        Reply::VoiceFlags(Err(ApiError::Precondition(_)))
    ));
    // Leaving when in none is fine and says nothing.
    let r = ask(&fake, Command::LeaveVoice);
    assert_eq!(r, vec![Reply::VoiceLeft(Ok(()))]);

    join(&fake, LOUNGE).unwrap();
    let r = ask(
        &fake,
        Command::UpdateVoiceState {
            self_mute: true,
            self_deaf: true,
        },
    );
    assert!(matches!(&r[0], Reply::VoiceFlags(Ok(s)) if s.self_mute && s.self_deaf));
    let r = ask(&fake, Command::LeaveVoice);
    assert!(matches!(
        &r[1],
        Reply::Gateway(GatewayUpdate::Event(Event::VoiceStateUpdate {
            user_id: TIM,
            state: None,
            ..
        }))
    ));
    assert_eq!(ids(&fake.voice_states(G)), [ANA, BRUNO]);

    // A voice channel has no messages.
    let r = ask(
        &fake,
        Command::Send {
            channel: LOUNGE,
            content: "hi".into(),
            nonce: "n".into(),
        },
    );
    assert!(matches!(
        &r[0],
        Reply::Sent {
            result: Err(ApiError::Precondition(_)),
            ..
        }
    ));
}

#[test]
fn moderation_needs_the_permission_and_a_higher_role_and_a_target_in_voice() {
    let (_app, fake) = booted(true);
    // Tim owns The Hideout.
    mute(&fake, ANA, true).unwrap();
    assert!(fake.voice_states(G)[0].server_mute);
    mute(&fake, ANA, false).unwrap();
    assert!(!fake.voice_states(G)[0].server_mute);
    assert!(
        matches!(mute(&fake, TIM, true), Err(ApiError::InvalidArgument(_))),
        "not yourself"
    );
    assert!(
        matches!(mute(&fake, CVETKA, true), Err(ApiError::Precondition(_))),
        "not in voice"
    );
    assert!(matches!(mute(&fake, 99, true), Err(ApiError::NotFound)));
    // In Garage Band Tim is a Roadie: roles, not voice.
    let r = moderate(
        &fake,
        Command::ServerMuteMember {
            guild: 2,
            user: ANA,
            muted: true,
        },
    );
    assert_eq!(r, Err(ApiError::PermissionDenied));

    // Moves: to a voice channel of the guild only.
    assert!(matches!(
        move_to(&fake, ANA, GENERAL),
        Err(ApiError::Precondition(_))
    ));
    assert_eq!(move_to(&fake, ANA, RECIPES), Err(ApiError::NotFound));
    move_to(&fake, ANA, GAME_NIGHT).unwrap();
    assert_eq!(
        fake.voice_states(G)
            .iter()
            .find(|s| s.user_id == ANA)
            .unwrap()
            .channel_id,
        GAME_NIGHT
    );
    // Already there: fine, nothing changes.
    move_to(&fake, ANA, GAME_NIGHT).unwrap();

    // Disconnecting removes them and says so.
    let r = ask(
        &fake,
        Command::DisconnectMember {
            guild: G,
            user: BRUNO,
        },
    );
    assert_eq!(
        r[0],
        Reply::VoiceModerated {
            guild: G,
            user: BRUNO,
            result: Ok(())
        }
    );
    assert!(matches!(
        &r[1],
        Reply::Gateway(GatewayUpdate::Event(Event::VoiceStateUpdate {
            user_id: BRUNO,
            state: None,
            ..
        }))
    ));
    assert_eq!(ids(&fake.voice_states(G)), [ANA]);

    // Moving ourselves tells our own client to rejoin.
    join(&fake, LOUNGE).unwrap();
    let r = ask(
        &fake,
        Command::MoveMember {
            guild: G,
            user: TIM,
            channel: GAME_NIGHT,
        },
    );
    assert!(r.iter().any(|r| matches!(
        r,
        Reply::Gateway(GatewayUpdate::Event(Event::VoiceMoved {
            guild_id: G,
            channel_id: GAME_NIGHT
        }))
    )));
}

#[test]
fn a_moderator_outranks_those_below_and_nobody_outranks_the_owner() {
    let (_app, fake, _) = bruno_the_moderator();
    fake.voice_join_as(CVETKA, LOUNGE);
    fake.voice_join_as(TIM, LOUNGE);
    // The announcements are for the app; they are not the answers asked for below.
    let _ = ask(&fake, Command::Refresh);
    // Bruno (Moderator, 2) over Ana (Designer, 1): yes. Over Cvetka (Admin, 3) and Tim (owner): no.
    mute(&fake, ANA, true).unwrap();
    assert_eq!(mute(&fake, CVETKA, true), Err(ApiError::PermissionDenied));
    assert_eq!(mute(&fake, TIM, true), Err(ApiError::PermissionDenied));
    assert_eq!(
        moderate(
            &fake,
            Command::DisconnectMember {
                guild: G,
                user: CVETKA
            }
        ),
        Err(ApiError::PermissionDenied)
    );
    assert_eq!(
        move_to(&fake, TIM, GAME_NIGHT),
        Err(ApiError::PermissionDenied)
    );
    move_to(&fake, ANA, GAME_NIGHT).unwrap();

    // Ana holds neither permission.
    let (_app, fake) = booted_as(true, "ana");
    assert_eq!(mute(&fake, BRUNO, true), Err(ApiError::PermissionDenied));
    assert_eq!(
        move_to(&fake, BRUNO, GAME_NIGHT),
        Err(ApiError::PermissionDenied)
    );
}

#[test]
fn a_new_stream_starts_with_the_snapshot_when_anyone_is_in_voice() {
    let (_app, fake) = booted(true);
    let r = ask(&fake, Command::LoadInitial);
    let snapshot = r.iter().find_map(|r| match r {
        Reply::Gateway(GatewayUpdate::Event(Event::VoiceSnapshot { guild_ids, states })) => {
            Some((guild_ids, states))
        }
        _ => None,
    });
    let (guilds, states) = snapshot.expect("a snapshot follows the first connect");
    assert_eq!(
        guilds,
        &[1, 2, 3],
        "every guild of the caller, whoever is in it"
    );
    assert_eq!(ids(states), [ANA, BRUNO]);

    // Nobody in voice: no snapshot at all.
    fake.voice_leave_as(ANA);
    fake.voice_leave_as(BRUNO);
    let _ = ask(&fake, Command::Refresh);
    let r = ask(&fake, Command::LoadInitial);
    assert!(!r.iter().any(|r| matches!(
        r,
        Reply::Gateway(GatewayUpdate::Event(Event::VoiceSnapshot { .. }))
    )));
}

// ---- gateway events become state -------------------------------------------------------------

#[test]
fn state_updates_add_move_and_remove_people() {
    let (mut app, fake, _) = tim();
    fake.voice_join_as(CVETKA, LOUNGE);
    app.pump();
    assert_eq!(who(&app, LOUNGE), [ANA, BRUNO, CVETKA]);

    // One channel at a time: switching moves them.
    fake.voice_join_as(ANA, GAME_NIGHT);
    app.pump();
    assert_eq!(
        (who(&app, LOUNGE), who(&app, GAME_NIGHT)),
        (vec![BRUNO, CVETKA], vec![ANA])
    );
    assert_eq!(app.state.data.voice[&G].len(), 3, "one state per user");

    // Leaving (state unset) removes them.
    fake.voice_leave_as(BRUNO);
    app.pump();
    assert_eq!(who(&app, LOUNGE), [CVETKA]);
    assert!(app.state.voice_state_of(BRUNO).is_none());

    // A flag change replaces the state in place.
    app.act(|s| s.toggle_mute());
    fake.voice_join_as(TIM, LOUNGE);
    app.pump();
    assert_eq!(who(&app, LOUNGE), [CVETKA, TIM]);
}

#[test]
fn another_guilds_leave_does_not_remove_someone_who_just_joined_here() {
    let (mut app, fake, _) = tim();
    app.act(|s| {
        s.outbox.push(Command::CreateChannel {
            guild: COOKING,
            name: "kitchen-call".into(),
            kind: ChannelKind::Voice,
            section: None,
            icon: ChannelIconRef::Default,
            color: PaletteColor::None,
        })
    });
    app.pump();
    let kitchen = app.state.channels_of(COOKING).iter().find(|c| c.name == "kitchen-call").unwrap().id;
    fake.voice_join_as(BRUNO, kitchen);
    app.pump();
    // Bruno left The Hideout's lounge for Cooking Club: the server announced both.
    assert_eq!(who(&app, kitchen), [BRUNO]);
    assert_eq!(who(&app, LOUNGE), [ANA]);
    assert_eq!(app.state.voice_state_of(BRUNO).unwrap().guild_id, COOKING);
}

#[test]
fn a_reopened_stream_forgets_voice_states_and_the_snapshot_restores_them() {
    let (mut app, fake, _) = tim();
    // A stale state the client believes in (the user left while it was away).
    let mut stale = app.state.voice_state_of(ANA).unwrap().clone();
    stale.user_id = CVETKA;
    app.state
        .data
        .voice
        .get_mut(&G)
        .unwrap()
        .insert(CVETKA, stale);
    assert_eq!(who(&app, LOUNGE).len(), 3);

    fake.disconnect(1);
    fake.reconnect();
    app.pump();
    assert_eq!(
        who(&app, LOUNGE),
        [ANA, BRUNO],
        "the snapshot is the truth now"
    );

    // Nobody in voice: no snapshot follows, and what was believed is gone all the same.
    fake.voice_leave_as(ANA);
    fake.voice_leave_as(BRUNO);
    app.pump();
    let mut stale = VoiceState {
        user_id: CVETKA,
        guild_id: G,
        channel_id: LOUNGE,
        ..app_state(&app)
    };
    stale.joined_at = std::time::UNIX_EPOCH;
    app.state
        .data
        .voice
        .entry(G)
        .or_default()
        .insert(CVETKA, stale);
    fake.disconnect(1);
    fake.reconnect();
    app.pump();
    assert!(who(&app, LOUNGE).is_empty());
}

/// Some voice state to build others from.
fn app_state(app: &App) -> VoiceState {
    let _ = app;
    VoiceState {
        guild_id: G,
        channel_id: LOUNGE,
        user_id: 0,
        self_mute: false,
        self_deaf: false,
        server_mute: false,
        self_video: false,
        self_stream: false,
        joined_at: std::time::UNIX_EPOCH,
    }
}

#[test]
fn a_snapshot_replaces_only_the_guilds_it_lists() {
    let (mut app, _fake, _) = tim();
    let other = VoiceState {
        guild_id: 2,
        channel_id: 21,
        user_id: CVETKA,
        ..app_state(&app)
    };
    app.act(|s| s.voice_update(2, CVETKA, Some(other)));
    let ana = app.state.voice_state_of(ANA).unwrap().clone();
    app.act(|s| s.voice_snapshot(&[G], vec![ana]));
    assert_eq!(who(&app, LOUNGE), [ANA], "The Hideout is replaced");
    assert!(
        app.state.voice_state_of(CVETKA).is_some(),
        "Garage Band was not listed"
    );
    app.act(|s| s.voice_snapshot(&[G, 2], vec![]));
    assert!(app.state.voice_state_of(ANA).is_none() && app.state.voice_state_of(CVETKA).is_none());
}

#[test]
fn a_move_means_rejoining_the_new_channel() {
    let (mut app, fake, engine) = tim();
    app.act(|s| s.join_voice(LOUNGE));
    app.pump();
    assert!(matches!(
        app.state.voice.session,
        VoiceSession::Connected {
            channel: LOUNGE,
            ..
        }
    ));
    engine.take();

    fake.voice_move_as_moderator(TIM, GAME_NIGHT);
    app.pump();
    assert!(matches!(
        &app.state.voice.session,
        VoiceSession::Connected { channel: GAME_NIGHT, url, token } if url == "ws://fake" && token == "fake"
    ));
    assert_eq!(
        engine.take(),
        [
            VoiceIntent::Disconnect,
            connect(),
            VoiceIntent::SetMute(false),
            VoiceIntent::SetDeaf(false)
        ],
        "the old connection is dropped, then the new room is joined"
    );
    assert_eq!(
        sent_voice(&fake),
        [
            Command::JoinVoice { channel: LOUNGE },
            Command::JoinVoice {
                channel: GAME_NIGHT
            }
        ]
    );
    assert_eq!(app.state.my_voice_channel(), Some(GAME_NIGHT));

    // A device that isn't in voice ignores the move (the account is in voice somewhere else).
    let (mut app, fake, engine) = tim();
    fake.voice_join_as(TIM, LOUNGE);
    app.pump();
    fake.voice_move_as_moderator(TIM, GAME_NIGHT);
    app.pump();
    assert_eq!(app.state.voice.session, VoiceSession::None);
    assert!(sent_voice(&fake).is_empty() && engine.take().is_empty());
}

#[test]
fn being_removed_from_the_channel_drops_the_connection() {
    let (mut app, fake, engine) = tim();
    app.act(|s| s.join_voice(LOUNGE));
    app.pump();
    engine.take();
    fake.voice_leave_as(TIM);
    app.pump();
    assert_eq!(app.state.voice.session, VoiceSession::None);
    assert_eq!(engine.take(), [VoiceIntent::Disconnect]);
    let toast = app.state.ui.toasts.back().unwrap();
    assert_eq!(
        (toast.kind, toast.message.as_str()),
        (ToastKind::Info, "You were disconnected from voice.")
    );

    // A snapshot that doesn't list us says the same.
    app.act(|s| s.join_voice(LOUNGE));
    app.pump();
    engine.take();
    let ana = app.state.voice_state_of(ANA).unwrap().clone();
    app.act(|s| s.voice_snapshot(&[G], vec![ana]));
    assert_eq!(app.state.voice.session, VoiceSession::None);
    assert_eq!(engine.take(), [VoiceIntent::Disconnect]);
}

// ---- joining, switching and leaving ----------------------------------------------------------

#[test]
fn joining_connects_the_engine_with_what_the_server_gave() {
    let (mut app, fake, engine) = tim();
    app.act(|s| s.join_voice(LOUNGE));
    assert_eq!(
        app.state.voice.session,
        VoiceSession::Joining { channel: LOUNGE }
    );
    assert_eq!(app.state.my_voice_channel(), Some(LOUNGE));
    assert!(
        engine.take().is_empty(),
        "nothing connects before the server answers"
    );
    app.pump();
    assert_eq!(
        app.state.voice.session,
        VoiceSession::Connected {
            channel: LOUNGE,
            url: "ws://fake".into(),
            token: "fake".into()
        }
    );
    assert_eq!(
        engine.take(),
        [
            connect(),
            VoiceIntent::SetMute(false),
            VoiceIntent::SetDeaf(false)
        ]
    );
    assert_eq!(who(&app, LOUNGE), [ANA, BRUNO, TIM]);
    // Everything was ordered right: one join, flags already right so nothing else.
    assert_eq!(sent_voice(&fake), [Command::JoinVoice { channel: LOUNGE }]);

    // Joining the channel we are in changes nothing; so does a text channel.
    app.act(|s| s.join_voice(LOUNGE));
    app.act(|s| s.join_voice(GENERAL));
    app.act(|s| s.join_voice(12345));
    assert_eq!(sent_voice(&fake).len(), 1);
    assert!(engine.take().is_empty());
}

#[test]
fn switching_channels_leaves_the_old_one_implicitly() {
    let (mut app, fake, engine) = tim();
    app.act(|s| s.join_voice(LOUNGE));
    app.pump();
    engine.take();
    app.act(|s| s.join_voice(GAME_NIGHT));
    assert_eq!(
        engine.take(),
        [VoiceIntent::Disconnect],
        "the engine leaves the old room at once"
    );
    assert_eq!(
        app.state.voice.session,
        VoiceSession::Joining {
            channel: GAME_NIGHT
        }
    );
    app.pump();
    assert!(matches!(
        app.state.voice.session,
        VoiceSession::Connected {
            channel: GAME_NIGHT,
            ..
        }
    ));
    assert_eq!(
        engine.take(),
        [
            connect(),
            VoiceIntent::SetMute(false),
            VoiceIntent::SetDeaf(false)
        ]
    );
    assert_eq!(
        sent_voice(&fake),
        [
            Command::JoinVoice { channel: LOUNGE },
            Command::JoinVoice {
                channel: GAME_NIGHT
            }
        ],
        "no LeaveVoice: the server moves us"
    );
    assert_eq!(
        (who(&app, LOUNGE), who(&app, GAME_NIGHT)),
        (vec![ANA, BRUNO], vec![TIM])
    );
}

#[test]
fn leaving_disconnects_and_tells_the_server() {
    let (mut app, fake, engine) = tim();
    app.act(|s| s.leave_voice());
    assert!(
        sent_voice(&fake).is_empty(),
        "leaving when in none does nothing"
    );
    app.act(|s| s.join_voice(LOUNGE));
    app.pump();
    engine.take();
    app.act(|s| s.leave_voice());
    assert_eq!(app.state.voice.session, VoiceSession::None);
    assert_eq!(engine.take(), [VoiceIntent::Disconnect]);
    app.pump();
    assert_eq!(sent_voice(&fake).last(), Some(&Command::LeaveVoice));
    assert_eq!(who(&app, LOUNGE), [ANA, BRUNO]);
}

#[test]
fn leaving_while_the_join_is_out_wins() {
    let (mut app, fake, engine) = tim();
    fake.set_latency(0.5);
    app.act(|s| s.join_voice(LOUNGE));
    app.act(|s| s.leave_voice());
    assert!(
        engine.take().is_empty(),
        "never connected, nothing to disconnect"
    );
    app.state.clock = 10.0;
    app.pump();
    assert_eq!(
        app.state.voice.session,
        VoiceSession::None,
        "the late answer is ignored"
    );
    assert!(engine.take().is_empty());
    assert_eq!(
        who(&app, LOUNGE),
        [ANA, BRUNO],
        "the server saw the join and then the leave"
    );
}

#[test]
fn a_refused_join_says_why_and_stays_out() {
    let (mut app, fake, engine) = tim();
    fake.voice_unavailable(true);
    app.act(|s| s.join_voice(LOUNGE));
    app.pump();
    assert_eq!(app.state.voice.session, VoiceSession::None);
    assert!(engine.take().is_empty());
    let toast = app.state.ui.toasts.back().unwrap();
    assert_eq!(
        (toast.kind, toast.message.as_str()),
        (ToastKind::Danger, "Voice isn't set up on this server.")
    );
}

#[test]
fn signing_out_disconnects_the_engine() {
    let (mut app, _fake, engine) = tim();
    app.act(|s| s.join_voice(LOUNGE));
    app.pump();
    engine.take();
    app.act(|s| s.sign_out());
    assert_eq!(app.state.voice.session, VoiceSession::None);
    assert_eq!(engine.take(), [VoiceIntent::Disconnect]);
}

// ---- mute and deafen -------------------------------------------------------------------------

#[test]
fn mute_and_deafen_flags_work_without_a_connection() {
    let (mut app, fake, engine) = tim();
    let flags = |a: &App| (a.state.voice.self_mute, a.state.voice.self_deaf);
    app.act(|s| s.toggle_mute());
    assert_eq!(flags(&app), (true, false));
    app.act(|s| s.toggle_mute());
    assert_eq!(flags(&app), (false, false));

    // Deafening implies muting; undeafening restores what the mute was before.
    app.act(|s| s.toggle_deaf());
    assert_eq!(flags(&app), (true, true));
    app.act(|s| s.toggle_deaf());
    assert_eq!(flags(&app), (false, false));
    app.act(|s| s.toggle_mute());
    app.act(|s| s.toggle_deaf());
    app.act(|s| s.toggle_deaf());
    assert_eq!(
        flags(&app),
        (true, false),
        "muted before, still muted after"
    );

    // Unmuting while deafened undeafens too.
    app.act(|s| s.toggle_deaf());
    assert_eq!(flags(&app), (true, true));
    app.act(|s| s.toggle_mute());
    assert_eq!(flags(&app), (false, false));

    assert!(
        sent_voice(&fake).is_empty(),
        "nothing is sent while not in voice"
    );
    assert!(engine.take().is_empty());
}

#[test]
fn flags_set_before_joining_apply_to_the_channel_joined() {
    let (mut app, fake, engine) = tim();
    app.act(|s| s.toggle_mute());
    app.act(|s| s.join_voice(LOUNGE));
    app.pump();
    assert_eq!(
        engine.take(),
        [
            connect(),
            VoiceIntent::SetMute(true),
            VoiceIntent::SetDeaf(false)
        ]
    );
    assert_eq!(
        sent_voice(&fake),
        [
            Command::JoinVoice { channel: LOUNGE },
            Command::UpdateVoiceState {
                self_mute: true,
                self_deaf: false
            }
        ],
        "the server starts us unmuted and is told"
    );
    assert!(
        app.state.voice_state_of(TIM).unwrap().self_mute,
        "and its answer shows"
    );
    // They stay across a leave.
    app.act(|s| s.leave_voice());
    assert!(app.state.voice.self_mute);
}

#[test]
fn toggling_while_connected_reaches_the_engine_and_the_server() {
    let (mut app, fake, engine) = tim();
    app.act(|s| s.join_voice(LOUNGE));
    app.pump();
    engine.take();
    app.act(|s| s.toggle_deaf());
    app.pump();
    assert_eq!(
        engine.take(),
        [VoiceIntent::SetMute(true), VoiceIntent::SetDeaf(true)]
    );
    let me = fake
        .voice_states(G)
        .into_iter()
        .find(|s| s.user_id == TIM)
        .unwrap();
    assert!(me.self_mute && me.self_deaf);
    assert!(app.state.voice_state_of(TIM).unwrap().self_deaf);
    app.act(|s| s.toggle_deaf());
    app.pump();
    assert_eq!(
        engine.take(),
        [VoiceIntent::SetMute(false), VoiceIntent::SetDeaf(false)]
    );
    let me = fake
        .voice_states(G)
        .into_iter()
        .find(|s| s.user_id == TIM)
        .unwrap();
    assert!(!me.self_mute && !me.self_deaf);
}

// ---- who may moderate ------------------------------------------------------------------------

#[test]
fn the_owner_may_moderate_everyone_but_not_server_mute_themself() {
    let (mut app, _fake, _) = tim();
    app.act(|s| s.join_voice(LOUNGE));
    app.pump();
    assert!(app.state.can_server_mute(ANA) && app.state.can_move_member(ANA));
    assert!(!app.state.can_server_mute(TIM), "never yourself");
    assert!(app.state.can_move_member(TIM), "moving yourself is allowed");
    assert!(
        !app.state.can_server_mute(CVETKA) && !app.state.can_move_member(CVETKA),
        "not in voice"
    );
}

#[test]
fn a_moderator_may_moderate_those_below_and_nobody_else() {
    let (mut app, fake, _) = wired("bruno");
    // Bruno holds MANAGE_CHANNELS only: no voice moderation, whoever is below him.
    assert!(!app.state.can_server_mute(ANA) && !app.state.can_move_member(ANA));
    fake.set_role_permissions(
        G,
        HIDEOUT_MODS,
        Permissions::MANAGE_CHANNELS | Permissions::MUTE_MEMBERS | Permissions::MOVE_MEMBERS,
    );
    fake.voice_join_as(CVETKA, LOUNGE);
    fake.voice_join_as(TIM, LOUNGE);
    app.pump();
    assert!(
        app.state.can_server_mute(ANA) && app.state.can_move_member(ANA),
        "Designer is below Moderator"
    );
    assert!(
        !app.state.can_server_mute(CVETKA) && !app.state.can_move_member(CVETKA),
        "Admin is above"
    );
    assert!(
        !app.state.can_server_mute(TIM) && !app.state.can_move_member(TIM),
        "nobody outranks the owner"
    );
    assert!(!app.state.can_server_mute(BRUNO), "not himself");
    assert!(
        app.state.can_move_member(BRUNO),
        "moving yourself skips the hierarchy"
    );
}

#[test]
fn a_plain_member_gets_no_moderation_and_the_calls_are_not_sent() {
    let (mut app, fake, _) = wired("ana");
    assert!(!app.state.can_server_mute(BRUNO) && !app.state.can_move_member(BRUNO));
    app.act(|s| {
        s.server_mute(BRUNO, true);
        s.move_member(BRUNO, GAME_NIGHT);
        s.disconnect_member(BRUNO);
    });
    app.pump();
    assert!(sent_voice(&fake).is_empty());
    assert_eq!(who(&app, LOUNGE), [ANA, BRUNO]);
}

#[test]
fn moderation_actions_send_the_calls_and_the_events_show_the_result() {
    let (mut app, fake, _) = tim();
    app.act(|s| s.server_mute(ANA, true));
    app.pump();
    assert!(app.state.voice_state_of(ANA).unwrap().server_mute);
    assert_eq!(
        sent_voice(&fake),
        [Command::ServerMuteMember {
            guild: G,
            user: ANA,
            muted: true
        }]
    );
    app.act(|s| s.server_mute(ANA, false));
    app.pump();
    assert!(!app.state.voice_state_of(ANA).unwrap().server_mute);

    app.act(|s| s.move_member(ANA, GAME_NIGHT));
    app.pump();
    assert_eq!(
        (who(&app, LOUNGE), who(&app, GAME_NIGHT)),
        (vec![BRUNO], vec![ANA])
    );
    // Where they already are, to a text channel or to a channel we don't know: nothing is sent.
    let before = sent_voice(&fake).len();
    app.act(|s| {
        s.move_member(ANA, GAME_NIGHT);
        s.move_member(ANA, GENERAL);
        s.move_member(ANA, 4242);
    });
    assert_eq!(sent_voice(&fake).len(), before);

    app.act(|s| s.disconnect_member(BRUNO));
    app.pump();
    assert!(app.state.voice_state_of(BRUNO).is_none());
    assert_eq!(
        sent_voice(&fake).last(),
        Some(&Command::DisconnectMember {
            guild: G,
            user: BRUNO
        })
    );
}

#[test]
fn a_refused_moderation_call_toasts_the_reason() {
    let (mut app, fake, _) = tim();
    // The fake still refuses what the client no longer offers: ask for it behind the client's back.
    app.act(|s| {
        s.outbox.push(Command::ServerMuteMember {
            guild: G,
            user: TIM,
            muted: true,
        })
    });
    app.pump();
    let toast = app.state.ui.toasts.back().unwrap();
    assert_eq!(toast.kind, ToastKind::Danger);
    assert_eq!(toast.message, "You cannot server-mute yourself.");
    let _ = fake;
}

#[test]
fn a_server_mute_by_a_moderator_shows_on_our_own_state() {
    let (mut app, _fake, _) = tim();
    app.act(|s| s.join_voice(LOUNGE));
    app.pump();
    assert!(!app.state.i_am_server_muted());
    let mut muted = app.state.voice_state_of(TIM).unwrap().clone();
    muted.server_mute = true;
    app.act(|s| s.voice_update(G, TIM, Some(muted)));
    assert!(app.state.i_am_server_muted());
    // Still in the same place, still connected: the server enforces it in the room.
    assert_eq!(who(&app, LOUNGE), [ANA, BRUNO, TIM]);
    assert!(matches!(app.state.voice.session, VoiceSession::Connected { .. }));
}

// ---- a voice channel is joined, not opened ---------------------------------------------------

#[test]
fn selecting_a_voice_channel_joins_it_and_keeps_the_text_channel_open() {
    let (mut app, fake, _) = tim();
    assert_eq!(app.state.current_channel(), Some(GENERAL));
    app.act(|s| s.select_channel(LOUNGE));
    app.pump();
    assert_eq!(app.state.current_channel(), Some(GENERAL));
    assert!(matches!(
        app.state.voice.session,
        VoiceSession::Connected {
            channel: LOUNGE,
            ..
        }
    ));
    assert!(
        !app.state.data.logs.contains_key(&LOUNGE),
        "no history is asked for"
    );
    assert!(!sent(&fake).iter().any(|c| matches!(
        c,
        Command::LoadMessages {
            channel: LOUNGE,
            ..
        }
    )));

    // Opening another text channel leaves the call alone.
    app.act(|s| s.select_channel(RANDOM));
    assert_eq!(app.state.current_channel(), Some(RANDOM));
    assert_eq!(app.state.voice.session.channel(), Some(LOUNGE));
    // Clicking the other voice channel switches calls; the text channel is still RANDOM.
    app.act(|s| s.select_channel(GAME_NIGHT));
    app.pump();
    assert_eq!(
        (
            app.state.current_channel(),
            app.state.voice.session.channel()
        ),
        (Some(RANDOM), Some(GAME_NIGHT))
    );
}

#[test]
fn stepping_with_the_keyboard_and_first_open_skip_voice_channels() {
    let (mut app, _fake, _) = tim();
    assert_eq!(app.state.first_text_channel(G), Some(GENERAL));
    // The last text channel in the sidebar is the end of the line: the voice channels after it
    // are not stepped into.
    let last_text = app
        .state
        .ordered_channels(G)
        .into_iter()
        .rfind(|c| c.kind == ChannelKind::Text)
        .map(|c| c.id)
        .unwrap();
    app.act(|s| s.select_channel(last_text));
    app.act(|s| s.step_channel(1));
    assert_eq!(app.state.current_channel(), Some(last_text));
    assert_eq!(app.state.voice.session, VoiceSession::None);
}

// ---- creating a channel ----------------------------------------------------------------------

#[test]
fn the_create_dialog_takes_a_kind_that_defaults_to_text() {
    let (mut app, fake, _) = tim();
    app.act(|s| s.open_dialog(DialogKind::CreateChannel { guild: G }));
    assert_eq!(
        app.state
            .ui
            .dialog
            .as_ref()
            .unwrap()
            .channel
            .as_ref()
            .unwrap()
            .kind,
        ChannelKind::Text
    );
    app.act(|s| {
        let d = s.ui.dialog.as_mut().unwrap();
        d.input.replace_text("notes");
        s.submit_dialog();
    });
    app.pump();
    let notes = app
        .state
        .channels_of(G)
        .iter()
        .find(|c| c.name == "notes")
        .unwrap()
        .clone();
    assert_eq!(notes.kind, ChannelKind::Text);
    assert_eq!(
        app.state.current_channel(),
        Some(notes.id),
        "a new text channel opens"
    );

    app.act(|s| s.open_dialog(DialogKind::CreateChannel { guild: G }));
    app.act(|s| {
        let d = s.ui.dialog.as_mut().unwrap();
        d.input.replace_text("Karaoke Night");
        d.channel.as_mut().unwrap().kind = ChannelKind::Voice;
        s.submit_dialog();
    });
    assert!(sent(&fake).iter().any(|c| matches!(
        c,
        Command::CreateChannel { name, kind: ChannelKind::Voice, .. } if name == "karaoke-night"
    )));
    app.pump();
    let karaoke = app
        .state
        .channels_of(G)
        .iter()
        .find(|c| c.name == "karaoke-night")
        .unwrap()
        .clone();
    assert_eq!(karaoke.kind, ChannelKind::Voice);
    assert!(app.state.ui.dialog.is_none());
    assert_eq!(
        app.state.current_channel(),
        Some(notes.id),
        "a new voice channel is not opened"
    );
    assert_eq!(
        app.state.voice.session,
        VoiceSession::None,
        "nor joined for us"
    );
}
