//! Voice and video for the desktop app, as a thin layer over the LiveKit Rust SDK.
//!
//! The UI never blocks on this crate: it calls the non-blocking command methods on
//! [`VoiceEngine`] and drains [`VoiceEvent`]s from the receiver returned by
//! [`VoiceEngine::new`] with `try_recv` in its poll loop. Everything else (the room, the
//! microphone, device changes, screen capture) runs on the tokio runtime handle passed in
//! and on SDK threads. See the README for the threading model, platform permissions and
//! what the SDK does not support.

mod audio;
mod capture;
mod engine;
mod micmeter;
mod session;
mod video;

use std::sync::mpsc::Receiver;

pub use video::{VideoFrame, VideoSink, i420_to_rgba, synthetic_rgba};
pub use audio::{LEVELS_HZ, SpeakingDetector, TxGate, level_of};
pub use session::{ConnState, StateMachine};

/// A participant's identity: our user id as a decimal string.
pub type Identity = String;

/// When the microphone transmits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VoiceMode {
    /// Transmit while the mic level (0..1, see [`level_of`]) is at or above `threshold`.
    VoiceActivity { threshold: f32 },
    /// Transmit while [`VoiceEngine::set_ptt`] says the key is held.
    PushToTalk,
}

impl Default for VoiceMode {
    fn default() -> Self {
        VoiceMode::VoiceActivity { threshold: 0.35 }
    }
}

/// Where microphone audio comes from.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum AudioInput {
    /// The platform audio device module: the selected input device, with WebRTC's echo
    /// cancellation, noise suppression and gain control.
    #[default]
    Device,
    /// A generated tone instead of a microphone. For tests; needs no audio hardware and no
    /// permission, and never plays anything through the speakers.
    #[doc(hidden)]
    Synthetic(SyntheticTone),
}

/// The tone behind [`AudioInput::Synthetic`].
#[derive(Clone, Debug, PartialEq)]
pub struct SyntheticTone {
    pub freq_hz: f32,
    /// 0..1 of full scale.
    pub amplitude: f32,
}

impl Default for SyntheticTone {
    fn default() -> Self {
        Self { freq_hz: 440.0, amplitude: 0.5 }
    }
}

/// Where camera and screen-share pictures come from.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum VideoInput {
    /// The real camera (the `camera` feature) and the desktop capturer.
    #[default]
    Device,
    /// A generated moving test picture for both the camera and a screen share, instead of any
    /// hardware, permission or system picker. For tests.
    #[doc(hidden)]
    Synthetic(SyntheticPicture),
}

/// The picture behind [`VideoInput::Synthetic`]: a colour gradient that scrolls with the frame
/// number, so consecutive frames differ.
#[derive(Clone, Debug, PartialEq)]
pub struct SyntheticPicture {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

impl Default for SyntheticPicture {
    fn default() -> Self {
        Self { width: 320, height: 180, fps: 15 }
    }
}

/// Options for one [`VoiceEngine::connect`].
#[derive(Clone, Debug, Default)]
pub struct ConnectOptions {
    pub audio_input: AudioInput,
    /// What feeds the camera and screen share (hardware unless a test says otherwise).
    pub video_input: VideoInput,
    /// Join muted (no mute signal is needed afterwards; [`VoiceEngine::set_mute`] still works).
    pub start_muted: bool,
    /// Join deafened (implies muted).
    pub start_deafened: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Audio,
    Video,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoKind {
    Camera,
    Screen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    Excellent,
    Good,
    Poor,
    Lost,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioDevice {
    /// Pass this to `set_input_device` / `set_output_device`. Stable across hot-plug.
    pub id: String,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenKind {
    /// A whole display. On macOS the system picker (ScreenCaptureKit) asks the user which one.
    Screen,
    Window,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenSource {
    pub id: u64,
    pub title: String,
    pub kind: ScreenKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum VoiceEvent {
    Connecting,
    Connected,
    Reconnecting,
    Disconnected { reason: String },
    ParticipantJoined { identity: Identity },
    ParticipantLeft { identity: Identity },
    /// A participant started or stopped talking. The local user is included.
    Speaking { identity: Identity, speaking: bool },
    /// Mic and playback levels, 0..1, about [`LEVELS_HZ`] times a second while anything is
    /// audible (one all-zero batch when it stops). The local mic is reported under the local
    /// identity once connected and under `""` before that (mic test). Everyone with an audio
    /// track in the room is in each batch.
    Levels(Vec<(Identity, f32)>),
    TrackMuted { identity: Identity, kind: MediaKind, muted: bool },
    Devices {
        inputs: Vec<AudioDevice>,
        outputs: Vec<AudioDevice>,
        default_input: Option<String>,
        default_output: Option<String>,
    },
    ScreenSources(Vec<ScreenSource>),
    /// A camera or screen track is available. `track_id` goes to
    /// [`VoiceEngine::subscribe_video`]. The local user's own camera and screen are reported
    /// too, for a preview.
    VideoTrack { identity: Identity, source: VideoKind, track_id: String },
    VideoTrackEnded { identity: Identity, track_id: String },
    ConnectionQuality { identity: Identity, quality: Quality },
    Error(String),
}

pub(crate) enum Command {
    Connect { url: String, token: String, opts: ConnectOptions },
    Disconnect,
    SetMute(bool),
    SetDeaf(bool),
    SetInputDevice(Option<String>),
    SetOutputDevice(Option<String>),
    SetInputGain(f32),
    SetMode(VoiceMode),
    SetPtt(bool),
    SetNoiseSuppression(bool),
    SetEchoCancellation(bool),
    SetUserVolume(Identity, f32),
    SetCamera(bool),
    SetScreenShare(Option<ScreenSource>),
    ListDevices,
    ListScreenSources,
    StartMicTest,
    StopMicTest,
}

/// The engine handle. Dropping it disconnects and stops the engine.
pub struct VoiceEngine {
    tx: tokio::sync::mpsc::UnboundedSender<engine::Msg>,
    videos: std::sync::Arc<video::Registry>,
}

impl VoiceEngine {
    /// Starts the engine on `runtime`. Events arrive on the returned receiver; drain it with
    /// `try_recv`. The channel is unbounded but levels are throttled, so it stays small as
    /// long as it is drained at least a few times a second.
    pub fn new(runtime: tokio::runtime::Handle) -> (VoiceEngine, Receiver<VoiceEvent>) {
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        let (tx, videos) = engine::spawn(runtime, event_tx);
        (VoiceEngine { tx, videos }, event_rx)
    }

    fn send(&self, cmd: Command) {
        let _ = self.tx.send(engine::Msg::Cmd(cmd));
    }

    /// Joins a room with the `url` and `token` from the server's `JoinVoice`. Emits
    /// `Connecting`, then `Connected` or `Disconnected { reason }`. Connecting while connected
    /// leaves the old room first.
    pub fn connect(&self, url: impl Into<String>, token: impl Into<String>, opts: ConnectOptions) {
        self.send(Command::Connect { url: url.into(), token: token.into(), opts });
    }

    pub fn disconnect(&self) {
        self.send(Command::Disconnect);
    }

    /// Mutes the microphone; others see a `TrackMuted`.
    pub fn set_mute(&self, muted: bool) {
        self.send(Command::SetMute(muted));
    }

    /// Stops playback of all remote audio and implies mute. Undeafening restores the mute
    /// state from before.
    pub fn set_deaf(&self, deaf: bool) {
        self.send(Command::SetDeaf(deaf));
    }

    /// `None` is the system default.
    pub fn set_input_device(&self, id: Option<String>) {
        self.send(Command::SetInputDevice(id));
    }

    /// `None` is the system default.
    pub fn set_output_device(&self, id: Option<String>) {
        self.send(Command::SetOutputDevice(id));
    }

    /// Linear gain on the microphone (1.0 = unchanged). Only applies to the synthetic input:
    /// the platform audio path has no gain control in this SDK (WebRTC's AGC is on instead).
    pub fn set_input_gain(&self, gain: f32) {
        self.send(Command::SetInputGain(gain));
    }

    pub fn set_mode(&self, mode: VoiceMode) {
        self.send(Command::SetMode(mode));
    }

    /// Push-to-talk key state. Ignored unless the mode is `PushToTalk`.
    pub fn set_ptt(&self, pressed: bool) {
        self.send(Command::SetPtt(pressed));
    }

    pub fn set_noise_suppression(&self, on: bool) {
        self.send(Command::SetNoiseSuppression(on));
    }

    pub fn set_echo_cancellation(&self, on: bool) {
        self.send(Command::SetEchoCancellation(on));
    }

    /// Per-participant playback volume. The SDK mixes remote audio inside WebRTC, so only
    /// silence (`volume <= 0.0`) takes effect; any other value plays at full volume.
    pub fn set_user_volume(&self, identity: impl Into<Identity>, volume: f32) {
        self.send(Command::SetUserVolume(identity.into(), volume));
    }

    /// Publishes or stops the camera. Needs the `camera` cargo feature, otherwise an
    /// `Error` event.
    pub fn set_camera(&self, on: bool) {
        self.send(Command::SetCamera(on));
    }

    /// Starts sharing `source` (from `list_screen_sources`), or stops with `None`.
    pub fn set_screen_share(&self, source: Option<ScreenSource>) {
        self.send(Command::SetScreenShare(source));
    }

    /// Replies with `VoiceEvent::Devices`.
    pub fn list_devices(&self) {
        self.send(Command::ListDevices);
    }

    /// Replies with `VoiceEvent::ScreenSources`.
    pub fn list_screen_sources(&self) {
        self.send(Command::ListScreenSources);
    }

    /// Opens the microphone and reports its level (under the identity `""` unless connected)
    /// without a room, for a settings meter.
    pub fn start_mic_test(&self) {
        self.send(Command::StartMicTest);
    }

    pub fn stop_mic_test(&self) {
        self.send(Command::StopMicTest);
    }

    /// The newest-frame handle for a track announced by `VideoTrack`.
    pub fn subscribe_video(&self, track_id: &str) -> VideoSink {
        VideoSink { cell: self.videos.cell(track_id) }
    }
}

impl Drop for VoiceEngine {
    fn drop(&mut self) {
        let _ = self.tx.send(engine::Msg::Shutdown);
    }
}
