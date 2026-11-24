//! The browser's voice engine: [`Engine`] over `js/voice.js`, which drives the `livekit-client`
//! SDK (the same LiveKit server and join token as the native engine). Audio and video: tiles are
//! fed by copying frames out of the browser's decoder (see `voice.js`, "the frame copy") into the
//! same `VideoSink`/`VideoFrame` path the native engine uses.
//!
//! The JS side reports events as JSON strings; they are parsed here into [`VoiceEvent`]s and queued
//! for the sink, which drains them from `poll` like the native engine's. Cues (join, leave, mute
//! clicks) are synthesized by `voice_engine::synth` and played through WebAudio.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use thehideout_voice_types::{ScreenSource, VideoFrame, VideoSink, VoiceEvent, VoiceMode};
use wasm_bindgen::prelude::*;

use crate::voice::Sound;
use crate::voice_web_events::parse;
use crate::voice_engine::{Engine, EngineSink, SoundOut, WakerCell, synth};

#[wasm_bindgen(module = "/js/voice.js")]
extern "C" {
    type VoiceClient;
    #[wasm_bindgen(constructor)]
    fn new(emit: &Closure<dyn FnMut(String)>, on_frame: &Closure<dyn FnMut(String, u32, u32, js_sys::Uint8ClampedArray)>) -> VoiceClient;
    #[wasm_bindgen(method)]
    fn connect(this: &VoiceClient, url: &str, token: &str, muted: bool, deaf: bool) -> js_sys::Promise;
    #[wasm_bindgen(method)]
    fn disconnect(this: &VoiceClient);
    #[wasm_bindgen(method, js_name = setMute)]
    fn set_mute(this: &VoiceClient, muted: bool);
    #[wasm_bindgen(method, js_name = setDeaf)]
    fn set_deaf(this: &VoiceClient, deaf: bool);
    #[wasm_bindgen(method, js_name = setUserVolume)]
    fn set_user_volume(this: &VoiceClient, identity: &str, volume: f32);
    #[wasm_bindgen(method, js_name = setInputDevice)]
    fn set_input_device(this: &VoiceClient, id: Option<String>) -> js_sys::Promise;
    #[wasm_bindgen(method, js_name = setOutputDevice)]
    fn set_output_device(this: &VoiceClient, id: Option<String>) -> js_sys::Promise;
    #[wasm_bindgen(method, js_name = setMode)]
    fn set_mode(this: &VoiceClient, voice_activity: bool, threshold: f32);
    #[wasm_bindgen(method, js_name = setCamera)]
    fn set_camera(this: &VoiceClient, on: bool) -> js_sys::Promise;
    #[wasm_bindgen(method, js_name = setCameraDevice)]
    fn set_camera_device(this: &VoiceClient, id: Option<String>) -> js_sys::Promise;
    #[wasm_bindgen(method, js_name = setScreenShare)]
    fn set_screen_share(this: &VoiceClient, on: bool) -> js_sys::Promise;
    #[wasm_bindgen(method, js_name = watchVideo)]
    fn watch_video(this: &VoiceClient, track: &str);
    #[wasm_bindgen(method, js_name = unwatchVideo)]
    fn unwatch_video(this: &VoiceClient, track: &str);
    #[wasm_bindgen(method, js_name = setPtt)]
    fn set_ptt(this: &VoiceClient, held: bool);
    #[wasm_bindgen(method, js_name = setNoiseSuppression)]
    fn set_noise_suppression(this: &VoiceClient, on: bool);
    #[wasm_bindgen(method, js_name = setEchoCancellation)]
    fn set_echo_cancellation(this: &VoiceClient, on: bool);
    #[wasm_bindgen(method, js_name = listDevices)]
    fn list_devices(this: &VoiceClient) -> js_sys::Promise;
    #[wasm_bindgen(method, js_name = startMicTest)]
    fn start_mic_test(this: &VoiceClient) -> js_sys::Promise;
    #[wasm_bindgen(method, js_name = stopMicTest)]
    fn stop_mic_test(this: &VoiceClient);

    #[wasm_bindgen(js_name = playPcm)]
    fn play_pcm(samples: &[f32], rate: u32);
    #[wasm_bindgen(js_name = unlockAudio)]
    fn unlock_audio_js();
}

/// Lets the page play audio from now on. Call it from the click that joins a voice channel.
pub fn unlock_audio() {
    unlock_audio_js();
}

/// Queues an event for the sink and wakes the app to drain it.
fn send(events: &Sender<VoiceEvent>, waker: &WakerCell, event: VoiceEvent) {
    let _ = events.send(event);
    if let Some(wake) = waker.lock().unwrap().as_ref() {
        wake();
    }
}

/// The newest frame of each watched track, written by the JS frame copy, read through a `VideoSink`.
type Cells = Arc<Mutex<HashMap<String, Arc<Mutex<Option<VideoFrame>>>>>>;

pub struct WebEngine {
    client: VoiceClient,
    tx: Sender<VoiceEvent>,
    waker: WakerCell,
    frames: Cells,
    // Held for as long as the engine: the JS client calls them.
    _emit: Closure<dyn FnMut(String)>,
    _on_frame: Closure<dyn FnMut(String, u32, u32, js_sys::Uint8ClampedArray)>,
}

impl WebEngine {
    fn start(waker: &WakerCell) -> (WebEngine, Receiver<VoiceEvent>) {
        let (tx, rx) = channel();
        let waker = Arc::clone(waker);
        let events = tx.clone();
        let wake = Arc::clone(&waker);
        let emit = Closure::<dyn FnMut(String)>::new(move |json: String| {
            if let Some(event) = parse(&json) {
                send(&events, &wake, event);
            }
        });
        let frames: Cells = Arc::default();
        let cells = Arc::clone(&frames);
        let on_frame = Closure::<dyn FnMut(String, u32, u32, js_sys::Uint8ClampedArray)>::new(
            move |track: String, width: u32, height: u32, rgba: js_sys::Uint8ClampedArray| {
                let cell = Arc::clone(cells.lock().unwrap().entry(track).or_default());
                let mut slot = cell.lock().unwrap();
                let seq = slot.as_ref().map_or(1, |f| f.seq + 1);
                *slot = Some(VideoFrame { width, height, rgba: rgba.to_vec().into(), seq });
            },
        );
        let client = VoiceClient::new(&emit, &on_frame);
        (WebEngine { client, tx, waker, frames, _emit: emit, _on_frame: on_frame }, rx)
    }

    /// Runs a JS promise to its end; a rejection becomes an `Error` event.
    fn run(&self, promise: js_sys::Promise) {
        let tx = self.tx.clone();
        let waker = Arc::clone(&self.waker);
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(e) = wasm_bindgen_futures::JsFuture::from(promise).await {
                let why = e.as_string().or_else(|| e.dyn_ref::<js_sys::Error>().map(|e| String::from(e.message()))).unwrap_or_default();
                send(&tx, &waker, VoiceEvent::Error(why));
            }
        });
    }
}

impl Engine for WebEngine {
    fn connect(&self, url: &str, token: &str, start_muted: bool, start_deafened: bool) {
        self.run(self.client.connect(url, token, start_muted, start_deafened));
    }
    fn disconnect(&self) {
        self.client.disconnect();
    }
    fn set_mute(&self, muted: bool) {
        self.client.set_mute(muted);
    }
    fn set_deaf(&self, deaf: bool) {
        self.client.set_deaf(deaf);
    }
    fn set_user_volume(&self, identity: &str, volume: f32) {
        self.client.set_user_volume(identity, volume);
    }
    fn set_input_device(&self, id: Option<String>) {
        self.run(self.client.set_input_device(id));
    }
    fn set_output_device(&self, id: Option<String>) {
        self.run(self.client.set_output_device(id));
    }
    fn set_mode(&self, mode: VoiceMode) {
        match mode {
            VoiceMode::VoiceActivity { threshold } => self.client.set_mode(true, threshold),
            VoiceMode::PushToTalk => self.client.set_mode(false, 0.0),
        }
    }
    fn set_ptt(&self, held: bool) {
        self.client.set_ptt(held);
    }
    fn set_noise_suppression(&self, on: bool) {
        self.client.set_noise_suppression(on);
    }
    fn set_echo_cancellation(&self, on: bool) {
        self.client.set_echo_cancellation(on);
    }
    fn list_devices(&self) {
        self.run(self.client.list_devices());
    }
    fn start_mic_test(&self) {
        self.run(self.client.start_mic_test());
    }
    fn stop_mic_test(&self) {
        self.client.stop_mic_test();
    }
    fn set_camera(&self, on: bool) {
        self.run(self.client.set_camera(on));
    }
    fn set_camera_device(&self, id: Option<String>) {
        self.run(self.client.set_camera_device(id));
    }
    /// The browser's own picker opens inside this call (it runs in the share button's click).
    fn set_screen_share(&self, source: Option<ScreenSource>) {
        self.run(self.client.set_screen_share(source.is_some()));
    }
    fn list_screen_sources(&self) {}
    fn video_frames(&self, track: &str) -> Option<VideoSink> {
        let cell = Arc::clone(self.frames.lock().unwrap().entry(track.to_owned()).or_default());
        self.client.watch_video(track);
        Some(VideoSink::new(move || cell.lock().unwrap().clone()))
    }
    fn release_video(&self, track: &str) {
        self.client.unwatch_video(track);
        self.frames.lock().unwrap().remove(track);
    }
}

/// Cues through WebAudio.
pub struct Cues;

impl SoundOut for Cues {
    fn play(&mut self, sound: Sound) {
        const RATE: u32 = 44_100;
        play_pcm(&synth(sound, RATE), RATE);
    }
}

/// The sink the web app installs; the engine (and its JS client) is built by the first join.
pub fn web_sink() -> EngineSink<WebEngine> {
    EngineSink::new(Box::new(WebEngine::start), Box::new(Cues))
}
