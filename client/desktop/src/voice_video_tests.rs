//! Camera, screen share and the voice stage: the toggles and the intents and server flags they
//! cause, a failure that reverts with a toast, the engine's tracks in state, the stage opening,
//! closing and resetting, which tile is big, the sink subscribing only to tiles that are on
//! screen, the share picker, and the same through the real view (names, presses).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{channel, Sender};

use rune::Runtime;
use serde_json::Value;
use thehideout_api::VoiceState;
use thehideout_design::components::ToastKind;
use thehideout_voice_types::{AudioDevice, ScreenKind, ScreenSource, VideoKind, VoiceEvent};

use crate::app::App;
use crate::backend::{Backend, Command};
use crate::fake::{FakeAudio, FakeBackend, ANA, BRUNO, GENERAL, LOUNGE, TIM};
use crate::scenes::{booted_as, voice_screen_as};
use crate::state::AppState;
use crate::tests::{click, has, runtime, settle};
use crate::voice::{Focus, MainView, Tile, TileKind, VoiceIntent, VoiceSink};
use crate::voice_engine::{mirrored, Engine, EngineSink, SoundOut};

// ---- helpers ---------------------------------------------------------------------------------

/// Records the intents the engine would get.
#[derive(Clone, Default)]
struct Recorder(Rc<RefCell<Vec<VoiceIntent>>>);

impl VoiceSink for Recorder {
    fn intent(&mut self, intent: VoiceIntent) {
        self.0.borrow_mut().push(intent);
    }
}

impl Recorder {
    fn take(&self) -> Vec<VoiceIntent> {
        std::mem::take(&mut self.0.borrow_mut())
    }
}

/// Tim connected to the lounge (Ana and Bruno there), the recorder wired in.
fn connected() -> (App, FakeBackend, Recorder) {
    let (mut app, fake) = booted_as(true, "tim");
    let rec = Recorder::default();
    app.set_voice_sink(Box::new(rec.clone()));
    app.act(|s| s.join_voice(LOUNGE));
    app.pump();
    rec.take();
    (app, fake, rec)
}

fn track(user: u64, kind: VideoKind) -> VoiceEvent {
    VoiceEvent::VideoTrack {
        identity: user.to_string(),
        source: kind,
        track_id: FakeAudio::track(user, kind),
    }
}

fn ended(user: u64, kind: VideoKind) -> VoiceEvent {
    VoiceEvent::VideoTrackEnded {
        identity: user.to_string(),
        track_id: FakeAudio::track(user, kind),
    }
}

fn video_commands(fake: &FakeBackend) -> Vec<(Option<bool>, Option<bool>)> {
    Backend::sent(&fake.clone())
        .into_iter()
        .filter_map(|c| match c {
            Command::UpdateVideoState {
                self_video,
                self_stream,
            } => Some((self_video, self_stream)),
            _ => None,
        })
        .collect()
}

fn mine(app: &App) -> VoiceState {
    app.state
        .voice_state_of(TIM)
        .cloned()
        .expect("tim is in voice")
}

fn window(id: u64, title: &str) -> ScreenSource {
    ScreenSource {
        id,
        title: title.into(),
        kind: ScreenKind::Window,
    }
}

fn display(id: u64) -> ScreenSource {
    ScreenSource {
        id,
        title: "Entire screen".into(),
        kind: ScreenKind::Screen,
    }
}

fn last_toast(s: &AppState) -> (ToastKind, String) {
    let t = s.ui.toasts.back().expect("a toast");
    (t.kind, t.message.clone())
}

// ---- the camera ------------------------------------------------------------------------------

#[test]
fn the_camera_toggle_asks_the_engine_and_the_server_flag_follows_the_track() {
    let (mut app, fake, rec) = connected();
    app.act(|s| s.toggle_camera());
    assert!(app.state.voice.camera);
    assert_eq!(rec.take(), [VoiceIntent::SetCamera(true)]);
    // Nothing is claimed until the engine says the track is up.
    app.pump();
    assert!(!mine(&app).self_video);
    assert_eq!(video_commands(&fake), []);

    app.state.voice_event(track(TIM, VideoKind::Camera));
    app.pump();
    assert_eq!(video_commands(&fake), [(Some(true), Some(false))]);
    assert!(mine(&app).self_video && !mine(&app).self_stream);

    app.act(|s| s.toggle_camera());
    assert!(!app.state.voice.camera);
    assert_eq!(rec.take(), [VoiceIntent::SetCamera(false)]);
    app.state.voice_event(ended(TIM, VideoKind::Camera));
    app.pump();
    assert_eq!(
        video_commands(&fake).last(),
        Some(&(Some(false), Some(false)))
    );
    assert!(!mine(&app).self_video);
}

#[test]
fn video_is_only_offered_while_connected() {
    let (mut app, _fake) = booted_as(true, "tim");
    app.act(|s| s.toggle_camera());
    app.act(|s| s.open_screen_picker());
    assert!(!app.state.voice.camera && !app.state.voice.picker);
    // Joining is not connected either.
    app.act(|s| s.join_voice(LOUNGE));
    app.act(|s| s.toggle_camera());
    assert!(!app.state.voice.camera);
}

#[test]
fn a_camera_that_fails_reverts_and_says_why() {
    let (mut app, _fake, _rec) = connected();
    app.act(|s| s.toggle_camera());
    assert!(app.state.voice.camera);
    app.state.voice_event(VoiceEvent::Error(
        "camera unsupported: build thehideout-voice with the `camera` feature".into(),
    ));
    assert!(!app.state.voice.camera, "the button state reverts");
    let (kind, message) = last_toast(&app.state);
    assert_eq!(kind, ToastKind::Danger);
    assert!(message.to_lowercase().contains("camera unsupported"), "{message}");
    // The same for a camera that was refused permission.
    app.act(|s| s.toggle_camera());
    app.state.voice_event(VoiceEvent::Error(
        "camera permission was not granted".into(),
    ));
    assert!(!app.state.voice.camera);
}

#[test]
fn an_error_leaves_a_camera_that_is_publishing_alone() {
    let (mut app, _fake, _rec) = connected();
    app.act(|s| s.toggle_camera());
    app.state.voice_event(track(TIM, VideoKind::Camera));
    app.state
        .voice_event(VoiceEvent::Error("something unrelated".into()));
    assert!(app.state.voice.camera);
}

#[test]
fn a_camera_that_stops_by_itself_clears_the_button_and_the_flag() {
    let (mut app, fake, _rec) = connected();
    app.act(|s| s.toggle_camera());
    app.state.voice_event(track(TIM, VideoKind::Camera));
    app.pump();
    app.state.voice_event(ended(TIM, VideoKind::Camera));
    assert!(!app.state.voice.camera);
    app.pump();
    assert!(!mine(&app).self_video);
    assert_eq!(
        video_commands(&fake).last(),
        Some(&(Some(false), Some(false)))
    );
}

// ---- the screen share ------------------------------------------------------------------------

#[test]
fn the_picker_lists_screens_then_windows_and_picking_starts_a_share() {
    let (mut app, fake, rec) = connected();
    app.act(|s| s.toggle_screen_share());
    assert!(
        app.state.voice.picker,
        "no share yet: the button opens the picker"
    );
    assert_eq!(rec.take(), [VoiceIntent::ListScreenSources]);
    assert!(
        app.state.share_choices().is_empty(),
        "nothing listed until the engine answers"
    );

    // The engine lists a window first; the picker shows the screen first.
    app.state.voice_event(VoiceEvent::ScreenSources(vec![
        window(101, "Terminal"),
        display(1),
        window(102, "Figma"),
    ]));
    let titles: Vec<String> = app
        .state
        .share_choices()
        .iter()
        .map(|s| s.title.clone())
        .collect();
    assert_eq!(titles, ["Entire screen", "Terminal", "Figma"]);

    app.act(|s| s.start_screen_share(window(102, "Figma")));
    assert!(!app.state.voice.picker);
    assert_eq!(app.state.voice.sharing, Some(window(102, "Figma")));
    assert_eq!(
        rec.take(),
        [VoiceIntent::SetScreenShare(Some(window(102, "Figma")))]
    );

    app.state.voice_event(track(TIM, VideoKind::Screen));
    app.pump();
    assert_eq!(video_commands(&fake), [(Some(false), Some(true))]);
    assert!(mine(&app).self_stream);

    // With a share running the same button stops it.
    app.act(|s| s.toggle_screen_share());
    assert!(app.state.voice.sharing.is_none() && !app.state.voice.picker);
    assert_eq!(rec.take(), [VoiceIntent::SetScreenShare(None)]);
    app.state.voice_event(ended(TIM, VideoKind::Screen));
    app.pump();
    assert!(!mine(&app).self_stream);
}

#[test]
fn closing_the_picker_starts_nothing() {
    let (mut app, _fake, rec) = connected();
    app.act(|s| s.open_screen_picker());
    rec.take();
    app.act(|s| s.close_screen_picker());
    assert!(!app.state.voice.picker && app.state.voice.sharing.is_none());
    assert_eq!(rec.take(), []);
}

#[test]
fn a_share_that_fails_reverts_and_closes_the_picker() {
    let (mut app, _fake, _rec) = connected();
    app.act(|s| s.start_screen_share(window(7, "Gone")));
    app.state
        .voice_event(VoiceEvent::Error("window \"Gone\" is gone".into()));
    assert!(app.state.voice.sharing.is_none());
    assert_eq!(last_toast(&app.state).0, ToastKind::Danger);
}

#[test]
fn a_share_the_capture_ends_clears_the_state() {
    let (mut app, _fake, _rec) = connected();
    app.act(|s| s.start_screen_share(display(1)));
    app.state.voice_event(track(TIM, VideoKind::Screen));
    app.state.voice_event(ended(TIM, VideoKind::Screen));
    assert!(app.state.voice.sharing.is_none());
}

// ---- tracks in state -------------------------------------------------------------------------

#[test]
fn video_tracks_populate_state_and_end() {
    let (mut app, _fake, _rec) = connected();
    assert!(app.state.voice_event(track(ANA, VideoKind::Camera)));
    assert!(app.state.voice_event(track(ANA, VideoKind::Screen)));
    assert!(app.state.voice_event(track(BRUNO, VideoKind::Camera)));
    let t = &app.state.voice.video_tracks;
    assert_eq!(t[&ANA].camera.as_deref(), Some("fake-2-camera"));
    assert_eq!(t[&ANA].screen.as_deref(), Some("fake-2-screen"));
    assert_eq!(t[&BRUNO].camera.as_deref(), Some("fake-3-camera"));
    assert!(
        !app.state.voice_event(track(ANA, VideoKind::Camera)),
        "a repeat changes nothing"
    );

    assert!(app.state.voice_event(ended(ANA, VideoKind::Camera)));
    assert_eq!(app.state.voice.video_tracks[&ANA].camera, None);
    assert!(app.state.voice.video_tracks[&ANA].screen.is_some());
    assert!(app.state.voice_event(ended(ANA, VideoKind::Screen)));
    assert!(
        !app.state.voice.video_tracks.contains_key(&ANA),
        "no tracks left, no entry"
    );
    assert!(
        !app.state.voice_event(ended(ANA, VideoKind::Screen)),
        "an unknown track is nothing"
    );
    // Someone else's track does not raise our flag.
    app.pump();
    assert!(!mine(&app).self_video);
}

#[test]
fn reports_about_a_connection_we_dont_have_are_ignored() {
    let (mut app, _fake) = booted_as(true, "tim");
    assert!(!app.state.voice_event(track(ANA, VideoKind::Camera)));
    assert!(app.state.voice.video_tracks.is_empty());
}

#[test]
fn a_dropped_connection_forgets_video() {
    let (mut app, _fake, _rec) = connected();
    app.act(|s| s.toggle_camera());
    app.state.voice_event(track(TIM, VideoKind::Camera));
    app.state.voice_event(track(ANA, VideoKind::Camera));
    app.state.voice_event(VoiceEvent::Disconnected {
        reason: "gone".into(),
    });
    assert!(!app.state.voice.camera);
    assert!(app.state.voice.video_tracks.is_empty());
}

#[test]
fn rejoining_clears_a_stale_camera_flag() {
    let (mut app, fake, _rec) = connected();
    app.act(|s| s.toggle_camera());
    app.state.voice_event(track(TIM, VideoKind::Camera));
    app.pump();
    assert!(mine(&app).self_video);
    // The engine dropped; the retry's JoinVoice finds the server still saying "camera on".
    app.state.voice_event(VoiceEvent::Disconnected {
        reason: "gone".into(),
    });
    app.state.voice.session = crate::voice::VoiceSession::Joining { channel: LOUNGE };
    app.state.voice_joined(
        LOUNGE,
        Ok(thehideout_api::VoiceJoin {
            url: "ws://fake".into(),
            token: "fake".into(),
            channel_id: LOUNGE,
        }),
    );
    app.pump();
    assert_eq!(
        video_commands(&fake).last(),
        Some(&(Some(false), Some(false)))
    );
    assert!(!mine(&app).self_video);
}

// ---- the stage -------------------------------------------------------------------------------

#[test]
fn the_stage_opens_from_my_channel_row_and_closes() {
    let (mut app, fake, _rec) = connected();
    assert_eq!(app.state.voice.view, MainView::Chat);
    app.act(|s| s.select_channel(LOUNGE));
    assert_eq!(app.state.voice.view, MainView::Stage);
    assert!(app.state.stage_open());
    // Pressing it again does not rejoin.
    app.pump();
    let joins = Backend::sent(&fake.clone())
        .iter()
        .filter(|c| matches!(c, Command::JoinVoice { .. }))
        .count();
    assert_eq!(joins, 1, "only the first press joined");
    app.act(|s| s.close_stage());
    assert_eq!(app.state.voice.view, MainView::Chat);
}

#[test]
fn the_stage_only_opens_while_connected() {
    let (mut app, _fake) = booted_as(true, "tim");
    app.act(|s| s.open_stage());
    assert_eq!(app.state.voice.view, MainView::Chat);
    app.act(|s| s.join_voice(LOUNGE));
    app.act(|s| s.open_stage());
    assert!(!app.state.stage_open(), "still joining");
    app.pump();
    app.act(|s| s.open_stage());
    assert!(app.state.stage_open());
}

#[test]
fn going_anywhere_returns_to_the_chat() {
    let (mut app, _fake, _rec) = connected();
    app.act(|s| s.open_stage());
    app.act(|s| s.select_channel(GENERAL));
    assert_eq!(app.state.voice.view, MainView::Chat);
    app.act(|s| s.open_stage());
    app.act(|s| s.go_home());
    assert_eq!(app.state.voice.view, MainView::Chat);
}

#[test]
fn leaving_voice_resets_the_stage_and_the_focus() {
    let (mut app, _fake, _rec) = connected();
    app.state.voice_event(track(ANA, VideoKind::Screen));
    app.act(|s| s.watch_stream(ANA));
    assert_eq!(app.state.voice.focus, Focus::Tile(ANA, TileKind::Screen));
    app.act(|s| s.leave_voice());
    assert_eq!(app.state.voice.view, MainView::Chat);
    assert_eq!(app.state.voice.focus, Focus::Auto);
    assert!(app.state.voice.video_tracks.is_empty());
    assert!(!app.state.stage_open());
}

#[test]
fn being_dropped_by_the_server_resets_the_stage() {
    let (mut app, fake, _rec) = connected();
    app.act(|s| s.open_stage());
    fake.voice_leave_as(TIM);
    app.pump();
    assert_eq!(app.state.voice.view, MainView::Chat);
}

fn tile(user: u64, kind: TileKind) -> (u64, TileKind) {
    (user, kind)
}

fn kinds(tiles: &[Tile]) -> Vec<(u64, TileKind)> {
    tiles.iter().map(|t| (t.user, t.kind)).collect()
}

#[test]
fn tiles_are_cameras_screens_or_an_avatar_in_join_order() {
    let (mut app, _fake, _rec) = connected();
    // Ana, Bruno, Tim are in the lounge, joined in that order.
    assert_eq!(
        kinds(&app.state.stage_tiles()),
        [
            tile(ANA, TileKind::Avatar),
            tile(BRUNO, TileKind::Avatar),
            tile(TIM, TileKind::Avatar)
        ]
    );
    app.state.voice_event(track(ANA, VideoKind::Camera));
    app.state.voice_event(track(ANA, VideoKind::Screen));
    app.state.voice_event(track(BRUNO, VideoKind::Screen));
    let tiles = app.state.stage_tiles();
    assert_eq!(
        kinds(&tiles),
        [
            tile(ANA, TileKind::Camera),
            tile(ANA, TileKind::Screen),
            // a screen alone replaces the avatar
            tile(BRUNO, TileKind::Screen),
            tile(TIM, TileKind::Avatar)
        ]
    );
    assert_eq!(tiles[0].track.as_deref(), Some("fake-2-camera"));
    assert_eq!(tiles[3].track, None);
}

#[test]
fn a_screen_share_is_big_unless_the_grid_was_chosen() {
    let (mut app, _fake, _rec) = connected();
    let tiles = app.state.stage_tiles();
    assert_eq!(app.state.stage_focus(&tiles), None, "no share: a grid");

    app.state.voice_event(track(ANA, VideoKind::Camera));
    app.state.voice_event(track(BRUNO, VideoKind::Screen));
    let tiles = app.state.stage_tiles();
    assert_eq!(
        app.state.stage_focus(&tiles),
        Some(tile(BRUNO, TileKind::Screen))
    );

    // Clicking a camera focuses it; clicking it again goes back to the grid, not to the share.
    app.act(|s| s.focus_tile(ANA, TileKind::Camera));
    let tiles = app.state.stage_tiles();
    assert_eq!(
        app.state.stage_focus(&tiles),
        Some(tile(ANA, TileKind::Camera))
    );
    app.act(|s| s.focus_tile(ANA, TileKind::Camera));
    let tiles = app.state.stage_tiles();
    assert_eq!(app.state.voice.focus, Focus::Grid);
    assert_eq!(app.state.stage_focus(&tiles), None);
    // Clicking the share while the auto focus is on it also goes to the grid.
    app.state.voice.focus = Focus::Auto;
    app.act(|s| s.focus_tile(BRUNO, TileKind::Screen));
    assert_eq!(app.state.voice.focus, Focus::Grid);
}

#[test]
fn a_focused_tile_that_goes_away_falls_back() {
    let (mut app, _fake, _rec) = connected();
    app.state.voice_event(track(ANA, VideoKind::Camera));
    app.act(|s| s.focus_tile(ANA, TileKind::Camera));
    app.state.voice_event(ended(ANA, VideoKind::Camera));
    let tiles = app.state.stage_tiles();
    assert_eq!(app.state.stage_focus(&tiles), None);
    // The choice was kept, so her camera is big again when it comes back.
    app.state.voice_event(track(ANA, VideoKind::Camera));
    let tiles = app.state.stage_tiles();
    assert_eq!(
        app.state.stage_focus(&tiles),
        Some(tile(ANA, TileKind::Camera))
    );
}

#[test]
fn watch_opens_the_stage_on_the_share() {
    let (mut app, _fake, _rec) = connected();
    app.state.voice_event(track(ANA, VideoKind::Screen));
    app.act(|s| s.watch_stream(ANA));
    assert!(app.state.stage_open());
    let tiles = app.state.stage_tiles();
    assert_eq!(
        app.state.stage_focus(&tiles),
        Some(tile(ANA, TileKind::Screen))
    );
    // Someone with no video: the stage opens on the grid.
    app.act(|s| s.close_stage());
    app.act(|s| s.watch_stream(BRUNO));
    assert_eq!(app.state.voice.focus, Focus::Auto);
}

#[test]
fn grid_columns_grow_with_the_tiles() {
    use crate::ui::voice_stage::grid_columns;
    let cols: Vec<usize> = [0, 1, 2, 3, 4, 5, 9, 10, 20]
        .into_iter()
        .map(grid_columns)
        .collect();
    assert_eq!(cols, [1, 1, 2, 2, 2, 3, 3, 4, 4]);
}

// ---- subscribing only to what is on screen ---------------------------------------------------

#[test]
fn the_sink_is_subscribed_only_while_a_tile_shows_the_track() {
    let (mut app, _fake, rec) = connected();
    app.state.voice_event(track(ANA, VideoKind::Camera));
    app.state.voice_event(track(TIM, VideoKind::Camera));
    app.pump();
    assert_eq!(
        rec.take(),
        [],
        "the transcript is showing: nothing to decode"
    );

    app.act(|s| s.open_stage());
    let watched = rec.take();
    assert_eq!(
        watched,
        [
            VoiceIntent::WatchVideo {
                track: "fake-2-camera".into(),
                mirror: false
            },
            // our own camera is a mirror
            VoiceIntent::WatchVideo {
                track: "fake-1-camera".into(),
                mirror: true
            },
        ]
    );
    app.act(|_| {});
    assert_eq!(rec.take(), [], "a second flush adds nothing");

    app.state.voice_event(track(BRUNO, VideoKind::Screen));
    app.act(|_| {});
    assert_eq!(
        rec.take(),
        [VoiceIntent::WatchVideo {
            track: "fake-3-screen".into(),
            mirror: false
        }]
    );

    app.state.voice_event(ended(ANA, VideoKind::Camera));
    app.act(|_| {});
    assert_eq!(
        rec.take(),
        [VoiceIntent::UnwatchVideo {
            track: "fake-2-camera".into()
        }]
    );

    app.act(|s| s.close_stage());
    let mut dropped = rec.take();
    dropped.sort_by_key(|i| format!("{i:?}"));
    assert_eq!(
        dropped,
        [
            VoiceIntent::UnwatchVideo {
                track: "fake-1-camera".into()
            },
            VoiceIntent::UnwatchVideo {
                track: "fake-3-screen".into()
            },
        ]
    );
    app.act(|s| s.open_stage());
    assert_eq!(rec.take().len(), 2, "watching again subscribes again");
}

// ---- the sink over a recording engine --------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Call {
    Camera(bool),
    Screen(Option<u64>),
    List,
    Subscribe(String),
    Release(String),
    CameraDevice(Option<String>),
}

#[derive(Clone, Default)]
struct Fake(Rc<RefCell<Vec<Call>>>);

impl Engine for Fake {
    fn connect(&self, _: &str, _: &str, _: bool, _: bool) {}
    fn disconnect(&self) {}
    fn set_mute(&self, _: bool) {}
    fn set_deaf(&self, _: bool) {}
    fn set_user_volume(&self, _: &str, _: f32) {}
    fn set_camera(&self, on: bool) {
        self.0.borrow_mut().push(Call::Camera(on));
    }
    fn set_screen_share(&self, source: Option<ScreenSource>) {
        self.0.borrow_mut().push(Call::Screen(source.map(|s| s.id)));
    }
    fn list_screen_sources(&self) {
        self.0.borrow_mut().push(Call::List);
    }
    fn set_camera_device(&self, id: Option<String>) {
        self.0.borrow_mut().push(Call::CameraDevice(id));
    }
    fn video_frames(&self, track: &str) -> Option<thehideout_voice_types::VideoSink> {
        self.0.borrow_mut().push(Call::Subscribe(track.into()));
        None
    }
    fn release_video(&self, track: &str) {
        self.0.borrow_mut().push(Call::Release(track.into()));
    }
}

struct Quiet;
impl SoundOut for Quiet {
    fn play(&mut self, _: crate::voice::Sound) {}
}

fn engine_sink() -> (EngineSink<Fake>, Fake, Sender<VoiceEvent>) {
    let calls = Fake::default();
    let (events, rx) = channel();
    let rx = Rc::new(RefCell::new(Some(rx)));
    let c = calls.clone();
    let sink = EngineSink::new(
        Box::new(move |_| (c.clone(), rx.borrow_mut().take().expect("built once"))),
        Box::new(Quiet),
    );
    (sink, calls, events)
}

fn connect_sink(sink: &mut EngineSink<Fake>) {
    sink.intent(VoiceIntent::Connect {
        url: "ws://x".into(),
        token: "t".into(),
    });
    sink.flushed();
}

#[test]
fn the_sink_turns_video_intents_into_engine_calls() {
    let (mut sink, calls, _events) = engine_sink();
    // Before any room the engine does not exist: a camera has nothing to publish into.
    sink.intent(VoiceIntent::SetCamera(true));
    assert!(!sink.built());
    connect_sink(&mut sink);
    sink.intent(VoiceIntent::SetCamera(true));
    sink.intent(VoiceIntent::ListScreenSources);
    sink.intent(VoiceIntent::SetScreenShare(Some(window(9, "w"))));
    sink.intent(VoiceIntent::SetScreenShare(None));
    sink.intent(VoiceIntent::SetCamera(false));
    assert_eq!(
        calls.0.take(),
        [
            Call::Camera(true),
            Call::List,
            Call::Screen(Some(9)),
            Call::Screen(None),
            Call::Camera(false)
        ]
    );
}

#[test]
fn the_sink_subscribes_on_watch_and_releases_on_unwatch() {
    let (mut sink, calls, events) = engine_sink();
    connect_sink(&mut sink);
    sink.intent(VoiceIntent::WatchVideo {
        track: "t1".into(),
        mirror: false,
    });
    sink.intent(VoiceIntent::WatchVideo {
        track: "t1".into(),
        mirror: false,
    });
    assert_eq!(
        calls.0.borrow().clone(),
        [Call::Subscribe("t1".into())],
        "once per track"
    );
    assert!(
        sink.video_pixels("t1").is_some(),
        "the view gets a picture to draw"
    );
    assert!(sink.video_pixels("t2").is_none());

    sink.intent(VoiceIntent::UnwatchVideo { track: "t1".into() });
    assert_eq!(
        calls.0.take(),
        [Call::Subscribe("t1".into()), Call::Release("t1".into())]
    );
    assert!(sink.video_pixels("t1").is_none());
    sink.intent(VoiceIntent::UnwatchVideo { track: "t1".into() });
    assert_eq!(
        calls.0.take(),
        [],
        "unwatching what is not watched does nothing"
    );

    // A track that ends, and a leave, drop their watches.
    sink.intent(VoiceIntent::WatchVideo {
        track: "t2".into(),
        mirror: false,
    });
    sink.intent(VoiceIntent::WatchVideo {
        track: "t3".into(),
        mirror: false,
    });
    events
        .send(VoiceEvent::VideoTrackEnded {
            identity: "2".into(),
            track_id: "t2".into(),
        })
        .unwrap();
    let got = sink.events();
    assert_eq!(got.len(), 1);
    assert!(sink.video_pixels("t2").is_none() && sink.video_pixels("t3").is_some());
    sink.intent(VoiceIntent::Disconnect);
    assert!(sink.video_pixels("t3").is_none());
}

#[test]
fn an_ended_track_and_a_teardown_release_what_the_engine_kept() {
    let (mut sink, calls, events) = engine_sink();
    connect_sink(&mut sink);
    for t in ["t2", "t3"] {
        sink.intent(VoiceIntent::WatchVideo { track: t.into(), mirror: false });
    }
    calls.0.take();
    // The track ends: the sink drops the watch itself, so it must release it too.
    events.send(VoiceEvent::VideoTrackEnded { identity: "2".into(), track_id: "t2".into() }).unwrap();
    sink.events();
    assert_eq!(calls.0.take(), [Call::Release("t2".into())]);
    // The app's later unwatch finds nothing and releases nothing a second time.
    sink.intent(VoiceIntent::UnwatchVideo { track: "t2".into() });
    assert_eq!(calls.0.take(), []);
    // Leaving releases every track still watched.
    sink.intent(VoiceIntent::Disconnect);
    assert_eq!(calls.0.take(), [Call::Release("t3".into())]);
    // And an ended track nobody watches releases nothing.
    events.send(VoiceEvent::VideoTrackEnded { identity: "2".into(), track_id: "t9".into() }).unwrap();
    sink.events();
    assert_eq!(calls.0.take(), []);
}

// ---- the camera picker (web) and the share toggle without the app's picker -----------------------

fn cameras(list: &[(&str, &str)], default: Option<&str>) -> VoiceEvent {
    VoiceEvent::Cameras {
        cameras: list.iter().map(|(i, n)| AudioDevice { id: (*i).into(), name: (*n).into() }).collect(),
        default: default.map(Into::into),
    }
}

fn audio(inputs: &[(&str, &str)], outputs: &[(&str, &str)]) -> VoiceEvent {
    let devs = |l: &[(&str, &str)]| l.iter().map(|(i, n)| AudioDevice { id: (*i).into(), name: (*n).into() }).collect();
    VoiceEvent::Devices { inputs: devs(inputs), outputs: devs(outputs), default_input: None, default_output: None }
}

/// The engine's report, then the flush that hands the intents it caused to the sink.
fn feed(app: &mut App, ev: VoiceEvent) {
    app.act(|s| {
        s.voice_event(ev);
    });
}

fn camera_intents(rec: &Recorder) -> Vec<Option<String>> {
    rec.take()
        .into_iter()
        .filter_map(|i| match i {
            VoiceIntent::SetCameraDevice(id) => Some(id),
            _ => None,
        })
        .collect()
}

#[test]
fn a_saved_camera_that_is_gone_falls_back_to_the_default_and_stays_chosen() {
    let (mut app, _fake, rec) = connected();
    app.act(|s| s.voice_set_camera(Some("cam-gone".into())));
    assert_eq!(camera_intents(&rec), [Some("cam-gone".into())], "chosen: sent");
    // The list arrives without it: the engine gets the default, the choice stays saved.
    feed(&mut app, cameras(&[("cam-1", "Built-in")], Some("cam-1")));
    assert_eq!(app.state.effective_camera(), None);
    assert_eq!(camera_intents(&rec), [None]);
    assert_eq!(app.state.voice.prefs.camera.as_deref(), Some("cam-gone"));
    // Back again: used again, and named as it is called now.
    feed(&mut app, cameras(&[("cam-1", "Built-in"), ("cam-gone", "USB cam")], None));
    assert_eq!(camera_intents(&rec), [Some("cam-gone".into())]);
    assert_eq!(app.state.voice.prefs.camera_name.as_deref(), Some("USB cam"));
}

#[test]
fn an_empty_camera_list_keeps_the_saved_camera() {
    let (mut app, _fake, rec) = connected();
    app.act(|s| s.voice_set_camera(Some("cam-1".into())));
    rec.take();
    // No permission yet: placeholders without an id, or nothing at all.
    feed(&mut app, cameras(&[("", "")], None));
    assert_eq!(app.state.effective_camera().as_deref(), Some("cam-1"));
    assert!(app.state.voice.devices.as_ref().unwrap().cameras.is_empty(), "placeholders are not cameras");
    assert_eq!(camera_intents(&rec), [Some("cam-1".into())]);
}

#[test]
fn choosing_a_camera_pushes_an_intent_only_on_change_and_never_an_empty_id() {
    let (mut app, _fake, rec) = connected();
    feed(&mut app, cameras(&[("cam-1", "Built-in")], None));
    rec.take();
    app.act(|s| s.voice_set_camera(Some("cam-1".into())));
    assert_eq!(camera_intents(&rec), [Some("cam-1".into())]);
    assert_eq!(app.state.voice.prefs.camera_name.as_deref(), Some("Built-in"));
    app.act(|s| s.voice_set_camera(Some("cam-1".into())));
    assert_eq!(camera_intents(&rec), [], "the same choice is not sent again");
    // An empty id means the default and is never saved as a camera.
    app.act(|s| s.voice_set_camera(Some(String::new())));
    assert_eq!(app.state.voice.prefs.camera, None);
    assert_eq!(camera_intents(&rec), [None]);
}

#[test]
fn the_camera_and_audio_lists_arrive_in_either_order_without_blanking_each_other() {
    for camera_first in [true, false] {
        let (mut app, _fake, _rec) = connected();
        app.act(|s| s.voice_set_input(Some("mic-1".into())));
        let (a, c) = (audio(&[("mic-1", "Mic")], &[("out-1", "Speakers")]), cameras(&[("cam-1", "Cam")], None));
        let order = if camera_first { [c, a] } else { [a, c] };
        for (i, ev) in order.into_iter().enumerate() {
            feed(&mut app, ev);
            if camera_first && i == 0 {
                // Only the cameras are in: the saved microphone must not look unplugged.
                assert_eq!(app.state.effective_input().as_deref(), Some("mic-1"), "camera list first");
            }
        }
        let d = app.state.voice.devices.as_ref().unwrap();
        assert_eq!((d.inputs.len(), d.outputs.len(), d.cameras.len()), (1, 1, 1), "camera_first={camera_first}");
        assert_eq!(app.state.effective_input().as_deref(), Some("mic-1"));
    }
}

#[test]
fn the_browser_share_starts_directly_from_the_click_and_a_cancel_is_quiet() {
    let (mut app, _fake, rec) = connected();
    // No app picker (the web build): one press sends the share request, no picker state.
    app.act(|s| s.toggle_screen_share_with(false));
    assert!(matches!(rec.take().as_slice(), [VoiceIntent::SetScreenShare(Some(_))]));
    assert!(app.state.voice.sharing.is_some() && !app.state.voice.picker);
    // The browser's picker was closed: an empty error reverts it without a toast.
    let toasts = app.state.ui.toasts.len();
    feed(&mut app, VoiceEvent::Error(String::new()));
    assert!(app.state.voice.sharing.is_none());
    assert_eq!(app.state.ui.toasts.len(), toasts, "no toast for a cancelled picker");
    // A real error still says so.
    app.act(|s| s.toggle_screen_share_with(false));
    rec.take();
    feed(&mut app, VoiceEvent::Error("Screen sharing problem: boom".into()));
    assert!(app.state.voice.sharing.is_none());
    assert_eq!(last_toast(&app.state).0, ToastKind::Danger);
    // With the app's picker the same press opens it instead.
    app.act(|s| s.toggle_screen_share_with(true));
    assert!(app.state.voice.picker);
}

#[test]
fn the_sink_remembers_the_camera_and_replays_it_on_a_later_engine() {
    let (mut sink, calls, _events) = engine_sink();
    // Chosen before any room (the engine is not built yet): kept in the config, nothing called.
    sink.intent(VoiceIntent::SetCameraDevice(Some("cam-2".into())));
    assert!(!sink.built());
    assert_eq!(calls.0.take(), []);
    connect_sink(&mut sink);
    assert_eq!(calls.0.take(), [Call::CameraDevice(Some("cam-2".into()))], "replayed on the engine built by the join");
    // Later changes reach the live engine, including back to the default.
    sink.intent(VoiceIntent::SetCameraDevice(None));
    assert_eq!(calls.0.take(), [Call::CameraDevice(None)]);
}

#[test]
fn mirroring_flips_every_row() {
    let rgba: Vec<u8> = (0..2u8)
        .flat_map(|row| (0..3u8).flat_map(move |x| [row, x, 0, 255]))
        .collect();
    let frame = thehideout_voice_types::VideoFrame {
        width: 3,
        height: 2,
        rgba: rgba.clone().into(),
        seq: 1,
    };
    let flipped = mirrored(&frame);
    assert_eq!(flipped.len(), rgba.len());
    assert_eq!(&flipped[..12], &[0, 2, 0, 255, 0, 1, 0, 255, 0, 0, 0, 255]);
    assert_eq!(&flipped[12..], &[1, 2, 0, 255, 1, 1, 0, 255, 1, 0, 0, 255]);
}

// ---- through the real view -------------------------------------------------------------------

fn walk<'a>(n: &'a Value, out: &mut Vec<&'a Value>) {
    out.push(n);
    for c in n["children"].as_array().into_iter().flatten() {
        walk(c, out);
    }
}

fn node(rt: &Runtime<App>, key: &str) -> Option<Value> {
    let tree = rt.accessibility_json();
    let mut all = Vec::new();
    walk(&tree, &mut all);
    all.into_iter().find(|n| n["key"] == key).cloned()
}

fn name_of(rt: &Runtime<App>, key: &str) -> String {
    node(rt, key).unwrap_or_else(|| panic!("no accessible element keyed {key}"))["name"]
        .as_str()
        .unwrap_or("")
        .to_owned()
}

/// Tim, signed in on the voice screen with a fake engine in which Ana has her camera on and Bruno
/// shares a screen.
fn ui() -> Runtime<App> {
    let (mut app, _fake) = voice_screen_as(true, "tim");
    app.set_voice_sink(Box::new(FakeAudio::with_remote(vec![
        (ANA, VideoKind::Camera),
        (BRUNO, VideoKind::Screen),
    ])));
    app.act(|s| s.join_voice(LOUNGE));
    app.pump();
    let mut rt = runtime(app);
    settle(&mut rt, 8);
    rt
}

fn act(rt: &mut Runtime<App>, f: impl FnOnce(&mut AppState)) {
    rt.update(|a, _| {
        a.act(f);
        a.pump();
    });
    settle(rt, 8);
}

#[test]
fn the_voice_bar_offers_camera_and_share_once_connected() {
    let mut rt = ui();
    assert_eq!(name_of(&rt, "voice-camera"), "Turn on camera");
    assert_eq!(name_of(&rt, "voice-share"), "Share screen");
    click(&mut rt, "voice-camera");
    settle(&mut rt, 8);
    assert!(rt.state.state.voice.camera);
    rt.update(|a, _| a.pump());
    settle(&mut rt, 8);
    assert_eq!(name_of(&rt, "voice-camera"), "Turn off camera");
}

#[test]
fn no_media_buttons_outside_voice() {
    let (app, _fake) = voice_screen_as(true, "tim");
    let mut rt = runtime(app);
    settle(&mut rt, 8);
    assert!(!has(&rt, "voice-camera") && !has(&rt, "voice-share"));
}

#[test]
fn the_share_button_opens_the_picker_and_a_row_starts_the_share() {
    let mut rt = ui();
    click(&mut rt, "voice-share");
    settle(&mut rt, 8);
    rt.update(|a, _| a.pump());
    settle(&mut rt, 8);
    assert!(has(&rt, "share-list"), "the picker is open");
    assert_eq!(name_of(&rt, "share-source-1"), "Entire screen, screen");
    assert_eq!(
        name_of(&rt, "share-source-101"),
        "Terminal \u{2014} thehideout, window"
    );
    click(&mut rt, "share-source-101");
    settle(&mut rt, 8);
    assert!(!has(&rt, "share-list"), "picking closes it");
    assert_eq!(
        rt.state.state.voice.sharing.as_ref().map(|s| s.id),
        Some(101)
    );
    rt.update(|a, _| a.pump());
    settle(&mut rt, 8);
    assert_eq!(name_of(&rt, "voice-share"), "Stop sharing");
}

#[test]
fn cancel_closes_the_picker() {
    let mut rt = ui();
    click(&mut rt, "voice-share");
    settle(&mut rt, 8);
    click(&mut rt, "share-cancel");
    settle(&mut rt, 8);
    assert!(!has(&rt, "share-list") && rt.state.state.voice.sharing.is_none());
}

#[test]
fn the_sidebar_shows_a_camera_and_a_live_pill() {
    let mut rt = ui();
    act(&mut rt, |s| {
        for (user, cam) in [(ANA, true), (BRUNO, false)] {
            if let Some(v) = s.data.voice.values_mut().find_map(|m| m.get_mut(&user)) {
                v.self_video = cam;
                v.self_stream = !cam;
            }
        }
    });
    assert!(has(&rt, "live"), "Bruno is live");
    assert!(has(&rt, "camera"), "Ana has her camera on");
    let ana = node(&rt, "voice-user-2").expect("ana's row");
    assert!(
        ana["description"]
            .as_str()
            .unwrap_or("")
            .contains("camera on"),
        "{ana}"
    );
    let bruno = node(&rt, "voice-user-3").expect("bruno's row");
    assert!(
        bruno["description"]
            .as_str()
            .unwrap_or("")
            .contains("sharing their screen"),
        "{bruno}"
    );
}

#[test]
fn the_voice_row_opens_the_stage_whose_tiles_are_named_buttons() {
    let mut rt = ui();
    assert!(!has(&rt, "voice-stage"));
    // The row of the channel we are in.
    act(&mut rt, |s| s.select_channel(LOUNGE));
    assert!(has(&rt, "voice-stage"));
    let ana = node(&rt, "tile-camera-2").expect("ana's camera tile");
    assert_eq!(ana["role"], "button");
    assert_eq!(ana["name"], "ana\u{2019}s camera");
    assert_eq!(name_of(&rt, "tile-screen-3"), "bruno\u{2019}s screen");
    assert_eq!(name_of(&rt, "tile-avatar-1"), "You");
    // Bruno's share is big on its own.
    assert!(has(&rt, "tile-screen-3"));
    click(&mut rt, "stage-back");
    settle(&mut rt, 8);
    assert!(!has(&rt, "voice-stage"));
}

#[test]
fn pressing_a_tile_focuses_it_and_again_returns_to_the_grid() {
    let mut rt = ui();
    act(&mut rt, |s| s.open_stage());
    click(&mut rt, "tile-camera-2");
    settle(&mut rt, 8);
    assert_eq!(
        rt.state.state.voice.focus,
        Focus::Tile(ANA, TileKind::Camera)
    );
    assert!(has(&rt, "stage-strip"));
    click(&mut rt, "tile-camera-2");
    settle(&mut rt, 8);
    assert_eq!(rt.state.state.voice.focus, Focus::Grid);
    assert!(!has(&rt, "stage-strip"));
}

#[test]
fn a_live_participants_menu_offers_watch() {
    let mut rt = ui();
    act(&mut rt, |s| {
        if let Some(v) = s.data.voice.values_mut().find_map(|m| m.get_mut(&BRUNO)) {
            v.self_stream = true;
        }
    });
    act(&mut rt, |s| {
        s.ui.menu = crate::state::Menu::Participant {
            user: BRUNO,
            at: None,
        }
    });
    let tree = rt.accessibility_json();
    let mut all = Vec::new();
    walk(&tree, &mut all);
    let item = all
        .into_iter()
        .find(|n| n["role"] == "menu-item" && n["name"] == "Watch stream")
        .and_then(|n| n["key"].as_str().map(str::to_owned))
        .expect("Watch stream is offered");
    click(&mut rt, &item);
    settle(&mut rt, 8);
    assert!(rt.state.state.stage_open());
    assert_eq!(
        rt.state.state.voice.focus,
        Focus::Tile(BRUNO, TileKind::Screen)
    );
}
