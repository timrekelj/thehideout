//! The voice sink against a recording engine: intents to engine calls, the engine's reports into
//! state (status, speaking, quality, drops and the reconnect backoff), server mute, and the cues.
//! No audio, no network: the real engine is proven end to end in `tests/voice_e2e.rs`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{Sender, channel};

use thehideout_api::{ApiError, VoiceJoin, VoiceState};
use thehideout_design::components::ToastKind;
use thehideout_voice::{Quality, VoiceEvent};

use crate::voice_engine::*;
use crate::backend::Command;
use crate::fake::{CVETKA, GAME_NIGHT, LOUNGE, TIM};
use crate::state::AppState;
use crate::voice::{
    RECONNECT_BACKOFF, Sound, VoiceIntent, VoiceLink, VoiceSession, VoiceSink, VoiceStatus,
};

#[derive(Clone, Debug, PartialEq)]
enum Call {
    Connect {
        url: String,
        token: String,
        muted: bool,
        deafened: bool,
    },
    Disconnect,
    Mute(bool),
    Deaf(bool),
    Volume(String, f32),
}

#[derive(Clone, Default)]
struct Fake(Rc<RefCell<Vec<Call>>>);

impl Engine for Fake {
    fn connect(&self, url: &str, token: &str, muted: bool, deafened: bool) {
        self.0.borrow_mut().push(Call::Connect {
            url: url.into(),
            token: token.into(),
            muted,
            deafened,
        });
    }
    fn disconnect(&self) {
        self.0.borrow_mut().push(Call::Disconnect);
    }
    fn set_mute(&self, m: bool) {
        self.0.borrow_mut().push(Call::Mute(m));
    }
    fn set_deaf(&self, d: bool) {
        self.0.borrow_mut().push(Call::Deaf(d));
    }
    fn set_user_volume(&self, id: &str, v: f32) {
        self.0.borrow_mut().push(Call::Volume(id.into(), v));
    }
}

#[derive(Clone, Default)]
struct Played(Rc<RefCell<Vec<Sound>>>);

impl SoundOut for Played {
    fn play(&mut self, s: Sound) {
        self.0.borrow_mut().push(s);
    }
}

/// A sink over a recording engine; `events` is the engine's side of the report channel.
struct Rig {
    sink: EngineSink<Fake>,
    calls: Fake,
    events: Sender<VoiceEvent>,
    played: Played,
}

fn rig() -> Rig {
    let calls = Fake::default();
    let (events, rx) = channel();
    let rx = Rc::new(RefCell::new(Some(rx)));
    let played = Played::default();
    let (c, r) = (calls.clone(), rx);
    let sink = EngineSink::new(
        Box::new(move |_| (c.clone(), r.borrow_mut().take().expect("built once"))),
        Box::new(played.clone()),
    );
    Rig {
        sink,
        calls,
        events,
        played,
    }
}

impl Rig {
    fn take(&self) -> Vec<Call> {
        std::mem::take(&mut self.calls.0.borrow_mut())
    }

    /// One flush: the intents, then the end of the batch.
    fn batch(&mut self, intents: impl IntoIterator<Item = VoiceIntent>) {
        for i in intents {
            self.sink.intent(i);
        }
        self.sink.flushed();
    }
}

fn connect(token: &str) -> VoiceIntent {
    VoiceIntent::Connect {
        url: "ws://lk".into(),
        token: token.into(),
    }
}

fn joined(token: &str) -> Call {
    Call::Connect {
        url: "ws://lk".into(),
        token: token.into(),
        muted: false,
        deafened: false,
    }
}

// ---- intents to engine calls ----

#[test]
fn the_engine_is_not_built_until_the_first_connect() {
    let mut r = rig();
    r.batch([
        VoiceIntent::SetMute(true),
        VoiceIntent::MuteUser { user: 5, muted: true },
        VoiceIntent::Disconnect,
    ]);
    assert!(!r.sink.built(), "flags and a leave build nothing");
    assert_eq!(r.take(), []);
    r.sink.sound(Sound::MuteOn);
    assert!(!r.sink.built(), "a click does not build the engine");
    assert_eq!(*r.played.0.borrow(), [Sound::MuteOn]);
}

#[test]
fn a_connect_carries_the_flags_that_follow_it() {
    let mut r = rig();
    r.batch([
        connect("t1"),
        VoiceIntent::SetMute(true),
        VoiceIntent::SetDeaf(false),
    ]);
    assert_eq!(
        r.take(),
        [Call::Connect {
            url: "ws://lk".into(),
            token: "t1".into(),
            muted: true,
            deafened: false
        }]
    );
    // Deafened implies muted at join.
    r.batch([
        VoiceIntent::Disconnect,
        connect("t2"),
        VoiceIntent::SetMute(true),
        VoiceIntent::SetDeaf(true),
    ]);
    assert_eq!(
        r.take(),
        [
            Call::Disconnect,
            Call::Connect {
                url: "ws://lk".into(),
                token: "t2".into(),
                muted: true,
                deafened: true
            }
        ]
    );
}

#[test]
fn flag_changes_after_the_connect_go_straight_through() {
    let mut r = rig();
    r.batch([connect("t"), VoiceIntent::SetMute(false), VoiceIntent::SetDeaf(false)]);
    r.take();
    r.batch([VoiceIntent::SetMute(true), VoiceIntent::SetDeaf(true)]);
    assert_eq!(r.take(), [Call::Mute(true), Call::Deaf(true)]);
    r.batch([VoiceIntent::Disconnect]);
    assert_eq!(r.take(), [Call::Disconnect]);
    r.batch([VoiceIntent::Disconnect]);
    assert_eq!(r.take(), [], "nothing to tear down twice");
}

#[test]
fn muted_for_me_is_user_volume_zero_and_survives_a_new_connection() {
    let mut r = rig();
    r.batch([connect("t"), VoiceIntent::SetMute(false), VoiceIntent::SetDeaf(false)]);
    r.take();
    r.batch([VoiceIntent::MuteUser { user: 42, muted: true }]);
    assert_eq!(r.take(), [Call::Volume("42".into(), 0.0)]);
    r.batch([VoiceIntent::MuteUser { user: 42, muted: false }]);
    assert_eq!(r.take(), [Call::Volume("42".into(), 1.0)]);
    r.batch([VoiceIntent::MuteUser { user: 7, muted: true }]);
    r.take();
    // A new connection starts from silence for the ones muted.
    r.batch([
        VoiceIntent::Disconnect,
        connect("t2"),
        VoiceIntent::SetMute(false),
        VoiceIntent::SetDeaf(false),
        VoiceIntent::MuteUser { user: 7, muted: true },
    ]);
    assert_eq!(
        r.take(),
        [Call::Disconnect, joined("t2"), Call::Volume("7".into(), 0.0)]
    );
}

#[test]
fn a_teardown_we_asked_for_is_not_a_drop() {
    let mut r = rig();
    r.batch([connect("t"), VoiceIntent::SetMute(false), VoiceIntent::SetDeaf(false)]);
    r.events.send(VoiceEvent::Connecting).unwrap();
    r.events.send(VoiceEvent::Connected).unwrap();
    assert_eq!(r.sink.events().len(), 2);
    // We leave; the engine's report of it is swallowed.
    r.batch([VoiceIntent::Disconnect]);
    r.events
        .send(VoiceEvent::Disconnected { reason: "client initiated".into() })
        .unwrap();
    assert_eq!(r.sink.events(), []);
    // Connecting again while connected: the old room's report is swallowed too.
    r.batch([connect("a"), VoiceIntent::SetMute(false), VoiceIntent::SetDeaf(false)]);
    r.batch([connect("b"), VoiceIntent::SetMute(false), VoiceIntent::SetDeaf(false)]);
    for e in [
        VoiceEvent::Connecting,
        VoiceEvent::Disconnected { reason: "reconnect requested".into() },
        VoiceEvent::Connecting,
        VoiceEvent::Connected,
    ] {
        r.events.send(e).unwrap();
    }
    let got = r.sink.events();
    assert!(
        !got.iter().any(|e| matches!(e, VoiceEvent::Disconnected { .. })),
        "{got:?}"
    );
    // A drop nobody asked for comes through.
    r.events
        .send(VoiceEvent::Disconnected { reason: "signal lost".into() })
        .unwrap();
    assert_eq!(
        r.sink.events(),
        [VoiceEvent::Disconnected { reason: "signal lost".into() }]
    );
}

// ---- events into state ----

const ME: u64 = TIM;
const GUILD: u64 = 1;

/// Tim, signed in on the fake world, which has a voice channel (the lounge) in his server.
fn state() -> AppState {
    crate::scenes::booted_as(true, "tim").0.state
}

fn vs(user: u64, channel: u64, server_mute: bool) -> VoiceState {
    VoiceState {
        guild_id: GUILD,
        channel_id: channel,
        user_id: user,
        self_mute: false,
        self_deaf: false,
        server_mute,
        self_video: false,
        self_stream: false,
        joined_at: std::time::SystemTime::UNIX_EPOCH,
    }
}

/// In voice in the lounge, with the engine told to connect.
fn in_voice() -> AppState {
    let mut s = state();
    s.voice.session = VoiceSession::Joining { channel: LOUNGE };
    s.voice_joined(
        LOUNGE,
        Ok(VoiceJoin {
            url: "ws://lk".into(),
            token: "tok".into(),
            channel_id: LOUNGE,
        }),
    );
    s.voice.intents.clear();
    s.outbox.clear();
    s
}

fn toasts(s: &AppState) -> Vec<(ToastKind, String)> {
    s.ui.toasts.iter().map(|t| (t.kind, t.message.clone())).collect()
}

#[test]
fn speaking_and_quality_reach_the_voice_bar_state() {
    let mut s = in_voice();
    assert!(s.voice_event(VoiceEvent::Connecting));
    assert_eq!(s.voice_status(), Some(VoiceStatus::Connecting));
    s.voice_event(VoiceEvent::Connected);
    assert_eq!(s.voice_status(), Some(VoiceStatus::Connected));
    s.voice_event(VoiceEvent::Speaking { identity: "7".into(), speaking: true });
    s.voice_event(VoiceEvent::Speaking { identity: "8".into(), speaking: true });
    s.voice_event(VoiceEvent::Speaking { identity: "7".into(), speaking: false });
    assert_eq!(s.voice.speaking, [8].into());
    // Only our own quality is shown.
    s.voice_event(VoiceEvent::ConnectionQuality { identity: "8".into(), quality: Quality::Poor });
    assert_eq!(s.voice.quality, None);
    s.voice_event(VoiceEvent::ConnectionQuality { identity: ME.to_string(), quality: Quality::Poor });
    assert_eq!(s.voice.quality, Some(Quality::Poor));
    s.voice_event(VoiceEvent::Reconnecting);
    assert_eq!(s.voice_status(), Some(VoiceStatus::Reconnecting));
    s.voice_event(VoiceEvent::Connected);
    assert_eq!(s.voice_status(), Some(VoiceStatus::Connected));
    s.voice_event(VoiceEvent::Error("microphone unavailable".into()));
    assert_eq!(
        toasts(&s).last().map(|t| t.0),
        Some(ToastKind::Danger),
        "an engine error is a toast"
    );
}

#[test]
fn reports_with_no_session_are_stale() {
    let mut s = state();
    assert!(!s.voice_event(VoiceEvent::Connected));
    assert!(!s.voice_event(VoiceEvent::Disconnected { reason: "x".into() }));
    assert!(s.outbox.is_empty() && toasts(&s).is_empty());
}

#[test]
fn an_unexpected_drop_toasts_and_rejoins_on_the_backoff() {
    let mut s = in_voice();
    s.voice_event(VoiceEvent::Connecting);
    s.voice_event(VoiceEvent::Connected);
    s.clock = 100.0;
    s.voice_event(VoiceEvent::Disconnected { reason: "signal lost".into() });
    assert_eq!(
        toasts(&s),
        [(ToastKind::Info, "Disconnected from voice. Reconnecting\u{2026}".into())]
    );
    assert_eq!(s.voice_status(), Some(VoiceStatus::Reconnecting));
    assert!(s.outbox.is_empty(), "the first try waits its second");
    // Not due yet: wakes at the due time.
    assert_eq!(s.voice_tick(100.5), (false, Some(101.0)));
    // Due: JoinVoice again, for a fresh token.
    assert_eq!(s.voice_tick(101.0), (true, None));
    assert!(matches!(s.outbox.as_slice(), [Command::JoinVoice { channel: LOUNGE }]));
    assert_eq!(s.voice.session, VoiceSession::Joining { channel: LOUNGE });
    assert_eq!(s.voice_status(), Some(VoiceStatus::Reconnecting));
    s.outbox.clear();
    // The new token connects the engine again.
    s.voice_joined(
        LOUNGE,
        Ok(VoiceJoin {
            url: "ws://lk".into(),
            token: "fresh".into(),
            channel_id: LOUNGE,
        }),
    );
    assert!(s.voice.intents.contains(&VoiceIntent::Connect {
        url: "ws://lk".into(),
        token: "fresh".into()
    }));
    // And once the engine is back the backoff starts over, with no join sound for a rejoin.
    s.voice.sounds_out.clear();
    s.voice_event(VoiceEvent::Connecting);
    s.voice_event(VoiceEvent::Connected);
    assert_eq!((s.voice.retry, s.voice.link), (None, VoiceLink::Live));
    assert!(!s.voice.sounds_out.contains(&Sound::Join));
}

#[test]
fn the_backoff_is_one_two_five_ten_then_leave_voice() {
    assert_eq!(RECONNECT_BACKOFF, [1.0, 2.0, 5.0, 10.0]);
    let mut s = in_voice();
    s.voice_event(VoiceEvent::Connected);
    let mut clock = 0.0;
    s.clock = clock;
    for wait in RECONNECT_BACKOFF {
        s.voice_event(VoiceEvent::Disconnected { reason: "x".into() });
        let due = s.voice.retry.and_then(|r| r.due).expect("a retry is scheduled");
        assert_eq!(due - clock, wait);
        clock = due;
        s.clock = clock;
        s.voice_tick(clock);
        assert!(matches!(s.outbox.pop(), Some(Command::JoinVoice { .. })));
        // The server hands out a token, but the engine fails again.
        s.voice_joined(
            LOUNGE,
            Ok(VoiceJoin { url: "ws://lk".into(), token: "t".into(), channel_id: LOUNGE }),
        );
    }
    // The fifth failure gives up: leave on the server too, so its state is consistent.
    s.voice_event(VoiceEvent::Disconnected { reason: "x".into() });
    assert_eq!(s.voice.session, VoiceSession::None);
    assert_eq!(s.voice.retry, None);
    assert!(matches!(s.outbox.last(), Some(Command::LeaveVoice)));
    assert_eq!(
        toasts(&s).last(),
        Some(&(ToastKind::Danger, "Couldn't reconnect to voice.".into()))
    );
}

#[test]
fn a_failed_rejoin_call_goes_on_with_the_backoff_and_a_leave_stops_it() {
    let mut s = in_voice();
    s.voice_event(VoiceEvent::Connected);
    s.clock = 0.0;
    s.voice_event(VoiceEvent::Disconnected { reason: "x".into() });
    s.voice_tick(1.0);
    s.outbox.clear();
    s.clock = 1.0;
    s.voice_joined(LOUNGE, Err(ApiError::Network("down".into())));
    let r = s.voice.retry.expect("still retrying");
    assert_eq!((r.attempt, r.due), (2, Some(3.0)));
    assert!(s.outbox.is_empty());
    // Leaving cancels it.
    s.leave_voice();
    s.outbox.clear();
    assert_eq!(s.voice_tick(3.0), (false, None));
    assert!(s.outbox.is_empty() && s.voice.retry.is_none());
}

#[test]
fn a_server_mute_overrides_the_self_flag_and_is_restored() {
    let mut s = in_voice();
    let st = |server_mute| vs(ME, LOUNGE, server_mute);
    s.voice_update(GUILD, ME, Some(st(false)));
    s.voice.intents.clear();
    s.voice_update(GUILD, ME, Some(st(true)));
    assert_eq!(s.voice.intents, [VoiceIntent::SetMute(true)], "muted though our flag is off");
    s.voice.intents.clear();
    // Unmuting ourselves while a moderator holds the mute keeps the engine muted.
    s.toggle_mute();
    s.toggle_mute();
    assert!(s.voice.intents.iter().all(|i| matches!(i, VoiceIntent::SetMute(true) | VoiceIntent::SetDeaf(false))));
    s.voice.intents.clear();
    s.voice_update(GUILD, ME, Some(st(false)));
    assert_eq!(s.voice.intents, [VoiceIntent::SetMute(false)], "lifted: back to our own flag");
    // With our own flag on, lifting the server mute leaves us muted.
    s.toggle_mute();
    s.voice_update(GUILD, ME, Some(st(true)));
    s.voice.intents.clear();
    s.voice_update(GUILD, ME, Some(st(false)));
    assert_eq!(s.voice.intents, [VoiceIntent::SetMute(true)]);
}

#[test]
fn muted_for_me_reaches_the_sink_as_an_intent_and_again_on_a_new_join() {
    let mut s = in_voice();
    s.toggle_muted_for_me(9);
    assert_eq!(s.voice.intents, [VoiceIntent::MuteUser { user: 9, muted: true }]);
    s.voice.intents.clear();
    s.voice.session = VoiceSession::Joining { channel: LOUNGE };
    s.voice_joined(
        LOUNGE,
        Ok(VoiceJoin { url: "ws://lk".into(), token: "t".into(), channel_id: LOUNGE }),
    );
    assert!(s.voice.intents.contains(&VoiceIntent::MuteUser { user: 9, muted: true }));
}

// ---- sounds ----

fn sounds(s: &mut AppState) -> Vec<Sound> {
    std::mem::take(&mut s.voice.sounds_out)
}

#[test]
fn sounds_follow_joins_leaves_and_toggles() {
    let mut s = in_voice();
    sounds(&mut s);
    s.voice_event(VoiceEvent::Connecting);
    s.voice_event(VoiceEvent::Connected);
    assert_eq!(sounds(&mut s), [Sound::Join]);
    let there = |user| vs(user, LOUNGE, false);
    let (peer, other) = (CVETKA, GAME_NIGHT);
    s.voice_update(GUILD, peer, Some(there(peer)));
    s.voice_update(GUILD, peer, Some(there(peer)));
    assert_eq!(sounds(&mut s), [Sound::PeerJoin], "only the arrival sounds");
    s.voice_update(GUILD, peer, None);
    assert_eq!(sounds(&mut s), [Sound::PeerLeave]);
    // Another channel is not ours.
    s.voice_update(GUILD, peer, Some(vs(peer, other, false)));
    assert_eq!(sounds(&mut s), []);
    s.toggle_mute();
    s.toggle_mute();
    s.toggle_deaf();
    s.toggle_deaf();
    assert_eq!(
        sounds(&mut s),
        [Sound::MuteOn, Sound::MuteOff, Sound::DeafOn, Sound::DeafOff]
    );
    s.leave_voice();
    assert_eq!(sounds(&mut s), [Sound::Disconnect]);
}

#[test]
fn deafened_hears_no_arrivals_but_our_toggles_still_click() {
    let mut s = in_voice();
    s.toggle_deaf();
    assert_eq!(sounds(&mut s), [Sound::DeafOn]);
    let peer = CVETKA;
    s.voice_update(GUILD, peer, Some(vs(peer, LOUNGE, false)));
    s.voice_update(GUILD, peer, None);
    assert_eq!(sounds(&mut s), [], "no arrivals or departures while deafened");
    s.toggle_deaf();
    assert_eq!(sounds(&mut s), [Sound::DeafOff]);
}

#[test]
fn sounds_off_silences_every_cue() {
    let mut s = in_voice();
    s.voice.sounds = false;
    s.toggle_mute();
    s.toggle_deaf();
    s.voice_event(VoiceEvent::Connected);
    s.leave_voice();
    assert_eq!(sounds(&mut s), []);
    assert!(state().voice.sounds, "on by default");
}

#[test]
fn a_drop_the_server_reports_plays_the_disconnect_cue() {
    let mut s = in_voice();
    sounds(&mut s);
    s.voice_update(GUILD, ME, None);
    assert_eq!(s.voice.session, VoiceSession::None);
    assert_eq!(sounds(&mut s), [Sound::Disconnect]);
}

#[test]
fn every_cue_is_short_quiet_and_starts_and_ends_at_zero() {
    use Sound::*;
    for sound in [Join, PeerJoin, PeerLeave, MuteOn, MuteOff, DeafOn, DeafOff, Disconnect] {
        for rate in [44_100, 48_000] {
            let pcm = synth(sound, rate);
            let secs = pcm.len() as f32 / rate as f32;
            assert!((0.05..0.5).contains(&secs), "{sound:?} lasts {secs}s");
            let peak = pcm.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!((0.05..0.25).contains(&peak), "{sound:?} peaks at {peak}");
            assert!(pcm[0].abs() < 0.01 && pcm.last().unwrap().abs() < 0.001);
            // No jump between neighbours that would click.
            let step = pcm.windows(2).fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()));
            assert!(step < 0.1, "{sound:?} jumps by {step}");
        }
    }
    assert_ne!(synth(PeerJoin, 48_000), synth(PeerLeave, 48_000));
}
