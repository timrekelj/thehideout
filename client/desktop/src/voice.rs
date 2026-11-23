//! Voice: who is in which voice channel (kept from the gateway), our own connection
//! (`VoiceSession`, mute and deafen flags) and the moderation actions. No audio happens here:
//! state changes queue [`VoiceIntent`]s, which `App::flush` hands to a [`VoiceSink`] (the audio
//! engine, wired elsewhere), the same way commands go to the backend.
//!
//! The flow, as the server defines it (`proto/README.md`, "Voice"): `JoinVoice` answers with a
//! LiveKit url and token, the engine connects with them; the server publishes `voice_state_update`
//! for everyone in the guild; a moderator's move arrives as `voice_moved`, and we call `JoinVoice`
//! again for the new channel. A stream (re)open forgets all voice states and a `voice_snapshot`
//! follows when anyone is in voice.

// The actions and selectors wait for the UI that calls them.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::{HashMap, HashSet};

use thehideout_api::{
    ApiError, Channel, ChannelKind, Permissions, VoiceJoin, VoiceState,
};
use thehideout_design::components::ToastKind;
use rune::{Key, KeyEvent, PhysicalKeyCode, Shortcut};
use serde::{Deserialize, Serialize};
use thehideout_voice_types::{AudioDevice, Quality, ScreenSource, VideoKind, VoiceEvent, VoiceMode};

use crate::backend::Command;
use crate::server_settings::error_text;
use crate::state::{AppState, Section, sentence};

/// The role editor's voice switches, listed after `PERMISSION_ROWS`. Connect, speak and video are
/// a baseline everyone has, so a switch for them would change nothing and is not offered.
pub const VOICE_PERMISSION_ROWS: [crate::server_settings::PermRow; 2] = [
    crate::server_settings::PermRow {
        bit: Permissions::MUTE_MEMBERS,
        name: "Mute members",
        help: "Server-mute people in voice channels.",
    },
    crate::server_settings::PermRow {
        bit: Permissions::MOVE_MEMBERS,
        name: "Move members",
        help: "Move people between voice channels and disconnect them.",
    },
];

/// Our own connection to a voice channel.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum VoiceSession {
    /// Not in voice (here).
    #[default]
    None,
    /// `JoinVoice(channel)` is out (also after a moderator moved us: the rejoin).
    Joining { channel: u64 },
    /// The server accepted us: the engine connects to `url` with `token`.
    Connected {
        channel: u64,
        url: String,
        token: String,
    },
}

impl VoiceSession {
    /// The channel we are joining or in.
    pub fn channel(&self) -> Option<u64> {
        match self {
            VoiceSession::None => None,
            VoiceSession::Joining { channel } | VoiceSession::Connected { channel, .. } => {
                Some(*channel)
            }
        }
    }
}

/// What the audio engine should do. Drained by the app; the engine's `set_mute` / `set_deaf`
/// take the flags as they are (deafened implies muted, already folded into `SetMute`).
#[derive(Clone, Debug, PartialEq)]
pub enum VoiceIntent {
    Connect { url: String, token: String },
    Disconnect,
    SetMute(bool),
    SetDeaf(bool),
    /// "Mute for me" on or off for one user (the engine takes it as volume 0 / 1).
    MuteUser { user: u64, muted: bool },
    /// The microphone to use (`None`: the system default). Settings, applied when the engine
    /// exists (it is built by the first connect, a mic test or a device refresh).
    SetInputDevice(Option<String>),
    SetOutputDevice(Option<String>),
    /// The camera to publish (`None`: the default). The web engine only.
    SetCameraDevice(Option<String>),
    SetMode(VoiceMode),
    /// The push-to-talk key went down or up.
    SetPtt(bool),
    SetNoiseSuppression(bool),
    SetEchoCancellation(bool),
    /// Ask the engine for the device lists (it answers with `VoiceEvent::Devices`).
    ListDevices,
    /// The settings page's mic test on or off.
    MicTest(bool),
    /// Publish the camera, or stop it.
    SetCamera(bool),
    /// Share this screen or window, or stop (`None`).
    SetScreenShare(Option<ScreenSource>),
    /// Ask the engine for the screens and windows that can be shared (`VoiceEvent::ScreenSources`).
    ListScreenSources,
    /// A tile showing `track` is visible: start delivering its frames. `mirror` flips them
    /// sideways (our own camera, shown as a mirror).
    WatchVideo { track: String, mirror: bool },
    /// No tile shows `track` any more: drop its subscription.
    UnwatchVideo { track: String },
}

/// A short cue the app plays: join and leave sounds and the mute and deafen clicks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    /// We joined a voice channel.
    Join,
    /// Someone joined the channel we are in.
    PeerJoin,
    /// Someone left the channel we are in.
    PeerLeave,
    MuteOn,
    MuteOff,
    DeafOn,
    DeafOff,
    /// We left voice, or were dropped.
    Disconnect,
}

/// Where [`VoiceIntent`]s go (the audio engine); `App::set_voice_sink` installs one.
pub trait VoiceSink {
    fn intent(&mut self, intent: VoiceIntent);
    /// Called after a batch of intents: a sink may hold work back to see the whole batch (the
    /// connect waits for the mute and deafen flags that follow it).
    fn flushed(&mut self) {}
    /// Plays a cue.
    fn sound(&mut self, _sound: Sound) {}
    /// What the engine reported since the last call.
    fn events(&mut self) -> Vec<VoiceEvent> {
        Vec::new()
    }
    /// The window's waker, rung when `events` has something.
    fn set_waker(&mut self, _waker: rune::Waker) {}
    /// The live picture of a watched video track: a `Pixels` the sink keeps replacing with the
    /// newest frame (only while a `WatchVideo` for it is in force). The view reads it.
    fn video_pixels(&self, _track: &str) -> Option<rune::Pixels> {
        None
    }
}

/// Hands the queued intents and cues to `sink` (dropped when there is none).
pub fn send_intents(state: &mut AppState, sink: Option<&mut (dyn VoiceSink + '_)>) {
    let intents = std::mem::take(&mut state.voice.intents);
    let sounds = std::mem::take(&mut state.voice.sounds_out);
    let Some(sink) = sink else { return };
    for intent in intents {
        sink.intent(intent);
    }
    for sound in sounds {
        sink.sound(sound);
    }
    sink.flushed();
}

/// Applies what the engine reported since the last call. True when something visible changed.
pub fn take_events(state: &mut AppState, sink: Option<&mut (dyn VoiceSink + '_)>) -> bool {
    let Some(sink) = sink else { return false };
    let mut changed = false;
    for ev in sink.events() {
        changed |= state.voice_event(ev);
    }
    changed
}

/// The wait before each reconnect try after the engine dropped us (seconds); when the list is
/// used up we leave voice with a message.
pub const RECONNECT_BACKOFF: [f64; 4] = [1.0, 2.0, 5.0, 10.0];

/// Where the engine's own connection stands (the voice bar's status).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum VoiceLink {
    Connecting,
    /// Also the state while no engine reports (scenes, tests).
    #[default]
    Live,
    Reconnecting,
}

/// A reconnect under way after the engine dropped us.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Retry {
    pub channel: u64,
    /// Tries made so far (an index into [`RECONNECT_BACKOFF`]).
    pub attempt: usize,
    /// When the next `JoinVoice` goes out; `None` while one is in flight.
    pub due: Option<f64>,
}

/// What the voice bar says about the connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceStatus {
    Connecting,
    Connected,
    Reconnecting,
}

/// Our own side of voice. The flags live here even when not connected, as in most chat apps:
/// they apply to the next channel joined.
pub struct VoiceLocal {
    pub session: VoiceSession,
    pub self_mute: bool,
    pub self_deaf: bool,
    /// The mute flag as it was when we deafened, restored by undeafening.
    muted_before_deaf: bool,
    /// Queued for the engine; `App::flush` drains it.
    pub intents: Vec<VoiceIntent>,
    /// Who is speaking right now (user ids); the audio engine feeds it, empty until audio exists.
    /// The sidebar rings the avatars of those listed.
    pub speaking: HashSet<u64>,
    /// People this device has muted for itself ("Mute for me"). Local only: nothing is sent to the
    /// server; the engine takes it as user volume 0 once it is wired.
    pub muted_for_me: HashSet<u64>,
    /// The pointer is over the mute button while a moderator's mute disables it (its tooltip).
    pub mute_tip: bool,
    /// The engine's connection, from its events.
    pub link: VoiceLink,
    /// Our connection quality as the engine reports it (`None` until it does).
    pub quality: Option<Quality>,
    /// A reconnect after the engine dropped us.
    pub retry: Option<Retry>,
    /// Whether the join, leave and click sounds play (the settings page switches it).
    pub sounds: bool,
    /// Cues queued for the sink; `App::flush` drains it.
    pub sounds_out: Vec<Sound>,
    /// The settings (devices, mode, keys, processing). `sounds` above is the live copy of
    /// `prefs.sounds`; [`AppState::voice_prefs`] puts them together to save.
    pub prefs: VoicePrefs,
    /// A change to `prefs` the saver has not been given yet (`App::flush` hands it over).
    pub prefs_dirty: bool,
    /// The engine's device lists, once it has answered (the page asks when it opens).
    pub devices: Option<VoiceDevices>,
    /// The settings page's mic test is on.
    pub mic_test: bool,
    /// Our microphone level as the meter draws it (0..1), from the engine's `Levels`.
    pub mic_level: f32,
    /// The push-to-talk key is held down.
    pub ptt_down: bool,
    /// Our camera is on (asked for; the engine's `VideoTrack` for us says it is publishing).
    pub camera: bool,
    /// The screen or window we share (asked for), or `None`.
    pub sharing: Option<ScreenSource>,
    /// The engine's list of what can be shared, once it has answered the picker's ask.
    pub screen_sources: Option<Vec<ScreenSource>>,
    /// The "Share your screen" picker is open.
    pub picker: bool,
    /// Who has a video track, by user id (ours included, for the preview).
    pub video_tracks: HashMap<u64, VideoTracks>,
    /// The tracks the sink was told to deliver (the visible tiles), for diffing.
    pub watching: HashSet<String>,
    /// The camera and LIVE flags last sent to the server (they follow our tracks, see `settle_video`).
    pub video_flags_sent: (bool, bool),
    /// What the main panel shows: the transcript, or the voice stage.
    pub view: MainView,
    /// Which stage tile is big.
    pub focus: Focus,
}

/// What the main panel shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MainView {
    #[default]
    Chat,
    /// The voice stage: a grid of video and avatar tiles, or one focused tile and a strip.
    Stage,
}

/// One person's video tracks (engine track ids).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VideoTracks {
    pub camera: Option<String>,
    pub screen: Option<String>,
}

/// What a stage tile shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileKind {
    Camera,
    Screen,
    /// No video: the avatar.
    Avatar,
}

/// One tile of the stage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tile {
    pub user: u64,
    pub kind: TileKind,
    /// The video track behind it (`None` for an avatar tile).
    pub track: Option<String>,
}

/// Which tile is the big one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Focus {
    /// A screen share is big when there is one, else everything is a grid.
    #[default]
    Auto,
    /// A grid, on purpose (the focused tile was clicked again).
    Grid,
    Tile(u64, TileKind),
}

/// When the microphone transmits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputMode {
    #[default]
    VoiceActivity,
    PushToTalk,
}

impl InputMode {
    pub const ALL: [InputMode; 2] = [InputMode::VoiceActivity, InputMode::PushToTalk];

    pub fn name(self) -> &'static str {
        match self {
            InputMode::VoiceActivity => "Voice activity",
            InputMode::PushToTalk => "Push to talk",
        }
    }
}

/// The default voice-activity gate, the engine's own (`VoiceMode::default`).
pub const DEFAULT_THRESHOLD: f32 = 0.35;

/// The default chord of the mute and deafen hotkeys: Cmd/Ctrl+Shift+M and +D (`mod` is ⌘ on
/// macOS and Ctrl elsewhere).
pub const DEFAULT_MUTE_KEY: &str = "mod+shift+m";
pub const DEFAULT_DEAFEN_KEY: &str = "mod+shift+d";

/// Everything on the Voice & audio page. Saved under [`crate::look::VOICE_KEY`]; unknown or missing
/// fields fall back to the defaults, so a file from another version still loads. The keys are
/// `Shortcut`s in their `Display` form (what rune says to persist); `None` is "unset".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoicePrefs {
    /// The input device's id (`None`: the system default) and the name it had when it was
    /// chosen, so the page can still name it when it is unplugged.
    pub input: Option<String>,
    pub input_name: Option<String>,
    pub output: Option<String>,
    pub output_name: Option<String>,
    /// The camera (web only), like the devices above.
    pub camera: Option<String>,
    pub camera_name: Option<String>,
    pub mode: InputMode,
    /// The voice-activity gate, 0..1 on the meter's scale.
    pub threshold: f32,
    pub ptt_key: Option<String>,
    pub mute_key: Option<String>,
    pub deafen_key: Option<String>,
    pub noise_suppression: bool,
    pub echo_cancellation: bool,
    pub sounds: bool,
}

impl Default for VoicePrefs {
    fn default() -> Self {
        VoicePrefs {
            input: None,
            input_name: None,
            output: None,
            output_name: None,
            camera: None,
            camera_name: None,
            mode: InputMode::VoiceActivity,
            threshold: DEFAULT_THRESHOLD,
            ptt_key: None,
            mute_key: Some(DEFAULT_MUTE_KEY.parse::<Shortcut>().map_or_else(|_| DEFAULT_MUTE_KEY.into(), |s| s.to_string())),
            deafen_key: Some(DEFAULT_DEAFEN_KEY.parse::<Shortcut>().map_or_else(|_| DEFAULT_DEAFEN_KEY.into(), |s| s.to_string())),
            noise_suppression: true,
            echo_cancellation: true,
            sounds: true,
        }
    }
}

/// The engine's audio devices, as `(id, name)`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VoiceDevices {
    pub inputs: Vec<(String, String)>,
    pub outputs: Vec<(String, String)>,
    /// The cameras the browser lists (web only).
    pub cameras: Vec<(String, String)>,
    /// The names of the system defaults, for "System default (name)".
    pub default_input: Option<String>,
    pub default_output: Option<String>,
    pub default_camera: Option<String>,
}

fn pairs(list: Vec<AudioDevice>) -> Vec<(String, String)> {
    list.into_iter().map(|d| (d.id, d.name)).collect()
}

impl VoiceDevices {
    fn has_input(&self, id: &str) -> bool {
        self.inputs.iter().any(|(i, _)| i == id)
    }
    fn has_output(&self, id: &str) -> bool {
        self.outputs.iter().any(|(i, _)| i == id)
    }
}

/// Which of the three hotkeys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceKey {
    PushToTalk,
    Mute,
    Deafen,
}

/// The voice engine's mode for `prefs`.
pub fn mode_of(prefs: &VoicePrefs) -> VoiceMode {
    match prefs.mode {
        InputMode::VoiceActivity => VoiceMode::VoiceActivity {
            threshold: prefs.threshold,
        },
        InputMode::PushToTalk => VoiceMode::PushToTalk,
    }
}

/// The name of the key a shortcut is on, as `Shortcut`'s `Display` ends it ("m", "f13", "space").
fn key_name(shortcut: &Shortcut) -> String {
    let text = shortcut.to_string();
    text.rsplit('+').next().unwrap_or("").to_owned()
}

/// Whether `ev` is `shortcut`'s key (modifiers aside). The physical key says it when the event
/// has one (it names the key by where it sits, whatever the layout and Shift make it type); an
/// event without one (a script, a test) is matched by what it types.
fn same_key(shortcut: &Shortcut, ev: &KeyEvent, physical: Option<PhysicalKeyCode>) -> bool {
    if let Some(p) = physical {
        return p == shortcut.physical_key();
    }
    let name = key_name(shortcut);
    match &ev.key {
        Key::Command(c) => name.len() == 1 && name.starts_with(c.to_ascii_lowercase()),
        Key::Text(t) if t == " " => name == "space",
        Key::Text(t) => name.len() == 1 && name.eq_ignore_ascii_case(t),
        Key::Function(n) => name == format!("f{n}"),
        Key::Enter => name == "enter",
        Key::Tab => name == "tab",
        Key::Escape => name == "escape",
        Key::Backspace => name == "backspace",
        Key::Delete => name == "delete",
        Key::Insert => name == "insert",
        Key::Home => name == "home",
        Key::End => name == "end",
        Key::PageUp => name == "pageup",
        Key::PageDown => name == "pagedown",
        Key::Left => name == "left",
        Key::Right => name == "right",
        Key::Up => name == "up",
        Key::Down => name == "down",
        _ => false,
    }
}

/// Whether `ev` is exactly `shortcut`: its key with its modifiers, no others.
fn chord_matches(shortcut: &Shortcut, ev: &KeyEvent, physical: Option<PhysicalKeyCode>) -> bool {
    let (want, held) = (shortcut.modifiers(), ev.mods);
    // Off macOS the Super key is its own modifier; on macOS it is the Command key `cmd` covers.
    let logo = cfg!(target_os = "macos") || want.logo == held.logo;
    want.shift == held.shift
        && want.alt == held.alt
        && want.cmd == held.cmd
        && want.ctrl == held.ctrl
        && logo
        && same_key(shortcut, ev, physical)
}

impl Default for VoiceLocal {
    fn default() -> Self {
        VoiceLocal {
            session: VoiceSession::None,
            self_mute: false,
            self_deaf: false,
            muted_before_deaf: false,
            intents: Vec::new(),
            speaking: HashSet::new(),
            muted_for_me: HashSet::new(),
            mute_tip: false,
            link: VoiceLink::Live,
            quality: None,
            retry: None,
            sounds: true,
            sounds_out: Vec::new(),
            prefs: VoicePrefs::default(),
            prefs_dirty: false,
            devices: None,
            mic_test: false,
            mic_level: 0.0,
            ptt_down: false,
            camera: false,
            sharing: None,
            screen_sources: None,
            picker: false,
            video_tracks: HashMap::new(),
            watching: HashSet::new(),
            video_flags_sent: (false, false),
            view: MainView::Chat,
            focus: Focus::Auto,
        }
    }
}

/// The text for a failed voice call.
fn voice_error_text(e: &ApiError) -> String {
    match e {
        ApiError::Network(m) if m.contains("not configured") => {
            "Voice isn't set up on this server.".to_owned()
        }
        ApiError::Precondition(m) => sentence(m),
        other => error_text(other),
    }
}

/// Whether this build can take part in voice (audio: the desktop app and the web client).
pub const fn available() -> bool {
    true
}

/// Whether this build can send and watch video and share the screen.
pub const fn video_available() -> bool {
    true
}

/// Whether sharing asks which screen or window in the app's own picker (the desktop); the browser
/// has its own, opened by the share request itself.
pub const fn app_screen_picker() -> bool {
    !cfg!(target_arch = "wasm32")
}

/// Said when a voice channel is opened where [`available`] is false.
pub const VOICE_UNAVAILABLE: &str = "Voice is available in the desktop app for now.";

impl AppState {
    // ---- queries -----------------------------------------------------------------------------

    fn find_channel(&self, id: u64) -> Option<&Channel> {
        self.data.channels.values().flatten().find(|c| c.id == id)
    }

    /// Everyone in `channel`, in the order they joined (ties by user id).
    pub fn voice_participants(&self, channel: u64) -> Vec<&VoiceState> {
        let Some(c) = self.find_channel(channel) else {
            return Vec::new();
        };
        let mut v: Vec<&VoiceState> = self
            .data
            .voice
            .get(&c.guild_id)
            .map(|m| m.values().filter(|s| s.channel_id == channel).collect())
            .unwrap_or_default();
        v.sort_by_key(|s| (s.joined_at, s.user_id));
        v
    }

    /// The voice channel we are in or joining on this device.
    pub fn my_voice_channel(&self) -> Option<u64> {
        self.voice.session.channel()
    }

    /// The names the voice bar shows: the channel we are in or joining, and its server.
    pub fn voice_channel_label(&self) -> Option<(String, String)> {
        let c = self.find_channel(self.voice.session.channel()?)?;
        let guild = self.guild(c.guild_id).map_or_else(String::new, |g| g.name.clone());
        Some((c.name.clone(), guild))
    }

    /// `user`'s voice state, wherever they are.
    pub fn voice_state_of(&self, user: u64) -> Option<&VoiceState> {
        self.data.voice.values().find_map(|m| m.get(&user))
    }

    /// Whether a moderator muted us (we can still toggle our own flag, but nothing is sent).
    pub fn i_am_server_muted(&self) -> bool {
        self.voice_state_of(self.my_id())
            .is_some_and(|s| s.server_mute)
    }

    /// Whether we hold `bit` in `guild` and outrank `user` there (or are `user`): the server's
    /// moderation rule.
    pub(crate) fn can_moderate(&self, guild: u64, user: u64, bit: Permissions) -> bool {
        let Some(g) = self.guild(guild) else {
            return false;
        };
        if !self.perms(guild).contains(bit) {
            return false;
        }
        let me = self.my_id();
        if user == me {
            // The server skips the hierarchy for yourself.
            return true;
        }
        let roles = self.data.roles.get(&guild).map_or(&[][..], Vec::as_slice);
        let held = |u: u64| {
            self.data
                .member_roles
                .get(&guild)
                .and_then(|m| m.get(&u))
                .map_or(&[][..], Vec::as_slice)
        };
        crate::perms::outranks_member(g, me, held(me), user, held(user), roles)
    }

    /// Whether the server-mute control is offered for `user` (who is in voice): MUTE_MEMBERS and
    /// a higher top role; never yourself.
    pub fn can_server_mute(&self, user: u64) -> bool {
        user != self.my_id()
            && self
                .voice_state_of(user)
                .is_some_and(|s| self.can_moderate(s.guild_id, user, Permissions::MUTE_MEMBERS))
    }

    /// Whether moving or disconnecting `user` (who is in voice) is offered: MOVE_MEMBERS and a
    /// higher top role (yourself included, as on the server).
    pub fn can_move_member(&self, user: u64) -> bool {
        self.voice_state_of(user)
            .is_some_and(|s| self.can_moderate(s.guild_id, user, Permissions::MOVE_MEMBERS))
    }

    /// Whether `user` is speaking (always false until the engine feeds `voice.speaking`).
    pub fn is_speaking(&self, user: u64) -> bool {
        self.voice.speaking.contains(&user) || (user == self.my_id() && self.ptt_transmitting())
    }

    /// Whether we muted `user` for ourselves.
    pub fn is_muted_for_me(&self, user: u64) -> bool {
        self.voice.muted_for_me.contains(&user)
    }

    /// "Mute for me" / "Unmute for me": a local flag, nobody else notices. Not for ourselves.
    pub fn toggle_muted_for_me(&mut self, user: u64) {
        if user == self.my_id() {
            return;
        }
        if !self.voice.muted_for_me.remove(&user) {
            self.voice.muted_for_me.insert(user);
        }
        let muted = self.voice.muted_for_me.contains(&user);
        self.push_intent(VoiceIntent::MuteUser { user, muted });
    }

    /// Replaces who is speaking (the engine's report).
    pub fn set_speaking(&mut self, users: impl IntoIterator<Item = u64>) {
        self.voice.speaking = users.into_iter().collect();
    }

    // ---- our own connection ------------------------------------------------------------------

    fn push_intent(&mut self, intent: VoiceIntent) {
        self.voice.intents.push(intent);
    }

    /// Queues a cue, unless sounds are off.
    fn play(&mut self, sound: Sound) {
        if self.voice.sounds {
            self.voice.sounds_out.push(sound);
        }
    }

    /// What the microphone should do: muted by us or by a moderator.
    fn mic_muted(&self) -> bool {
        self.voice.self_mute || self.i_am_server_muted()
    }

    /// Joins a voice channel of one of our servers; joining another one first leaves the current
    /// (the server moves us, the engine reconnects). Joining the channel we are already in is a
    /// no-op. Text channels and unknown ids are ignored.
    pub fn join_voice(&mut self, channel: u64) {
        let Some(c) = self.find_channel(channel) else {
            return;
        };
        if c.kind != ChannelKind::Voice || self.voice.session.channel() == Some(channel) {
            return;
        }
        let guild = c.guild_id;
        // The join click is the user gesture that lets the page play audio.
        #[cfg(target_arch = "wasm32")]
        crate::voice_web::unlock_audio();
        if !available() {
            self.toast(ToastKind::Info, VOICE_UNAVAILABLE);
            return;
        }
        if !self.perms(guild).contains(Permissions::CONNECT) {
            self.toast(ToastKind::Danger, "You can't join voice channels here.");
            return;
        }
        if matches!(self.voice.session, VoiceSession::Connected { .. }) {
            self.push_intent(VoiceIntent::Disconnect);
        }
        self.voice.session = VoiceSession::Joining { channel };
        self.outbox.push(Command::JoinVoice { channel });
    }

    /// Leaves voice (a no-op when in none).
    pub fn leave_voice(&mut self) {
        let was = std::mem::take(&mut self.voice.session);
        if was == VoiceSession::None {
            return;
        }
        self.voice.retry = None;
        self.voice.speaking.clear();
        self.release_ptt();
        self.reset_video(true);
        if matches!(was, VoiceSession::Connected { .. }) {
            self.push_intent(VoiceIntent::Disconnect);
        }
        self.play(Sound::Disconnect);
        self.outbox.push(Command::LeaveVoice);
    }

    /// Sends our flags to the server and the engine, once connected.
    fn flags_changed(&mut self) {
        let (mute, deaf) = (self.voice.self_mute, self.voice.self_deaf);
        if matches!(self.voice.session, VoiceSession::Connected { .. }) {
            let heard = self.mic_muted();
            self.push_intent(VoiceIntent::SetMute(heard));
            self.push_intent(VoiceIntent::SetDeaf(deaf));
            self.outbox.push(Command::UpdateVoiceState {
                self_mute: mute,
                self_deaf: deaf,
            });
        }
    }

    /// The click for a flag change, from the flags before it.
    fn flags_sound(&mut self, was: (bool, bool)) {
        let now = (self.voice.self_mute, self.voice.self_deaf);
        if now.1 != was.1 {
            self.play(if now.1 { Sound::DeafOn } else { Sound::DeafOff });
        } else if now.0 != was.0 {
            self.play(if now.0 { Sound::MuteOn } else { Sound::MuteOff });
        }
    }

    /// Mutes or unmutes the microphone. Unmuting while deafened undeafens too.
    pub fn toggle_mute(&mut self) {
        let was = (self.voice.self_mute, self.voice.self_deaf);
        let v = &mut self.voice;
        if v.self_deaf {
            v.self_deaf = false;
            v.self_mute = false;
        } else {
            v.self_mute = !v.self_mute;
        }
        self.flags_sound(was);
        self.flags_changed();
    }

    /// Deafens or undeafens. Deafening mutes; undeafening restores the mute we had before.
    pub fn toggle_deaf(&mut self) {
        let was = (self.voice.self_mute, self.voice.self_deaf);
        let v = &mut self.voice;
        if v.self_deaf {
            v.self_deaf = false;
            v.self_mute = v.muted_before_deaf;
        } else {
            v.muted_before_deaf = v.self_mute;
            v.self_deaf = true;
            v.self_mute = true;
        }
        self.flags_sound(was);
        self.flags_changed();
    }

    // ---- moderation --------------------------------------------------------------------------

    /// Server-mutes or unmutes `user`, who is in voice. Ignored unless the control is offered.
    pub fn server_mute(&mut self, user: u64, muted: bool) {
        if !self.can_server_mute(user) {
            return;
        }
        if let Some(s) = self.voice_state_of(user) {
            let guild = s.guild_id;
            self.outbox.push(Command::ServerMuteMember { guild, user, muted });
        }
    }

    /// Moves `user`, who is in voice, to another voice channel of the same server. Ignored unless
    /// allowed, or when they are already there.
    pub fn move_member(&mut self, user: u64, channel: u64) {
        let Some(s) = self.voice_state_of(user) else {
            return;
        };
        let guild = s.guild_id;
        let same = s.channel_id == channel;
        let voice_here = self
            .channel(guild, channel)
            .is_some_and(|c| c.kind == ChannelKind::Voice);
        if same || !voice_here || !self.can_move_member(user) {
            return;
        }
        self.outbox.push(Command::MoveMember { guild, user, channel });
    }

    /// Removes `user` from their voice channel. Ignored unless allowed.
    pub fn disconnect_member(&mut self, user: u64) {
        if !self.can_move_member(user) {
            return;
        }
        if let Some(s) = self.voice_state_of(user) {
            let guild = s.guild_id;
            self.outbox.push(Command::DisconnectMember { guild, user });
        }
    }

    // ---- replies and events (see apply.rs) ---------------------------------------------------

    /// `JoinVoice` answered.
    pub fn voice_joined(&mut self, channel: u64, result: Result<VoiceJoin, ApiError>) {
        // Only the join we are still waiting for counts: a newer one or a leave overtook it.
        if self.voice.session != (VoiceSession::Joining { channel }) {
            return;
        }
        match result {
            Ok(j) => {
                self.voice.session = VoiceSession::Connected {
                    channel,
                    url: j.url.clone(),
                    token: j.token.clone(),
                };
                self.voice.link = if self.voice.retry.is_some() {
                    VoiceLink::Reconnecting
                } else {
                    VoiceLink::Live
                };
                self.voice.quality = None;
                let (mute, deaf) = (self.voice.self_mute, self.voice.self_deaf);
                let heard = self.mic_muted();
                self.push_intent(VoiceIntent::Connect {
                    url: j.url,
                    token: j.token,
                });
                self.push_intent(VoiceIntent::SetMute(heard));
                self.push_intent(VoiceIntent::SetDeaf(deaf));
                // A new connection has forgotten who this device muted.
                let mut muted: Vec<u64> = self.voice.muted_for_me.iter().copied().collect();
                muted.sort_unstable();
                for user in muted {
                    self.push_intent(VoiceIntent::MuteUser { user, muted: true });
                }
                // The server starts us unmuted (a same-server move keeps the old flags): tell it
                // what we really are.
                let theirs = self
                    .voice_state_of(self.my_id())
                    .map_or((false, false), |s| (s.self_mute, s.self_deaf));
                if theirs != (mute, deaf) {
                    self.outbox.push(Command::UpdateVoiceState {
                        self_mute: mute,
                        self_deaf: deaf,
                    });
                }
                // A new connection publishes no video yet: a camera or LIVE flag left from the
                // old one (a same-channel rejoin keeps them) is stale.
                if self
                    .voice_state_of(self.my_id())
                    .is_some_and(|s| s.self_video || s.self_stream)
                {
                    self.outbox.push(Command::UpdateVideoState {
                        self_video: Some(false),
                        self_stream: Some(false),
                    });
                }
            }
            // A failed try of a reconnect goes on with the backoff.
            Err(_) if self.voice.retry.is_some() => self.retry_later(channel),
            Err(e) => {
                self.voice.session = VoiceSession::None;
                self.toast(ToastKind::Danger, voice_error_text(&e));
            }
        }
    }

    /// A failed `UpdateVoiceState`, `LeaveVoice` or moderation call.
    pub fn voice_failed(&mut self, e: &ApiError) {
        self.toast(ToastKind::Danger, voice_error_text(e));
    }

    /// The stream (re)opened: the server's view is unknown until a `voice_snapshot` arrives.
    pub fn clear_voice(&mut self) {
        self.data.voice.clear();
    }

    /// `voice_state_update`: `None` = the user left voice in `guild`.
    pub fn voice_update(&mut self, guild: u64, user: u64, state: Option<VoiceState>) {
        let me = self.my_id();
        let mine = match &self.voice.session {
            VoiceSession::Connected { channel, .. } => Some(*channel),
            _ => None,
        };
        let was_here = mine.is_some() && self.voice_state_of(user).map(|s| s.channel_id) == mine;
        let was_muted = self.i_am_server_muted();
        if state.is_some() {
            // A user is in one voice channel at a time, so a state in another guild is stale.
            for (g, users) in &mut self.data.voice {
                if *g != guild {
                    users.remove(&user);
                }
            }
        }
        let now_here = state.as_ref().is_some_and(|s| Some(s.channel_id) == mine);
        match state {
            Some(st) => {
                self.data.voice.entry(guild).or_default().insert(user, st);
            }
            None => {
                if let Some(users) = self.data.voice.get_mut(&guild) {
                    users.remove(&user);
                }
                // Removed from the channel we are connected to (disconnected by a moderator, the
                // join window ran out, left on another device): drop the connection.
                if user == me {
                    self.dropped_from_voice(guild);
                }
            }
        }
        if user == me {
            self.sync_server_mute(was_muted);
        } else if !self.voice.self_deaf {
            if now_here && !was_here {
                self.play(Sound::PeerJoin);
            } else if was_here && !now_here {
                self.play(Sound::PeerLeave);
            }
        }
    }

    /// A moderator's mute (or its lifting) reaches the engine whatever our own flag says.
    fn sync_server_mute(&mut self, was: bool) {
        if was != self.i_am_server_muted()
            && matches!(self.voice.session, VoiceSession::Connected { .. })
        {
            let heard = self.mic_muted();
            self.push_intent(VoiceIntent::SetMute(heard));
        }
    }

    /// `voice_snapshot`: replaces the voice states of the listed guilds.
    pub fn voice_snapshot(&mut self, guilds: &[u64], states: Vec<VoiceState>) {
        let was_muted = self.i_am_server_muted();
        let mut by_guild: HashMap<u64, HashMap<u64, VoiceState>> =
            guilds.iter().map(|g| (*g, HashMap::new())).collect();
        for st in states {
            by_guild.entry(st.guild_id).or_default().insert(st.user_id, st);
        }
        for (g, users) in by_guild {
            self.data.voice.insert(g, users);
        }
        let me = self.my_id();
        if let VoiceSession::Connected { channel, .. } = &self.voice.session
            && let Some(g) = self.find_channel(*channel).map(|c| c.guild_id)
            && guilds.contains(&g)
            && self.data.voice.get(&g).is_none_or(|u| !u.contains_key(&me))
        {
            self.dropped_from_voice(g);
        }
        self.sync_server_mute(was_muted);
    }

    /// The guild is gone from our lists (left, removed, deleted): its voice states go, and the
    /// connection too when it was to one of its channels.
    pub fn drop_guild_voice(&mut self, guild: u64) {
        self.dropped_from_voice(guild);
        self.data.voice.remove(&guild);
    }

    /// We are no longer in a voice channel of `guild` according to the server.
    fn dropped_from_voice(&mut self, guild: u64) {
        let VoiceSession::Connected { channel, .. } = &self.voice.session else {
            return;
        };
        if self.find_channel(*channel).map(|c| c.guild_id) != Some(guild) {
            return;
        }
        self.voice.session = VoiceSession::None;
        self.voice.retry = None;
        self.voice.speaking.clear();
        self.release_ptt();
        self.reset_video(true);
        self.push_intent(VoiceIntent::Disconnect);
        self.play(Sound::Disconnect);
        self.toast(ToastKind::Info, "You were disconnected from voice.");
    }

    /// `voice_moved`: a moderator moved us. The server closed the old connection; join the new
    /// channel. Ignored when this device is not in voice.
    pub fn voice_moved(&mut self, channel: u64) {
        if self.voice.session == VoiceSession::None {
            return;
        }
        if matches!(self.voice.session, VoiceSession::Connected { .. }) {
            self.push_intent(VoiceIntent::Disconnect);
        }
        self.voice.retry = None;
        self.reset_video(false);
        self.voice.session = VoiceSession::Joining { channel };
        self.outbox.push(Command::JoinVoice { channel });
    }

    // ---- the engine's reports (see voice_engine.rs) ------------------------------------------

    /// What the voice bar shows, `None` outside voice.
    pub fn voice_status(&self) -> Option<VoiceStatus> {
        let v = &self.voice;
        match v.session {
            VoiceSession::None => None,
            VoiceSession::Joining { .. } if v.retry.is_some() => Some(VoiceStatus::Reconnecting),
            VoiceSession::Joining { .. } => Some(VoiceStatus::Connecting),
            VoiceSession::Connected { .. } => Some(match v.link {
                VoiceLink::Live => VoiceStatus::Connected,
                VoiceLink::Reconnecting => VoiceStatus::Reconnecting,
                VoiceLink::Connecting if v.retry.is_some() => VoiceStatus::Reconnecting,
                VoiceLink::Connecting => VoiceStatus::Connecting,
            }),
        }
    }

    /// One report of the engine. Returns whether something visible changed. Reports about a
    /// connection we no longer want (no session) are stale and ignored.
    pub fn voice_event(&mut self, ev: VoiceEvent) -> bool {
        let connected = matches!(self.voice.session, VoiceSession::Connected { .. });
        match ev {
            VoiceEvent::Connecting if connected => {
                self.voice.link = VoiceLink::Connecting;
                true
            }
            VoiceEvent::Connected if connected => {
                if self.voice.link == VoiceLink::Connecting && self.voice.retry.is_none() {
                    self.play(Sound::Join);
                }
                self.voice.link = VoiceLink::Live;
                self.voice.retry = None;
                true
            }
            VoiceEvent::Reconnecting if connected => {
                self.voice.link = VoiceLink::Reconnecting;
                true
            }
            VoiceEvent::Disconnected { .. } if connected => {
                // The engine gave up on its own (its network reconnect failed, or the room was
                // closed on us): what a deliberate leave does is already done by the time its
                // report arrives, so this is a drop. Try again with a fresh token.
                let Some(channel) = self.voice.session.channel() else {
                    return false;
                };
                self.voice.link = VoiceLink::Reconnecting;
                self.voice.quality = None;
                self.voice.speaking.clear();
                // The room is gone, and our camera and share with it.
                self.reset_video(false);
                self.play(Sound::Disconnect);
                if self.voice.retry.is_none() {
                    self.toast(ToastKind::Info, "Disconnected from voice. Reconnecting\u{2026}");
                }
                self.retry_later(channel);
                true
            }
            VoiceEvent::Speaking { identity, speaking } if connected => {
                let Ok(user) = identity.parse::<u64>() else {
                    return false;
                };
                if speaking {
                    self.voice.speaking.insert(user)
                } else {
                    self.voice.speaking.remove(&user)
                }
            }
            VoiceEvent::ConnectionQuality { identity, quality }
                if connected && identity == self.my_id().to_string() =>
            {
                let changed = self.voice.quality != Some(quality);
                self.voice.quality = Some(quality);
                changed
            }
            VoiceEvent::Devices {
                inputs,
                outputs,
                default_input,
                default_output,
            } => {
                self.devices_listed(inputs, outputs, default_input, default_output);
                true
            }
            VoiceEvent::Cameras { cameras, default } => {
                self.cameras_listed(cameras, default);
                true
            }
            VoiceEvent::Levels(batch) => self.mic_levels(batch),
            VoiceEvent::VideoTrack {
                identity,
                source,
                track_id,
            } if connected => self.video_track(&identity, source, track_id),
            VoiceEvent::VideoTrackEnded { identity, track_id } if connected => {
                self.video_track_ended(&identity, &track_id)
            }
            VoiceEvent::ScreenSources(list) => {
                self.voice.screen_sources = Some(list);
                self.voice.picker
            }
            // The mic test could not open the microphone: switch it off and say why.
            VoiceEvent::Error(message) if self.voice.mic_test => {
                self.voice.mic_test = false;
                self.voice.mic_level = 0.0;
                self.toast(ToastKind::Danger, sentence(&message));
                true
            }
            VoiceEvent::Error(message) if self.voice.session != VoiceSession::None => {
                // A camera or share that never started (no camera support, permission refused,
                // the window is gone) is switched back off, so its button tells the truth.
                self.revert_unpublished_video();
                self.toast(ToastKind::Danger, sentence(&message));
                true
            }
            _ => false,
        }
    }

    /// Schedules the next reconnect try (`JoinVoice` again, for a fresh token) after the wait the
    /// backoff says, or leaves voice when the tries are used up.
    fn retry_later(&mut self, channel: u64) {
        let attempt = self.voice.retry.map_or(0, |r| r.attempt);
        let Some(wait) = RECONNECT_BACKOFF.get(attempt) else {
            self.voice.retry = None;
            self.voice.session = VoiceSession::None;
            self.voice.link = VoiceLink::Live;
            self.reset_video(true);
            self.push_intent(VoiceIntent::Disconnect);
            self.outbox.push(Command::LeaveVoice);
            self.toast(ToastKind::Danger, "Couldn't reconnect to voice.");
            return;
        };
        self.voice.retry = Some(Retry {
            channel,
            attempt: attempt + 1,
            due: Some(self.clock + wait),
        });
    }

    /// Time-driven voice state, called from `tick`: starts a due reconnect try. Returns whether
    /// something visible changed and when to be woken next.
    pub fn voice_tick(&mut self, clock: f64) -> (bool, Option<f64>) {
        let Some(r) = self.voice.retry else {
            return (false, None);
        };
        // Left, moved or dropped meanwhile: nothing to retry.
        if self.voice.session.channel() != Some(r.channel) {
            self.voice.retry = None;
            return (false, None);
        }
        match r.due {
            Some(due) if clock >= due => {
                self.voice.retry = Some(Retry { due: None, ..r });
                self.voice.session = VoiceSession::Joining { channel: r.channel };
                self.outbox.push(Command::JoinVoice { channel: r.channel });
                (true, None)
            }
            Some(due) => (false, Some(due)),
            None => (false, None),
        }
    }
}

// ---- settings: devices, mode, keys, the mic test, hotkeys ----------------------------------------

/// The recorders of the three hotkeys on the Voice & audio page, built from the prefs when the page
/// opens (a recorder keeps its own copy of its chord while it captures).
#[derive(Clone, Debug, Default)]
pub struct VoiceKeys {
    pub ptt: rune_kit::ShortcutRecorderState,
    pub mute: rune_kit::ShortcutRecorderState,
    pub deafen: rune_kit::ShortcutRecorderState,
}

impl VoiceKeys {
    pub fn of(prefs: &VoicePrefs) -> VoiceKeys {
        let parse = |k: &Option<String>| k.as_deref().and_then(|s| s.parse::<Shortcut>().ok());
        VoiceKeys {
            ptt: rune_kit::ShortcutRecorderState::new(parse(&prefs.ptt_key)),
            mute: rune_kit::ShortcutRecorderState::new(parse(&prefs.mute_key)),
            deafen: rune_kit::ShortcutRecorderState::new(parse(&prefs.deafen_key)),
        }
    }

    /// A shortcut is being recorded: keys belong to the recorder.
    pub fn recording(&self) -> bool {
        self.ptt.is_recording() || self.mute.is_recording() || self.deafen.is_recording()
    }
}

/// Which device list's menu is open on the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum VoiceMenu {
    #[default]
    None,
    Input,
    Output,
    Camera,
}

/// The first option of both device lists.
pub const SYSTEM_DEFAULT: &str = "System default";

/// One device option on the page: its label and the id behind it (`None`: the system default).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceOption {
    pub label: String,
    pub id: Option<String>,
}

/// The options of one list: "System default" (with the default's name once known), the devices,
/// and, when the saved device is not among them, that device by its saved name marked
/// "(unavailable)". Returns the options and the index of the selected one.
pub fn device_options(
    devices: Option<&[(String, String)]>,
    default_name: Option<&str>,
    saved: Option<(&str, Option<&str>)>,
) -> (Vec<DeviceOption>, usize) {
    let mut out = vec![DeviceOption {
        label: match default_name {
            Some(n) => format!("{SYSTEM_DEFAULT} ({n})"),
            None => SYSTEM_DEFAULT.to_owned(),
        },
        id: None,
    }];
    for (id, name) in devices.unwrap_or(&[]) {
        out.push(DeviceOption {
            label: name.clone(),
            id: Some(id.clone()),
        });
    }
    let mut selected = 0;
    if let Some((id, name)) = saved {
        match out.iter().position(|o| o.id.as_deref() == Some(id)) {
            Some(i) => selected = i,
            // Only a list the engine really answered can say the device is gone.
            None if devices.is_some() => {
                out.push(DeviceOption {
                    label: format!("{} (unavailable)", name.unwrap_or(id)),
                    id: Some(id.to_owned()),
                });
                selected = out.len() - 1;
            }
            None => {}
        }
    }
    (out, selected)
}

impl AppState {
    /// The prefs as they are saved: `sounds` lives in `voice.sounds`.
    pub fn voice_prefs(&self) -> VoicePrefs {
        VoicePrefs {
            sounds: self.voice.sounds,
            ..self.voice.prefs.clone()
        }
    }

    /// Takes the saved prefs (at startup) and queues them for the engine, which applies them when
    /// it is built.
    pub fn load_voice_prefs(&mut self, prefs: VoicePrefs) {
        self.voice.sounds = prefs.sounds;
        self.voice.prefs = prefs;
        self.voice.prefs_dirty = false;
        self.queue_voice_settings();
    }

    /// Every setting as an intent, in the order the engine wants them.
    fn queue_voice_settings(&mut self) {
        let p = self.voice.prefs.clone();
        let input = self.effective_input();
        let output = self.effective_output();
        self.push_intent(VoiceIntent::SetInputDevice(input));
        self.push_intent(VoiceIntent::SetOutputDevice(output));
        if p.camera.is_some() {
            let camera = self.effective_camera();
            self.push_intent(VoiceIntent::SetCameraDevice(camera));
        }
        self.push_intent(VoiceIntent::SetMode(mode_of(&p)));
        self.push_intent(VoiceIntent::SetNoiseSuppression(p.noise_suppression));
        self.push_intent(VoiceIntent::SetEchoCancellation(p.echo_cancellation));
    }

    /// The input the engine should use: the saved one, or the default when the engine has
    /// listed devices and the saved one is not there (it stays saved, for when it is back).
    pub fn effective_input(&self) -> Option<String> {
        let id = self.voice.prefs.input.as_ref()?;
        match &self.voice.devices {
            Some(d) if !d.has_input(id) => None,
            _ => Some(id.clone()),
        }
    }

    pub fn effective_output(&self) -> Option<String> {
        let id = self.voice.prefs.output.as_ref()?;
        match &self.voice.devices {
            Some(d) if !d.has_output(id) => None,
            _ => Some(id.clone()),
        }
    }

    /// The camera the engine should use: the saved one, or the default when the browser listed
    /// cameras and the saved one is not there.
    pub fn effective_camera(&self) -> Option<String> {
        let id = self.voice.prefs.camera.as_ref()?;
        match &self.voice.devices {
            Some(d) if !d.cameras.is_empty() && !d.cameras.iter().any(|(i, _)| i == id) => None,
            _ => Some(id.clone()),
        }
    }

    fn cameras_listed(&mut self, cameras: Vec<AudioDevice>, default: Option<String>) {
        let cameras = pairs(cameras);
        let default_camera = default.and_then(|id| cameras.iter().find(|(i, _)| *i == id).map(|(_, n)| n.clone()));
        let saved = self.voice.prefs.camera.clone().and_then(|id| cameras.iter().find(|(i, _)| *i == id).map(|(_, n)| n.clone()));
        let devices = self.voice.devices.get_or_insert_with(VoiceDevices::default);
        devices.cameras = cameras;
        devices.default_camera = default_camera;
        if saved.is_some() {
            self.edit_voice_prefs(|p| p.camera_name = saved);
        }
        let camera = self.effective_camera();
        self.push_intent(VoiceIntent::SetCameraDevice(camera));
    }

    /// Chooses the camera (`None`: the browser's default).
    pub fn voice_set_camera(&mut self, id: Option<String>) {
        let name = id.as_ref().and_then(|i| {
            let d = self.voice.devices.as_ref()?;
            d.cameras.iter().find(|(x, _)| x == i).map(|(_, n)| n.clone())
        });
        let name = name.or_else(|| id.as_ref().and(self.voice.prefs.camera_name.clone()));
        let changed = self.edit_voice_prefs(|p| {
            p.camera = id;
            p.camera_name = name;
        });
        if changed {
            let camera = self.effective_camera();
            self.push_intent(VoiceIntent::SetCameraDevice(camera));
        }
    }

    /// Applies `f` to the prefs; a real change is queued for the saver. True when it changed.
    fn edit_voice_prefs(&mut self, f: impl FnOnce(&mut VoicePrefs)) -> bool {
        let before = self.voice.prefs.clone();
        f(&mut self.voice.prefs);
        let changed = self.voice.prefs != before;
        if changed {
            self.voice.prefs_dirty = true;
        }
        changed
    }

    fn devices_listed(
        &mut self,
        inputs: Vec<AudioDevice>,
        outputs: Vec<AudioDevice>,
        default_input: Option<String>,
        default_output: Option<String>,
    ) {
        let (inputs, outputs) = (pairs(inputs), pairs(outputs));
        let name_of = |list: &[(String, String)], id: Option<String>| {
            let id = id?;
            list.iter().find(|(i, _)| *i == id).map(|(_, n)| n.clone())
        };
        let devices = VoiceDevices {
            default_input: name_of(&inputs, default_input),
            default_output: name_of(&outputs, default_output),
            inputs,
            outputs,
            // The cameras come in their own report.
            ..self.voice.devices.clone().unwrap_or_default()
        };
        // A saved device that is here again is named as it is called now.
        let name = |list: &[(String, String)], id: &Option<String>| {
            let id = id.as_ref()?;
            list.iter().find(|(i, _)| i == id).map(|(_, n)| n.clone())
        };
        let (iname, oname) = (
            name(&devices.inputs, &self.voice.prefs.input),
            name(&devices.outputs, &self.voice.prefs.output),
        );
        self.edit_voice_prefs(|p| {
            if iname.is_some() {
                p.input_name = iname;
            }
            if oname.is_some() {
                p.output_name = oname;
            }
        });
        self.voice.devices = Some(devices);
        // A saved device that is gone falls back to the default; one that is back is used again.
        let input = self.effective_input();
        let output = self.effective_output();
        self.push_intent(VoiceIntent::SetInputDevice(input));
        self.push_intent(VoiceIntent::SetOutputDevice(output));
    }

    /// Our own level from a `Levels` batch (the microphone is the identity `""` outside a room, our
    /// id inside one). Rises at once and falls by halves, and the final all-zero batch snaps it to
    /// silence. Only while someone looks (the mic test, or the page open): it would otherwise
    /// redraw the window 15 times a second all through a call.
    fn mic_levels(&mut self, batch: Vec<(String, f32)>) -> bool {
        if !self.voice.mic_test && self.ui.settings != Some(Section::Voice) {
            return false;
        }
        let me = self.my_id().to_string();
        let Some(level) = batch
            .iter()
            .find(|(id, _)| id.is_empty() || *id == me)
            .map(|(_, l)| l.clamp(0.0, 1.0))
        else {
            return false;
        };
        let was = self.voice.mic_level;
        self.voice.mic_level = if level >= was {
            level
        } else if level == 0.0 {
            0.0
        } else {
            (was + level) / 2.0
        };
        (self.voice.mic_level - was).abs() > 0.005
    }

    pub fn open_voice_settings(&mut self) {
        self.open_settings();
        self.ui.settings = Some(Section::Voice);
        self.ui.voice_keys = VoiceKeys::of(&self.voice.prefs);
        self.ui.voice_menu = VoiceMenu::None;
        self.push_intent(VoiceIntent::ListDevices);
    }

    /// Chooses the input device (`None`: the system default).
    pub fn voice_set_input(&mut self, id: Option<String>) {
        let name = id.as_ref().and_then(|i| {
            let d = self.voice.devices.as_ref()?;
            d.inputs.iter().find(|(x, _)| x == i).map(|(_, n)| n.clone())
        });
        // Choosing the unavailable entry again keeps its saved name.
        let name = name.or_else(|| id.as_ref().and(self.voice.prefs.input_name.clone()));
        let changed = self.edit_voice_prefs(|p| {
            p.input = id;
            p.input_name = name;
        });
        if changed {
            let input = self.effective_input();
            self.push_intent(VoiceIntent::SetInputDevice(input));
        }
    }

    pub fn voice_set_output(&mut self, id: Option<String>) {
        let name = id.as_ref().and_then(|i| {
            let d = self.voice.devices.as_ref()?;
            d.outputs.iter().find(|(x, _)| x == i).map(|(_, n)| n.clone())
        });
        let name = name.or_else(|| id.as_ref().and(self.voice.prefs.output_name.clone()));
        let changed = self.edit_voice_prefs(|p| {
            p.output = id;
            p.output_name = name;
        });
        if changed {
            let output = self.effective_output();
            self.push_intent(VoiceIntent::SetOutputDevice(output));
        }
    }

    pub fn voice_set_mode(&mut self, mode: InputMode) {
        if self.edit_voice_prefs(|p| p.mode = mode) {
            // A key held while the mode changes away from push to talk must not keep sending.
            if mode != InputMode::PushToTalk {
                self.release_ptt();
            }
            let m = mode_of(&self.voice.prefs);
            self.push_intent(VoiceIntent::SetMode(m));
        }
    }

    /// Moves the voice-activity gate to `threshold` (0..1, kept to whole percent).
    pub fn voice_set_threshold(&mut self, threshold: f32) {
        let t = (threshold.clamp(0.0, 1.0) * 100.0).round() / 100.0;
        if self.edit_voice_prefs(|p| p.threshold = t) {
            let m = mode_of(&self.voice.prefs);
            self.push_intent(VoiceIntent::SetMode(m));
        }
    }

    pub fn voice_set_noise_suppression(&mut self, on: bool) {
        if self.edit_voice_prefs(|p| p.noise_suppression = on) {
            self.push_intent(VoiceIntent::SetNoiseSuppression(on));
        }
    }

    pub fn voice_set_echo_cancellation(&mut self, on: bool) {
        if self.edit_voice_prefs(|p| p.echo_cancellation = on) {
            self.push_intent(VoiceIntent::SetEchoCancellation(on));
        }
    }

    pub fn voice_set_sounds(&mut self, on: bool) {
        if self.voice.sounds != on {
            self.voice.sounds = on;
            self.voice.prefs_dirty = true;
        }
    }

    /// The chord bound to `key`, if any.
    pub fn voice_shortcut(&self, key: VoiceKey) -> Option<Shortcut> {
        let p = &self.voice.prefs;
        let saved = match key {
            VoiceKey::PushToTalk => &p.ptt_key,
            VoiceKey::Mute => &p.mute_key,
            VoiceKey::Deafen => &p.deafen_key,
        };
        saved.as_deref().and_then(|s| s.parse().ok())
    }

    /// Binds (or, with `None`, unbinds) a hotkey. A chord another voice hotkey has is refused.
    pub fn voice_set_key(
        &mut self,
        key: VoiceKey,
        chord: Option<&Shortcut>,
    ) -> Result<(), rune::ShortcutError> {
        if let Some(c) = chord {
            let taken = [VoiceKey::PushToTalk, VoiceKey::Mute, VoiceKey::Deafen]
                .into_iter()
                .filter(|k| *k != key)
                .any(|k| self.voice_shortcut(k).as_ref() == Some(c));
            if taken {
                return Err(rune::ShortcutError::Conflict);
            }
        }
        let text = chord.map(Shortcut::to_string);
        self.edit_voice_prefs(|p| match key {
            VoiceKey::PushToTalk => p.ptt_key = text,
            VoiceKey::Mute => p.mute_key = text,
            VoiceKey::Deafen => p.deafen_key = text,
        });
        if key == VoiceKey::PushToTalk {
            self.release_ptt();
        }
        Ok(())
    }

    /// Starts or stops the mic test.
    pub fn toggle_mic_test(&mut self) {
        let on = !self.voice.mic_test;
        self.voice.mic_test = on;
        if !on {
            self.voice.mic_level = 0.0;
        }
        self.push_intent(VoiceIntent::MicTest(on));
    }

    /// Stops a mic test whose page is gone (closed, or left for another page or screen). Runs on
    /// every flush, so it holds however the page was left.
    pub fn settle_voice_page(&mut self) {
        if self.voice.mic_test && self.ui.settings != Some(Section::Voice) {
            self.voice.mic_test = false;
            self.voice.mic_level = 0.0;
            self.push_intent(VoiceIntent::MicTest(false));
        }
        if self.ui.settings != Some(Section::Voice) {
            self.ui.voice_menu = VoiceMenu::None;
        }
    }

    // ---- hotkeys ---------------------------------------------------------------------------

    /// Whether we are in push-to-talk mode with the key held, in a room, and not muted: the mic is
    /// open because of the key (our own speaking ring shows it).
    pub fn ptt_transmitting(&self) -> bool {
        self.voice.prefs.mode == InputMode::PushToTalk
            && self.voice.ptt_down
            && matches!(self.voice.session, VoiceSession::Connected { .. })
            && !self.mic_muted()
    }

    /// Whether the mic follows a push-to-talk key (the user strip says so).
    pub fn push_to_talk(&self) -> bool {
        self.voice.prefs.mode == InputMode::PushToTalk
    }

    fn release_ptt(&mut self) {
        if std::mem::take(&mut self.voice.ptt_down) {
            self.push_intent(VoiceIntent::SetPtt(false));
        }
    }

    /// A key no focused control took. The mute and deafen hotkeys toggle; the push-to-talk key
    /// (in push-to-talk mode) opens the mic while held. `physical` is the key's place on the
    /// keyboard (`Tx::physical_key`), `repeat` an auto-repeat of a held key. True when taken.
    ///
    /// Typing: a focused text field takes the keys it types before they get here, so a bare
    /// push-to-talk key (a letter) does not transmit while the composer has focus; function keys
    /// and chords with Cmd/Ctrl/Alt pass through the field and work everywhere. While a shortcut
    /// is being recorded the recorder owns the keyboard.
    pub fn voice_hotkey(
        &mut self,
        ev: &KeyEvent,
        physical: Option<PhysicalKeyCode>,
        repeat: bool,
    ) -> bool {
        if self.screen != crate::state::Screen::Main || self.ui.voice_keys.recording() {
            return false;
        }
        if self.push_to_talk()
            && let Some(ptt) = self.voice_shortcut(VoiceKey::PushToTalk)
            && chord_matches(&ptt, ev, physical)
        {
            if !self.voice.ptt_down {
                self.voice.ptt_down = true;
                self.push_intent(VoiceIntent::SetPtt(true));
            }
            return true;
        }
        if let Some(c) = self.voice_shortcut(VoiceKey::Mute)
            && chord_matches(&c, ev, physical)
        {
            if !repeat {
                self.toggle_mute();
            }
            return true;
        }
        if let Some(c) = self.voice_shortcut(VoiceKey::Deafen)
            && chord_matches(&c, ev, physical)
        {
            if !repeat {
                self.toggle_deaf();
            }
            return true;
        }
        false
    }

    /// A key came up. The push-to-talk key closes the mic whatever modifiers are held by then
    /// (they are often let go first).
    pub fn voice_key_up(&mut self, ev: &KeyEvent, physical: Option<PhysicalKeyCode>) -> bool {
        if !self.voice.ptt_down {
            return false;
        }
        match self.voice_shortcut(VoiceKey::PushToTalk) {
            Some(ptt) if same_key(&ptt, ev, physical) => {
                self.release_ptt();
                true
            }
            _ => false,
        }
    }

    /// The window lost or got focus. A key held when it is lost never reports its release here,
    /// so push to talk lets go, or it would transmit until the next press.
    pub fn voice_window_focus(&mut self, focused: bool) {
        if !focused {
            self.release_ptt();
        }
    }
}

// ---- camera, screen share and the stage ------------------------------------------------------------

impl AppState {
    /// Whether we are connected to a voice channel (video is only offered then).
    pub fn voice_connected(&self) -> bool {
        matches!(self.voice.session, VoiceSession::Connected { .. })
    }

    /// Our own video tracks (the engine reports them back for the preview).
    fn my_video(&self) -> VideoTracks {
        self.voice
            .video_tracks
            .get(&self.my_id())
            .cloned()
            .unwrap_or_default()
    }

    /// The camera button: turns our camera on or off. The server's flag follows the engine's
    /// report that the track is up (or gone), so a camera that fails never shows as on.
    pub fn toggle_camera(&mut self) {
        if !self.voice_connected() {
            return;
        }
        self.voice.camera = !self.voice.camera;
        let on = self.voice.camera;
        self.push_intent(VoiceIntent::SetCamera(on));
    }

    /// The share button: stops our share, or opens the picker to start one.
    pub fn toggle_screen_share(&mut self) {
        if !self.voice_connected() {
            return;
        }
        if self.voice.sharing.is_some() {
            self.stop_screen_share();
        } else if app_screen_picker() {
            self.open_screen_picker();
        } else {
            // The browser asks what to share when the engine starts it, from this click.
            self.start_screen_share(ScreenSource {
                id: 0,
                title: "Your screen".into(),
                kind: thehideout_voice_types::ScreenKind::Screen,
            });
        }
    }

    /// Opens the picker and asks the engine what can be shared.
    pub fn open_screen_picker(&mut self) {
        if !self.voice_connected() {
            return;
        }
        self.voice.picker = true;
        self.voice.screen_sources = None;
        self.push_intent(VoiceIntent::ListScreenSources);
    }

    pub fn close_screen_picker(&mut self) {
        self.voice.picker = false;
    }

    /// Starts sharing `source` (a choice in the picker).
    pub fn start_screen_share(&mut self, source: ScreenSource) {
        self.voice.picker = false;
        if !self.voice_connected() {
            return;
        }
        self.voice.sharing = Some(source.clone());
        self.push_intent(VoiceIntent::SetScreenShare(Some(source)));
    }

    pub fn stop_screen_share(&mut self) {
        if self.voice.sharing.take().is_some() {
            self.push_intent(VoiceIntent::SetScreenShare(None));
        }
    }

    /// The screens first, then the windows, each in the engine's order (the picker's rows).
    pub fn share_choices(&self) -> Vec<ScreenSource> {
        let all = self.voice.screen_sources.as_deref().unwrap_or(&[]);
        let (screens, windows): (Vec<_>, Vec<_>) = all
            .iter()
            .cloned()
            .partition(|s| s.kind == thehideout_voice_types::ScreenKind::Screen);
        screens.into_iter().chain(windows).collect()
    }

    /// Forgets everything about video: ours (the engine has dropped it, or is about to) and
    /// everyone's. `leaving` also returns the main panel to the transcript.
    fn reset_video(&mut self, leaving: bool) {
        self.voice.camera = false;
        self.voice.sharing = None;
        self.voice.picker = false;
        self.voice.video_tracks.clear();
        self.voice.video_flags_sent = (false, false);
        if leaving {
            self.voice.view = MainView::Chat;
            self.voice.focus = Focus::Auto;
        }
    }

    /// The engine reported a track. Our own camera or screen becoming live raises the server's
    /// camera / LIVE flag.
    fn video_track(&mut self, identity: &str, source: VideoKind, track_id: String) -> bool {
        let Ok(user) = identity.parse::<u64>() else {
            return false;
        };
        let entry = self.voice.video_tracks.entry(user).or_default();
        let slot = match source {
            VideoKind::Camera => &mut entry.camera,
            VideoKind::Screen => &mut entry.screen,
        };
        if slot.as_ref() == Some(&track_id) {
            return false;
        }
        *slot = Some(track_id);
        true
    }

    fn video_track_ended(&mut self, identity: &str, track_id: &str) -> bool {
        let Ok(user) = identity.parse::<u64>() else {
            return false;
        };
        let mine = user == self.my_id();
        let Some(entry) = self.voice.video_tracks.get_mut(&user) else {
            return false;
        };
        let mut camera_ended = false;
        let mut screen_ended = false;
        if entry.camera.as_deref() == Some(track_id) {
            entry.camera = None;
            camera_ended = true;
        }
        if entry.screen.as_deref() == Some(track_id) {
            entry.screen = None;
            screen_ended = true;
        }
        if *entry == VideoTracks::default() {
            self.voice.video_tracks.remove(&user);
        }
        if !camera_ended && !screen_ended {
            return false;
        }
        if mine {
            // Stopped by us, or by the capture itself (the shared window closed).
            if camera_ended {
                self.voice.camera = false;
            }
            if screen_ended {
                self.voice.sharing = None;
            }
        }
        true
    }

    /// Tells the server about our camera and screen when they differ from what it was last told.
    /// Decided once per flush, not per event, so a camera and a share that stop together send one
    /// call (two in flight at once could land out of order).
    fn sync_video_flags(&mut self) {
        let now = self.my_video();
        let flags = (now.camera.is_some(), now.screen.is_some());
        if !self.voice_connected() || flags == self.voice.video_flags_sent {
            return;
        }
        self.voice.video_flags_sent = flags;
        self.outbox.push(Command::UpdateVideoState {
            self_video: Some(flags.0),
            self_stream: Some(flags.1),
        });
    }

    /// An engine error: whatever we asked for and the engine never published is off again.
    fn revert_unpublished_video(&mut self) {
        let mine = self.my_video();
        if self.voice.camera && mine.camera.is_none() {
            self.voice.camera = false;
        }
        if self.voice.sharing.is_some() && mine.screen.is_none() {
            self.voice.sharing = None;
        }
        self.voice.picker = false;
    }

    // ---- the stage -------------------------------------------------------------------------

    /// Shows the voice stage in place of the transcript (only while connected).
    pub fn open_stage(&mut self) {
        if self.voice_connected() {
            self.voice.view = MainView::Stage;
        }
    }

    /// Back to the transcript.
    pub fn close_stage(&mut self) {
        self.voice.view = MainView::Chat;
    }

    /// Whether the stage is what the main panel shows now.
    pub fn stage_open(&self) -> bool {
        self.voice.view == MainView::Stage && self.voice_connected()
    }

    /// "Watch": opens the stage with `user`'s screen share big (their camera if they have no share).
    pub fn watch_stream(&mut self, user: u64) {
        if !self.voice_connected() {
            return;
        }
        let kind = match self.voice.video_tracks.get(&user) {
            Some(t) if t.screen.is_some() => TileKind::Screen,
            Some(t) if t.camera.is_some() => TileKind::Camera,
            _ => TileKind::Avatar,
        };
        self.voice.view = MainView::Stage;
        self.voice.focus = if kind == TileKind::Avatar {
            Focus::Auto
        } else {
            Focus::Tile(user, kind)
        };
    }

    /// The stage's tiles, in the order people joined: a camera tile, a screen tile, or an avatar
    /// tile for someone with neither. Empty when we are not connected.
    pub fn stage_tiles(&self) -> Vec<Tile> {
        let Some(channel) = self.voice.session.channel().filter(|_| self.voice_connected()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for p in self.voice_participants(channel) {
            let tracks = self.voice.video_tracks.get(&p.user_id);
            let camera = tracks.and_then(|t| t.camera.clone());
            let screen = tracks.and_then(|t| t.screen.clone());
            if camera.is_none() && screen.is_none() {
                out.push(Tile { user: p.user_id, kind: TileKind::Avatar, track: None });
            }
            if let Some(track) = camera {
                out.push(Tile { user: p.user_id, kind: TileKind::Camera, track: Some(track) });
            }
            if let Some(track) = screen {
                out.push(Tile { user: p.user_id, kind: TileKind::Screen, track: Some(track) });
            }
        }
        out
    }

    /// The big tile of `tiles`, or `None` for the plain grid: the chosen one while it exists,
    /// else (unless the grid was chosen) the first screen share.
    pub fn stage_focus(&self, tiles: &[Tile]) -> Option<(u64, TileKind)> {
        let has = |u, k| tiles.iter().any(|t| t.user == u && t.kind == k);
        match self.voice.focus {
            Focus::Tile(u, k) if has(u, k) => Some((u, k)),
            Focus::Grid => None,
            _ => tiles
                .iter()
                .find(|t| t.kind == TileKind::Screen)
                .map(|t| (t.user, t.kind)),
        }
    }

    /// A click on a tile: it becomes the big one; the big one goes back to the grid.
    pub fn focus_tile(&mut self, user: u64, kind: TileKind) {
        let tiles = self.stage_tiles();
        self.voice.focus = if self.stage_focus(&tiles) == Some((user, kind)) {
            Focus::Grid
        } else {
            Focus::Tile(user, kind)
        };
    }

    /// The tracks a tile shows right now: every tile of the stage while it is open.
    fn visible_tracks(&self) -> Vec<(String, bool)> {
        if !self.stage_open() {
            return Vec::new();
        }
        let me = self.my_id();
        self.stage_tiles()
            .into_iter()
            .filter_map(|t| {
                let mirror = t.user == me && t.kind == TileKind::Camera;
                t.track.map(|id| (id, mirror))
            })
            .collect()
    }

    /// Subscribes the sink to the tracks the stage shows and drops the ones it no longer shows,
    /// so a hidden stage costs no decoding. Runs on every flush.
    pub fn settle_video(&mut self) {
        self.sync_video_flags();
        let want = self.visible_tracks();
        let ids: HashSet<&str> = want.iter().map(|(id, _)| id.as_str()).collect();
        let mut gone: Vec<String> = self
            .voice
            .watching
            .iter()
            .filter(|t| !ids.contains(t.as_str()))
            .cloned()
            .collect();
        gone.sort();
        for track in gone {
            self.voice.watching.remove(&track);
            self.push_intent(VoiceIntent::UnwatchVideo { track });
        }
        for (track, mirror) in want {
            if self.voice.watching.insert(track.clone()) {
                self.push_intent(VoiceIntent::WatchVideo { track, mirror });
            }
        }
    }
}
