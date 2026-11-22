//! The Voice & audio page and the voice hotkeys: the prefs and their round trip, the controls and
//! the engine calls they cause (a recording engine behind the real sink), the mic test and its end,
//! mute, deafen and push to talk from the keyboard, and a saved device that is gone.
//!
//! No audio: the engine is a recorder; the real one is proven in `tests/voice_e2e.rs`.
//!
//! Typing, as chosen: a focused text field takes the keys it types before the app's key handler
//! sees them, so a bare push-to-talk key (a letter) does not transmit while the composer has
//! focus, and chords (Cmd/Ctrl, Alt) and function keys, which a field leaves alone, work there too.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};

use rune::prefs::Prefs;
use rune::{Key, KeyEvent, Mods, Runtime};
use thehideout_voice::{AudioDevice, VoiceEvent, VoiceMode};

use crate::app::{fake_now, App};
use crate::fake::{LOUNGE, TIM};
use crate::format::Tz;
use crate::scenes::booted;
use crate::state::{AppState, Section};
use crate::tests::{click, has, key, rect_of, runtime, settle};
use crate::voice::{
    device_options, InputMode, VoiceIntent, VoiceKey, VoicePrefs, DEFAULT_THRESHOLD,
};
use crate::voice_engine::{Engine, EngineSink, SoundOut};

// ---- a recording engine behind the real sink ----

#[derive(Clone, Debug, PartialEq)]
enum Call {
    Connect,
    Disconnect,
    Mute(bool),
    Deaf(bool),
    Input(Option<String>),
    Output(Option<String>),
    Mode(VoiceMode),
    Ptt(bool),
    Noise(bool),
    Echo(bool),
    List,
    TestOn,
    TestOff,
}

#[derive(Clone, Default)]
struct Rec(Rc<RefCell<Vec<Call>>>);

impl Rec {
    fn push(&self, c: Call) {
        self.0.borrow_mut().push(c);
    }
}

impl Engine for Rec {
    fn connect(&self, _: &str, _: &str, _: bool, _: bool) {
        self.push(Call::Connect);
    }
    fn disconnect(&self) {
        self.push(Call::Disconnect);
    }
    fn set_mute(&self, m: bool) {
        self.push(Call::Mute(m));
    }
    fn set_deaf(&self, d: bool) {
        self.push(Call::Deaf(d));
    }
    fn set_user_volume(&self, _: &str, _: f32) {}
    fn set_input_device(&self, id: Option<String>) {
        self.push(Call::Input(id));
    }
    fn set_output_device(&self, id: Option<String>) {
        self.push(Call::Output(id));
    }
    fn set_mode(&self, mode: VoiceMode) {
        self.push(Call::Mode(mode));
    }
    fn set_ptt(&self, held: bool) {
        self.push(Call::Ptt(held));
    }
    fn set_noise_suppression(&self, on: bool) {
        self.push(Call::Noise(on));
    }
    fn set_echo_cancellation(&self, on: bool) {
        self.push(Call::Echo(on));
    }
    fn list_devices(&self) {
        self.push(Call::List);
    }
    fn start_mic_test(&self) {
        self.push(Call::TestOn);
    }
    fn stop_mic_test(&self) {
        self.push(Call::TestOff);
    }
}

struct Quiet;

impl SoundOut for Quiet {
    fn play(&mut self, _: crate::voice::Sound) {}
}

/// The app over the fake world with the recording engine installed.
struct Rig {
    rt: Runtime<App>,
    calls: Rec,
    /// The engine's side of the report channel.
    events: Sender<VoiceEvent>,
}

fn sink() -> (EngineSink<Rec>, Rec, Sender<VoiceEvent>) {
    let calls = Rec::default();
    let (events, rx) = channel();
    let rx = Rc::new(RefCell::new(Some(rx)));
    let c = calls.clone();
    let sink = EngineSink::new(
        Box::new(move |_| (c.clone(), rx.borrow_mut().take().expect("built once"))),
        Box::new(Quiet),
    );
    (sink, calls, events)
}

fn rig_with(prepare: impl FnOnce(&mut App)) -> Rig {
    let (mut app, _fake) = booted(true);
    let (sink, calls, events) = sink();
    app.set_voice_sink(Box::new(sink));
    prepare(&mut app);
    // The page is taller than the default window: every control stays on screen to be clicked.
    let mut rt = runtime(app);
    rt.set_viewport((1280.0, 1400.0), 1.0);
    settle(&mut rt, 2);
    Rig { rt, calls, events }
}

fn rig() -> Rig {
    rig_with(|_| {})
}

impl Rig {
    fn take(&self) -> Vec<Call> {
        std::mem::take(&mut self.calls.0.borrow_mut())
    }

    fn state(&self) -> &AppState {
        &self.rt.state.state
    }

    fn act(&mut self, f: impl FnOnce(&mut AppState)) {
        self.rt.update(|a, _| a.act(f));
        settle(&mut self.rt, 2);
    }

    fn open_page(&mut self) {
        self.act(|s| s.open_voice_settings());
        settle(&mut self.rt, 3);
    }

    fn join(&mut self) {
        self.act(|s| s.join_voice(LOUNGE));
        settle(&mut self.rt, 30);
        assert!(self.state().voice_status().is_some(), "joined");
    }

    /// Reports devices the way the engine does, and lets the poll take them.
    fn devices(&mut self, inputs: &[(&str, &str)], outputs: &[(&str, &str)]) {
        let list = |l: &[(&str, &str)]| -> Vec<AudioDevice> {
            l.iter()
                .map(|(id, name)| AudioDevice {
                    id: (*id).into(),
                    name: (*name).into(),
                })
                .collect()
        };
        self.events
            .send(VoiceEvent::Devices {
                inputs: list(inputs),
                outputs: list(outputs),
                default_input: inputs.first().map(|(id, _)| (*id).into()),
                default_output: outputs.first().map(|(id, _)| (*id).into()),
            })
            .unwrap();
        // The poll runs before a frame when something woke it: ask for one.
        self.rt.update(|a, _| a.pump());
        settle(&mut self.rt, 3);
    }
}

/// `m` is the platform's primary modifier with Shift: the chord of the default hotkeys.
fn chord(c: char) -> KeyEvent {
    KeyEvent {
        key: Key::Command(c),
        mods: Mods {
            shift: true,
            cmd: true,
            ctrl: !cfg!(target_os = "macos"),
            logo: cfg!(target_os = "macos"),
            alt: false,
        },
    }
}

fn plain(key: Key) -> KeyEvent {
    KeyEvent {
        key,
        mods: Mods::default(),
    }
}

fn set_key(rig: &mut Rig, which: VoiceKey, chord: &str) {
    let c: rune::Shortcut = chord.parse().unwrap();
    rig.act(|s| s.voice_set_key(which, Some(&c)).unwrap());
}

// ---- prefs ----

#[test]
fn defaults_are_voice_activity_with_both_filters_on_and_the_hotkeys_bound() {
    let p = VoicePrefs::default();
    assert_eq!(p.mode, InputMode::VoiceActivity);
    assert_eq!(p.threshold, DEFAULT_THRESHOLD);
    assert!(p.noise_suppression && p.echo_cancellation && p.sounds);
    assert!(p.mute_key.is_some() && p.deafen_key.is_some());
    assert_eq!(
        p.ptt_key, None,
        "push to talk has no key until one is picked"
    );
    assert_eq!((p.input, p.output), (None, None));
    // A file from another version (nothing, or other fields only) loads as the defaults.
    assert_eq!(
        serde_json::from_str::<VoicePrefs>("{}").unwrap(),
        VoicePrefs::default()
    );
    assert_eq!(
        serde_json::from_str::<VoicePrefs>(r#"{"later_field": 3, "mode": "push-to-talk"}"#)
            .unwrap()
            .mode,
        InputMode::PushToTalk
    );
    // A cleared key stays cleared rather than coming back as the default.
    let cleared: VoicePrefs = serde_json::from_str(r#"{"mute_key": null}"#).unwrap();
    assert_eq!(cleared.mute_key, None);
    assert!(cleared.deafen_key.is_some());
}

#[test]
fn prefs_are_saved_and_come_back_after_a_restart() {
    let dir = std::env::temp_dir().join(format!("thehideout-voice-prefs-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("prefs.json");
    let saved = {
        let mut app = App::start(
            Box::new(crate::fake::FakeBackend::new()),
            AppState::new(fake_now, Tz::Fixed(0)),
            Arc::new(Mutex::new(Prefs::at(&path))),
        );
        let s = &mut app.state;
        s.voice_set_input(Some("mic-1".into()));
        s.voice_set_mode(InputMode::PushToTalk);
        s.voice_set_threshold(0.62);
        s.voice_set_noise_suppression(false);
        s.voice_set_echo_cancellation(false);
        s.voice_set_sounds(false);
        let f13: rune::Shortcut = "f13".parse().unwrap();
        s.voice_set_key(VoiceKey::PushToTalk, Some(&f13)).unwrap();
        s.voice_set_key(VoiceKey::Mute, None).unwrap();
        let saved = s.voice_prefs();
        app.flush();
        drop(app); // the saver finishes what it was handed
        saved
    };
    assert_eq!(saved.mode, InputMode::PushToTalk);
    let app = App::start(
        Box::new(crate::fake::FakeBackend::new()),
        AppState::new(fake_now, Tz::Fixed(0)),
        Arc::new(Mutex::new(Prefs::at(&path))),
    );
    assert_eq!(app.state.voice_prefs(), saved);
    assert!(
        !app.state.voice.sounds,
        "the live sounds flag is the saved one"
    );
    assert_eq!(app.state.voice.prefs.mute_key, None);
    // The saved settings are queued for the engine as soon as the app starts.
    assert!(app
        .state
        .voice
        .intents
        .contains(&VoiceIntent::SetMode(VoiceMode::PushToTalk)));
    assert!(app
        .state
        .voice
        .intents
        .contains(&VoiceIntent::SetNoiseSuppression(false)));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn an_unchanged_setting_saves_and_sends_nothing() {
    let mut r = rig();
    r.open_page();
    r.take();
    r.act(|s| s.voice.prefs_dirty = false);
    r.act(|s| s.voice_set_noise_suppression(true));
    r.act(|s| s.voice_set_mode(InputMode::VoiceActivity));
    assert!(!r.state().voice.prefs_dirty);
    assert_eq!(r.take(), []);
}

// ---- the page and the engine calls ----

#[test]
fn the_settings_nav_opens_the_page_and_asks_for_devices() {
    let mut r = rig();
    r.act(|s| s.open_settings());
    assert!(has(&r.rt, "settings-nav-voice"));
    click(&mut r.rt, "settings-nav-voice");
    settle(&mut r.rt, 3);
    assert_eq!(r.state().ui.settings, Some(Section::Voice));
    assert!(has(&r.rt, "page-voice"));
    assert_eq!(
        r.take(),
        [Call::List],
        "the lists are asked for when the page opens"
    );
    // No input volume or per-user volume: those are not offered.
    assert!(!has(&r.rt, "set-input-volume") && !has(&r.rt, "set-user-volume"));
}

#[test]
fn the_switches_send_noise_suppression_and_echo_cancellation() {
    let mut r = rig();
    r.open_page();
    r.take();
    click(&mut r.rt, "set-noise");
    settle(&mut r.rt, 2);
    click(&mut r.rt, "set-echo");
    settle(&mut r.rt, 2);
    assert_eq!(r.take(), [Call::Noise(false), Call::Echo(false)]);
    assert!(!r.state().voice.prefs.noise_suppression && !r.state().voice.prefs.echo_cancellation);
    click(&mut r.rt, "set-noise");
    settle(&mut r.rt, 2);
    assert_eq!(r.take(), [Call::Noise(true)]);
}

#[test]
fn the_sounds_switch_is_the_live_sounds_flag_and_is_saved() {
    let mut r = rig();
    r.open_page();
    assert!(r.state().voice.sounds);
    click(&mut r.rt, "set-sounds");
    settle(&mut r.rt, 2);
    assert!(!r.state().voice.sounds);
    assert!(!r.state().voice_prefs().sounds);
    // With sounds off a mute click plays nothing.
    r.act(|s| s.toggle_mute());
    assert!(r.state().voice.sounds_out.is_empty());
}

#[test]
fn choosing_devices_and_a_mode_sends_them() {
    let mut r = rig();
    r.open_page();
    r.devices(
        &[("mic-1", "Built-in"), ("mic-2", "Headset")],
        &[("out-1", "Speakers"), ("out-2", "Headset")],
    );
    r.take();
    r.act(|s| s.voice_set_input(Some("mic-2".into())));
    r.act(|s| s.voice_set_output(Some("out-2".into())));
    r.act(|s| s.voice_set_mode(InputMode::PushToTalk));
    r.act(|s| s.voice_set_mode(InputMode::VoiceActivity));
    r.act(|s| s.voice_set_input(None));
    assert_eq!(
        r.take(),
        [
            Call::Input(Some("mic-2".into())),
            Call::Output(Some("out-2".into())),
            Call::Mode(VoiceMode::PushToTalk),
            Call::Mode(VoiceMode::VoiceActivity {
                threshold: DEFAULT_THRESHOLD
            }),
            Call::Input(None),
        ]
    );
    assert_eq!(
        r.state().voice.prefs.output_name.as_deref(),
        Some("Headset")
    );
}

#[test]
fn the_select_lists_system_default_first_and_choosing_one_applies_it() {
    let mut r = rig();
    r.open_page();
    r.devices(
        &[("mic-1", "Built-in"), ("mic-2", "Headset")],
        &[("out-1", "Speakers")],
    );
    r.take();
    click(&mut r.rt, "voice-input-select-trigger");
    settle(&mut r.rt, 3);
    assert_eq!(r.state().ui.voice_menu, crate::voice::VoiceMenu::Input);
    assert!(has(&r.rt, "option0"), "the list is open");
    click(&mut r.rt, "option2");
    settle(&mut r.rt, 3);
    assert_eq!(r.take(), [Call::Input(Some("mic-2".into()))]);
    assert_eq!(r.state().ui.voice_menu, crate::voice::VoiceMenu::None);
}

#[test]
fn the_sensitivity_slider_moves_the_gate_with_the_pointer_and_the_arrows() {
    let mut r = rig();
    r.open_page();
    r.take();
    let [x, y, w, h] = rect_of(&r.rt, "voice-sensitivity");
    // A press a quarter of the way along the track (inset by the thumb's radius).
    let usable = w - thehideout_design::components::THUMB;
    r.rt.pointer_down(x + 8.0 + 0.25 * usable, y + h / 2.0);
    r.rt.pointer_up(x + 8.0 + 0.25 * usable, y + h / 2.0);
    settle(&mut r.rt, 3);
    assert!(
        (r.state().voice.prefs.threshold - 0.25).abs() < 0.02,
        "{}",
        r.state().voice.prefs.threshold
    );
    let t = r.state().voice.prefs.threshold;
    assert_eq!(
        r.take().last(),
        Some(&Call::Mode(VoiceMode::VoiceActivity { threshold: t })),
    );
    key(&mut r.rt, Key::Right);
    settle(&mut r.rt, 2);
    assert!((r.state().voice.prefs.threshold - (t + 0.01)).abs() < 1e-4);
    key(&mut r.rt, Key::End);
    settle(&mut r.rt, 2);
    assert_eq!(r.state().voice.prefs.threshold, 1.0);
}

#[test]
fn the_page_shows_the_sensitivity_in_voice_activity_and_the_key_in_push_to_talk() {
    let mut r = rig();
    r.open_page();
    assert!(has(&r.rt, "voice-sensitivity") && has(&r.rt, "meter-marker"));
    assert!(!has(&r.rt, "voice-key-ptt"));
    r.act(|s| s.voice_set_mode(InputMode::PushToTalk));
    assert!(has(&r.rt, "voice-key-ptt") && has(&r.rt, "ptt-note"));
    assert!(!has(&r.rt, "voice-sensitivity"));
    assert!(
        !has(&r.rt, "meter-marker"),
        "no gate to draw in push to talk"
    );
}

#[test]
fn settings_reach_an_engine_built_by_the_first_connect() {
    let mut r = rig_with(|app| {
        app.state.load_voice_prefs(VoicePrefs {
            input: Some("mic-9".into()),
            mode: InputMode::PushToTalk,
            noise_suppression: false,
            ..Default::default()
        });
    });
    // Nothing built yet: the settings wait in the sink.
    r.act(|_| {});
    assert_eq!(r.take(), []);
    r.join();
    let calls = r.take();
    let connect = calls
        .iter()
        .position(|c| *c == Call::Connect)
        .expect("connected");
    assert_eq!(
        calls[..connect],
        [
            Call::Input(Some("mic-9".into())),
            Call::Output(None),
            Call::Mode(VoiceMode::PushToTalk),
            Call::Noise(false),
            Call::Echo(true),
        ],
        "the engine has every setting before it joins"
    );
}

// ---- the mic test ----

#[test]
fn the_mic_test_starts_on_toggle_and_shows_the_level() {
    let mut r = rig();
    r.open_page();
    r.take();
    click(&mut r.rt, "voice-mic-test");
    settle(&mut r.rt, 2);
    assert!(r.state().voice.mic_test);
    assert_eq!(r.take(), [Call::TestOn]);
    r.events
        .send(VoiceEvent::Levels(vec![(String::new(), 0.6)]))
        .unwrap();
    r.rt.update(|a, _| a.pump());
    settle(&mut r.rt, 3);
    assert_eq!(r.state().voice.mic_level, 0.6);
    // Falls by halves, and silence snaps.
    r.events
        .send(VoiceEvent::Levels(vec![(String::new(), 0.2)]))
        .unwrap();
    r.rt.update(|a, _| a.pump());
    assert!((r.state().voice.mic_level - 0.4).abs() < 1e-6);
    r.events
        .send(VoiceEvent::Levels(vec![(String::new(), 0.0)]))
        .unwrap();
    r.rt.update(|a, _| a.pump());
    assert_eq!(r.state().voice.mic_level, 0.0);
    click(&mut r.rt, "voice-mic-test");
    settle(&mut r.rt, 2);
    assert!(!r.state().voice.mic_test);
    assert_eq!(r.take(), [Call::TestOff]);
}

#[test]
fn the_mic_test_stops_however_the_page_is_left() {
    // Escape, the close button, and another settings page.
    for leave in ["escape", "close", "nav-appearance", "nav-security"] {
        let mut r = rig();
        r.open_page();
        click(&mut r.rt, "voice-mic-test");
        settle(&mut r.rt, 2);
        r.take();
        match leave {
            "escape" => key(&mut r.rt, Key::Escape),
            "close" => click(&mut r.rt, "settings-close"),
            "nav-appearance" => click(&mut r.rt, "settings-nav-appearance"),
            _ => click(&mut r.rt, "settings-nav-security"),
        }
        settle(&mut r.rt, 3);
        assert!(!r.state().voice.mic_test, "{leave}: test still on");
        assert_eq!(r.take(), [Call::TestOff], "{leave}");
        assert_eq!(r.state().voice.mic_level, 0.0, "{leave}");
    }
}

#[test]
fn levels_are_ignored_while_nobody_is_looking() {
    let mut r = rig();
    r.act(|s| s.open_voice_settings());
    r.act(|s| s.close_settings());
    r.events
        .send(VoiceEvent::Levels(vec![(String::new(), 0.9)]))
        .unwrap();
    r.rt.update(|a, _| a.pump());
    assert_eq!(
        r.state().voice.mic_level,
        0.0,
        "no redraw churn all through a call"
    );
}

#[test]
fn a_mic_test_that_cannot_open_the_microphone_switches_off_and_says_so() {
    let mut r = rig();
    r.open_page();
    click(&mut r.rt, "voice-mic-test");
    settle(&mut r.rt, 2);
    r.events
        .send(VoiceEvent::Error("mic test: permission denied".into()))
        .unwrap();
    r.rt.update(|a, _| a.pump());
    settle(&mut r.rt, 2);
    assert!(!r.state().voice.mic_test);
    assert!(r
        .state()
        .ui
        .toasts
        .iter()
        .any(|t| t.message.contains("permission denied")));
}

// ---- hotkeys ----

#[test]
fn the_default_hotkeys_toggle_mute_and_deafen_and_tell_the_engine() {
    let mut r = rig();
    r.join();
    r.take();
    r.rt.key(chord('m'));
    settle(&mut r.rt, 2);
    assert!(r.state().voice.self_mute);
    assert_eq!(r.take(), [Call::Mute(true), Call::Deaf(false)]);
    r.rt.key(chord('d'));
    settle(&mut r.rt, 2);
    assert!(r.state().voice.self_deaf && r.state().voice.self_mute);
    assert_eq!(r.take(), [Call::Mute(true), Call::Deaf(true)]);
    r.rt.key(chord('d'));
    settle(&mut r.rt, 2);
    assert!(!r.state().voice.self_deaf, "the same chord undeafens");
    // A key that only shares the letter does nothing.
    r.take();
    r.rt.key(plain(Key::Text("m".into())));
    settle(&mut r.rt, 2);
    assert_eq!(r.take(), []);
}

#[test]
fn hotkeys_work_before_joining_and_while_typing_in_the_composer() {
    let mut r = rig();
    r.rt.key(chord('m'));
    settle(&mut r.rt, 2);
    assert!(
        r.state().voice.self_mute,
        "the flag is kept for the next call"
    );
    click(&mut r.rt, "composer/scroll/input");
    r.rt.key(chord('m'));
    settle(&mut r.rt, 2);
    assert!(
        !r.state().voice.self_mute,
        "a chord passes through a focused field"
    );
    assert_eq!(r.state().composer().text, "", "and types nothing");
}

#[test]
fn a_rebound_hotkey_replaces_the_default_and_a_clash_is_refused() {
    let mut r = rig();
    set_key(&mut r, VoiceKey::Mute, "mod+alt+j");
    r.rt.key(chord('m'));
    settle(&mut r.rt, 2);
    assert!(!r.state().voice.self_mute, "the old chord is free");
    r.rt.key(KeyEvent {
        key: Key::Command('j'),
        mods: Mods {
            alt: true,
            cmd: true,
            ctrl: !cfg!(target_os = "macos"),
            logo: cfg!(target_os = "macos"),
            shift: false,
        },
    });
    settle(&mut r.rt, 2);
    assert!(r.state().voice.self_mute);
    // The deafen chord cannot take the mute chord.
    let taken: rune::Shortcut = "mod+alt+j".parse().unwrap();
    assert!(matches!(
        r.rt.state
            .state
            .voice_set_key(VoiceKey::Deafen, Some(&taken)),
        Err(rune::ShortcutError::Conflict)
    ));
    // Backspace in the recorder clears a key.
    r.act(|s| s.voice_set_key(VoiceKey::Mute, None).unwrap());
    assert_eq!(r.state().voice.prefs.mute_key, None);
}

#[test]
fn no_hotkey_fires_while_a_shortcut_is_being_recorded() {
    let mut r = rig();
    r.open_page();
    // Clicking a recorder starts its capture; the recorder owns the keyboard.
    click(&mut r.rt, "voice-key-mute");
    settle(&mut r.rt, 2);
    assert!(r.state().ui.voice_keys.recording());
    r.rt.state.state.screen = crate::state::Screen::Main;
    assert!(!r.rt.state.state.voice_hotkey(&chord('d'), None, false));
    assert!(!r.state().voice.self_deaf);
}

#[test]
fn recording_a_new_chord_binds_it_through_the_page() {
    let mut r = rig();
    r.open_page();
    click(&mut r.rt, "voice-key-mute");
    settle(&mut r.rt, 2);
    // The recorder takes physical keys: Ctrl+Alt+K, as a window reports them.
    use rune::{PhysicalInput, PhysicalKeyCode};
    let mods = Mods {
        ctrl: true,
        alt: true,
        ..Mods::default()
    };
    r.rt.physical_input(PhysicalInput::Key {
        key: PhysicalKeyCode::KeyK,
        mods,
        pressed: true,
        repeat: false,
    });
    settle(&mut r.rt, 2);
    assert_eq!(
        r.state().voice.prefs.mute_key.as_deref(),
        Some("ctrl+alt+k")
    );
    assert!(!r.state().ui.voice_keys.recording());
}

// ---- push to talk ----

fn ptt_rig() -> Rig {
    let mut r = rig();
    r.act(|s| s.voice_set_mode(InputMode::PushToTalk));
    set_key(&mut r, VoiceKey::PushToTalk, "f13");
    r.join();
    r.take();
    r
}

#[test]
fn push_to_talk_press_and_release_open_and_close_the_mic() {
    let mut r = ptt_rig();
    r.rt.key(plain(Key::Function(13)));
    settle(&mut r.rt, 2);
    assert!(r.state().voice.ptt_down);
    assert_eq!(r.take(), [Call::Ptt(true)]);
    // The held key repeats; the mic is already open.
    r.rt.key(plain(Key::Function(13)));
    settle(&mut r.rt, 2);
    assert_eq!(r.take(), []);
    assert!(r.rt.key_up(plain(Key::Function(13))));
    settle(&mut r.rt, 2);
    assert!(!r.state().voice.ptt_down);
    assert_eq!(r.take(), [Call::Ptt(false)]);
}

#[test]
fn push_to_talk_lets_go_when_the_window_loses_focus() {
    let mut r = ptt_rig();
    r.rt.key(plain(Key::Function(13)));
    settle(&mut r.rt, 2);
    r.take();
    r.rt.window_focus(false);
    settle(&mut r.rt, 2);
    assert!(!r.state().voice.ptt_down);
    assert_eq!(r.take(), [Call::Ptt(false)], "no stuck transmit");
    // Coming back changes nothing.
    r.rt.window_focus(true);
    settle(&mut r.rt, 2);
    assert_eq!(r.take(), []);
}

#[test]
fn push_to_talk_lets_go_when_the_mode_changes_the_key_changes_or_we_leave() {
    for how in ["mode", "key", "leave"] {
        let mut r = ptt_rig();
        r.rt.key(plain(Key::Function(13)));
        settle(&mut r.rt, 2);
        r.take();
        match how {
            "mode" => r.act(|s| s.voice_set_mode(InputMode::VoiceActivity)),
            "key" => r.act(|s| {
                let f14: rune::Shortcut = "f14".parse().unwrap();
                s.voice_set_key(VoiceKey::PushToTalk, Some(&f14)).unwrap()
            }),
            _ => r.act(|s| s.leave_voice()),
        }
        assert!(!r.state().voice.ptt_down, "{how}");
        assert!(r.take().contains(&Call::Ptt(false)), "{how}");
    }
}

#[test]
fn the_push_to_talk_key_does_nothing_in_voice_activity_or_unset() {
    let mut r = ptt_rig();
    r.act(|s| s.voice_set_mode(InputMode::VoiceActivity));
    r.take();
    r.rt.key(plain(Key::Function(13)));
    settle(&mut r.rt, 2);
    assert_eq!(r.take(), []);
    // Push to talk with no key: nothing to press, and the bar says so.
    r.act(|s| {
        s.voice_set_mode(InputMode::PushToTalk);
        s.voice_set_key(VoiceKey::PushToTalk, None).unwrap();
    });
    r.take();
    r.rt.key(plain(Key::Function(13)));
    settle(&mut r.rt, 2);
    assert_eq!(r.take(), []);
    assert!(
        has(&r.rt, "voice-ptt"),
        "the voice bar says there is no key"
    );
}

#[test]
fn the_own_speaking_ring_shows_while_the_push_to_talk_key_is_held() {
    let mut r = ptt_rig();
    assert!(!r.state().is_speaking(TIM));
    r.rt.key(plain(Key::Function(13)));
    settle(&mut r.rt, 2);
    assert!(r.state().is_speaking(TIM), "transmitting");
    // Muted, the key opens nothing.
    r.rt.key_up(plain(Key::Function(13)));
    r.act(|s| s.toggle_mute());
    r.rt.key(plain(Key::Function(13)));
    settle(&mut r.rt, 2);
    assert!(!r.state().is_speaking(TIM));
}

/// Typing, as chosen: a bare key is the composer's text, so push to talk on a letter is not heard
/// while typing; the same key outside a field is.
#[test]
fn a_bare_letter_key_is_typing_in_the_composer_and_push_to_talk_elsewhere() {
    let mut r = rig();
    r.act(|s| s.voice_set_mode(InputMode::PushToTalk));
    set_key(&mut r, VoiceKey::PushToTalk, "v");
    r.join();
    r.take();
    click(&mut r.rt, "composer/scroll/input");
    r.rt.key(plain(Key::Text("v".into())));
    settle(&mut r.rt, 2);
    assert_eq!(r.state().composer().text, "v");
    assert!(!r.state().voice.ptt_down);
    assert_eq!(r.take(), []);
}

// ---- a saved device that is gone ----

#[test]
fn a_missing_saved_device_falls_back_to_the_default_and_is_marked_unavailable() {
    let mut r = rig_with(|app| {
        app.state.load_voice_prefs(VoicePrefs {
            input: Some("mic-gone".into()),
            input_name: Some("AirPods".into()),
            ..Default::default()
        });
    });
    r.open_page();
    // Before the engine has answered the saved device is used as it is.
    assert_eq!(r.state().effective_input().as_deref(), Some("mic-gone"));
    r.take();
    r.devices(&[("mic-1", "Built-in")], &[("out-1", "Speakers")]);
    assert_eq!(
        r.state().effective_input(),
        None,
        "the engine gets the default"
    );
    assert_eq!(r.take(), [Call::Input(None), Call::Output(None)]);
    assert_eq!(
        r.state().voice.prefs.input.as_deref(),
        Some("mic-gone"),
        "but it stays chosen, for when it is back"
    );
    let d = r.state().voice.devices.as_ref().unwrap();
    let (opts, selected) = device_options(
        Some(&d.inputs),
        d.default_input.as_deref(),
        Some(("mic-gone", Some("AirPods"))),
    );
    assert_eq!(opts[selected].label, "AirPods (unavailable)");
    assert_eq!(opts[0].label, "System default (Built-in)");
    // Plugged in again: used again, and named as it is called now.
    r.devices(
        &[("mic-1", "Built-in"), ("mic-gone", "AirPods Pro")],
        &[("out-1", "Speakers")],
    );
    assert_eq!(
        r.take(),
        [Call::Input(Some("mic-gone".into())), Call::Output(None)]
    );
    assert_eq!(
        r.state().voice.prefs.input_name.as_deref(),
        Some("AirPods Pro")
    );
}

#[test]
fn the_page_names_the_unavailable_device_in_its_select() {
    let mut r = rig_with(|app| {
        app.state.load_voice_prefs(VoicePrefs {
            output: Some("out-gone".into()),
            output_name: Some("Old Speakers".into()),
            ..Default::default()
        });
    });
    r.open_page();
    r.devices(&[("mic-1", "Built-in")], &[("out-1", "Speakers")]);
    let tree = r.rt.accessibility_json();
    let mut found = false;
    fn walk(n: &serde_json::Value, found: &mut bool) {
        if n["key"] == "voice-output-select-trigger" {
            *found |= n.to_string().contains("Old Speakers (unavailable)");
        }
        for c in n["children"].as_array().into_iter().flatten() {
            walk(c, found);
        }
    }
    walk(&tree, &mut found);
    assert!(found, "the select shows 'Old Speakers (unavailable)'");
}

#[test]
fn options_without_a_device_list_do_not_claim_the_saved_one_is_gone() {
    let (opts, selected) = device_options(None, None, Some(("mic-1", Some("Headset"))));
    assert_eq!(
        opts.len(),
        1,
        "only System default until the engine answers"
    );
    assert_eq!(selected, 0);
}
