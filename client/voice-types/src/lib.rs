//! The plain types of the voice/video layer: events, devices, modes and pictures. No engine, no
//! audio or WebRTC dependency, so the UI compiles for the browser too. `thehideout-voice` (the
//! native LiveKit engine) re-exports everything here.

use std::sync::Arc;

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

/// One decoded picture, tightly packed RGBA8 (`width * height * 4` bytes, no row padding).
#[derive(Clone, Debug)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
    /// Increases with every new picture on the track, so the UI can skip a texture upload
    /// when it has already seen this one.
    pub seq: u64,
}

/// A handle on one video track's newest frame. Cheap to clone; all clones see the same frame.
/// An engine builds one over whatever holds its newest picture.
#[derive(Clone)]
pub struct VideoSink {
    latest: Arc<dyn Fn() -> Option<VideoFrame> + Send + Sync>,
}

impl VideoSink {
    pub fn new(latest: impl Fn() -> Option<VideoFrame> + Send + Sync + 'static) -> Self {
        Self { latest: Arc::new(latest) }
    }

    /// The newest frame as RGBA8, or `None` before the first one arrives. Calling again
    /// without a new frame returns the same `Arc` (same `seq`).
    pub fn latest_frame(&self) -> Option<VideoFrame> {
        (self.latest)()
    }
}
