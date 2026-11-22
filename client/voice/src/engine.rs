//! The engine actor. One task owns all state; commands, room events and background results
//! reach it as [`Msg`]s on a single channel, so no locks are needed and the UI-facing handle
//! never waits on it.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use livekit::prelude::*;
use livekit::webrtc::audio_frame::AudioFrame;
use livekit::webrtc::audio_source::native::NativeAudioSource;
use livekit::webrtc::audio_source::{AudioSourceOptions, RtcAudioSource};
use livekit::webrtc::audio_stream::native::NativeAudioStream;
use livekit::webrtc::stats::RtcStats;
use livekit::webrtc::video_source::native::NativeVideoSource;
use livekit::webrtc::video_source::{RtcVideoSource, VideoResolution};
use livekit::webrtc::video_stream::native::NativeVideoStream;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::task::AbortHandle;

use crate::audio::{
    LEVELS_HZ, LevelBatcher, LevelCell, SpeakingDetector, TxGate, level_from_linear, level_of, sine_frame,
    speaking_threshold,
};
use crate::capture::{self, Capture, OnEnd};
use crate::micmeter::MicMeter;
use crate::session::StateMachine;
use crate::video::{Pending, Registry, VideoCell};
use crate::{
    AudioDevice, AudioInput, Command, ConnectOptions, Identity, MediaKind, Quality, ScreenSource, VideoInput,
    VideoKind, VoiceEvent, VoiceMode,
};

const TICK: Duration = Duration::from_millis(20);
const SAMPLE_RATE: u32 = 48_000;
const FRAME_SAMPLES: usize = 480; // 10 ms mono

#[allow(clippy::large_enum_variant)]
pub(crate) enum Msg {
    Cmd(Command),
    Shutdown,
    ConnectDone { epoch: u64, res: Result<Joined, String> },
    Room { epoch: u64, ev: RoomEvent },
    RoomClosed { epoch: u64 },
    VideoPublished { epoch: u64, kind: VideoKind, res: Result<LocalTrackPublication, String> },
    CaptureEnded { epoch: u64, kind: VideoKind, reason: Option<String> },
    ScreenSources(Result<Vec<ScreenSource>, String>),
}

pub(crate) struct Joined {
    room: Arc<Room>,
    events: UnboundedReceiver<RoomEvent>,
    mic_pub: Option<LocalTrackPublication>,
    publish_error: Option<String>,
}

pub(crate) fn spawn(
    runtime: tokio::runtime::Handle,
    events: Sender<VoiceEvent>,
) -> (UnboundedSender<Msg>, Arc<Registry>) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let videos = Arc::new(Registry::default());
    let actor = Actor::new(events, tx.clone(), videos.clone());
    runtime.spawn(actor.run(rx));
    (tx, videos)
}

/// Aborts its task when dropped.
struct Task(AbortHandle);

impl Drop for Task {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn spawn_task(f: impl std::future::Future<Output = ()> + Send + 'static) -> Task {
    Task(tokio::spawn(f).abort_handle())
}

struct Mic {
    track: LocalAudioTrack,
    meter: Arc<LevelCell>,
    input: AudioInput,
    /// The synthetic input's source, to keep its processing options in step.
    source: Option<NativeAudioSource>,
    /// Outside a room the platform capture cannot be tapped, so the settings meter reads
    /// the device through `cpal` instead.
    test_meter: Option<MicMeter>,
    _tasks: Vec<Task>,
}

struct RemoteAudio {
    identity: Identity,
    track: RemoteAudioTrack,
    meter: Arc<LevelCell>,
    _sink: Task,
}

struct RemoteVideo {
    identity: Identity,
    _sink: Task,
}

struct LocalVideo {
    cell: Arc<VideoCell>,
    capture: Capture,
    /// The track sid once published; `None` while the publish is in flight.
    track_id: Option<String>,
}

struct Sess {
    room: Arc<Room>,
    mic_pub: Option<LocalTrackPublication>,
    participants: HashSet<Identity>,
    remote_audio: HashMap<String, RemoteAudio>,
    remote_video: HashMap<String, RemoteVideo>,
    speakers: HashSet<Identity>,
    camera: Option<LocalVideo>,
    screen: Option<LocalVideo>,
    /// Reads the local mic level from the track's stats (platform capture only).
    _mic_stats: Option<Task>,
    _forward: Task,
}

struct Actor {
    events: Sender<VoiceEvent>,
    me: UnboundedSender<Msg>,
    videos: Arc<Registry>,
    sm: StateMachine,
    epoch: u64,
    // settings
    muted: bool,
    deaf: bool,
    gate: TxGate,
    gain: Arc<AtomicU32>,
    noise_suppression: bool,
    echo_cancellation: bool,
    input_device: Option<String>,
    output_device: Option<String>,
    volumes: HashMap<Identity, f32>,
    // audio
    audio: Option<PlatformAudio>,
    mic: Option<Mic>,
    mic_test: bool,
    tx_applied: Option<bool>,
    speaking: SpeakingDetector,
    levels: LevelBatcher,
    local_identity: Identity,
    video_input: VideoInput,
    sess: Option<Sess>,
}

fn kind_of(source: TrackSource) -> VideoKind {
    match source {
        TrackSource::Screenshare => VideoKind::Screen,
        _ => VideoKind::Camera,
    }
}

fn quality_of(q: ConnectionQuality) -> Quality {
    match q {
        ConnectionQuality::Excellent => Quality::Excellent,
        ConnectionQuality::Good => Quality::Good,
        ConnectionQuality::Poor => Quality::Poor,
        ConnectionQuality::Lost => Quality::Lost,
    }
}

impl Actor {
    fn new(events: Sender<VoiceEvent>, me: UnboundedSender<Msg>, videos: Arc<Registry>) -> Self {
        Self {
            events,
            me,
            videos,
            sm: StateMachine::default(),
            epoch: 0,
            muted: false,
            deaf: false,
            gate: TxGate::new(VoiceMode::default()),
            gain: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            noise_suppression: true,
            echo_cancellation: true,
            input_device: None,
            output_device: None,
            volumes: HashMap::new(),
            audio: None,
            mic: None,
            mic_test: false,
            tx_applied: None,
            speaking: SpeakingDetector::default(),
            levels: LevelBatcher::new(LEVELS_HZ),
            local_identity: String::new(),
            video_input: VideoInput::Device,
            sess: None,
        }
    }

    fn emit(&self, e: VoiceEvent) {
        let _ = self.events.send(e);
    }

    fn emit_opt(&self, e: Option<VoiceEvent>) {
        if let Some(e) = e {
            self.emit(e);
        }
    }

    fn error(&self, e: impl std::fmt::Display) {
        self.emit(VoiceEvent::Error(e.to_string()));
    }

    async fn run(mut self, mut rx: UnboundedReceiver<Msg>) {
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                msg = rx.recv() => match msg {
                    None | Some(Msg::Shutdown) => break,
                    Some(m) => self.handle(m),
                },
                _ = tick.tick() => self.on_tick(),
            }
        }
        let room = self.sess.as_ref().map(|s| s.room.clone());
        self.sm = StateMachine::default();
        self.sess = None;
        if let Some(room) = room {
            let _ = tokio::time::timeout(Duration::from_secs(2), room.close()).await;
        }
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Cmd(c) => self.command(c),
            Msg::Shutdown => {}
            Msg::ConnectDone { epoch, res } => self.connect_done(epoch, res),
            Msg::Room { epoch, ev } if epoch == self.epoch => self.room_event(ev),
            Msg::RoomClosed { epoch } if epoch == self.epoch && self.sm.is_active() => {
                self.teardown("connection closed");
            }
            Msg::VideoPublished { epoch, kind, res } if epoch == self.epoch => self.video_published(kind, res),
            Msg::VideoPublished { res: Ok(publication), .. } => {
                // The room it was published to is already gone; nothing to unpublish.
                drop(publication);
            }
            Msg::CaptureEnded { epoch, kind, reason } if epoch == self.epoch => {
                if let Some(r) = reason {
                    self.error(r);
                }
                self.stop_video(kind);
            }
            Msg::ScreenSources(res) => match res {
                Ok(list) => self.emit(VoiceEvent::ScreenSources(list)),
                Err(e) => self.error(e),
            },
            _ => {} // stale
        }
    }

    // ---- commands --------------------------------------------------------------------------

    fn command(&mut self, cmd: Command) {
        match cmd {
            Command::Connect { url, token, opts } => self.connect(url, token, opts),
            Command::Disconnect => {
                if self.sm.is_active() {
                    self.teardown("client initiated");
                }
            }
            Command::SetMute(m) => {
                self.muted = m;
                self.apply_mute();
            }
            Command::SetDeaf(d) => {
                self.deaf = d;
                self.apply_mute();
            }
            Command::SetInputDevice(id) => {
                self.input_device = id;
                self.apply_input_device();
                self.restart_test_meter();
            }
            Command::SetOutputDevice(id) => {
                self.output_device = id;
                self.apply_output_device();
            }
            Command::SetInputGain(g) => {
                self.gain.store(g.max(0.0).to_bits(), Ordering::Relaxed);
            }
            Command::SetMode(m) => {
                self.gate.set_mode(m);
                self.tx_applied = None;
            }
            Command::SetPtt(p) => {
                self.gate.set_ptt(p);
            }
            Command::SetNoiseSuppression(on) => {
                self.noise_suppression = on;
                self.apply_processing();
            }
            Command::SetEchoCancellation(on) => {
                self.echo_cancellation = on;
                self.apply_processing();
            }
            Command::SetUserVolume(identity, v) => {
                self.volumes.insert(identity, v);
                self.apply_remote_audio();
            }
            Command::SetCamera(on) => {
                if on {
                    self.start_video(VideoKind::Camera, None);
                } else {
                    self.stop_video(VideoKind::Camera);
                }
            }
            Command::SetScreenShare(source) => match source {
                Some(s) => self.start_video(VideoKind::Screen, Some(s)),
                None => self.stop_video(VideoKind::Screen),
            },
            Command::ListDevices => self.list_devices(),
            Command::ListScreenSources => {
                let me = self.me.clone();
                tokio::task::spawn_blocking(move || {
                    let _ = me.send(Msg::ScreenSources(capture::list_screen_sources()));
                });
            }
            Command::StartMicTest => self.start_mic_test(),
            Command::StopMicTest => self.stop_mic_test(),
        }
    }

    fn connect(&mut self, url: String, token: String, opts: ConnectOptions) {
        if self.sm.is_active() {
            self.teardown("reconnect requested");
        }
        self.epoch += 1;
        let epoch = self.epoch;
        { let e = self.sm.begin_connect(); self.emit_opt(e); }
        self.muted = opts.start_muted;
        self.deaf = opts.start_deafened;
        self.video_input = opts.video_input.clone();
        if let Some(m) = &mut self.mic {
            m.test_meter = None; // the room reports the level from here on
        }
        self.gate.set_muted(self.muted || self.deaf);

        let mic_track = match self.ensure_mic(&opts.audio_input) {
            Ok(()) => self.mic.as_ref().map(|m| m.track.clone()),
            Err(e) => {
                self.error(format!("microphone unavailable, joining listen-only: {e}"));
                None
            }
        };
        let me = self.me.clone();
        tokio::spawn(async move {
            let res = async {
                let (room, events) =
                    Room::connect(&url, &token, RoomOptions::default()).await.map_err(|e| e.to_string())?;
                let room = Arc::new(room);
                let (mut mic_pub, mut publish_error) = (None, None);
                if let Some(track) = mic_track {
                    let options = TrackPublishOptions { source: TrackSource::Microphone, ..Default::default() };
                    match room.local_participant().publish_track(LocalTrack::Audio(track), options).await {
                        Ok(p) => mic_pub = Some(p),
                        Err(e) => publish_error = Some(format!("could not publish the microphone: {e}")),
                    }
                }
                Ok(Joined { room, events, mic_pub, publish_error })
            }
            .await;
            let _ = me.send(Msg::ConnectDone { epoch, res });
        });
    }

    fn connect_done(&mut self, epoch: u64, res: Result<Joined, String>) {
        if epoch != self.epoch || !self.sm.is_active() {
            // Disconnected (or reconnected elsewhere) while this was in flight.
            if let Ok(j) = res {
                tokio::spawn(async move {
                    let _ = j.room.close().await;
                });
            }
            return;
        }
        match res {
            Err(e) => {
                { let e = self.sm.disconnected(e); self.emit_opt(e); }
                self.release_mic_if_idle();
            }
            Ok(mut j) => {
                if let Some(e) = j.publish_error.take() {
                    self.error(e);
                }
                self.local_identity = j.room.local_participant().identity().to_string();
                let me = self.me.clone();
                let mut events = j.events;
                let forward = spawn_task(async move {
                    while let Some(ev) = events.recv().await {
                        if me.send(Msg::Room { epoch, ev }).is_err() {
                            return;
                        }
                    }
                    let _ = me.send(Msg::RoomClosed { epoch });
                });
                let mic_stats = self.mic.as_ref().filter(|m| m.input == AudioInput::Device).map(|m| {
                    let (track, meter) = (m.track.clone(), m.meter.clone());
                    spawn_task(async move {
                        let mut every = tokio::time::interval(Duration::from_millis(50));
                        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                        loop {
                            every.tick().await;
                            let Ok(stats) = track.get_stats().await else { continue };
                            for s in stats {
                                if let RtcStats::MediaSource(m) = s {
                                    meter.record(level_from_linear(m.audio.audio_level));
                                }
                            }
                        }
                    })
                });
                let existing: Vec<Identity> =
                    j.room.remote_participants().keys().map(|k| k.to_string()).collect();
                self.sess = Some(Sess {
                    room: j.room,
                    mic_pub: j.mic_pub,
                    participants: HashSet::new(),
                    remote_audio: HashMap::new(),
                    remote_video: HashMap::new(),
                    speakers: HashSet::new(),
                    camera: None,
                    screen: None,
                    _mic_stats: mic_stats,
                    _forward: forward,
                });
                { let e = self.sm.connected(); self.emit_opt(e); }
                for id in existing {
                    self.ensure_participant(&id);
                }
                self.apply_mute();
            }
        }
    }

    /// Leaves the room (if any) and resets everything tied to it.
    fn teardown(&mut self, reason: &str) {
        self.epoch += 1; // events from the old room are stale from here on
        if let Some(mut s) = self.sess.take() {
            for (id, v) in s.remote_video.drain() {
                self.videos.remove(&id);
                self.emit(VoiceEvent::VideoTrackEnded { identity: v.identity.clone(), track_id: id });
            }
            for lv in [s.camera.take(), s.screen.take()].into_iter().flatten() {
                if let Some(id) = lv.track_id {
                    self.videos.remove(&id);
                    self.emit(VoiceEvent::VideoTrackEnded { identity: self.local_identity.clone(), track_id: id });
                }
            }
            let room = s.room.clone();
            tokio::spawn(async move {
                let _ = room.close().await;
            });
        }
        { let e = self.sm.disconnected(reason); self.emit_opt(e); }
        self.release_mic_if_idle();
        self.local_identity.clear();
        self.tx_applied = None;
    }

    fn release_mic_if_idle(&mut self) {
        if !self.mic_test && !self.sm.is_active() {
            self.mic = None;
            self.speaking = SpeakingDetector::default();
        }
    }

    // ---- microphone ------------------------------------------------------------------------

    fn ensure_audio(&mut self) -> Result<&PlatformAudio, String> {
        if self.audio.is_none() {
            let audio = PlatformAudio::new().map_err(|e| format!("audio device module: {e}"))?;
            self.audio = Some(audio);
            self.apply_processing();
            self.apply_input_device();
            self.apply_output_device();
        }
        Ok(self.audio.as_ref().unwrap())
    }

    fn ensure_mic(&mut self, input: &AudioInput) -> Result<(), String> {
        if self.mic.as_ref().is_some_and(|m| &m.input == input) {
            return Ok(());
        }
        self.mic = None;
        self.tx_applied = None;
        let meter = Arc::new(LevelCell::default());
        let mut tasks = Vec::new();
        let (track, source) = match input {
            AudioInput::Device => {
                let rtc_source = self.ensure_audio()?.rtc_source();
                (LocalAudioTrack::create_audio_track("microphone", rtc_source), None)
            }
            AudioInput::Synthetic(tone) => {
                let source = NativeAudioSource::new(self.source_options(), SAMPLE_RATE, 1, 100);
                let track =
                    LocalAudioTrack::create_audio_track("microphone", RtcAudioSource::Native(source.clone()));
                let (m, gain, s, tone) = (meter.clone(), self.gain.clone(), source.clone(), tone.clone());
                tasks.push(spawn_task(async move {
                    let mut every = tokio::time::interval(Duration::from_millis(10));
                    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    let mut frame = AudioFrame::new(SAMPLE_RATE, 1, FRAME_SAMPLES as u32);
                    let mut phase = 0.0f32;
                    loop {
                        every.tick().await;
                        let g = f32::from_bits(gain.load(Ordering::Relaxed));
                        let amp = (tone.amplitude * g).clamp(0.0, 1.0);
                        sine_frame(frame.data.to_mut(), SAMPLE_RATE, tone.freq_hz, amp, &mut phase);
                        m.record(level_of(&frame.data));
                        if s.capture_frame(&frame).await.is_err() {
                            return;
                        }
                    }
                }));
                (track, Some(source))
            }
        };
        self.mic = Some(Mic { track, meter, input: input.clone(), source, test_meter: None, _tasks: tasks });
        Ok(())
    }

    fn source_options(&self) -> AudioSourceOptions {
        AudioSourceOptions {
            echo_cancellation: self.echo_cancellation,
            noise_suppression: self.noise_suppression,
            auto_gain_control: true,
        }
    }

    fn start_mic_test(&mut self) {
        self.mic_test = true;
        let input = self.mic.as_ref().map(|m| m.input.clone()).unwrap_or_default();
        if let Err(e) = self.ensure_mic(&input) {
            self.mic_test = false;
            return self.error(format!("mic test: {e}"));
        }
        self.restart_test_meter();
    }

    /// (Re)opens the settings meter on the selected input, unless a room is already
    /// reporting the local level.
    fn restart_test_meter(&mut self) {
        let name = self.selected_input_name();
        let connected = self.sm.is_active();
        let Some(m) = &mut self.mic else { return };
        m.test_meter = None;
        if !self.mic_test || connected || m.input != AudioInput::Device {
            return;
        }
        match MicMeter::start(name, m.meter.clone()) {
            Ok(meter) => m.test_meter = Some(meter),
            Err(e) => {
                self.mic_test = false;
                self.error(format!("mic test: {e}"));
            }
        }
    }

    /// The listed name of the chosen input; `None` for the system default.
    fn selected_input_name(&self) -> Option<String> {
        let (id, audio) = (self.input_device.as_ref()?, self.audio.as_ref()?);
        if id == "default" {
            return None;
        }
        audio.recording_devices().find(|d| d.id.as_str() == id).map(|d| d.name)
    }

    fn stop_mic_test(&mut self) {
        self.mic_test = false;
        if let Some(m) = &mut self.mic {
            m.test_meter = None;
        }
        self.release_mic_if_idle();
    }

    fn apply_processing(&mut self) {
        if let Some(m) = &self.mic
            && let Some(src) = &m.source
        {
            src.set_audio_options(self.source_options());
        }
        let Some(audio) = &self.audio else { return };
        let r = audio
            .set_echo_cancellation(self.echo_cancellation, false)
            .and_then(|_| audio.set_noise_suppression(self.noise_suppression, false));
        if let Err(e) = r {
            self.error(format!("audio processing: {e}"));
        }
    }

    fn apply_input_device(&self) {
        let Some(audio) = &self.audio else { return };
        let id = match &self.input_device {
            Some(id) => RecordingDeviceId::from_unchecked_guid(id),
            None => RecordingDeviceId::from_unchecked_guid(&default_id(audio.recording_devices().map(|d| d.id.to_string()))),
        };
        if let Err(e) = audio.switch_recording_device(&id) {
            self.error(format!("input device: {e}"));
        }
    }

    fn apply_output_device(&self) {
        let Some(audio) = &self.audio else { return };
        let id = match &self.output_device {
            Some(id) => PlayoutDeviceId::from_unchecked_guid(id),
            None => PlayoutDeviceId::from_unchecked_guid(&default_id(audio.playout_devices().map(|d| d.id.to_string()))),
        };
        if let Err(e) = audio.switch_playout_device(&id) {
            self.error(format!("output device: {e}"));
        }
    }

    fn list_devices(&mut self) {
        let audio = match self.ensure_audio() {
            Ok(a) => a,
            Err(e) => {
                self.error(e);
                return self.emit(VoiceEvent::Devices {
                    inputs: vec![],
                    outputs: vec![],
                    default_input: None,
                    default_output: None,
                });
            }
        };
        let inputs: Vec<AudioDevice> = audio
            .recording_devices()
            .map(|d| AudioDevice { id: d.id.to_string(), name: d.name })
            .collect();
        let outputs: Vec<AudioDevice> = audio
            .playout_devices()
            .map(|d| AudioDevice { id: d.id.to_string(), name: d.name })
            .collect();
        let default_input = (!inputs.is_empty()).then(|| default_id(inputs.iter().map(|d| d.id.clone())));
        let default_output = (!outputs.is_empty()).then(|| default_id(outputs.iter().map(|d| d.id.clone())));
        self.emit(VoiceEvent::Devices { inputs, outputs, default_input, default_output });
    }

    // ---- mute, deafen, remote audio --------------------------------------------------------

    fn apply_mute(&mut self) {
        let muted = self.muted || self.deaf;
        self.gate.set_muted(muted);
        if let Some(p) = self.sess.as_ref().and_then(|s| s.mic_pub.clone())
            && p.is_muted() != muted
        {
            if muted {
                p.mute();
            } else {
                p.unmute();
            }
        }
        // The SDK flips the track's enabled flag itself; have the gate decide again.
        self.tx_applied = None;
        self.apply_remote_audio();
    }

    fn apply_tx(&mut self, transmit: bool) {
        if self.tx_applied == Some(transmit) {
            return;
        }
        self.tx_applied = Some(transmit);
        if let Some(m) = &self.mic {
            if transmit {
                m.track.enable();
            } else {
                m.track.disable();
            }
        }
    }

    fn apply_remote_audio(&self) {
        let Some(s) = &self.sess else { return };
        for ra in s.remote_audio.values() {
            let on = !self.deaf && self.volumes.get(&ra.identity).copied().unwrap_or(1.0) > 0.0;
            ra.track.rtc_track().set_enabled(on);
        }
    }

    // ---- 20 ms tick ------------------------------------------------------------------------

    fn on_tick(&mut self) {
        let now = Instant::now();
        let local = self.local_identity.clone();
        if let Some(mic) = &self.mic {
            let level = mic.meter.take();
            if let Some(l) = level {
                self.gate.feed(l, now);
            }
            let l = level.unwrap_or(0.0);
            let transmit = self.gate.transmitting(now);
            self.apply_tx(transmit);
            // While joining, the local identity is not known yet: report once it is (a mic
            // test outside a room reports under "").
            if !local.is_empty() || !self.sm.is_active() {
                self.levels.push(&local, l);
                let threshold = speaking_threshold(self.gate.mode());
                if let Some(s) = self.speaking.update(l, threshold, transmit, now) {
                    self.emit(VoiceEvent::Speaking { identity: local, speaking: s });
                }
            }
        }
        if let Some(s) = &self.sess {
            for ra in s.remote_audio.values() {
                self.levels.push(&ra.identity, ra.meter.take().unwrap_or(0.0));
            }
        }
        if let Some(batch) = self.levels.flush(now) {
            self.emit(VoiceEvent::Levels(batch));
        }
    }

    // ---- room events -----------------------------------------------------------------------

    fn ensure_participant(&mut self, id: &str) {
        let Some(s) = &mut self.sess else { return };
        if s.participants.insert(id.to_string()) {
            self.emit(VoiceEvent::ParticipantJoined { identity: id.to_string() });
        }
    }

    fn room_event(&mut self, ev: RoomEvent) {
        match ev {
            RoomEvent::ParticipantConnected(p) => self.ensure_participant(p.identity().as_str()),
            RoomEvent::ParticipantDisconnected(p) => self.participant_left(p.identity().to_string()),
            RoomEvent::TrackSubscribed { track, publication, participant } => {
                let identity = participant.identity().to_string();
                self.ensure_participant(&identity);
                let sid = publication.sid().to_string();
                match track {
                    RemoteTrack::Audio(t) => self.remote_audio_subscribed(sid, identity, t),
                    RemoteTrack::Video(t) => self.remote_video_subscribed(sid, identity, kind_of(publication.source()), t),
                }
            }
            RoomEvent::TrackUnsubscribed { publication, participant, .. } => {
                let sid = publication.sid().to_string();
                let Some(s) = &mut self.sess else { return };
                s.remote_audio.remove(&sid);
                if s.remote_video.remove(&sid).is_some() {
                    self.videos.remove(&sid);
                    self.emit(VoiceEvent::VideoTrackEnded {
                        identity: participant.identity().to_string(),
                        track_id: sid,
                    });
                }
            }
            RoomEvent::TrackMuted { participant, publication } => self.track_muted(participant, publication, true),
            RoomEvent::TrackUnmuted { participant, publication } => self.track_muted(participant, publication, false),
            RoomEvent::ActiveSpeakersChanged { speakers } => {
                let local = self.local_identity.clone();
                let now: HashSet<Identity> =
                    speakers.iter().map(|p| p.identity().to_string()).filter(|i| *i != local).collect();
                let Some(s) = &mut self.sess else { return };
                let before = std::mem::replace(&mut s.speakers, now.clone());
                let mut changes: Vec<VoiceEvent> = Vec::new();
                for id in now.difference(&before) {
                    changes.push(VoiceEvent::Speaking { identity: id.clone(), speaking: true });
                }
                for id in before.difference(&now) {
                    changes.push(VoiceEvent::Speaking { identity: id.clone(), speaking: false });
                }
                for c in changes {
                    self.emit(c);
                }
            }
            RoomEvent::ConnectionQualityChanged { quality, participant } => {
                self.emit(VoiceEvent::ConnectionQuality {
                    identity: participant.identity().to_string(),
                    quality: quality_of(quality),
                });
            }
            RoomEvent::Reconnecting => { let e = self.sm.reconnecting(); self.emit_opt(e); }
            RoomEvent::Reconnected => { let e = self.sm.connected(); self.emit_opt(e); }
            RoomEvent::Disconnected { reason } => self.teardown(&format!("{reason:?}")),
            _ => {}
        }
    }

    fn participant_left(&mut self, id: Identity) {
        let Some(s) = &mut self.sess else { return };
        if !s.participants.remove(&id) {
            return;
        }
        s.remote_audio.retain(|_, a| a.identity != id);
        let gone: Vec<String> =
            s.remote_video.iter().filter(|(_, v)| v.identity == id).map(|(k, _)| k.clone()).collect();
        for k in &gone {
            s.remote_video.remove(k);
        }
        let was_speaking = s.speakers.remove(&id);
        for k in gone {
            self.videos.remove(&k);
            self.emit(VoiceEvent::VideoTrackEnded { identity: id.clone(), track_id: k });
        }
        if was_speaking {
            self.emit(VoiceEvent::Speaking { identity: id.clone(), speaking: false });
        }
        self.emit(VoiceEvent::ParticipantLeft { identity: id });
    }

    fn remote_audio_subscribed(&mut self, sid: String, identity: Identity, track: RemoteAudioTrack) {
        let meter = Arc::new(LevelCell::default());
        // The decoded PCM, only measured here; playback itself is the platform device's job.
        let mut stream = NativeAudioStream::new(track.rtc_track(), SAMPLE_RATE as i32, 1);
        let m = meter.clone();
        let sink = spawn_task(async move {
            while let Some(frame) = stream.next().await {
                m.record(level_of(&frame.data));
            }
        });
        if let Some(s) = &mut self.sess {
            s.remote_audio.insert(sid, RemoteAudio { identity, track, meter, _sink: sink });
        }
        self.apply_remote_audio();
    }

    fn remote_video_subscribed(&mut self, sid: String, identity: Identity, source: VideoKind, track: RemoteVideoTrack) {
        let cell = self.videos.cell(&sid);
        let mut stream = NativeVideoStream::new(track.rtc_track());
        let sink = spawn_task(async move {
            while let Some(frame) = stream.next().await {
                cell.put(Pending::Buffer(frame.buffer));
            }
        });
        if let Some(s) = &mut self.sess {
            s.remote_video.insert(sid.clone(), RemoteVideo { identity: identity.clone(), _sink: sink });
        }
        self.emit(VoiceEvent::VideoTrack { identity, source, track_id: sid });
    }

    fn track_muted(&self, participant: Participant, publication: TrackPublication, muted: bool) {
        let kind = match publication.kind() {
            TrackKind::Audio => MediaKind::Audio,
            TrackKind::Video => MediaKind::Video,
        };
        self.emit(VoiceEvent::TrackMuted { identity: participant.identity().to_string(), kind, muted });
    }

    // ---- camera and screen share -----------------------------------------------------------

    fn start_video(&mut self, kind: VideoKind, source: Option<ScreenSource>) {
        let epoch = self.epoch;
        let Some(s) = &mut self.sess else {
            return self.error("join a voice channel before sharing video");
        };
        let slot = if kind == VideoKind::Camera { &mut s.camera } else { &mut s.screen };
        if slot.is_some() {
            return;
        }
        let (video, name, source_kind) = match kind {
            VideoKind::Camera => (
                NativeVideoSource::new(VideoResolution { width: 1280, height: 720 }, false),
                "camera",
                TrackSource::Camera,
            ),
            VideoKind::Screen => (
                NativeVideoSource::new(VideoResolution { width: 1920, height: 1080 }, true),
                "screen",
                TrackSource::Screenshare,
            ),
        };
        let cell = Arc::new(VideoCell::default());
        let me = self.me.clone();
        let on_end: OnEnd = Arc::new(move |reason| {
            let _ = me.send(Msg::CaptureEnded { epoch, kind, reason });
        });
        let started = match (kind, source) {
            (_, _) if matches!(self.video_input, VideoInput::Synthetic(_)) => {
                let VideoInput::Synthetic(pic) = &self.video_input else { unreachable!() };
                Ok(capture::start_synthetic(pic.clone(), video.clone(), cell.clone()))
            }
            (VideoKind::Camera, _) => capture::start_camera(video.clone(), cell.clone(), on_end),
            (VideoKind::Screen, Some(src)) => Ok(capture::start_screen(src, video.clone(), cell.clone(), on_end)),
            (VideoKind::Screen, None) => return,
        };
        let capture = match started {
            Ok(c) => c,
            Err(e) => return self.error(e),
        };
        *slot = Some(LocalVideo { cell, capture, track_id: None });
        let track = LocalVideoTrack::create_video_track(name, RtcVideoSource::Native(video));
        let room = s.room.clone();
        let me = self.me.clone();
        tokio::spawn(async move {
            let options = TrackPublishOptions { source: source_kind, ..Default::default() };
            let res = room
                .local_participant()
                .publish_track(LocalTrack::Video(track), options)
                .await
                .map_err(|e| format!("could not publish {name}: {e}"));
            let _ = me.send(Msg::VideoPublished { epoch, kind, res });
        });
    }

    fn video_published(&mut self, kind: VideoKind, res: Result<LocalTrackPublication, String>) {
        let local = self.local_identity.clone();
        let Some(s) = &mut self.sess else { return };
        let slot = if kind == VideoKind::Camera { &mut s.camera } else { &mut s.screen };
        match res {
            Err(e) => {
                *slot = None;
                self.error(e);
            }
            Ok(publication) => {
                let id = publication.sid().to_string();
                match slot {
                    Some(lv) => {
                        lv.track_id = Some(id.clone());
                        self.videos.insert(&id, lv.cell.clone());
                        self.emit(VoiceEvent::VideoTrack { identity: local, source: kind, track_id: id });
                    }
                    None => {
                        // Stopped while publishing: take it straight back down.
                        let room = s.room.clone();
                        tokio::spawn(async move {
                            let _ = room.local_participant().unpublish_track(&publication.sid()).await;
                        });
                    }
                }
            }
        }
    }

    fn stop_video(&mut self, kind: VideoKind) {
        let local = self.local_identity.clone();
        let Some(s) = &mut self.sess else { return };
        let slot = if kind == VideoKind::Camera { &mut s.camera } else { &mut s.screen };
        let Some(lv) = slot.take() else { return };
        drop(lv.capture);
        if let Some(id) = lv.track_id {
            let room = s.room.clone();
            let sid: TrackSid = match id.clone().try_into() {
                Ok(sid) => sid,
                Err(_) => return,
            };
            tokio::spawn(async move {
                let _ = room.local_participant().unpublish_track(&sid).await;
            });
            self.videos.remove(&id);
            self.emit(VoiceEvent::VideoTrackEnded { identity: local, track_id: id });
        }
    }
}

/// The id of the system default device if the list has one (the platform module lists it as
/// "default"), else the first device.
fn default_id(ids: impl Iterator<Item = String>) -> String {
    let ids: Vec<String> = ids.collect();
    if ids.iter().any(|i| i == "default") {
        "default".to_string()
    } else {
        ids.into_iter().next().unwrap_or_else(|| "default".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::default_id;

    #[test]
    fn default_device_prefers_the_system_default_entry() {
        let l = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(default_id(l(&["80", "default", "85"]).into_iter()), "default");
        assert_eq!(default_id(l(&["80", "85"]).into_iter()), "80");
        assert_eq!(default_id(l(&[]).into_iter()), "default");
    }
}
