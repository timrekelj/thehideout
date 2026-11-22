//! Voice channels: the in-memory voice state, LiveKit access tokens, the small
//! LiveKit server-API surface the server needs, and the glue that keeps state,
//! gateway events and LiveKit rooms consistent. See `voice_service.proto` and
//! the README "Voice" section for the model.
//!
//! LiveKit moves the media; this server decides who may join and who is in
//! which channel. Room name = `voice-<channel_id>`, participant identity = the
//! user id (decimal), participant name = the username.

use std::collections::{HashMap, HashSet};
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
    /// LiveKit participant SID of the connection `participant_joined` last
    /// confirmed; a `participant_left` for another SID is a stale connection.
    pub sid: Option<String>,
    /// When the user joined this channel (not restarted by a same-channel
    /// rejoin); the connect grace runs from here.
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
///
/// Server mutes are kept per (guild, user), apart from the entries: a mute
/// outlives leaving and rejoining (also through another guild's voice), and
/// ends only with an unmute or the member leaving the guild. Like the entries
/// they are not persisted, so a restart clears them (the clients and LiveKit
/// reconcile through `JoinVoice` anyway).
#[derive(Default)]
pub struct VoiceStates {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    users: HashMap<u64, VoiceEntry>,
    /// `(guild_id, user_id)` pairs that are server-muted.
    mutes: HashSet<(u64, u64)>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl VoiceStates {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, user_id: u64) -> Option<VoiceEntry> {
        lock(&self.inner).users.get(&user_id).cloned()
    }

    /// Whether the user is server-muted in the guild, in voice or not.
    pub fn server_muted(&self, guild_id: u64, user_id: u64) -> bool {
        lock(&self.inner).mutes.contains(&(guild_id, user_id))
    }

    /// Puts the user in `channel_id` and returns `(previous, current)`. The
    /// same channel again is a reconnect: everything stays, including
    /// `connected` and the grace timer (the live LiveKit participant is
    /// replaced by the new connection, whose `participant_joined` confirms
    /// again). Another channel of the same guild keeps the self mute / deaf
    /// flags and resets video / stream; another guild starts fresh. Then the
    /// user must connect to LiveKit again. The server mute always follows the
    /// per-(guild, user) record.
    pub fn join(&self, user_id: u64, guild_id: u64, channel_id: u64) -> (Option<VoiceEntry>, VoiceEntry) {
        let mut inner = lock(&self.inner);
        let previous = inner.users.get(&user_id).cloned();
        let mut entry = VoiceEntry {
            guild_id,
            channel_id,
            user_id,
            self_mute: false,
            self_deaf: false,
            server_mute: inner.mutes.contains(&(guild_id, user_id)),
            self_video: false,
            self_stream: false,
            joined_at: Utc::now(),
            connected: false,
            sid: None,
            since: Instant::now(),
        };
        if let Some(prev) = previous.as_ref().filter(|p| p.guild_id == guild_id) {
            entry.self_mute = prev.self_mute;
            entry.self_deaf = prev.self_deaf;
            if prev.channel_id == channel_id {
                entry.self_video = prev.self_video;
                entry.self_stream = prev.self_stream;
                entry.joined_at = prev.joined_at;
                entry.connected = prev.connected;
                entry.sid = prev.sid.clone();
                entry.since = prev.since;
            }
        }
        inner.users.insert(user_id, entry.clone());
        (previous, entry)
    }

    /// Applies `change` to the user's entry and returns the result.
    pub fn update(&self, user_id: u64, change: impl FnOnce(&mut VoiceEntry)) -> Option<VoiceEntry> {
        let mut inner = lock(&self.inner);
        let entry = inner.users.get_mut(&user_id)?;
        change(entry);
        Some(entry.clone())
    }

    /// Like [`update`](Self::update), but only if the user is in a channel of
    /// `guild_id` (nothing changes otherwise).
    pub fn update_in_guild(&self, user_id: u64, guild_id: u64, change: impl FnOnce(&mut VoiceEntry)) -> Option<VoiceEntry> {
        let mut inner = lock(&self.inner);
        let entry = inner.users.get_mut(&user_id).filter(|e| e.guild_id == guild_id)?;
        change(entry);
        Some(entry.clone())
    }

    /// Server-mutes or unmutes the user in the guild and returns their entry.
    /// Only applied while they are still in `channel_id` of that guild (they
    /// may have moved or left since the caller looked); otherwise nothing
    /// changes and the result is `None`.
    pub fn set_server_mute(&self, guild_id: u64, user_id: u64, channel_id: u64, muted: bool) -> Option<VoiceEntry> {
        let mut inner = lock(&self.inner);
        let Inner { users, mutes } = &mut *inner;
        let entry = users.get_mut(&user_id).filter(|e| e.guild_id == guild_id && e.channel_id == channel_id)?;
        if muted {
            mutes.insert((guild_id, user_id));
        } else {
            mutes.remove(&(guild_id, user_id));
        }
        entry.server_mute = muted;
        Some(entry.clone())
    }

    pub fn remove(&self, user_id: u64) -> Option<VoiceEntry> {
        lock(&self.inner).users.remove(&user_id)
    }

    /// Removes the user only if they are in `channel_id` and, when LiveKit
    /// confirmed a connection (a recorded SID), only if `sid` is that
    /// connection: a late `participant_left` of an old connection does not
    /// drop a fresh rejoin.
    pub fn remove_if_in_channel(&self, user_id: u64, channel_id: u64, sid: &str) -> Option<VoiceEntry> {
        let mut inner = lock(&self.inner);
        let matches = inner
            .users
            .get(&user_id)
            .is_some_and(|e| e.channel_id == channel_id && e.sid.as_deref().is_none_or(|recorded| recorded == sid));
        if matches {
            inner.users.remove(&user_id)
        } else {
            None
        }
    }

    /// Removes everyone in `channel_id`.
    pub fn remove_channel(&self, channel_id: u64) -> Vec<VoiceEntry> {
        let mut removed = Vec::new();
        lock(&self.inner).users.retain(|_, e| {
            let keep = e.channel_id != channel_id;
            if !keep {
                removed.push(e.clone());
            }
            keep
        });
        removed
    }

    /// The user left the guild: forgets their server mute there and removes
    /// them if they are in a channel of it.
    pub fn remove_guild_member(&self, guild_id: u64, user_id: u64) -> Option<VoiceEntry> {
        let mut inner = lock(&self.inner);
        inner.mutes.remove(&(guild_id, user_id));
        if inner.users.get(&user_id).is_some_and(|e| e.guild_id == guild_id) {
            inner.users.remove(&user_id)
        } else {
            None
        }
    }

    /// Marks the user connected with LiveKit participant `sid` (recorded when
    /// not empty); `None` if they are not in `channel_id`.
    pub fn confirm(&self, user_id: u64, channel_id: u64, sid: &str) -> Option<VoiceEntry> {
        let mut inner = lock(&self.inner);
        let entry = inner.users.get_mut(&user_id).filter(|e| e.channel_id == channel_id)?;
        entry.connected = true;
        if !sid.is_empty() {
            entry.sid = Some(sid.to_owned());
        }
        Some(entry.clone())
    }

    /// Removes users who joined more than `grace` ago and never connected.
    pub fn expire_unconnected(&self, grace: Duration) -> Vec<VoiceEntry> {
        let mut removed = Vec::new();
        lock(&self.inner).users.retain(|_, e| {
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
            lock(&self.inner).users.values().filter(|e| guild_ids.contains(&e.guild_id)).cloned().collect();
        found.sort_by_key(|e| (e.guild_id, e.channel_id, e.joined_at, e.user_id));
        found
    }

    pub fn in_channel(&self, channel_id: u64) -> Vec<VoiceEntry> {
        let mut found: Vec<VoiceEntry> = lock(&self.inner).users.values().filter(|e| e.channel_id == channel_id).cloned().collect();
        found.sort_by_key(|e| (e.joined_at, e.user_id));
        found
    }

    pub fn len(&self) -> usize {
        lock(&self.inner).users.len()
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
    kick_participant(state.livekit.admin.as_ref(), channel_id, user_id).await;
}

/// [`kick`] without the app state (the sweeper must not hold the voice states
/// alive).
async fn kick_participant(admin: &dyn LiveKitAdmin, channel_id: u64, user_id: u64) {
    let (room, identity) = (room_name(channel_id), user_id.to_string());
    match tokio::time::timeout(LIVEKIT_CALL_TIMEOUT, admin.remove_participant(&room, &identity)).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::debug!(%room, %identity, error = %e, "removing the LiveKit participant failed"),
        Err(_) => tracing::warn!(%room, %identity, "removing the LiveKit participant timed out"),
    }
}

/// Pushes the permissions a member should have in their room to LiveKit
/// (after a server mute or permission change, and when they connect).
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

/// A member's roles, or the permissions of roles, changed: re-pushes the
/// publish permissions of the connected members of `guild_id` who are in voice
/// (just `user_id` when given), so losing SPEAK or VIDEO takes effect during
/// the call. Members who have not connected get theirs when they do.
pub async fn reenforce_permissions(state: &AppState, guild_id: u64, user_id: Option<u64>) {
    if state.livekit.config.is_none() {
        return;
    }
    let entries: Vec<VoiceEntry> = match user_id {
        Some(user_id) => state.voice.get(user_id).into_iter().collect(),
        None => state.voice.in_guilds(&[guild_id]),
    };
    for entry in entries.iter().filter(|e| e.guild_id == guild_id && e.connected) {
        enforce_publish_sources(state, entry).await;
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
/// - `participant_joined`: confirms the user's join and records the
///   participant `sid`; a user who is not (or no longer) in that channel by our
///   books is disconnected (a stale token). The member's current publish
///   permissions are pushed, covering a mute or role change that happened
///   before they connected.
/// - `participant_left`: the user leaves voice if still booked in that room (a
///   move or rejoin already changed the booking, which is then left alone), and
///   if `sid` is the connection we recorded: a late event of a replaced
///   connection (same identity) is ignored.
/// - `room_finished`: everyone booked in that room leaves.
pub async fn handle_webhook_event(state: &AppState, event: &str, room: &str, identity: &str, sid: &str) {
    let Some(channel_id) = channel_of_room(room) else { return };
    let user = identity.parse::<u64>().ok();
    match (event, user) {
        ("participant_joined", Some(user_id)) => match state.voice.confirm(user_id, channel_id, sid) {
            Some(entry) => enforce_publish_sources(state, &entry).await,
            None => kick(state, channel_id, user_id).await,
        },
        ("participant_left", Some(user_id)) => {
            if let Some(entry) = state.voice.remove_if_in_channel(user_id, channel_id, sid) {
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

/// Smallest gap between two "webhook may not be reaching the server" warnings.
const UNCONFIRMED_WARN_EVERY: Duration = Duration::from_secs(5 * 60);

/// Whether a warning may be logged now, given when the last one was; records
/// `now` when it may.
fn warn_due(last: &mut Option<Instant>, now: Instant, every: Duration) -> bool {
    if last.is_some_and(|l| now.duration_since(l) < every) {
        return false;
    }
    *last = Some(now);
    true
}

/// Starts the task that drops users who joined but never connected to LiveKit
/// within `state.livekit.connect_grace`, and disconnects their LiveKit
/// participant in case it is live after all (its `participant_joined` never
/// reached us). Ends when the voice states are gone.
pub fn spawn_sweeper(state: &AppState) {
    if tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    let states: Weak<VoiceStates> = Arc::downgrade(&state.voice);
    let events = state.events.clone();
    let admin = state.livekit.admin.clone();
    let configured = state.livekit.config.is_some();
    let grace = state.livekit.connect_grace;
    let period = (grace / 3).clamp(Duration::from_millis(50), Duration::from_secs(5));
    let shutdown = state.shutdown.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(period);
        let mut last_warning = None;
        loop {
            tokio::select! {
                _ = tick.tick() => {}
                _ = shutdown.triggered() => return,
            }
            let Some(states) = states.upgrade() else { return };
            let dropped = states.expire_unconnected(grace);
            drop(states);
            for entry in dropped {
                tracing::debug!(user = entry.user_id, channel = entry.channel_id, "voice join never connected; dropping");
                if warn_due(&mut last_warning, Instant::now(), UNCONFIRMED_WARN_EVERY) {
                    tracing::warn!(
                        "a voice join never got a LiveKit participant_joined; LIVEKIT webhook URL may not reach this server \
                         (LiveKit's webhook.urls must point at <server>/livekit/webhook, signed with the same key and secret), \
                         or the client could not connect to LIVEKIT_URL"
                    );
                }
                publish_left(&events, entry.guild_id, entry.user_id);
                if configured {
                    kick_participant(admin.as_ref(), entry.channel_id, entry.user_id).await;
                }
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
        states.set_server_mute(10, 1, 100, true).unwrap();
        states.update(1, |e| {
            e.self_mute = true;
            e.self_video = true;
        });
        // Same guild, other channel: mute flags stay, video resets.
        let (prev, moved) = states.join(1, 10, 101);
        assert_eq!(prev.unwrap().channel_id, 100);
        assert!(moved.server_mute && moved.self_mute && !moved.self_video);
        assert_eq!(states.len(), 1);
        // Other guild: fresh self flags, and no server mute there.
        let (_, other) = states.join(1, 11, 200);
        assert!(!other.server_mute && !other.self_mute);
        assert!(states.in_channel(101).is_empty() && states.in_channel(200).len() == 1);
        // Back in the first guild the server mute is still on.
        assert!(states.join(1, 10, 100).1.server_mute);
    }

    #[test]
    fn server_mute_is_kept_per_guild_and_user_apart_from_the_entry() {
        let states = VoiceStates::new();
        states.join(1, 10, 100);
        let muted = states.set_server_mute(10, 1, 100, true).unwrap();
        assert!(muted.server_mute && states.server_muted(10, 1) && !states.server_muted(11, 1));
        // Leaving keeps it; rejoining brings it back.
        states.remove(1);
        assert!(states.server_muted(10, 1));
        assert!(states.join(1, 10, 100).1.server_mute);
        // A stale target (moved, other guild, gone) changes nothing.
        assert!(states.set_server_mute(10, 1, 999, false).is_none());
        assert!(states.set_server_mute(11, 1, 100, false).is_none());
        assert!(states.server_muted(10, 1) && states.get(1).unwrap().server_mute);
        states.join(1, 10, 101);
        // Self flags change only for the guild the caller checked.
        assert!(states.update_in_guild(1, 11, |e| e.self_video = true).is_none() && !states.get(1).unwrap().self_video);
        assert!(states.update_in_guild(1, 10, |e| e.self_video = true).is_some_and(|e| e.self_video));
        assert!(states.set_server_mute(10, 1, 100, false).is_none(), "moved to 101 meanwhile");
        assert!(states.server_muted(10, 1));
        // Unmute clears it; a member leaving the guild forgets it.
        assert!(!states.set_server_mute(10, 1, 101, false).unwrap().server_mute);
        assert!(!states.server_muted(10, 1) && !states.join(1, 10, 101).1.server_mute);
        states.set_server_mute(10, 1, 101, true).unwrap();
        assert!(states.remove_guild_member(10, 1).is_some());
        assert!(!states.server_muted(10, 1));
        assert!(!states.join(1, 10, 100).1.server_mute);
    }

    #[test]
    fn same_channel_rejoin_keeps_connection_and_grace_timer() {
        let states = VoiceStates::new();
        let (_, first) = states.join(1, 10, 100);
        let confirmed = states.confirm(1, 100, "PA_1").unwrap();
        assert!(confirmed.connected);
        let (_, again) = states.join(1, 10, 100);
        assert!(again.connected && again.sid.as_deref() == Some("PA_1"));
        assert_eq!((again.since, again.joined_at), (first.since, first.joined_at));
        assert!(states.expire_unconnected(Duration::ZERO).is_empty());
        // Another channel starts over.
        let (_, moved) = states.join(1, 10, 101);
        assert!(!moved.connected && moved.sid.is_none());
        assert_eq!(states.expire_unconnected(Duration::ZERO).len(), 1);
    }

    #[test]
    fn participant_left_with_an_old_sid_is_ignored() {
        let states = VoiceStates::new();
        states.join(1, 10, 100);
        states.confirm(1, 100, "PA_new").unwrap();
        assert!(states.remove_if_in_channel(1, 100, "PA_old").is_none());
        assert!(states.remove_if_in_channel(1, 999, "PA_new").is_none());
        assert!(states.remove_if_in_channel(1, 100, "PA_new").is_some());
        // Without a recorded SID any participant_left of the room counts.
        states.join(1, 10, 100);
        assert!(states.remove_if_in_channel(1, 100, "PA_any").is_some());
    }

    #[test]
    fn unconfirmed_warnings_are_rate_limited() {
        let (mut last, start) = (None, Instant::now());
        let every = Duration::from_secs(60);
        assert!(warn_due(&mut last, start, every));
        assert!(!warn_due(&mut last, start + Duration::from_secs(59), every));
        assert!(warn_due(&mut last, start + Duration::from_secs(61), every));
        assert!(!warn_due(&mut last, start + Duration::from_secs(100), every));
    }

    #[test]
    fn expiry_confirm_and_removal() {
        let states = VoiceStates::new();
        states.join(1, 10, 100);
        states.join(2, 10, 100);
        states.confirm(2, 100, "").unwrap();
        assert!(states.confirm(2, 999, "").is_none());
        let expired = states.expire_unconnected(Duration::ZERO);
        assert_eq!(expired.iter().map(|e| e.user_id).collect::<Vec<_>>(), vec![1]);
        assert!(states.remove_if_in_channel(2, 999, "").is_none());
        assert_eq!(states.remove_channel(100).len(), 1);
        assert!(states.is_empty());
    }
}
