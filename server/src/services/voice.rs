//! `VoiceService` and the LiveKit webhook endpoint. State, tokens and LiveKit
//! calls live in `crate::voice`; see `voice_service.proto` for the contract.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use connectrpc::{ConnectError, RequestContext, Response, ServiceRequest, ServiceResult};
use livekit_api::webhooks::WebhookError;

use crate::AppState;
use crate::access::{channel_kind, find_user, is_member};
use crate::auth::{authenticate, internal};
use crate::config::LiveKitConfig;
use crate::convert::{id_from_db, id_to_db};
use crate::permissions::{CONNECT, MOVE_MEMBERS, MUTE_MEMBERS, SPEAK, VIDEO, Standing, require_permission, standing};
use crate::proto::thehideout::v1::*;
use crate::voice::{self, VoiceEntry, publish_left, publish_state};

/// JoinVoice calls allowed per user within [`JOIN_WINDOW`].
pub const JOIN_MAX: usize = 30;
const JOIN_WINDOW: Duration = Duration::from_secs(60);

pub struct VoiceSvc {
    state: AppState,
}

impl VoiceSvc {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    fn config(&self) -> Result<Arc<LiveKitConfig>, ConnectError> {
        self.state
            .livekit
            .config
            .clone()
            .ok_or_else(|| ConnectError::unavailable("voice is not configured on this server"))
    }

    /// The guild of `channel_id` (`NotFound` for unknown channels).
    async fn guild_of_channel(&self, channel_id: u64) -> Result<u64, ConnectError> {
        self.state
            .session
            .execute_unpaged("SELECT guild_id FROM channels_by_id WHERE id = ?", (id_to_db(channel_id),))
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .maybe_first_row::<(i64,)>()
            .map_err(internal)?
            .map(|(g,)| id_from_db(g))
            .ok_or_else(|| ConnectError::not_found("channel not found"))
    }

    /// Succeeds for a voice channel of `guild_id`.
    async fn require_voice_channel(&self, guild_id: u64, channel_id: u64) -> Result<(), ConnectError> {
        if channel_kind(&self.state, guild_id, channel_id).await? == ChannelKind::CHANNEL_KIND_VOICE {
            Ok(())
        } else {
            Err(ConnectError::failed_precondition("not a voice channel"))
        }
    }

    /// Checks `caller` may moderate `target_id` and that the target is in a
    /// voice channel of the guild; returns the target's entry and standing.
    async fn moderation_target(
        &self,
        caller_id: u64,
        caller: &Standing,
        guild_id: u64,
        target_id: u64,
    ) -> Result<(VoiceEntry, Standing), ConnectError> {
        let target = standing(&self.state, target_id, guild_id)
            .await
            .map_err(|_| ConnectError::not_found("member not found"))?;
        if target_id != caller_id && !caller.outranks_member(&target) {
            return Err(ConnectError::permission_denied("member's highest role is not below yours"));
        }
        let entry = self
            .state
            .voice
            .get(target_id)
            .filter(|e| e.guild_id == guild_id)
            .ok_or_else(|| ConnectError::failed_precondition("member is not in a voice channel"))?;
        Ok((entry, target))
    }
}

impl VoiceService for VoiceSvc {
    async fn join_voice(&self, ctx: RequestContext, req: ServiceRequest<'_, JoinVoiceRequest>) -> ServiceResult<JoinVoiceResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let config = self.config()?;
        self.state.limiter.check(&format!("voice_join:{user_id}"), JOIN_MAX, JOIN_WINDOW)?;

        let channel_id = req.channel_id;
        let guild_id = self.guild_of_channel(channel_id).await?;
        if !is_member(&self.state, user_id, guild_id).await? {
            return Err(ConnectError::not_found("channel not found"));
        }
        self.require_voice_channel(guild_id, channel_id).await?;
        let caller = standing(&self.state, user_id, guild_id).await?;
        if !caller.has(CONNECT) {
            return Err(ConnectError::permission_denied("missing permission"));
        }
        let username = find_user(&self.state, id_to_db(user_id))
            .await?
            .map(|u| u.username)
            .ok_or_else(|| ConnectError::unauthenticated("unknown user"))?;

        // Sign first so a failure leaves the voice state untouched. The mute
        // flag a user keeps within the guild decides the microphone grant.
        let kept_mute = self
            .state
            .voice
            .get(user_id)
            .is_some_and(|e| e.guild_id == guild_id && e.server_mute);
        let sources = voice::publish_sources(caller.has(SPEAK), caller.has(VIDEO), kept_mute);
        let token = voice::access_token(&config, user_id, &username, channel_id, &sources)?;

        let (previous, entry) = self.state.voice.join(user_id, guild_id, channel_id);
        publish_state(&self.state.events, &entry);
        if let Some(prev) = previous.filter(|p| p.channel_id != channel_id) {
            if prev.guild_id != guild_id {
                publish_left(&self.state.events, prev.guild_id, user_id);
            }
            // The old token still names the old room: close that connection.
            voice::kick(&self.state, prev.channel_id, user_id).await;
        }
        Response::ok(JoinVoiceResponse { url: config.url.clone(), token, channel_id, ..Default::default() })
    }

    async fn leave_voice(&self, ctx: RequestContext, _req: ServiceRequest<'_, LeaveVoiceRequest>) -> ServiceResult<LeaveVoiceResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        voice::leave(&self.state, user_id).await;
        Response::ok(LeaveVoiceResponse::default())
    }

    async fn update_voice_state(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, UpdateVoiceStateRequest>,
    ) -> ServiceResult<UpdateVoiceStateResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let current = self
            .state
            .voice
            .get(user_id)
            .ok_or_else(|| ConnectError::failed_precondition("not in a voice channel"))?;
        if req.self_video == Some(true) || req.self_stream == Some(true) {
            let me = standing(&self.state, user_id, current.guild_id).await?;
            if !me.has(VIDEO) {
                return Err(ConnectError::permission_denied("missing permission"));
            }
        }
        let (self_mute, self_deaf, self_video, self_stream) = (req.self_mute, req.self_deaf, req.self_video, req.self_stream);
        let entry = self
            .state
            .voice
            .update(user_id, |e| {
                e.self_mute = self_mute.unwrap_or(e.self_mute);
                e.self_deaf = self_deaf.unwrap_or(e.self_deaf);
                e.self_video = self_video.unwrap_or(e.self_video);
                e.self_stream = self_stream.unwrap_or(e.self_stream);
            })
            .ok_or_else(|| ConnectError::failed_precondition("not in a voice channel"))?;
        publish_state(&self.state.events, &entry);
        Response::ok(UpdateVoiceStateResponse { state: Some(entry.to_proto()).into(), ..Default::default() })
    }

    async fn server_mute_member(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, ServerMuteMemberRequest>,
    ) -> ServiceResult<ServerMuteMemberResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let (guild_id, target_id, muted) = (req.guild_id, req.user_id, req.muted);
        let caller = require_permission(&self.state, user_id, guild_id, MUTE_MEMBERS).await?;
        if target_id == user_id {
            return Err(ConnectError::invalid_argument("you cannot server-mute yourself"));
        }
        let (entry, _) = self.moderation_target(user_id, &caller, guild_id, target_id).await?;
        let entry = self
            .state
            .voice
            .update(target_id, |e| e.server_mute = muted)
            .filter(|e| e.channel_id == entry.channel_id)
            .ok_or_else(|| ConnectError::failed_precondition("member is not in a voice channel"))?;
        publish_state(&self.state.events, &entry);
        voice::enforce_publish_sources(&self.state, &entry).await;
        Response::ok(ServerMuteMemberResponse { state: Some(entry.to_proto()).into(), ..Default::default() })
    }

    async fn move_member(&self, ctx: RequestContext, req: ServiceRequest<'_, MoveMemberRequest>) -> ServiceResult<MoveMemberResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let (guild_id, target_id, channel_id) = (req.guild_id, req.user_id, req.channel_id);
        let caller = require_permission(&self.state, user_id, guild_id, MOVE_MEMBERS).await?;
        let (current, target) = self.moderation_target(user_id, &caller, guild_id, target_id).await?;
        if self.guild_of_channel(channel_id).await? != guild_id {
            return Err(ConnectError::not_found("channel not found"));
        }
        self.require_voice_channel(guild_id, channel_id).await?;
        if !target.has(CONNECT) {
            return Err(ConnectError::failed_precondition("member cannot connect to voice"));
        }
        if current.channel_id == channel_id {
            return Response::ok(MoveMemberResponse { state: Some(current.to_proto()).into(), ..Default::default() });
        }
        let (_, entry) = self.state.voice.join(target_id, guild_id, channel_id);
        publish_state(&self.state.events, &entry);
        // Close the old connection (and its token), then tell the client where to go.
        voice::kick(&self.state, current.channel_id, target_id).await;
        self.state.events.notify_user_voice_moved(target_id, VoiceMoved { guild_id, channel_id, ..Default::default() });
        Response::ok(MoveMemberResponse { state: Some(entry.to_proto()).into(), ..Default::default() })
    }

    async fn disconnect_member(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, DisconnectMemberRequest>,
    ) -> ServiceResult<DisconnectMemberResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let (guild_id, target_id) = (req.guild_id, req.user_id);
        let caller = require_permission(&self.state, user_id, guild_id, MOVE_MEMBERS).await?;
        self.moderation_target(user_id, &caller, guild_id, target_id).await?;
        voice::leave(&self.state, target_id).await;
        Response::ok(DisconnectMemberResponse::default())
    }
}

/// `POST /livekit/webhook`: LiveKit's webhook receiver. The body is verified
/// against the `Authorization` JWT (signed with the API secret, carrying the
/// SHA-256 of the body). 503 when voice is not configured, 401 for a bad
/// signature, 400 for a body that is not a webhook event.
pub fn routes() -> Router<AppState> {
    Router::new().route("/livekit/webhook", post(webhook))
}

async fn webhook(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> StatusCode {
    let Some(receiver) = state.livekit.webhook_receiver() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let auth = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.strip_prefix("Bearer ").unwrap_or(v).trim());
    let (Some(auth), Ok(body)) = (auth, std::str::from_utf8(&body)) else {
        return StatusCode::UNAUTHORIZED;
    };
    let event = match receiver.receive(body, auth) {
        Ok(event) => event,
        Err(WebhookError::InvalidData(_)) => return StatusCode::BAD_REQUEST,
        Err(_) => return StatusCode::UNAUTHORIZED,
    };
    let room = event.room.as_ref().map(|r| r.name.as_str()).unwrap_or_default();
    let identity = event.participant.as_ref().map(|p| p.identity.as_str()).unwrap_or_default();
    voice::handle_webhook_event(&state, &event.event, room, identity).await;
    StatusCode::OK
}
