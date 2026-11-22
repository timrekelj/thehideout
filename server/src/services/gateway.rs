use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use connectrpc::{ConnectError, RequestContext, Response, ServiceRequest, ServiceResult, ServiceStream};
use tokio::sync::broadcast::{self, error::RecvError};
use tokio::time::{Instant, Interval, MissedTickBehavior};
use tokio_stream::{StreamExt, StreamMap};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

use crate::AppState;
use crate::auth::{authenticate, bearer_token, hash_token, internal, session_exists};
use crate::convert::{id_from_db, id_to_db, system_time_to_proto};
use crate::events::{EventPayload, UserControl};
use crate::proto::thehideout::v1::event::Payload;
use crate::proto::thehideout::v1::*;
use crate::streams::{Denied, StreamPermit};

pub struct GatewaySvc {
    state: AppState,
}

impl GatewaySvc {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

impl GatewayService for GatewaySvc {
    /// Streams events for every guild the caller belongs to and every DM channel they
    /// participate in, plus any guild they join or DM opened with them while the stream is
    /// open.
    ///
    /// Each stream numbers its events from 1 and sends a heartbeat immediately and then every
    /// heartbeat interval. Events are not replayed: anything published before the stream
    /// opened (or while it was disconnected) must be fetched with the `List*` RPCs.
    ///
    /// Slow clients: each guild buffers a bounded number of events per receiver. If a client
    /// falls behind and events are dropped, the stream ends with `ResourceExhausted` rather
    /// than silently skipping events; the client should reconnect and resync via the `List*`
    /// RPCs.
    async fn subscribe(
        &self,
        ctx: RequestContext,
        _req: ServiceRequest<'_, SubscribeRequest>,
    ) -> ServiceResult<ServiceStream<SubscribeResponse>> {
        let user_id = authenticate(&self.state, &ctx).await?;
        // Before any other work, so a refused stream costs one session lookup.
        let permit = self.state.streams.acquire(user_id).map_err(|denied| {
            tracing::warn!(user = user_id, ?denied, open = self.state.streams.open(), "gateway stream refused");
            match denied {
                Denied::User => ConnectError::resource_exhausted("too many open event streams for this account; close one and retry"),
                Denied::Server => ConnectError::resource_exhausted("the server has too many open event streams; retry shortly"),
            }
        })?;
        if self.state.shutdown.is_triggered() {
            return Err(ConnectError::unavailable("server is shutting down"));
        }
        let token_hash = hash_token(bearer_token(&ctx).expect("authenticate succeeded"));
        // Before the heartbeat clock starts, so the bookkeeping does not eat into the first interval.
        crate::stats::touch_user(&self.state, user_id).await;
        let bus = &self.state.events;

        // Subscribe to the control channel before reading memberships/DMs so a guild joined
        // or a DM opened in between is not missed (re-adding an already-subscribed channel is
        // harmless).
        let control = bus.subscribe_user(user_id);
        let guild_ids = self
            .state
            .session
            .execute_unpaged("SELECT guild_id FROM guilds_by_user WHERE user_id = ?", (id_to_db(user_id),))
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .rows::<(i64,)>()
            .map_err(internal)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(internal)?;
        let dm_ids = self
            .state
            .session
            .execute_unpaged("SELECT dm_id FROM dms_by_user WHERE user_id = ?", (id_to_db(user_id),))
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .rows::<(i64,)>()
            .map_err(internal)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(internal)?;
        let mut guilds = StreamMap::new();
        for (id,) in guild_ids {
            let id = id_from_db(id);
            guilds.insert(id, BroadcastStream::new(bus.subscribe_guild(id)));
        }
        let mut dms = StreamMap::new();
        for (id,) in dm_ids {
            let id = id_from_db(id);
            dms.insert(id, BroadcastStream::new(bus.subscribe_dm(id)));
        }

        let mut heartbeat = tokio::time::interval_at(
            Instant::now() + self.state.heartbeat_interval,
            self.state.heartbeat_interval,
        );
        heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);

        self.state.live.connections.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // After the guild subscriptions exist, so a change in between is not
        // missed (a repeated state is harmless). Sent only when someone is in voice.
        let guild_list: Vec<u64> = guilds.keys().copied().collect();
        let mut pending = VecDeque::new();
        if let Some(snapshot) = voice_snapshot(&self.state, &guild_list) {
            pending.push_back(snapshot);
        }
        let session = EventStream {
            state: self.state.clone(),
            pending,
            control: Some(control),
            guilds,
            dms,
            heartbeat,
            sequence: 0,
            token_hash: token_hash.clone(),
            last_check: Instant::now(),
            recheck_interval: self.state.heartbeat_interval.min(Duration::from_secs(60)),
            _permit: permit,
            _presence: self.state.push.presence.open(user_id, &token_hash),
        };
        Response::stream_ok(futures::stream::unfold(Some(session), |session| async move {
            let mut session = session?;
            match session.next_event().await {
                Ok(event) => Some((Ok(SubscribeResponse { event: event.into(), ..Default::default() }), Some(session))),
                Err(err) => Some((Err(err), None)),
            }
        }))
    }
}

/// State of one open Subscribe stream.
struct EventStream {
    state: AppState,
    /// Events queued behind the first heartbeat (voice snapshots).
    pending: VecDeque<Payload>,
    /// `None` only while dropping.
    control: Option<broadcast::Receiver<UserControl>>,
    guilds: StreamMap<u64, BroadcastStream<Arc<EventPayload>>>,
    dms: StreamMap<u64, BroadcastStream<Arc<EventPayload>>>,
    heartbeat: Interval,
    sequence: u64,
    /// Hash of the token the stream was opened with, re-checked periodically.
    token_hash: String,
    last_check: Instant,
    recheck_interval: Duration,
    /// Holds this stream's slot against the per-user and global caps.
    _permit: StreamPermit,
    /// Counts the stream as live for push suppression (see `push::Presence`).
    _presence: crate::push::PresenceGuard,
}

impl EventStream {
    /// Waits for the next outgoing event. The first call returns a heartbeat immediately.
    async fn next_event(&mut self) -> Result<Event, ConnectError> {
        let payload = if self.sequence == 0 {
            heartbeat_payload()
        } else if let Some(queued) = self.pending.pop_front() {
            queued
        } else {
            self.next_payload().await?
        };
        self.sequence += 1;
        Ok(Event {
            sequence: self.sequence,
            payload: Some(payload),
            ..Default::default()
        })
    }

    async fn next_payload(&mut self) -> Result<Payload, ConnectError> {
        let control = self.control.as_mut().expect("control receiver present while streaming");
        tokio::select! {
            // First, so a stream with a busy bus still ends promptly on shutdown.
            biased;
            _ = self.state.shutdown.triggered() => Err(ConnectError::unavailable("server is shutting down; reconnect")),
            _ = self.heartbeat.tick() => {
                // Re-verify the session so a token that expired (Scylla TTL) or was
                // revoked cannot keep an open stream alive past its validity.
                if self.last_check.elapsed() >= self.recheck_interval {
                    self.last_check = Instant::now();
                    if !session_exists(&self.state, &self.token_hash).await? {
                        return Err(ConnectError::unauthenticated("session expired"));
                    }
                }
                Ok(heartbeat_payload())
            }
            msg = control.recv() => match msg {
                Ok(UserControl::JoinedGuild(guild)) => {
                    // Subscribe to the guild, then tell the client about it so other
                    // devices of the user pick it up without a restart.
                    let rx = self.state.events.subscribe_guild(guild.id);
                    self.guilds.insert(guild.id, BroadcastStream::new(rx));
                    if let Some(snapshot) = voice_snapshot(&self.state, &[guild.id]) {
                        self.pending.push_back(snapshot);
                    }
                    Ok(Payload::GuildAdded(guild.into()))
                }
                Ok(UserControl::OpenedDm(channel)) => {
                    // Subscribe to the DM channel, then emit the event itself so the
                    // client learns about the new conversation on this stream.
                    let rx = self.state.events.subscribe_dm(channel.id);
                    self.dms.insert(channel.id, BroadcastStream::new(rx));
                    Ok(Payload::DmOpened(channel.into()))
                }
                Ok(UserControl::ProfileUpdated(user)) => Ok(Payload::UserUpdated(user.into())),
                Ok(UserControl::VoiceMoved(moved)) => Ok(Payload::VoiceMoved(moved.into())),
                Ok(UserControl::SettingsUpdated(settings)) => Ok(Payload::NotificationSettingsUpdated(settings.into())),
                Ok(UserControl::RemovedFromGuild(removed)) => {
                    // Stop listening first: nothing of this guild is delivered after this.
                    self.guilds.remove(&removed.guild_id);
                    Ok(Payload::GuildRemoved(removed.into()))
                }
                Err(RecvError::Lagged(_)) => Err(lagged()),
                Err(RecvError::Closed) => Err(internal("user control channel closed")),
            },
            Some((_, item)) = self.guilds.next(), if !self.guilds.is_empty() => match item {
                Ok(event) => Ok(to_payload(&event)),
                Err(BroadcastStreamRecvError::Lagged(_)) => Err(lagged()),
            },
            Some((_, item)) = self.dms.next(), if !self.dms.is_empty() => match item {
                Ok(event) => Ok(to_payload(&event)),
                Err(BroadcastStreamRecvError::Lagged(_)) => Err(lagged()),
            },
        }
    }
}

impl Drop for EventStream {
    /// Releases every receiver first so the bus can drop channels nobody listens to.
    fn drop(&mut self) {
        self.guilds.clear();
        self.dms.clear();
        self.control = None;
        self.state.events.prune();
        self.state.live.connections.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

fn heartbeat_payload() -> Payload {
    Payload::Heartbeat(
        Heartbeat {
            server_time: system_time_to_proto(SystemTime::now()).into(),
            ..Default::default()
        }
        .into(),
    )
}

fn to_payload(event: &EventPayload) -> Payload {
    match event.clone() {
        EventPayload::MessageCreated(m) => Payload::MessageCreated(m.into()),
        EventPayload::MessageUpdated(m) => Payload::MessageUpdated(
            MessageUpdated {
                message: m.clone().into(),
                ..Default::default()
            }
            .into(),
        ),
        EventPayload::ChannelCreated(c) => Payload::ChannelCreated(c.into()),
        EventPayload::GuildMemberJoined(j) => Payload::GuildMemberJoined(j.into()),
        EventPayload::GuildUpdated(g) => Payload::GuildUpdated(g.into()),
        EventPayload::ChannelUpdated(c) => Payload::ChannelUpdated(c.into()),
        EventPayload::LayoutUpdated(l) => Payload::LayoutUpdated(l.into()),
        EventPayload::RolesUpdated(r) => Payload::RolesUpdated(r.into()),
        EventPayload::MemberUpdated(m) => Payload::MemberUpdated(m.into()),
        EventPayload::EmojisUpdated(e) => Payload::EmojisUpdated(e.into()),
        EventPayload::ReactionAdded(r) => Payload::ReactionAdded(r.into()),
        EventPayload::ReactionRemoved(r) => Payload::ReactionRemoved(r.into()),
        EventPayload::VoiceStateUpdate(v) => Payload::VoiceStateUpdate(v.into()),
        EventPayload::MessageDeleted(d) => Payload::MessageDeleted(d.into()),
        EventPayload::GuildMemberRemoved(r) => Payload::GuildMemberRemoved(r.into()),
        EventPayload::ChannelDeleted(c) => Payload::ChannelDeleted(c.into()),
    }
}

/// The voice states of `guild_ids` as a `voice_snapshot` payload; `None` when nobody is in voice there.
fn voice_snapshot(state: &AppState, guild_ids: &[u64]) -> Option<Payload> {
    let states = state.voice.in_guilds(guild_ids);
    if states.is_empty() {
        return None;
    }
    Some(Payload::VoiceSnapshot(
        VoiceSnapshot {
            guild_ids: guild_ids.to_vec(),
            states: states.iter().map(|e| e.to_proto()).collect(),
            ..Default::default()
        }
        .into(),
    ))
}

fn lagged() -> ConnectError {
    ConnectError::resource_exhausted(
        "event stream fell behind and events were dropped; reconnect and resync via the List* RPCs",
    )
}
