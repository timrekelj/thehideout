//! The production [`VoiceSink`]: it owns a `thehideout-voice` engine and turns the state's voice
//! intents into engine calls, the engine's reports into events for the state (`App::poll` feeds
//! them to `AppState::voice_event`), and plays the join, leave and click sounds.
//!
//! Nothing here starts until it is needed: the engine (and the tokio runtime it runs on, a small
//! one of its own, so `real.rs` stays untouched) is built on the first `Connect`, and the sound
//! output opens a device only when a sound plays. Scenes, tests and the fake backend never
//! install this sink, so they never touch audio.
//!
//! Intents arrive as a batch (`Connect`, then the mute and deafen flags): the connect is held
//! until `flushed`, so the engine joins with the right flags before the microphone publishes.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rune::{Pixels, Waker};
use thehideout_voice_types::{ScreenSource, VideoFrame, VideoSink, VoiceEvent, VoiceMode};

use crate::voice::{Sound, VoiceIntent, VoiceSink};

/// Set this (to anything) to feed a generated tone instead of the microphone: for end-to-end
/// runs on machines with no audio hardware or permission. Nothing reaches the speakers.
pub const SYNTHETIC_ENV: &str = "THEHIDEOUT_VOICE_SYNTHETIC";

/// What the sink needs from the engine (the real one is [`LiveEngine`]; tests record calls).
pub trait Engine {
    fn connect(&self, url: &str, token: &str, start_muted: bool, start_deafened: bool);
    fn disconnect(&self);
    fn set_mute(&self, muted: bool);
    fn set_deaf(&self, deaf: bool);
    /// Only 0 (silence) takes effect in the real engine.
    fn set_user_volume(&self, identity: &str, volume: f32);
    // The settings page's calls. They default to nothing so a test engine need only record what it
    // is about.
    fn set_input_device(&self, _id: Option<String>) {}
    fn set_output_device(&self, _id: Option<String>) {}
    fn set_mode(&self, _mode: VoiceMode) {}
    fn set_ptt(&self, _held: bool) {}
    fn set_noise_suppression(&self, _on: bool) {}
    fn set_echo_cancellation(&self, _on: bool) {}
    fn list_devices(&self) {}
    fn start_mic_test(&self) {}
    fn stop_mic_test(&self) {}
    fn set_camera(&self, _on: bool) {}
    fn set_screen_share(&self, _source: Option<ScreenSource>) {}
    fn list_screen_sources(&self) {}
    /// The newest-frame handle of a video track, `None` when the engine has no pictures (a test
    /// engine). Called once when a tile starts watching it.
    fn video_frames(&self, _track: &str) -> Option<VideoSink> {
        None
    }
    /// A tile stopped watching `track`: the subscription goes.
    fn release_video(&self, _track: &str) {}
}

/// Hands the newest frame of a track to a `Pixels` (the picture a tile draws) until `stop`. It
/// polls the engine's newest-frame cell at about 60 Hz and sets the picture only when the frame
/// is new, so the window repaints once per frame and not otherwise (`Pixels::set` wakes just the
/// windows showing it).
struct Watch {
    pixels: Pixels,
    stop: Arc<AtomicBool>,
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The frame flipped left to right (our own camera, shown as a mirror).
pub fn mirrored(f: &VideoFrame) -> Vec<u8> {
    let row = f.width as usize * 4;
    let mut out = Vec::with_capacity(f.rgba.len());
    for line in f.rgba.chunks_exact(row) {
        for x in (0..f.width as usize).rev() {
            out.extend_from_slice(&line[x * 4..x * 4 + 4]);
        }
    }
    out
}

fn pump(sink: VideoSink, pixels: Pixels, mirror: bool, stop: Arc<AtomicBool>) {
    let spawned = std::thread::Builder::new().name("thehideout-video".into()).spawn(move || {
        let mut seen = 0;
        while !stop.load(Ordering::Relaxed) {
            if let Some(f) = sink.latest_frame()
                && f.seq != seen
            {
                seen = f.seq;
                let rgba = if mirror { mirrored(&f) } else { f.rgba.to_vec() };
                pixels.set(f.width, f.height, rgba);
            }
            std::thread::sleep(Duration::from_millis(16));
        }
    });
    if let Err(e) = spawned {
        eprintln!("thehideout: video thread: {e}");
    }
}

/// The settings the engine is told about, kept so a connect, a mic test or a device refresh that
/// builds the engine starts it with them. A field is `None` until the app has said it, so an engine
/// the app never configured keeps its own defaults.
#[derive(Default)]
struct Config {
    input: Option<Option<String>>,
    output: Option<Option<String>>,
    mode: Option<VoiceMode>,
    ns: Option<bool>,
    ec: Option<bool>,
    ptt: bool,
}

impl Config {
    fn apply<E: Engine>(&self, engine: &E) {
        if let Some(i) = &self.input {
            engine.set_input_device(i.clone());
        }
        if let Some(o) = &self.output {
            engine.set_output_device(o.clone());
        }
        if let Some(m) = self.mode {
            engine.set_mode(m);
        }
        if let Some(n) = self.ns {
            engine.set_noise_suppression(n);
        }
        if let Some(e) = self.ec {
            engine.set_echo_cancellation(e);
        }
        if self.ptt {
            engine.set_ptt(true);
        }
    }
}

/// Where cues are played.
pub trait SoundOut {
    fn play(&mut self, sound: Sound);
}

/// The window's waker, filled in by `set_waker` once the window exists.
pub type WakerCell = Arc<Mutex<Option<Waker>>>;

/// Builds the engine on first use; the receiver carries its events.
type Make<E> = Box<dyn FnMut(&WakerCell) -> (E, Receiver<VoiceEvent>)>;

pub struct EngineSink<E: Engine> {
    make: Make<E>,
    engine: Option<(E, Receiver<VoiceEvent>)>,
    waker: WakerCell,
    out: Box<dyn SoundOut>,
    /// A `Connect` waiting for the flags that follow it in the same batch.
    pending: Option<(String, String)>,
    mute: bool,
    deaf: bool,
    /// Users this device muted (user id to muted); applied again to every new connection.
    muted_users: HashMap<u64, bool>,
    /// The engine has a room, or an attempt at one, that we have not torn down.
    active: bool,
    /// `Disconnected` reports still to come for teardowns we asked for. They are not drops.
    expected: u32,
    config: Config,
    /// The video tracks tiles watch, by track id.
    watches: HashMap<String, Watch>,
}

impl<E: Engine> EngineSink<E> {
    pub fn new(make: Make<E>, out: Box<dyn SoundOut>) -> Self {
        EngineSink {
            make,
            engine: None,
            waker: Arc::default(),
            out,
            pending: None,
            mute: false,
            deaf: false,
            muted_users: HashMap::new(),
            active: false,
            expected: 0,
            config: Config::default(),
            watches: HashMap::new(),
        }
    }

    /// The engine, built (and given the settings) if it does not exist yet. The settings page needs
    /// one before any room: the device lists and the mic test.
    fn engine(&mut self) -> &E {
        if self.engine.is_none() {
            let waker = self.waker.clone();
            let built = (self.make)(&waker);
            self.config.apply(&built.0);
            self.engine = Some(built);
        }
        &self.engine.as_ref().expect("built above").0
    }

    /// Runs `f` on the engine if it exists; the setting is already in `config` for one built later.
    fn with_engine(&self, f: impl FnOnce(&E)) {
        if let Some((engine, _)) = &self.engine {
            f(engine);
        }
    }

    /// Whether the engine exists yet (it is built by the first connect).
    #[cfg(test)]
    pub fn built(&self) -> bool {
        self.engine.is_some()
    }

    fn tear_down(&mut self) {
        self.watches.clear();
        if self.active
            && let Some((engine, _)) = &self.engine
        {
            engine.disconnect();
            self.expected += 1;
        }
        self.active = false;
    }

    fn volume(muted: bool) -> f32 {
        if muted { 0.0 } else { 1.0 }
    }
}

impl<E: Engine> VoiceSink for EngineSink<E> {
    fn intent(&mut self, intent: VoiceIntent) {
        match intent {
            VoiceIntent::Connect { url, token } => self.pending = Some((url, token)),
            VoiceIntent::Disconnect => {
                self.pending = None;
                self.tear_down();
            }
            VoiceIntent::SetMute(m) => {
                self.mute = m;
                if self.pending.is_none()
                    && let Some((engine, _)) = &self.engine
                {
                    engine.set_mute(m);
                }
            }
            VoiceIntent::SetDeaf(d) => {
                self.deaf = d;
                if self.pending.is_none()
                    && let Some((engine, _)) = &self.engine
                {
                    engine.set_deaf(d);
                }
            }
            VoiceIntent::SetInputDevice(id) => {
                self.config.input = Some(id.clone());
                self.with_engine(|e| e.set_input_device(id));
            }
            VoiceIntent::SetOutputDevice(id) => {
                self.config.output = Some(id.clone());
                self.with_engine(|e| e.set_output_device(id));
            }
            VoiceIntent::SetMode(mode) => {
                self.config.mode = Some(mode);
                self.with_engine(|e| e.set_mode(mode));
            }
            VoiceIntent::SetPtt(held) => {
                self.config.ptt = held;
                self.with_engine(|e| e.set_ptt(held));
            }
            VoiceIntent::SetNoiseSuppression(on) => {
                self.config.ns = Some(on);
                self.with_engine(|e| e.set_noise_suppression(on));
            }
            VoiceIntent::SetEchoCancellation(on) => {
                self.config.ec = Some(on);
                self.with_engine(|e| e.set_echo_cancellation(on));
            }
            VoiceIntent::ListDevices => self.engine().list_devices(),
            VoiceIntent::MicTest(true) => self.engine().start_mic_test(),
            VoiceIntent::MicTest(false) => self.with_engine(|e| e.stop_mic_test()),
            VoiceIntent::SetCamera(on) => self.with_engine(|e| e.set_camera(on)),
            VoiceIntent::SetScreenShare(source) => self.with_engine(|e| e.set_screen_share(source)),
            VoiceIntent::ListScreenSources => self.engine().list_screen_sources(),
            VoiceIntent::WatchVideo { track, mirror } => {
                if self.watches.contains_key(&track) {
                    return;
                }
                let Some((engine, _)) = &self.engine else { return };
                let pixels = Pixels::new();
                let stop = Arc::new(AtomicBool::new(false));
                if let Some(sink) = engine.video_frames(&track) {
                    pump(sink, pixels.clone(), mirror, stop.clone());
                }
                self.watches.insert(track, Watch { pixels, stop });
            }
            VoiceIntent::UnwatchVideo { track } => {
                if self.watches.remove(&track).is_some() {
                    self.with_engine(|e| e.release_video(&track));
                }
            }
            VoiceIntent::MuteUser { user, muted } => {
                self.muted_users.insert(user, muted);
                if self.pending.is_none()
                    && let Some((engine, _)) = &self.engine
                {
                    engine.set_user_volume(&user.to_string(), Self::volume(muted));
                }
            }
        }
    }

    fn flushed(&mut self) {
        let Some((url, token)) = self.pending.take() else {
            return;
        };
        if self.active {
            // The engine leaves the old room first and reports it.
            self.expected += 1;
        }
        let (mute, deaf) = (self.mute || self.deaf, self.deaf);
        self.engine();
        let Some((engine, _)) = &self.engine else {
            return;
        };
        engine.connect(&url, &token, mute, deaf);
        for (user, muted) in &self.muted_users {
            if *muted {
                engine.set_user_volume(&user.to_string(), 0.0);
            }
        }
        self.active = true;
    }

    fn sound(&mut self, sound: Sound) {
        self.out.play(sound);
    }

    fn events(&mut self) -> Vec<VoiceEvent> {
        let Some((_, rx)) = &self.engine else {
            return Vec::new();
        };
        let mut out = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            // A teardown we asked for is reported once, in order, so each such report is
            // matched with one request; anything else is the engine giving up.
            if let VoiceEvent::VideoTrackEnded { track_id, .. } = &ev {
                // A track that is gone has nothing to watch.
                self.watches.remove(track_id);
            }
            if matches!(ev, VoiceEvent::Disconnected { .. }) {
                if self.expected > 0 {
                    self.expected -= 1;
                    continue;
                }
                self.active = false;
            }
            out.push(ev);
        }
        out
    }

    fn set_waker(&mut self, waker: Waker) {
        *self.waker.lock().unwrap() = Some(waker);
    }

    fn video_pixels(&self, track: &str) -> Option<Pixels> {
        self.watches.get(track).map(|w| w.pixels.clone())
    }
}

// The native engine (LiveKit, cpal): not built for the browser, where Phase 2 adds an `Engine` of
// its own behind the same trait.
#[cfg(not(target_arch = "wasm32"))]
pub use live::*;

#[cfg(not(target_arch = "wasm32"))]
mod live {
use super::*;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use thehideout_voice::{AudioInput, ConnectOptions, SyntheticPicture, VideoInput, VoiceEngine};

// ---- the real engine -----------------------------------------------------------------------

/// A `VoiceEngine` with the runtime it runs on.
pub struct LiveEngine {
    engine: VoiceEngine,
    synthetic: bool,
    runtime: Option<tokio::runtime::Runtime>,
}

impl LiveEngine {
    /// Starts the engine on a runtime of its own. A thread forwards its events and rings the
    /// window's waker for each, so the window wakes when voice has news (the engine's own
    /// channel has no waker).
    fn start(synthetic: bool, waker: &WakerCell) -> (LiveEngine, Receiver<VoiceEvent>) {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("thehideout-voice")
            .enable_all()
            .build()
            .expect("start the voice runtime");
        let (engine, raw) = VoiceEngine::new(runtime.handle().clone());
        let (tx, rx) = channel();
        let waker = Arc::clone(waker);
        let spawned = std::thread::Builder::new()
            .name("thehideout-voice-events".into())
            .spawn(move || {
                // Ends when the engine is dropped (its sender goes with it).
                while let Ok(ev) = raw.recv() {
                    if tx.send(ev).is_err() {
                        break;
                    }
                    if let Some(wake) = waker.lock().unwrap().as_ref() {
                        wake();
                    }
                }
            });
        if let Err(e) = spawned {
            eprintln!("thehideout: voice events thread: {e}");
        }
        (
            LiveEngine {
                engine,
                synthetic,
                runtime: Some(runtime),
            },
            rx,
        )
    }
}

impl Drop for LiveEngine {
    fn drop(&mut self) {
        // Dropping a runtime from inside async code panics; shutting it down in the background
        // never blocks the window either.
        if let Some(rt) = self.runtime.take() {
            rt.shutdown_background();
        }
    }
}

impl Engine for LiveEngine {
    fn connect(&self, url: &str, token: &str, start_muted: bool, start_deafened: bool) {
        let audio_input = if self.synthetic {
            AudioInput::Synthetic(Default::default())
        } else {
            AudioInput::Device
        };
        let video_input = if self.synthetic {
            VideoInput::Synthetic(SyntheticPicture::default())
        } else {
            VideoInput::Device
        };
        self.engine.connect(
            url,
            token,
            ConnectOptions {
                audio_input,
                video_input,
                start_muted,
                start_deafened,
            },
        );
    }

    fn disconnect(&self) {
        self.engine.disconnect();
    }

    fn set_mute(&self, muted: bool) {
        self.engine.set_mute(muted);
    }

    fn set_deaf(&self, deaf: bool) {
        self.engine.set_deaf(deaf);
    }

    fn set_user_volume(&self, identity: &str, volume: f32) {
        self.engine.set_user_volume(identity, volume);
    }

    fn set_input_device(&self, id: Option<String>) {
        self.engine.set_input_device(id);
    }

    fn set_output_device(&self, id: Option<String>) {
        self.engine.set_output_device(id);
    }

    fn set_mode(&self, mode: VoiceMode) {
        self.engine.set_mode(mode);
    }

    fn set_ptt(&self, held: bool) {
        self.engine.set_ptt(held);
    }

    fn set_noise_suppression(&self, on: bool) {
        self.engine.set_noise_suppression(on);
    }

    fn set_echo_cancellation(&self, on: bool) {
        self.engine.set_echo_cancellation(on);
    }

    fn list_devices(&self) {
        self.engine.list_devices();
    }

    fn start_mic_test(&self) {
        self.engine.start_mic_test();
    }

    fn stop_mic_test(&self) {
        self.engine.stop_mic_test();
    }

    fn set_camera(&self, on: bool) {
        self.engine.set_camera(on);
    }

    fn set_screen_share(&self, source: Option<ScreenSource>) {
        self.engine.set_screen_share(source);
    }

    fn list_screen_sources(&self) {
        self.engine.list_screen_sources();
    }

    fn video_frames(&self, track: &str) -> Option<VideoSink> {
        Some(self.engine.subscribe_video(track))
    }
}

/// The sink the real app installs. `synthetic` feeds a tone instead of the microphone.
pub fn live_sink(synthetic: bool) -> EngineSink<LiveEngine> {
    live_sink_with(synthetic, Box::new(Speaker::default()))
}

/// [`live_sink`] with cues going to `out` (end-to-end tests keep them off the speakers).
pub fn live_sink_with(synthetic: bool, out: Box<dyn SoundOut>) -> EngineSink<LiveEngine> {
    EngineSink::new(
        Box::new(move |waker| LiveEngine::start(synthetic, waker)),
        out,
    )
}

/// [`live_sink`] as the app uses it: synthetic when [`SYNTHETIC_ENV`] is set.
pub fn app_sink() -> EngineSink<LiveEngine> {
    live_sink(std::env::var_os(SYNTHETIC_ENV).is_some())
}

// ---- sounds ----------------------------------------------------------------------------------

/// One note of a cue: frequency in Hz and length in seconds.
type Note = (f32, f32);

/// The cue's notes. All short and soft, two-note blips going up for "something arrived" and down
/// for "something left", single or paired lower tones for the mute and deafen clicks.
fn notes(sound: Sound) -> &'static [Note] {
    match sound {
        Sound::Join => &[(523.25, 0.09), (783.99, 0.15)],
        Sound::PeerJoin => &[(659.25, 0.07), (880.0, 0.11)],
        Sound::PeerLeave => &[(880.0, 0.07), (659.25, 0.11)],
        Sound::MuteOn => &[(329.63, 0.08)],
        Sound::MuteOff => &[(440.0, 0.08)],
        Sound::DeafOn => &[(440.0, 0.06), (293.66, 0.09)],
        Sound::DeafOff => &[(293.66, 0.06), (440.0, 0.09)],
        Sound::Disconnect => &[(587.33, 0.08), (392.0, 0.16)],
    }
}

/// How loud a cue is at its peak (of full scale): present, never startling.
const CUE_GAIN: f32 = 0.16;

/// The cue as mono samples at `rate` Hz: each note a sine with a hint of its octave, a 6 ms
/// fade in and a smooth fade out, so notes start and end at zero and never click.
pub fn synth(sound: Sound, rate: u32) -> Vec<f32> {
    let rate_f = rate as f32;
    let mut out = Vec::new();
    for &(hz, secs) in notes(sound) {
        let n = (secs * rate_f) as usize;
        let attack = (0.006 * rate_f) as usize;
        for i in 0..n {
            let t = i as f32 / rate_f;
            let phase = std::f32::consts::TAU * hz * t;
            let tone = phase.sin() + 0.2 * (2.0 * phase).sin();
            let fade_in = (i as f32 / attack.max(1) as f32).min(1.0);
            let left = (n - i) as f32 / n as f32;
            // Raised cosine over the whole note: loudest at the start, zero at the end.
            let fade_out = 0.5 * (1.0 + (std::f32::consts::PI * (1.0 - left)).cos());
            out.push(CUE_GAIN * tone / 1.2 * fade_in * fade_out.powf(0.8));
        }
    }
    out.extend(std::iter::repeat_n(0.0, (0.01 * rate_f) as usize));
    out
}

/// Plays cues through the default output device with `cpal`. Each cue opens a short stream on a
/// thread of its own (a `cpal` stream is not `Send`) and closes it when the cue ends, so
/// nothing is held open between cues and a changed default device is followed.
#[derive(Default)]
pub struct Speaker {
    busy: Arc<std::sync::atomic::AtomicUsize>,
}

impl SoundOut for Speaker {
    fn play(&mut self, sound: Sound) {
        use std::sync::atomic::Ordering;
        // A burst of cues (everyone joining at once) must not pile up threads.
        if self.busy.fetch_add(1, Ordering::SeqCst) >= 6 {
            self.busy.fetch_sub(1, Ordering::SeqCst);
            return;
        }
        let busy = Arc::clone(&self.busy);
        let spawned = std::thread::Builder::new()
            .name("thehideout-cue".into())
            .spawn({
                let busy = Arc::clone(&busy);
                move || {
                    if let Err(e) = play_blocking(sound) {
                        eprintln!("thehideout: cue {sound:?} not played: {e}");
                    }
                    busy.fetch_sub(1, Ordering::SeqCst);
                }
            });
        if spawned.is_err() {
            busy.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

fn play_blocking(sound: Sound) -> Result<(), String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no output device")?;
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let config = supported.config();
    let (rate, channels) = (config.sample_rate, config.channels as usize);
    let pcm = synth(sound, rate);
    let secs = pcm.len() as f32 / rate as f32;
    let mut at = 0usize;
    let mut next = move || {
        let s = pcm.get(at).copied().unwrap_or(0.0);
        at += 1;
        s
    };
    let on_error = |e| eprintln!("thehideout: cue output: {e}");
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => device.build_output_stream(
            config,
            move |data: &mut [f32], _| {
                for frame in data.chunks_mut(channels) {
                    let s = next();
                    frame.fill(s);
                }
            },
            on_error,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            config,
            move |data: &mut [i16], _| {
                for frame in data.chunks_mut(channels) {
                    let s = (next() * i16::MAX as f32) as i16;
                    frame.fill(s);
                }
            },
            on_error,
            None,
        ),
        other => return Err(format!("unsupported output sample format {other:?}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    // The cue, then a little for the device's buffer to drain.
    std::thread::sleep(std::time::Duration::from_secs_f32(secs + 0.25));
    drop(stream);
    Ok(())
}

}
