//! The browser's voice engine: [`Engine`] over `js/voice.js`, which drives the `livekit-client`
//! SDK (the same LiveKit server and join token as the native engine). Audio only; video and
//! screen share stay desktop-only for now.
//!
//! The JS side reports events as JSON strings; they are parsed here into [`VoiceEvent`]s and queued
//! for the sink, which drains them from `poll` like the native engine's. Cues (join, leave, mute
//! clicks) are synthesized by `voice_engine::synth` and played through WebAudio.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::Arc;

use thehideout_voice_types::{AudioDevice, MediaKind, Quality, ScreenSource, VoiceEvent, VoiceMode};
use wasm_bindgen::prelude::*;

use crate::voice::Sound;
use crate::voice_engine::{Engine, EngineSink, SoundOut, WakerCell, synth};

#[wasm_bindgen(module = "/js/voice.js")]
extern "C" {
    type VoiceClient;
    #[wasm_bindgen(constructor)]
    fn new(emit: &Closure<dyn FnMut(String)>) -> VoiceClient;
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

/// One event as the JS side reports it.
fn parse(json: &str) -> Option<VoiceEvent> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let text = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_owned);
    let devices = |k: &str| -> Vec<AudioDevice> {
        v.get(k)
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|d| Some(AudioDevice { id: d.get("id")?.as_str()?.to_owned(), name: d.get("name")?.as_str()?.to_owned() }))
                    .collect()
            })
            .unwrap_or_default()
    };
    Some(match v.get("t")?.as_str()? {
        "connecting" => VoiceEvent::Connecting,
        "connected" => VoiceEvent::Connected,
        "reconnecting" => VoiceEvent::Reconnecting,
        "disconnected" => VoiceEvent::Disconnected { reason: text("reason").unwrap_or_default() },
        "participantJoined" => VoiceEvent::ParticipantJoined { identity: text("identity")? },
        "participantLeft" => VoiceEvent::ParticipantLeft { identity: text("identity")? },
        "speaking" => VoiceEvent::Speaking { identity: text("identity")?, speaking: v.get("speaking")?.as_bool()? },
        "trackMuted" => VoiceEvent::TrackMuted { identity: text("identity")?, kind: MediaKind::Audio, muted: v.get("muted")?.as_bool()? },
        "quality" => VoiceEvent::ConnectionQuality {
            identity: text("identity")?,
            quality: match text("quality")?.as_str() {
                "excellent" => Quality::Excellent,
                "good" => Quality::Good,
                "poor" => Quality::Poor,
                _ => Quality::Lost,
            },
        },
        "levels" => VoiceEvent::Levels(
            v.get("levels")?
                .as_array()?
                .iter()
                .filter_map(|p| Some((p.get(0)?.as_str()?.to_owned(), p.get(1)?.as_f64()? as f32)))
                .collect(),
        ),
        "devices" => VoiceEvent::Devices {
            inputs: devices("inputs"),
            outputs: devices("outputs"),
            default_input: text("defaultInput"),
            default_output: text("defaultOutput"),
        },
        "error" => VoiceEvent::Error(text("message")?),
        _ => return None,
    })
}

pub struct WebEngine {
    client: VoiceClient,
    tx: Sender<VoiceEvent>,
    // Held for as long as the engine: the JS client calls it.
    _emit: Closure<dyn FnMut(String)>,
}

impl WebEngine {
    fn start(waker: &WakerCell) -> (WebEngine, Receiver<VoiceEvent>) {
        let (tx, rx) = channel();
        let waker = Arc::clone(waker);
        let events = tx.clone();
        let emit = Closure::<dyn FnMut(String)>::new(move |json: String| {
            if let Some(event) = parse(&json) {
                let _ = events.send(event);
                if let Some(wake) = waker.lock().unwrap().as_ref() {
                    wake();
                }
            }
        });
        let client = VoiceClient::new(&emit);
        (WebEngine { client, tx, _emit: emit }, rx)
    }

    /// Runs a JS promise to its end; a rejection becomes an `Error` event.
    fn run(&self, promise: js_sys::Promise) {
        let tx = self.tx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(e) = wasm_bindgen_futures::JsFuture::from(promise).await {
                let why = e.as_string().or_else(|| e.dyn_ref::<js_sys::Error>().map(|e| String::from(e.message()))).unwrap_or_default();
                let _ = tx.send(VoiceEvent::Error(why));
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
    // Camera and screen share: desktop only for now (the web UI offers neither).
    fn set_camera(&self, _on: bool) {}
    fn set_screen_share(&self, _source: Option<ScreenSource>) {}
    fn list_screen_sources(&self) {}
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

