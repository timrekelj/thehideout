//! Voice channels: the in-memory voice state, LiveKit access tokens, the small
//! LiveKit server-API surface the server needs, and the glue that keeps state,
//! gateway events and LiveKit rooms consistent. See `voice_service.proto` and
//! the README "Voice" section for the model.
//!
//! LiveKit moves the media; this server decides who may join and who is in
//! which channel. Room name = `voice-<channel_id>`, participant identity = the
//! user id (decimal), participant name = the username.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use connectrpc::ConnectError;
use livekit_api::access_token::{AccessToken, VideoGrants};
use livekit_api::services::room::{RoomClient, UpdateParticipantOptions};
use livekit_api::webhooks::WebhookReceiver;

use crate::AppState;
use crate::config::LiveKitConfig;
use crate::convert::datetime_to_proto;
use crate::events::{EventBus, EventPayload};
use crate::permissions::{SPEAK, VIDEO, standing};
use crate::proto::thehideout::v1::{VoiceState, VoiceStateUpdate};

/// Prefix of a voice channel's LiveKit room name.
pub const ROOM_PREFIX: &str = "voice-";
/// How long a join token is valid for connecting.
pub const TOKEN_TTL: Duration = Duration::from_secs(10 * 60);
/// A user who joined but never connected to LiveKit within this long is dropped.
pub const CONNECT_GRACE: Duration = Duration::from_secs(30);
/// Longest wait for one LiveKit server-API call.
const LIVEKIT_CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// LiveKit room of a voice channel.
pub fn room_name(channel_id: u64) -> String {
    format!("{ROOM_PREFIX}{channel_id}")
}

/// The channel of a room name made by [`room_name`].
pub fn channel_of_room(room: &str) -> Option<u64> {
    room.strip_prefix(ROOM_PREFIX)?.parse().ok()
}

/// A media source a participant may publish (LiveKit `TrackSource`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishSource {
    Microphone,
    Camera,
    ScreenShare,
    ScreenShareAudio,
}

impl PublishSource {
    /// Name in a LiveKit token's `canPublishSources`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Microphone => "microphone",
            Self::Camera => "camera",
            Self::ScreenShare => "screen_share",
            Self::ScreenShareAudio => "screen_share_audio",
        }
    }

    /// LiveKit `TrackSource` enum number.
    fn number(self) -> i32 {
        match self {
            Self::Camera => 1,
            Self::Microphone => 2,
            Self::ScreenShare => 3,
            Self::ScreenShareAudio => 4,
        }
    }
}

/// What a participant may publish: nothing without SPEAK (listen only); the
/// microphone unless server-muted; camera and screen share (with its audio)
/// only with VIDEO.
pub fn publish_sources(speak: bool, video: bool, server_mute: bool) -> Vec<PublishSource> {
    let mut sources = Vec::new();
    if speak {
        if !server_mute {
            sources.push(PublishSource::Microphone);
        }
        if video {
            sources.extend([PublishSource::Camera, PublishSource::ScreenShare, PublishSource::ScreenShareAudio]);
        }
    }
    sources
}

/// A LiveKit access token for `user_id` in the channel's room: join, subscribe,
/// and publish only the given sources (none = `can_publish` false).
pub fn access_token(
    config: &LiveKitConfig,
    user_id: u64,
    username: &str,
    channel_id: u64,
    sources: &[PublishSource],
) -> Result<String, ConnectError> {
    AccessToken::with_api_key(&config.api_key, &config.api_secret.0)
        .with_identity(&user_id.to_string())
        .with_name(username)
        .with_ttl(TOKEN_TTL)
        .with_grants(VideoGrants {
            room_join: true,
            room: room_name(channel_id),
            can_publish: Some(!sources.is_empty()),
            can_subscribe: Some(true),
            can_publish_sources: sources.iter().map(|s| s.as_str().to_owned()).collect(),
            ..Default::default()
        })
        .to_jwt()
        .map_err(|e| {
            tracing::error!(error = %e, "signing a LiveKit token failed");
            ConnectError::internal("internal error")
        })
}

/// The LiveKit server-API calls the server makes. A trait so tests can record
/// them instead of reaching a LiveKit server.
#[async_trait]
pub trait LiveKitAdmin: Send + Sync {
    /// Disconnects the participant (identity = user id) from the room.
    async fn remove_participant(&self, room: &str, identity: &str) -> Result<(), String>;
    /// Replaces the participant's publish permission: only `sources` may be
    /// published (an empty list revokes publishing). Tracks of a source that is
    /// no longer allowed are unpublished by LiveKit.
    async fn set_publish_sources(&self, room: &str, identity: &str, sources: &[PublishSource]) -> Result<(), String>;
}

/// [`LiveKitAdmin`] over LiveKit's RoomService (Twirp over HTTP).
pub struct LiveKitApiClient {
    client: RoomClient,
}

impl LiveKitApiClient {
    pub fn new(config: &LiveKitConfig) -> Self {
        Self { client: RoomClient::with_api_key(&config.api_url, &config.api_key, &config.api_secret.0) }
    }
}

#[async_trait]
impl LiveKitAdmin for LiveKitApiClient {
    async fn remove_participant(&self, room: &str, identity: &str) -> Result<(), String> {
        self.client.remove_participant(room, identity).await.map_err(|e| e.to_string())
    }

    async fn set_publish_sources(&self, room: &str, identity: &str, sources: &[PublishSource]) -> Result<(), String> {
        let mut options = UpdateParticipantOptions::default();
        let permission = options.permission.get_or_insert_with(Default::default);
        permission.can_subscribe = true;
        permission.can_publish = !sources.is_empty();
        permission.can_publish_data = permission.can_publish;
        permission.can_publish_sources = sources.iter().map(|s| s.number()).collect();
        self.client.update_participant(room, identity, options).await.map(|_| ()).map_err(|e| e.to_string())
    }
}

/// Stand-in used while voice is not configured; never called.
struct NoAdmin;

#[async_trait]
impl LiveKitAdmin for NoAdmin {
    async fn remove_participant(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }

    async fn set_publish_sources(&self, _: &str, _: &str, _: &[PublishSource]) -> Result<(), String> {
        Ok(())
    }
}

/// LiveKit settings and server-API client of the app.
#[derive(Clone)]
pub struct LiveKit {
    /// `None`: voice is not configured (`JoinVoice` is UNAVAILABLE).
    pub config: Option<Arc<LiveKitConfig>>,
    pub admin: Arc<dyn LiveKitAdmin>,
    /// How long a joined user may take to connect to LiveKit.
    pub connect_grace: Duration,
}

impl LiveKit {
    /// Voice off.
    pub fn disabled() -> Self {
        Self { config: None, admin: Arc::new(NoAdmin), connect_grace: CONNECT_GRACE }
    }

    /// Voice on, calling the real LiveKit server API.
    pub fn new(config: LiveKitConfig) -> Self {
        let admin = Arc::new(LiveKitApiClient::new(&config));
        Self::with_admin(config, admin)
    }

    /// Voice on with a custom server-API client (tests).
    pub fn with_admin(config: LiveKitConfig, admin: Arc<dyn LiveKitAdmin>) -> Self {
        Self { config: Some(Arc::new(config)), admin, connect_grace: CONNECT_GRACE }
    }

    /// The webhook signature verifier; `None` when voice is not configured.
    pub fn webhook_receiver(&self) -> Option<WebhookReceiver> {
        let config = self.config.as_ref()?;
        Some(WebhookReceiver::new(livekit_api::access_token::TokenVerifier::with_api_key(
            &config.api_key,
            &config.api_secret.0,
        )))
    }
}

/// One user's voice state plus the bookkeeping that is not sent to clients.
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceEntry {
    pub guild_id: u64,
    pub channel_id: u64,
    pub user_id: u64,
    pub self_mute: bool,
    pub self_deaf: bool,
    pub server_mute: bool,
    pub self_video: bool,
    pub self_stream: bool,
    pub joined_at: DateTime<Utc>,
    /// LiveKit confirmed (`participant_joined`) the connection.
    pub connected: bool,
    /// When the user last (re)joined; the connect grace runs from here.
    pub since: Instant,
}

impl VoiceEntry {
    pub fn to_proto(&self) -> VoiceState {
        VoiceState {
            guild_id: self.guild_id,
            channel_id: self.channel_id,
            user_id: self.user_id,
            self_mute: self.self_mute,
            self_deaf: self.self_deaf,
            server_mute: self.server_mute,
            self_video: self.self_video,
            self_stream: self.self_stream,
            joined_at: datetime_to_proto(self.joined_at).into(),
            ..Default::default()
        }
    }
}

/// Who is in which voice channel. In memory only: lost on restart, single node.
/// At most one entry per user.
#[derive(Default)]
pub struct VoiceStates {
    users: Mutex<HashMap<u64, VoiceEntry>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl VoiceStates {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, user_id: u64) -> Option<VoiceEntry> {
        lock(&self.users).get(&user_id).cloned()
    }

    /// Puts the user in `channel_id` and returns `(previous, current)`. The
    /// same channel again keeps the state (a reconnect); another channel of the
    /// same guild keeps the mute flags (the server mute must survive a move)
    /// and resets video / stream; another guild starts fresh. Either way the
    /// user must connect to LiveKit again.
    pub fn join(&self, user_id: u64, guild_id: u64, channel_id: u64) -> (Option<VoiceEntry>, VoiceEntry) {
        let mut users = lock(&self.users);
        let previous = users.get(&user_id).cloned();
        let mut entry = VoiceEntry {
            guild_id,
            channel_id,
            user_id,
            self_mute: false,
            self_deaf: false,
            server_mute: false,
            self_video: false,
            self_stream: false,
            joined_at: Utc::now(),
            connected: false,
            since: Instant::now(),
        };
        if let Some(prev) = previous.as_ref().filter(|p| p.guild_id == guild_id) {
            entry.self_mute = prev.self_mute;
            entry.self_deaf = prev.self_deaf;
            entry.server_mute = prev.server_mute;
            if prev.channel_id == channel_id {
                entry.self_video = prev.self_video;
                entry.self_stream = prev.self_stream;
                entry.joined_at = prev.joined_at;
            }
        }
        users.insert(user_id, entry.clone());
        (previous, entry)
    }

    /// Applies `change` to the user's entry and returns the result.
    pub fn update(&self, user_id: u64, change: impl FnOnce(&mut VoiceEntry)) -> Option<VoiceEntry> {
        let mut users = lock(&self.users);
        let entry = users.get_mut(&user_id)?;
        change(entry);
        Some(entry.clone())
    }

    pub fn remove(&self, user_id: u64) -> Option<VoiceEntry> {
        lock(&self.users).remove(&user_id)
    }

    /// Removes the user only if they are in `channel_id`.
    pub fn remove_if_in_channel(&self, user_id: u64, channel_id: u64) -> Option<VoiceEntry> {
        let mut users = lock(&self.users);
        if users.get(&user_id).is_some_and(|e| e.channel_id == channel_id) {
            users.remove(&user_id)
        } else {
            None
        }
    }

    /// Removes everyone in `channel_id`.
    pub fn remove_channel(&self, channel_id: u64) -> Vec<VoiceEntry> {
        let mut removed = Vec::new();
        lock(&self.users).retain(|_, e| {
            let keep = e.channel_id != channel_id;
            if !keep {
                removed.push(e.clone());
            }
            keep
        });
        removed
    }

    /// Removes the user if they are in a channel of `guild_id`.
    pub fn remove_guild_member(&self, guild_id: u64, user_id: u64) -> Option<VoiceEntry> {
        let mut users = lock(&self.users);
        if users.get(&user_id).is_some_and(|e| e.guild_id == guild_id) {
            users.remove(&user_id)
        } else {
            None
        }
    }

    /// Marks the user connected; false if they are not in `channel_id`.
    pub fn confirm(&self, user_id: u64, channel_id: u64) -> Option<VoiceEntry> {
        let mut users = lock(&self.users);
        let entry = users.get_mut(&user_id).filter(|e| e.channel_id == channel_id)?;
        entry.connected = true;
        Some(entry.clone())
    }

    /// Removes users who joined more than `grace` ago and never connected.
    pub fn expire_unconnected(&self, grace: Duration) -> Vec<VoiceEntry> {
        let mut removed = Vec::new();
        lock(&self.users).retain(|_, e| {
            let expired = !e.connected && e.since.elapsed() >= grace;
            if expired {
                removed.push(e.clone());
            }
            !expired
        });
        removed
    }

    /// Everyone in a channel of one of the guilds, ordered by guild, channel, join time.
    pub fn in_guilds(&self, guild_ids: &[u64]) -> Vec<VoiceEntry> {
        let mut found: Vec<VoiceEntry> =
            lock(&self.users).values().filter(|e| guild_ids.contains(&e.guild_id)).cloned().collect();
        found.sort_by_key(|e| (e.guild_id, e.channel_id, e.joined_at, e.user_id));
        found
    }

    pub fn in_channel(&self, channel_id: u64) -> Vec<VoiceEntry> {
        let mut found: Vec<VoiceEntry> = lock(&self.users).values().filter(|e| e.channel_id == channel_id).cloned().collect();
        found.sort_by_key(|e| (e.joined_at, e.user_id));
        found
    }

    pub fn len(&self) -> usize {
        lock(&self.users).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Tells the guild about `entry` (the user's new state).
pub fn publish_state(events: &EventBus, entry: &VoiceEntry) {
    events.publish_to_guild(
        entry.guild_id,
        EventPayload::VoiceStateUpdate(VoiceStateUpdate {
            guild_id: entry.guild_id,
            user_id: entry.user_id,
            state: Some(entry.to_proto()).into(),
            ..Default::default()
        }),
    );
}

/// Tells the guild the user is no longer in voice.
pub fn publish_left(events: &EventBus, guild_id: u64, user_id: u64) {
    events.publish_to_guild(
        guild_id,
        EventPayload::VoiceStateUpdate(VoiceStateUpdate { guild_id, user_id, state: None.into(), ..Default::default() }),
    );
}

/// Disconnects the user's LiveKit participant; failures are logged, never fatal
/// (the participant may not have connected yet or may be gone already).
pub async fn kick(state: &AppState, channel_id: u64, user_id: u64) {
    if state.livekit.config.is_none() {
        return;
    }
    let (room, identity) = (room_name(channel_id), user_id.to_string());
    match tokio::time::timeout(LIVEKIT_CALL_TIMEOUT, state.livekit.admin.remove_participant(&room, &identity)).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::debug!(%room, %identity, error = %e, "removing the LiveKit participant failed"),
        Err(_) => tracing::warn!(%room, %identity, "removing the LiveKit participant timed out"),
    }
}

/// Pushes the permissions a member should have in their room to LiveKit
/// (after a server mute change, and on connect for a server-muted member).
pub async fn enforce_publish_sources(state: &AppState, entry: &VoiceEntry) {
    if state.livekit.config.is_none() {
        return;
    }
    let Ok(target) = standing(state, entry.user_id, entry.guild_id).await else { return };
    let sources = publish_sources(target.has(SPEAK), target.has(VIDEO), entry.server_mute);
    let (room, identity) = (room_name(entry.channel_id), entry.user_id.to_string());
    match tokio::time::timeout(LIVEKIT_CALL_TIMEOUT, state.livekit.admin.set_publish_sources(&room, &identity, &sources)).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::warn!(%room, %identity, error = %e, "updating LiveKit publish permissions failed"),
        Err(_) => tracing::warn!(%room, %identity, "updating LiveKit publish permissions timed out"),
    }
}

/// Removes a user from voice: publishes the departure and disconnects them
/// from the room. Returns whether they were in voice.
pub async fn leave(state: &AppState, user_id: u64) -> bool {
    let Some(entry) = state.voice.remove(user_id) else { return false };
    publish_left(&state.events, entry.guild_id, entry.user_id);
    kick(state, entry.channel_id, entry.user_id).await;
    true
}

/// A voice channel was deleted: everyone in it leaves. To be called by the
/// channel-deletion path (there is no DeleteChannel RPC yet).
pub async fn on_channel_deleted(state: &AppState, channel_id: u64) {
    for entry in state.voice.remove_channel(channel_id) {
        publish_left(&state.events, entry.guild_id, entry.user_id);
        kick(state, entry.channel_id, entry.user_id).await;
    }
}

/// A member left, was removed from or was banned from the guild: they leave
/// voice there. To be called by those paths (there are no such RPCs yet).
pub async fn on_member_removed(state: &AppState, guild_id: u64, user_id: u64) {
    if let Some(entry) = state.voice.remove_guild_member(guild_id, user_id) {
        publish_left(&state.events, entry.guild_id, entry.user_id);
        kick(state, entry.channel_id, entry.user_id).await;
    }
}

/// Applies a verified LiveKit webhook event to the voice state.
///
/// - `participant_joined`: confirms the user's join; a user who is not (or no
///   longer) in that channel by our books is disconnected (a stale token).
///   A server-muted member gets the muted permissions pushed again, covering a
///   mute that happened before they connected.
/// - `participant_left`: the user leaves voice if still booked in that room (a
///   move or rejoin already changed the booking, which is then left alone).
/// - `room_finished`: everyone booked in that room leaves.
pub async fn handle_webhook_event(state: &AppState, event: &str, room: &str, identity: &str) {
    let Some(channel_id) = channel_of_room(room) else { return };
    let user = identity.parse::<u64>().ok();
    match (event, user) {
        ("participant_joined", Some(user_id)) => match state.voice.confirm(user_id, channel_id) {
            Some(entry) if entry.server_mute => enforce_publish_sources(state, &entry).await,
            Some(_) => {}
            None => kick(state, channel_id, user_id).await,
        },
        ("participant_left", Some(user_id)) => {
            if let Some(entry) = state.voice.remove_if_in_channel(user_id, channel_id) {
                publish_left(&state.events, entry.guild_id, entry.user_id);
            }
        }
        ("room_finished", _) => {
            for entry in state.voice.remove_channel(channel_id) {
                publish_left(&state.events, entry.guild_id, entry.user_id);
            }
        }
        _ => {}
    }
}

/// Starts the task that drops users who joined but never connected to LiveKit
/// within `state.livekit.connect_grace`. Ends when the voice states are gone.
pub fn spawn_sweeper(state: &AppState) {
    if tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    let states: Weak<VoiceStates> = Arc::downgrade(&state.voice);
    let events = state.events.clone();
    let grace = state.livekit.connect_grace;
    let period = (grace / 3).clamp(Duration::from_millis(50), Duration::from_secs(5));
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(period);
        loop {
            tick.tick().await;
            let Some(states) = states.upgrade() else { return };
            for entry in states.expire_unconnected(grace) {
                tracing::debug!(user = entry.user_id, channel = entry.channel_id, "voice join never connected; dropping");
                publish_left(&events, entry.guild_id, entry.user_id);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_names_round_trip() {
        assert_eq!(room_name(42), "voice-42");
        assert_eq!(channel_of_room("voice-42"), Some(42));
        assert_eq!(channel_of_room("lobby"), None);
        assert_eq!(channel_of_room("voice-x"), None);
    }

    #[test]
    fn publish_sources_follow_permissions_and_mute() {
        use PublishSource::*;
        assert_eq!(publish_sources(true, true, false), vec![Microphone, Camera, ScreenShare, ScreenShareAudio]);
        assert_eq!(publish_sources(true, false, false), vec![Microphone]);
        assert_eq!(publish_sources(true, true, true), vec![Camera, ScreenShare, ScreenShareAudio]);
        assert!(publish_sources(true, false, true).is_empty());
        assert!(publish_sources(false, true, false).is_empty());
    }

    #[test]
    fn one_entry_per_user_and_join_rules() {
        let states = VoiceStates::new();
        let (prev, first) = states.join(1, 10, 100);
        assert!(prev.is_none() && !first.connected);
        states.update(1, |e| {
            e.server_mute = true;
            e.self_mute = true;
            e.self_video = true;
        });
        // Same guild, other channel: mute flags stay, video resets.
        let (prev, moved) = states.join(1, 10, 101);
        assert_eq!(prev.unwrap().channel_id, 100);
        assert!(moved.server_mute && moved.self_mute && !moved.self_video);
        assert_eq!(states.len(), 1);
        // Other guild: fresh.
        let (_, other) = states.join(1, 11, 200);
        assert!(!other.server_mute && !other.self_mute);
        assert!(states.in_channel(101).is_empty() && states.in_channel(200).len() == 1);
    }

    #[test]
    fn expiry_confirm_and_removal() {
        let states = VoiceStates::new();
        states.join(1, 10, 100);
        states.join(2, 10, 100);
        states.confirm(2, 100).unwrap();
        assert!(states.confirm(2, 999).is_none());
        let expired = states.expire_unconnected(Duration::ZERO);
        assert_eq!(expired.iter().map(|e| e.user_id).collect::<Vec<_>>(), vec![1]);
        assert!(states.remove_if_in_channel(2, 999).is_none());
        assert_eq!(states.remove_channel(100).len(), 1);
        assert!(states.is_empty());
    }
}
