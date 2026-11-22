//! The fake world's voice channels. A child module of `fake` (see the `#[path]` there), so it
//! reaches the world's private fields. The rules are the server's (`voice_service.proto`):
//!
//! - a user is in at most one voice channel; joining another leaves the old one, and a move
//!   inside a guild keeps the mute flags (the server mute must survive it) while another guild
//!   starts fresh; joining the channel you are in keeps the state;
//! - only voice channels of guilds you belong to can be joined (`NotFound` / `Precondition` /
//!   `PermissionDenied` without CONNECT); a text channel is a precondition failure;
//! - server-muting needs MUTE_MEMBERS, moving and disconnecting MOVE_MEMBERS, both need a higher
//!   top role than the target (the owner outranks everyone, nobody mutes the owner or
//!   themself), and the target must be in a voice channel of the guild;
//! - a move of *us* is announced with `voice_moved`, and the client rejoins;
//! - a new stream starts with a `voice_snapshot` when anyone is in voice.
//!
//! The seeded scene: in The Hideout's last section, "Voice", Ana and Bruno are in the channel
//! `lounge` (Bruno has muted himself) and `game-night` is empty.
//! Joining returns `{ url: "ws://fake", token: "fake" }`.

#![cfg_attr(not(test), allow(dead_code))]

use super::*;
use thehideout_api::VoiceJoin;

/// Where everyone is. `states` keep the order people joined in.
#[derive(Default)]
pub(super) struct Voice {
    states: Vec<VoiceState>,
    /// A clock for `joined_at`: each join is a second after the last.
    tick: i64,
    /// Joining answers like a server without LiveKit.
    pub(super) unavailable: bool,
}

impl World {
    /// The Hideout's voice section with its two channels, and Ana and Bruno in the first.
    pub(super) fn seed_voice(&mut self) {
        let voice = |id: u64, position: i32, name: &str, icon: &str| Channel {
            id,
            guild_id: HIDEOUT,
            name: name.into(),
            created_at: time(utc(2026, 1, 6, 9, 0)),
            section_id: Some(VOICE),
            position,
            icon: ChannelIcon::new(icon),
            icon_ref: ChannelIconRef::Phosphor(icon.into()),
            color: PaletteColor::None,
            topic: String::new(),
            kind: ChannelKind::Voice,
        };
        self.channels.push(voice(LOUNGE, 0, "lounge", "speaker-high"));
        self.channels.push(voice(GAME_NIGHT, 1, "game-night", "game-controller"));
        if let Some(l) = self.layouts.get_mut(&HIDEOUT) {
            l.sections.push(Section {
                id: VOICE,
                guild_id: HIDEOUT,
                name: "Voice".into(),
                position: 3,
            });
            l.entries.push(LayoutEntry {
                section_id: Some(VOICE),
                channel_ids: vec![LOUNGE, GAME_NIGHT],
            });
        }
        // Ana joined first; Bruno joined later and muted himself.
        let ana = self.new_voice_state(ANA, LOUNGE);
        let mut bruno = self.new_voice_state(BRUNO, LOUNGE);
        bruno.self_mute = true;
        self.voice.states.push(ana);
        self.voice.states.push(bruno);
    }

    fn new_voice_state(&mut self, user: u64, channel: u64) -> VoiceState {
        self.voice.tick += 1;
        let guild = self
            .channels
            .iter()
            .find(|c| c.id == channel)
            .map_or(0, |c| c.guild_id);
        VoiceState {
            guild_id: guild,
            channel_id: channel,
            user_id: user,
            self_mute: false,
            self_deaf: false,
            server_mute: false,
            self_video: false,
            self_stream: false,
            joined_at: time(now_unix() - 3600 + self.voice.tick * 60),
        }
    }

    pub(super) fn voice_states_of(&self, guild: u64) -> Vec<VoiceState> {
        self.voice
            .states
            .iter()
            .filter(|s| s.guild_id == guild)
            .cloned()
            .collect()
    }

    fn voice_state_of(&self, user: u64) -> Option<&VoiceState> {
        self.voice.states.iter().find(|s| s.user_id == user)
    }

    fn voice_event(guild: u64, user: u64, state: Option<VoiceState>) -> Reply {
        Reply::Gateway(GatewayUpdate::Event(Event::VoiceStateUpdate {
            guild_id: guild,
            user_id: user,
            state,
        }))
    }

    /// The voice states of every guild we are in, when anyone is in voice (a new stream's first
    /// voice event).
    pub(super) fn voice_snapshot(&self) -> Option<Reply> {
        let guild_ids: Vec<u64> = self
            .guilds
            .iter()
            .filter(|g| {
                self.members
                    .get(&g.id)
                    .is_some_and(|m| m.contains(&self.me.id))
            })
            .map(|g| g.id)
            .collect();
        let states: Vec<VoiceState> = self
            .voice
            .states
            .iter()
            .filter(|s| guild_ids.contains(&s.guild_id))
            .cloned()
            .collect();
        (!states.is_empty()).then_some(Reply::Gateway(GatewayUpdate::Event(
            Event::VoiceSnapshot { guild_ids, states },
        )))
    }

    /// Puts `user` into `channel` the way the server's `join` does. Returns the new state and the
    /// guild they left, when it was another one.
    fn voice_join(&mut self, user: u64, channel: u64) -> (VoiceState, Option<u64>) {
        let mut new = self.new_voice_state(user, channel);
        let prev = self.voice_state_of(user).cloned();
        if let Some(p) = prev.as_ref().filter(|p| p.guild_id == new.guild_id) {
            new.self_mute = p.self_mute;
            new.self_deaf = p.self_deaf;
            new.server_mute = p.server_mute;
            if p.channel_id == channel {
                new.self_video = p.self_video;
                new.self_stream = p.self_stream;
                new.joined_at = p.joined_at;
            }
        }
        match self.voice.states.iter().position(|s| s.user_id == user) {
            Some(i) if prev.as_ref().is_some_and(|p| p.channel_id == channel) => {
                self.voice.states[i] = new.clone();
            }
            Some(i) => {
                self.voice.states.remove(i);
                self.voice.states.push(new.clone());
            }
            None => self.voice.states.push(new.clone()),
        }
        let left = prev.map(|p| p.guild_id).filter(|g| *g != new.guild_id);
        (new, left)
    }

    fn is_member(&self, guild: u64, user: u64) -> bool {
        self.members.get(&guild).is_some_and(|m| m.contains(&user))
    }

    /// The server's `outranks_member`: the owner always, nobody else the owner, anyone else only
    /// members whose top role is strictly below their own.
    fn outranks_member(&self, guild: u64, me: u64, target: u64) -> bool {
        let (owner, _, top) = self.standing(guild, me);
        let (target_owner, _, target_top) = self.standing(guild, target);
        if owner {
            return true;
        }
        if target_owner {
            return false;
        }
        top.is_some_and(|t| target_top.is_none_or(|p| p < t))
    }

    /// The caller's check on a moderation call: the guild exists, they are in it and hold `bit`.
    fn moderator_check(&self, guild: u64, me: u64, bit: Permissions) -> Result<(), ApiError> {
        if !self.guilds.iter().any(|g| g.id == guild) || !self.is_member(guild, me) {
            return Err(ApiError::NotFound);
        }
        if !self.has_perm(guild, me, bit) {
            return Err(ApiError::PermissionDenied);
        }
        Ok(())
    }

    /// The target of a moderation call, in the server's order: a member, below the caller in the
    /// hierarchy (the caller themself is fine), in a voice channel of the guild.
    fn moderation_target(&self, guild: u64, me: u64, target: u64) -> Result<VoiceState, ApiError> {
        if !self.is_member(guild, target) {
            return Err(ApiError::NotFound);
        }
        if target != me && !self.outranks_member(guild, me, target) {
            return Err(ApiError::PermissionDenied);
        }
        self.voice_state_of(target)
            .filter(|s| s.guild_id == guild)
            .cloned()
            .ok_or_else(|| ApiError::Precondition("member is not in a voice channel".into()))
    }

    /// Answers the voice commands; any other command comes back for `handle`.
    pub(super) fn voice_command(&mut self, cmd: Command) -> Result<Vec<Reply>, Command> {
        let me = self.me.id;
        Ok(match cmd {
            Command::JoinVoice { channel } => {
                let r = self.try_join(me, channel);
                match r {
                    Err(e) => vec![Reply::VoiceJoined {
                        channel,
                        result: Err(e),
                    }],
                    Ok((state, left)) => {
                        let mut out = vec![
                            Reply::VoiceJoined {
                                channel,
                                result: Ok(VoiceJoin {
                                    url: "ws://fake".into(),
                                    token: "fake".into(),
                                    channel_id: channel,
                                }),
                            },
                            Self::voice_event(state.guild_id, me, Some(state)),
                        ];
                        out.extend(left.map(|g| Self::voice_event(g, me, None)));
                        out
                    }
                }
            }
            Command::LeaveVoice => {
                let mut out = vec![Reply::VoiceLeft(Ok(()))];
                out.extend(self.voice_remove(me));
                out
            }
            Command::UpdateVoiceState {
                self_mute,
                self_deaf,
            } => match self.voice.states.iter_mut().find(|s| s.user_id == me) {
                None => vec![Reply::VoiceFlags(Err(ApiError::Precondition(
                    "not in a voice channel".into(),
                )))],
                Some(s) => {
                    s.self_mute = self_mute;
                    s.self_deaf = self_deaf;
                    let s = s.clone();
                    vec![
                        Reply::VoiceFlags(Ok(s.clone())),
                        Self::voice_event(s.guild_id, me, Some(s)),
                    ]
                }
            },
            Command::UpdateVideoState {
                self_video,
                self_stream,
            } => match self.voice.states.iter_mut().find(|s| s.user_id == me) {
                None => vec![Reply::VoiceFlags(Err(ApiError::Precondition(
                    "not in a voice channel".into(),
                )))],
                Some(s) => {
                    if let Some(v) = self_video {
                        s.self_video = v;
                    }
                    if let Some(v) = self_stream {
                        s.self_stream = v;
                    }
                    let s = s.clone();
                    vec![
                        Reply::VoiceFlags(Ok(s.clone())),
                        Self::voice_event(s.guild_id, me, Some(s)),
                    ]
                }
            },
            Command::ServerMuteMember { guild, user, muted } => {
                let r = self
                    .moderator_check(guild, me, Permissions::MUTE_MEMBERS)
                    .and_then(|()| {
                        if user == me {
                            Err(ApiError::InvalidArgument(
                                "you cannot server-mute yourself".into(),
                            ))
                        } else {
                            self.moderation_target(guild, me, user)
                        }
                    });
                match r {
                    Err(e) => vec![Self::moderated(guild, user, Err(e))],
                    Ok(_) => {
                        let s = self
                            .voice
                            .states
                            .iter_mut()
                            .find(|s| s.user_id == user)
                            .map(|s| {
                                s.server_mute = muted;
                                s.clone()
                            });
                        vec![
                            Self::moderated(guild, user, Ok(())),
                            Self::voice_event(guild, user, s),
                        ]
                    }
                }
            }
            Command::MoveMember {
                guild,
                user,
                channel,
            } => match self.try_move(guild, me, user, channel) {
                Err(e) => vec![Self::moderated(guild, user, Err(e))],
                Ok(None) => vec![Self::moderated(guild, user, Ok(()))],
                Ok(Some(state)) => {
                    let mut out = vec![
                        Self::moderated(guild, user, Ok(())),
                        Self::voice_event(guild, user, Some(state)),
                    ];
                    if user == me {
                        out.push(Self::moved(guild, channel));
                    }
                    out
                }
            },
            Command::DisconnectMember { guild, user } => {
                match self
                    .moderator_check(guild, me, Permissions::MOVE_MEMBERS)
                    .and_then(|()| self.moderation_target(guild, me, user))
                {
                    Err(e) => vec![Self::moderated(guild, user, Err(e))],
                    Ok(_) => {
                        let mut out = vec![Self::moderated(guild, user, Ok(()))];
                        out.extend(self.voice_remove(user));
                        out
                    }
                }
            }
            other => return Err(other),
        })
    }

    fn moderated(guild: u64, user: u64, result: Result<(), ApiError>) -> Reply {
        Reply::VoiceModerated {
            guild,
            user,
            result,
        }
    }

    fn moved(guild: u64, channel: u64) -> Reply {
        Reply::Gateway(GatewayUpdate::Event(Event::VoiceMoved {
            guild_id: guild,
            channel_id: channel,
        }))
    }

    fn try_join(&mut self, user: u64, channel: u64) -> Result<(VoiceState, Option<u64>), ApiError> {
        if self.voice.unavailable {
            return Err(ApiError::Network(
                "voice is not configured on this server".into(),
            ));
        }
        let c = self
            .channels
            .iter()
            .find(|c| c.id == channel)
            .ok_or(ApiError::NotFound)?;
        let (guild, kind) = (c.guild_id, c.kind);
        if !self.is_member(guild, user) {
            return Err(ApiError::NotFound);
        }
        if kind != ChannelKind::Voice {
            return Err(ApiError::Precondition("not a voice channel".into()));
        }
        if !self.has_perm(guild, user, Permissions::CONNECT) {
            return Err(ApiError::PermissionDenied);
        }
        Ok(self.voice_join(user, channel))
    }

    /// A move by `me` of `target`; `None` when they are already there.
    fn try_move(
        &mut self,
        guild: u64,
        me: u64,
        target: u64,
        channel: u64,
    ) -> Result<Option<VoiceState>, ApiError> {
        self.moderator_check(guild, me, Permissions::MOVE_MEMBERS)?;
        let current = self.moderation_target(guild, me, target)?;
        let c = self
            .channels
            .iter()
            .find(|c| c.id == channel)
            .filter(|c| c.guild_id == guild)
            .ok_or(ApiError::NotFound)?;
        if c.kind != ChannelKind::Voice {
            return Err(ApiError::Precondition("not a voice channel".into()));
        }
        if !self.has_perm(guild, target, Permissions::CONNECT) {
            return Err(ApiError::Precondition(
                "member cannot connect to voice".into(),
            ));
        }
        if current.channel_id == channel {
            return Ok(None);
        }
        Ok(Some(self.voice_join(target, channel).0))
    }

    /// Takes `user` out of voice; the event for it when they were in.
    fn voice_remove(&mut self, user: u64) -> Option<Reply> {
        let at = self.voice.states.iter().position(|s| s.user_id == user)?;
        let gone = self.voice.states.remove(at);
        Some(Self::voice_event(gone.guild_id, user, None))
    }

    // ---- tests and scenes drive the world directly -------------------------------------------

    pub(super) fn voice_join_as(&mut self, user: u64, channel: u64) {
        if let Ok((state, left)) = self.try_join(user, channel) {
            self.enqueue(Self::voice_event(state.guild_id, user, Some(state)));
            if let Some(g) = left {
                self.enqueue(Self::voice_event(g, user, None));
            }
        }
    }

    pub(super) fn voice_leave_as(&mut self, user: u64) {
        if let Some(r) = self.voice_remove(user) {
            self.enqueue(r);
        }
    }

    pub(super) fn voice_move_as_moderator(&mut self, user: u64, channel: u64) {
        let Some(guild) = self
            .channels
            .iter()
            .find(|c| c.id == channel && c.kind == ChannelKind::Voice)
            .map(|c| c.guild_id)
        else {
            return;
        };
        if self
            .voice_state_of(user)
            .is_none_or(|s| s.channel_id == channel)
        {
            return;
        }
        let (state, _) = self.voice_join(user, channel);
        self.enqueue(Self::voice_event(guild, user, Some(state)));
        if user == self.me.id {
            self.enqueue(Self::moved(guild, channel));
        }
    }

    pub(super) fn set_role_permissions(&mut self, guild: u64, role: u64, permissions: Permissions) {
        if let Some(r) = self
            .roles
            .get_mut(&guild)
            .and_then(|rs| rs.iter_mut().find(|r| r.id == role))
        {
            r.permissions = permissions;
            let e = self.roles_event(guild);
            self.enqueue(e);
        }
    }
}

// ---- the fake audio engine -----------------------------------------------------------------------

/// The devices the fake world's audio engine lists: a laptop's own, a monitor's and a headset.
/// The first of each is the system default.
pub fn fake_devices_event() -> thehideout_voice::VoiceEvent {
    let list = |names: &[(&str, &str)]| -> Vec<thehideout_voice::AudioDevice> {
        names
            .iter()
            .map(|(id, name)| thehideout_voice::AudioDevice {
                id: (*id).into(),
                name: (*name).into(),
            })
            .collect()
    };
    thehideout_voice::VoiceEvent::Devices {
        inputs: list(&[
            ("mic-builtin", "MacBook Pro Microphone"),
            ("mic-display", "Studio Display Microphone"),
            ("mic-headset", "USB Headset"),
        ]),
        outputs: list(&[
            ("out-builtin", "MacBook Pro Speakers"),
            ("out-display", "Studio Display Speakers"),
            ("out-headset", "USB Headset"),
        ]),
        default_input: Some("mic-builtin".into()),
        default_output: Some("out-builtin".into()),
    }
}

/// The level the fake mic test reports: fixed, so a run looks the same every time.
pub const FAKE_MIC_LEVEL: f32 = 0.55;

/// A voice sink for the fake world: it answers the settings page's device refresh with
/// [`fake_devices_event`] and the mic test with one fixed level, plays the camera and screen share
/// (our own tracks come back as the engine reports them, `remote` people announce theirs on
/// connect), and draws generated pictures for watched tracks. Real runs never install it; the
/// `--fake` window uses [`FakeAudio::demo`], scenes and tests install the one they need.
#[derive(Default)]
pub struct FakeAudio {
    events: Vec<thehideout_voice::VoiceEvent>,
    /// Who else has video once we connect.
    remote: Vec<(u64, thehideout_voice::VideoKind)>,
    /// Redraw watched tracks about ten times a second (the window); off, a track gets its first
    /// picture only (scenes and tests: the same pixels every run).
    animate: bool,
    watches: std::collections::HashMap<String, FakeWatch>,
}

struct FakeWatch {
    pixels: rune::Pixels,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for FakeWatch {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// The track id the fake engine gives `user`'s camera or screen.
pub fn fake_track(user: u64, kind: thehideout_voice::VideoKind) -> String {
    let k = match kind {
        thehideout_voice::VideoKind::Camera => "camera",
        thehideout_voice::VideoKind::Screen => "screen",
    };
    format!("fake-{user}-{k}")
}

/// What can be shared on the fake machine: the display (the system picker on macOS) and windows.
pub fn fake_screen_sources() -> Vec<thehideout_voice::ScreenSource> {
    use thehideout_voice::{ScreenKind, ScreenSource};
    let w = |id, title: &str| ScreenSource { id, title: title.into(), kind: ScreenKind::Window };
    vec![
        ScreenSource { id: 1, title: "Entire screen".into(), kind: ScreenKind::Screen },
        w(101, "Terminal \u{2014} thehideout"),
        w(102, "Figma \u{2014} Hideout app"),
        w(103, "Rune documentation \u{2014} Safari"),
    ]
}

impl FakeAudio {
    /// The window's fake audio: Ana has her camera on, and pictures move.
    pub fn demo() -> FakeAudio {
        FakeAudio {
            remote: vec![(ANA, thehideout_voice::VideoKind::Camera)],
            animate: true,
            ..FakeAudio::default()
        }
    }

    /// The track id the fake engine gives `user`'s camera or screen.
    pub fn track(user: u64, kind: thehideout_voice::VideoKind) -> String {
        fake_track(user, kind)
    }

    /// People with video when we connect (scenes and tests).
    pub fn with_remote(remote: Vec<(u64, thehideout_voice::VideoKind)>) -> FakeAudio {
        FakeAudio { remote, ..FakeAudio::default() }
    }

    fn mine(&mut self, kind: thehideout_voice::VideoKind, on: bool) {
        use thehideout_voice::VoiceEvent;
        let (identity, track_id) = (TIM.to_string(), fake_track(TIM, kind));
        self.events.push(if on {
            VoiceEvent::VideoTrack { identity, source: kind, track_id }
        } else {
            VoiceEvent::VideoTrackEnded { identity, track_id }
        });
    }
}

impl crate::voice::VoiceSink for FakeAudio {
    fn intent(&mut self, intent: crate::voice::VoiceIntent) {
        use crate::voice::VoiceIntent;
        use thehideout_voice::{VideoKind, VoiceEvent};
        match intent {
            VoiceIntent::ListDevices => self.events.push(fake_devices_event()),
            VoiceIntent::MicTest(on) => self.events.push(VoiceEvent::Levels(vec![(
                String::new(),
                if on { FAKE_MIC_LEVEL } else { 0.0 },
            )])),
            VoiceIntent::Connect { .. } => {
                for (user, kind) in self.remote.clone() {
                    self.events.push(VoiceEvent::VideoTrack {
                        identity: user.to_string(),
                        source: kind,
                        track_id: fake_track(user, kind),
                    });
                }
            }
            VoiceIntent::Disconnect => self.watches.clear(),
            VoiceIntent::SetCamera(on) => self.mine(VideoKind::Camera, on),
            VoiceIntent::SetScreenShare(source) => self.mine(VideoKind::Screen, source.is_some()),
            VoiceIntent::ListScreenSources => self.events.push(VoiceEvent::ScreenSources(fake_screen_sources())),
            VoiceIntent::WatchVideo { track, mirror } => {
                let pixels = rune::Pixels::new();
                let (w, h) = fake_size(&track);
                pixels.set(w, h, fake_frame(&track, 0, w, h, mirror));
                let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                if self.animate {
                    let (px, flag, t) = (pixels.clone(), stop.clone(), track.clone());
                    std::thread::spawn(move || {
                        let mut seq = 0;
                        while !flag.load(std::sync::atomic::Ordering::Relaxed) {
                            seq += 1;
                            px.set(w, h, fake_frame(&t, seq, w, h, mirror));
                            std::thread::sleep(std::time::Duration::from_millis(100));
                        }
                    });
                }
                self.watches.insert(track, FakeWatch { pixels, stop });
            }
            VoiceIntent::UnwatchVideo { track } => {
                self.watches.remove(&track);
            }
            _ => {}
        }
    }

    fn events(&mut self) -> Vec<thehideout_voice::VoiceEvent> {
        std::mem::take(&mut self.events)
    }

    fn video_pixels(&self, track: &str) -> Option<rune::Pixels> {
        self.watches.get(track).map(|w| w.pixels.clone())
    }
}

/// The size of a fake track's pictures: a camera is 16:9 at 640 wide, a screen 16:10 at 1280.
fn fake_size(track: &str) -> (u32, u32) {
    if track.ends_with("screen") { (1280, 800) } else { (640, 360) }
}

/// A generated picture for a fake track, frame `seq` (the same bytes every run): a camera is a
/// gradient in the person's colour with a head-and-shoulders silhouette and a light bar that moves
/// with `seq`; a screen is a dark desktop with a window of text lines and a block cursor that moves.
/// `mirror` flips it left to right, as the app does for our own camera.
pub fn fake_frame(track: &str, seq: u64, w: u32, h: u32, mirror: bool) -> Vec<u8> {
    let user: u64 = track.split('-').nth(1).and_then(|u| u.parse().ok()).unwrap_or(0);
    let screen = track.ends_with("screen");
    let hue = [210.0_f32, 330.0, 150.0, 30.0, 270.0][(user % 5) as usize];
    let mut px = vec![0u8; (w * h * 4) as usize];
    let hsl = |h: f32, s: f32, l: f32| -> [u8; 3] {
        let a = s * l.min(1.0 - l);
        let f = |n: f32| {
            let k = (n + h / 30.0) % 12.0;
            l - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
        };
        [(f(0.0) * 255.0) as u8, (f(8.0) * 255.0) as u8, (f(4.0) * 255.0) as u8]
    };
    let (wf, hf) = (w as f32, h as f32);
    for y in 0..h {
        for x in 0..w {
            let (xf, yf) = (x as f32, y as f32);
            let rgb = if screen {
                // desktop, a window, a title bar and lines of text
                let (wx0, wy0, wx1, wy1) = (wf * 0.06, hf * 0.08, wf * 0.94, hf * 0.92);
                if xf < wx0 || xf > wx1 || yf < wy0 || yf > wy1 {
                    hsl(hue, 0.35, 0.16 + 0.06 * (yf / hf))
                } else if yf < wy0 + 34.0 {
                    [52, 54, 60]
                } else {
                    let row = ((yf - wy0 - 34.0) / 22.0) as u32;
                    let in_row = ((yf - wy0 - 34.0) % 22.0) < 10.0;
                    let len = 0.25 + 0.55 * ((row.wrapping_mul(2654435761u32) >> 8) % 100) as f32 / 100.0;
                    let col = (xf - wx0 - 24.0) / (wx1 - wx0 - 48.0);
                    if in_row && (0.0..len).contains(&col) { hsl(hue + (row % 3) as f32 * 40.0, 0.5, 0.7) } else { [30, 31, 36] }
                }
            } else {
                let base = hsl(hue, 0.55, 0.30 + 0.25 * (1.0 - yf / hf));
                // head and shoulders
                let (cx, cy, r) = (wf / 2.0, hf * 0.42, hf * 0.17);
                let head = (xf - cx).powi(2) + (yf - cy).powi(2) < r * r;
                let body = yf > hf * 0.66 && ((xf - cx) / (wf * 0.30)).powi(2) + ((yf - hf * 0.92) / (hf * 0.26)).powi(2) < 1.0;
                if head || body { hsl(hue, 0.35, 0.78) } else { base }
            };
            let i = ((y * w + x) * 4) as usize;
            px[i..i + 3].copy_from_slice(&rgb);
            px[i + 3] = 255;
        }
    }
    // the moving part: a light bar (camera) or a block cursor (screen)
    let (bx, by, bw, bh) = if screen {
        let t = (seq % 40) as f32 / 40.0;
        (wf * 0.1 + t * wf * 0.5, hf * 0.30, 12.0, 18.0)
    } else {
        let t = (seq % 60) as f32 / 60.0;
        (t * wf, 0.0, 6.0, hf)
    };
    for y in (by as u32).min(h)..((by + bh) as u32).min(h) {
        for x in (bx as u32).min(w)..((bx + bw) as u32).min(w) {
            let i = ((y * w + x) * 4) as usize;
            px[i..i + 3].copy_from_slice(&[255, 255, 255]);
        }
    }
    if mirror {
        for line in px.chunks_exact_mut(w as usize * 4) {
            let mut left = 0;
            let mut right = w as usize - 1;
            while left < right {
                for c in 0..4 {
                    line.swap(left * 4 + c, right * 4 + c);
                }
                left += 1;
                right -= 1;
            }
        }
    }
    px
}
