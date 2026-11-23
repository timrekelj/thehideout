//! Voice end to end: two app instances (the app's state, the real backend and the real voice
//! sink, driven the way `App::poll` drives them) against the real server running in-process, and
//! a real LiveKit server. Both join a voice channel with a generated tone for a microphone;
//! one sees the other speak. A second test has one app publish a generated picture as its camera
//! and then as a screen share, and the other watch both on its stage (a frame with real pixels).
//!
//! Needs `docker compose up -d` (ScyllaDB on 9042 and LiveKit on 7880 with the dev key
//! `devkey` / `secret`), so it is ignored by default:
//!
//! ```text
//! cargo test -p thehideout-desktop --test voice_e2e -- --ignored --nocapture
//! ```
//!
//! The binary has no library target, so the modules the app state needs are included by path.

#![allow(dead_code, unused_imports)]

#[path = "../src/account.rs"]
mod account;
#[path = "../src/admin.rs"]
mod admin;
#[path = "../src/apply.rs"]
mod apply;
#[path = "../src/attach.rs"]
mod attach;
#[path = "../src/backend.rs"]
mod backend;
#[path = "../src/emoji.rs"]
mod emoji;
#[path = "../src/format.rs"]
mod format;
#[path = "../src/forms.rs"]
mod forms;
#[path = "../src/layout.rs"]
mod layout;
#[path = "../src/lifecycle.rs"]
mod lifecycle;
#[path = "../src/look.rs"]
mod look;
#[path = "../src/media.rs"]
mod media;
#[path = "../src/moderation.rs"]
mod moderation;
#[path = "../src/moderation_ui.rs"]
mod moderation_ui;
#[path = "../src/perms.rs"]
mod perms;
#[path = "../src/real.rs"]
mod real;
#[path = "../src/server_settings.rs"]
mod server_settings;
#[path = "../src/state.rs"]
mod state;
#[path = "../src/structure.rs"]
mod structure;
#[path = "../src/voice.rs"]
mod voice;
#[path = "../src/voice_engine.rs"]
mod voice_engine;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use backend::{Backend, Command};
use format::Tz;
use rand::Rng;
use real::RealBackend;
use rune::prefs::Prefs;
use scylla::client::caching_session::CachingSession;
use scylla::client::session_builder::SessionBuilder;
use state::{AppState, Screen};
use thehideout_api::ChannelKind;
use thehideout_server::snowflake::Snowflake;
use thehideout_server::{build_app, migrations};
use tokio::net::TcpListener;
use voice::{Sound, VoiceSession, VoiceSink};
use voice_engine::{EngineSink, LiveEngine, SoundOut};

const PASSWORD: &str = "correct horse battery";

/// The real server with voice on, signing tokens for the dev LiveKit key of `docker-compose.yml`.
async fn start_server() -> (String, thehideout_server::AppState) {
    let session = SessionBuilder::new()
        .known_node("127.0.0.1:9042")
        .build()
        .await
        .expect("connect to ScyllaDB at 127.0.0.1:9042 (is `docker compose up -d` running?)");
    let keyspace = format!("test_{:016x}", rand::rng().random::<u64>());
    migrations::run(&session, &keyspace)
        .await
        .expect("run migrations");
    let mut state = thehideout_server::AppState::new(
        Arc::new(CachingSession::from(session, 64)),
        Arc::new(Snowflake::new(0)),
        keyspace,
        Duration::from_millis(200),
    );
    let config = thehideout_server::config::LiveKitConfig::new(
        "ws://127.0.0.1:7880",
        None,
        "devkey".into(),
        "secret".into(),
    )
    .expect("livekit config");
    state = state.with_livekit(thehideout_server::voice::LiveKit::new(config));
    let app = build_app(state.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    (format!("http://{addr}"), state)
}

/// Cues are recorded, never played: a test run stays silent.
#[derive(Clone, Default)]
struct Cues(Arc<Mutex<Vec<Sound>>>);

impl SoundOut for Cues {
    fn play(&mut self, sound: Sound) {
        self.0.lock().unwrap().push(sound);
    }
}

/// One app instance: its state, the real backend and the real voice sink.
struct App {
    name: &'static str,
    state: AppState,
    backend: RealBackend,
    sink: EngineSink<LiveEngine>,
    cues: Cues,
    wakes: Arc<AtomicUsize>,
    started: Instant,
}

impl App {
    fn new(name: &'static str, url: &str) -> App {
        let prefs = std::env::temp_dir().join(format!(
            "thehideout-voice-e2e-{:016x}.json",
            rand::rng().random::<u64>()
        ));
        let backend = RealBackend::for_server(Prefs::at(&prefs), url);
        let cues = Cues::default();
        // A generated tone for a microphone: no hardware, no permission, nothing to the speakers.
        let mut sink = voice_engine::live_sink_with(true, Box::new(cues.clone()));
        let wakes = Arc::new(AtomicUsize::new(0));
        let w = Arc::clone(&wakes);
        sink.set_waker(Arc::new(move || {
            w.fetch_add(1, Ordering::SeqCst);
        }));
        App {
            name,
            state: AppState::new(app_now, Tz::Local),
            backend,
            sink,
            cues,
            wakes,
            started: Instant::now(),
        }
    }

    /// One turn of what `app::poll` does: intents and commands out, replies and the engine's
    /// reports in, time forward.
    fn step(&mut self) {
        let clock = self.started.elapsed().as_secs_f64();
        // What `App::flush` does: the sink is subscribed to the tiles on screen first.
        self.state.settle_video();
        voice::send_intents(&mut self.state, Some(&mut self.sink));
        for cmd in std::mem::take(&mut self.state.outbox) {
            self.backend.submit(cmd);
        }
        let mut tx = rune::Tx::default();
        for r in self.backend.poll(clock) {
            apply::apply(&mut self.state, r, &mut tx);
        }
        voice::take_events(&mut self.state, Some(&mut self.sink));
        self.state.tick(clock);
    }

    fn id(&self) -> u64 {
        self.state.my_id()
    }

    fn lounge(&self) -> Option<u64> {
        self.state
            .data
            .channels
            .values()
            .flatten()
            .find(|c| c.name == "lounge")
            .map(|c| c.id)
    }

    fn seen(&self) -> Vec<Sound> {
        self.cues.0.lock().unwrap().clone()
    }
}

fn app_now() -> std::time::SystemTime {
    std::time::SystemTime::now()
}

/// Steps every app until `done` holds.
fn until(what: &str, secs: u64, apps: &mut [&mut App], done: impl Fn(&[&mut App]) -> bool) {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        for a in apps.iter_mut() {
            a.step();
        }
        if done(apps) {
            return;
        }
        if Instant::now() > end {
            let report: Vec<String> = apps
                .iter()
                .map(|a| {
                    format!(
                        "{}: session {:?}, speaking {:?}, link {:?}, retry {:?}, in voice {:?}, toasts {:?}",
                        a.name,
                        a.state.voice.session.channel(),
                        a.state.voice.speaking,
                        a.state.voice.link,
                        a.state.voice.retry,
                        a.lounge().map(|c| a
                            .state
                            .voice_participants(c)
                            .iter()
                            .map(|s| s.user_id)
                            .collect::<Vec<_>>()),
                        a.state.ui.toasts.iter().map(|t| t.message.clone()).collect::<Vec<_>>(),
                    )
                })
                .collect();
            panic!("timed out waiting for {what}:\n{}", report.join("\n"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn sign_in(app: &mut App) {
    app.state.outbox.push(Command::Login {
        username: app.name.into(),
        password: PASSWORD.into(),
    });
    until("sign-in", 15, &mut [&mut *app], |a| a[0].state.screen == Screen::Main);
}

/// The runtime, the server, two signed-in apps (alice made the server and its `lounge`, bob joined
/// it) and the lounge's id.
type World = (tokio::runtime::Runtime, thehideout_server::AppState, App, App, u64);

fn world() -> World {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (url, server) = rt.block_on(start_server());
    for name in ["alice", "bob"] {
        rt.block_on(thehideout_server::accounts::create_user(
            &server,
            thehideout_server::accounts::NewUser {
                username: name,
                password: Some(PASSWORD),
                email: Some(&format!("{name}@example.test")),
            },
        ))
        .expect("create account");
    }
    let (mut alice, mut bob) = (App::new("alice", &url), App::new("bob", &url));
    sign_in(&mut alice);
    sign_in(&mut bob);

    // Alice makes a server with a voice channel; Bob joins with its invite code.
    alice.state.outbox.push(Command::CreateGuild { name: "friends".into() });
    until("the guild", 15, &mut [&mut alice], |a| !a[0].state.data.guilds.is_empty());
    let guild = alice.state.data.guilds[0].clone();
    alice.state.outbox.push(Command::CreateChannel {
        guild: guild.id,
        name: "lounge".into(),
        kind: ChannelKind::Voice,
        section: None,
        icon: Default::default(),
        color: Default::default(),
    });
    until("the lounge", 15, &mut [&mut alice], |a| a[0].lounge().is_some());
    bob.state.outbox.push(Command::JoinGuild { code: guild.invite_code.clone() });
    until("bob in the guild", 15, &mut [&mut bob], |b| b[0].lounge().is_some());
    let lounge = alice.lounge().unwrap();
    (rt, server, alice, bob, lounge)
}

fn drop_keyspace(rt: &tokio::runtime::Runtime, server: &thehideout_server::AppState) {
    rt.block_on(async {
        let _ = server
            .session
            .get_session()
            .query_unpaged(format!("DROP KEYSPACE IF EXISTS {}", server.keyspace), ())
            .await;
    });
}

#[test]
#[ignore = "needs `docker compose up -d` (ScyllaDB and LiveKit)"]
fn two_apps_join_a_voice_channel_and_one_hears_the_other_speak() {
    let (rt, server, mut alice, mut bob, lounge) = world();

    // Both join voice through the state, as a click on the channel does.
    alice.state.join_voice(lounge);
    until("alice's connection", 30, &mut [&mut alice, &mut bob], |a| {
        // Her own mic is heard locally once the engine is connected and publishing.
        a[0].state.voice.speaking.contains(&a[0].id())
    });
    bob.state.join_voice(lounge);
    let (a_id, b_id) = (alice.id(), bob.id());

    // The server's voice state shows both, on both sides.
    until("both in the channel", 30, &mut [&mut alice, &mut bob], |a| {
        [0, 1].iter().all(|i| {
            let mut ids: Vec<u64> = a[*i]
                .state
                .voice_participants(lounge)
                .iter()
                .map(|s| s.user_id)
                .collect();
            ids.sort_unstable();
            ids == [a_id.min(b_id), a_id.max(b_id)]
        })
    });

    // Bob's app sees Alice speak: her tone reached LiveKit, which named her the active speaker.
    until("bob hearing alice", 30, &mut [&mut alice, &mut bob], |a| {
        a[1].state.is_speaking(a[0].id())
    });
    assert!(
        matches!(bob.state.voice.session, VoiceSession::Connected { channel, .. } if channel == lounge)
    );
    assert!(bob.state.voice.quality.is_some() || bob.state.voice.speaking.contains(&b_id));

    // Alice muting herself reaches the server and silences her for Bob.
    alice.state.toggle_mute();
    until("alice muted for bob", 30, &mut [&mut alice, &mut bob], |a| {
        a[1].state.voice_state_of(a[0].id()).is_some_and(|s| s.self_mute)
            && !a[1].state.is_speaking(a[0].id())
    });
    assert!(alice.seen().contains(&Sound::MuteOn));
    alice.state.toggle_mute();
    until("alice heard again", 30, &mut [&mut alice, &mut bob], |a| {
        a[1].state.is_speaking(a[0].id())
    });

    // Bob's cues: Alice was already in when he joined (no sound for her), and his own join sounded.
    assert!(bob.seen().contains(&Sound::Join), "{:?}", bob.seen());
    assert!(alice.seen().contains(&Sound::Join) && alice.seen().contains(&Sound::PeerJoin));
    assert!(bob.wakes.load(Ordering::SeqCst) > 0, "engine reports wake the window");

    // Leaving: the server state clears and the engine disconnects.
    alice.state.leave_voice();
    until("alice gone", 30, &mut [&mut alice, &mut bob], |a| {
        a[0].state.voice.session == VoiceSession::None
            && a[1].state.voice_participants(lounge).iter().all(|s| s.user_id != a[0].id())
    });
    // The cue queued by the last update is handed over on the next turn.
    bob.step();
    assert!(bob.seen().contains(&Sound::PeerLeave), "{:?}", bob.seen());
    assert!(alice.seen().contains(&Sound::Disconnect));
    // Bob stops hearing her.
    until("bob hearing silence", 30, &mut [&mut alice, &mut bob], |a| {
        !a[1].state.is_speaking(a[0].id())
    });
    bob.state.leave_voice();
    until("bob gone", 15, &mut [&mut bob], |b| b[0].state.voice.session == VoiceSession::None);
    rt.block_on(async {
        let _ = server
            .session
            .get_session()
            .query_unpaged(format!("DROP KEYSPACE IF EXISTS {}", server.keyspace), ())
            .await;
    });
}

/// How many pixels of a watched track's picture have been delivered, and whether it has content
/// (not one flat colour).
fn picture(app: &App, track: &str) -> Option<(u32, u32, bool)> {
    let p = app.sink.video_pixels(track)?;
    let (w, h) = p.size();
    if w == 0 || h == 0 {
        return None;
    }
    let rgba = p.rgba();
    let varied = rgba.chunks(4).any(|px| px[..3] != rgba[..3]);
    Some((w, h, varied && rgba.len() == (w * h * 4) as usize))
}

#[test]
#[ignore = "needs `docker compose up -d` (ScyllaDB and LiveKit)"]
fn one_app_publishes_a_camera_and_a_screen_and_the_other_watches_both() {
    let (rt, server, mut alice, mut bob, lounge) = world();
    alice.state.join_voice(lounge);
    bob.state.join_voice(lounge);
    let (a_id, b_id) = (alice.id(), bob.id());
    until("both connected", 30, &mut [&mut alice, &mut bob], |a| {
        [0, 1].iter().all(|i| {
            // Each is in the room and hears its own generated tone: the engine is connected.
            a[*i].state.voice.speaking.contains(&a[*i].id())
                && a[*i].state.voice_participants(lounge).len() == 2
        })
    });
    // Bob has the stage open before anything is published: nothing to watch yet.
    bob.state.open_stage();
    assert!(bob.state.stage_open());

    // Alice turns her camera on: a generated picture stands in for the hardware.
    alice.state.toggle_camera();
    until("alice's camera published", 30, &mut [&mut alice, &mut bob], |a| {
        // The server's flag, seen by Bob, and Bob's own track for her.
        a[1].state.voice_state_of(a[0].id()).is_some_and(|s| s.self_video)
            && a[1].state.voice.video_tracks.get(&a[0].id()).is_some_and(|t| t.camera.is_some())
    });
    let camera = bob.state.voice.video_tracks[&a_id].camera.clone().unwrap();
    // Alice's own preview is a track of hers too.
    assert!(alice.state.voice.video_tracks.get(&a_id).is_some_and(|t| t.camera.is_some()));

    // The stage subscribed Bob's sink to it, and a real picture arrives.
    until("bob receiving alice's camera", 30, &mut [&mut alice, &mut bob], |a| {
        picture(a[1], &camera).is_some()
    });
    let (w, h, varied) = picture(&bob, &camera).unwrap();
    assert!(w >= 16 && h >= 16, "{w}x{h}");
    assert!(varied, "the picture is not one flat colour");
    // Bob's tiles: Alice's camera and his own avatar, no one big.
    let kinds: Vec<_> = bob.state.stage_tiles().iter().map(|t| (t.user, t.kind)).collect();
    assert!(kinds.contains(&(a_id, voice::TileKind::Camera)), "{kinds:?}");
    assert!(kinds.contains(&(b_id, voice::TileKind::Avatar)), "{kinds:?}");

    // Frames keep coming: the picture changes (the generated one scrolls).
    let before = bob.sink.video_pixels(&camera).unwrap().signature();
    until("a newer frame", 30, &mut [&mut alice, &mut bob], |a| {
        a[1].sink.video_pixels(&camera).unwrap().signature() != before
    });

    // Alice also shares a screen (the same generator): a second track, and the share is big.
    alice.state.start_screen_share(thehideout_voice::ScreenSource {
        id: 1,
        title: "Entire screen".into(),
        kind: thehideout_voice::ScreenKind::Screen,
    });
    until("alice's share published", 30, &mut [&mut alice, &mut bob], |a| {
        a[1].state.voice_state_of(a[0].id()).is_some_and(|s| s.self_stream)
            && a[1].state.voice.video_tracks.get(&a[0].id()).is_some_and(|t| t.screen.is_some())
    });
    let screen = bob.state.voice.video_tracks[&a_id].screen.clone().unwrap();
    until("bob receiving alice's screen", 30, &mut [&mut alice, &mut bob], |a| {
        picture(a[1], &screen).is_some()
    });
    let tiles = bob.state.stage_tiles();
    assert_eq!(bob.state.stage_focus(&tiles), Some((a_id, voice::TileKind::Screen)));

    // Bob closes the stage: his subscriptions go; Alice's tracks stay up.
    bob.state.close_stage();
    bob.step();
    assert!(bob.sink.video_pixels(&camera).is_none() && bob.sink.video_pixels(&screen).is_none());
    assert!(bob.state.voice.watching.is_empty());

    // Alice stops both: the flags drop for Bob, and her tracks end for him.
    alice.state.toggle_camera();
    alice.state.stop_screen_share();
    until("both tracks gone", 30, &mut [&mut alice, &mut bob], |a| {
        a[1].state.voice_state_of(a[0].id()).is_some_and(|s| !s.self_video && !s.self_stream)
            && !a[1].state.voice.video_tracks.contains_key(&a[0].id())
    });
    assert!(!alice.state.voice.camera && alice.state.voice.sharing.is_none());

    alice.state.leave_voice();
    bob.state.leave_voice();
    until("both gone", 15, &mut [&mut alice, &mut bob], |a| {
        a[0].state.voice.session == VoiceSession::None && a[1].state.voice.session == VoiceSession::None
    });
    drop_keyspace(&rt, &server);
}
