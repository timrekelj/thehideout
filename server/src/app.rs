use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::http::{HeaderName, HeaderValue, Method};
use axum::routing::get;
use connectrpc::Router as ConnectRouter;
use scylla::client::caching_session::CachingSession;
use tower_http::cors::{AllowOrigin, CorsLayer};
use url::Url;

fn default_public_url() -> Url {
    Url::parse("http://localhost:8080").expect("valid default URL")
}

use crate::auth::SeenThrottle;
use crate::email::Mailer;
use crate::embeds::EmbedSettings;
use crate::events::EventBus;
use crate::media::attachments::AttachmentSettings;
use crate::media::{self, MAX_INPUT_BYTES};
use crate::ratelimit::RateLimiter;
use crate::services::auth::AuthMemory;
use crate::services::{AdminSvc, AuthSvc, DmSvc, EmojiSvc, GatewaySvc, GuildSvc, InviteSvc, MediaSvc, MessageSvc, RoleSvc, VoiceSvc};
use crate::snowflake::Snowflake;
use crate::storage::Storage;

/// Shared application state handed to every service.
#[derive(Clone)]
pub struct AppState {
    pub session: Arc<CachingSession>,
    pub snowflake: Arc<Snowflake>,
    pub keyspace: String,
    pub events: Arc<EventBus>,
    pub limiter: Arc<RateLimiter>,
    /// How often the gateway sends a heartbeat on each open stream.
    pub heartbeat_interval: Duration,
    /// Object storage for media; `None` makes the media RPCs and `/media`
    /// answer UNAVAILABLE while everything else works.
    pub storage: Option<Storage>,
    /// Limits and timings of message attachments.
    pub attachments: AttachmentSettings,
    /// Link embed pipeline (SSRF-safe unfurling of URLs in messages).
    pub embeds: EmbedSettings,
    /// Usernames promoted to site admin at startup and at sign-up
    /// (`THEHIDEOUT_ADMINS`, normalised).
    pub admins: Arc<HashSet<String>>,
    /// Open gateway streams and "active today" bookkeeping.
    pub live: Arc<crate::stats::Live>,
    /// Serialises SetAdmin so two concurrent revocations cannot remove the last admin.
    pub admin_lock: Arc<tokio::sync::Mutex<()>>,
    /// Outgoing email (in-memory by default; see [`AppState::with_mailer`]).
    pub mailer: Mailer,
    /// Base URL of links in emails and the WebAuthn origin (`THEHIDEOUT_PUBLIC_URL`).
    pub public_url: Url,
    /// The WebAuthn relying party; `None` when `public_url` has no domain name.
    pub webauthn: Option<Arc<webauthn_rs::Webauthn>>,
    /// Short-lived auth state: second-factor challenges, TOTP setups, passkey ceremonies.
    pub auth: Arc<AuthMemory>,
    /// Throttles session "last seen" writes.
    pub seen: Arc<SeenThrottle>,
    /// Who is in which voice channel (memory only).
    pub voice: Arc<crate::voice::VoiceStates>,
    /// LiveKit settings and server-API client; voice is off when unconfigured.
    pub livekit: crate::voice::LiveKit,
}

impl AppState {
    pub fn new(
        session: Arc<CachingSession>,
        snowflake: Arc<Snowflake>,
        keyspace: String,
        heartbeat_interval: Duration,
    ) -> Self {
        Self {
            session,
            snowflake,
            keyspace,
            events: Arc::new(EventBus::new()),
            limiter: Arc::new(RateLimiter::new()),
            heartbeat_interval,
            storage: None,
            attachments: AttachmentSettings::default(),
            embeds: EmbedSettings::default(),
            admins: Arc::default(),
            live: Arc::default(),
            admin_lock: Arc::default(),
            mailer: Mailer::memory(),
            webauthn: crate::passkeys::relying_party(&default_public_url()),
            public_url: default_public_url(),
            auth: Arc::default(),
            seen: Arc::default(),
            voice: Arc::default(),
            livekit: crate::voice::LiveKit::disabled(),
        }
    }

    /// Enables voice channels with the given LiveKit settings (`LIVEKIT_*`, tests).
    pub fn with_livekit(mut self, livekit: crate::voice::LiveKit) -> Self {
        self.livekit = livekit;
        self
    }

    /// Sets the public base URL (and rebuilds the WebAuthn relying party).
    pub fn with_public_url(mut self, url: Url) -> Self {
        self.webauthn = crate::passkeys::relying_party(&url);
        self.public_url = url;
        self
    }

    /// Sets the email sender.
    pub fn with_mailer(mut self, mailer: Mailer) -> Self {
        self.mailer = mailer;
        self
    }

    /// `public_url` without a trailing slash, for building links.
    pub fn public_base(&self) -> String {
        self.public_url.as_str().trim_end_matches('/').to_owned()
    }

    /// Sets the usernames promoted to site admin (`THEHIDEOUT_ADMINS`).
    pub fn with_admins(mut self, admins: impl IntoIterator<Item = String>) -> Self {
        self.admins = Arc::new(admins.into_iter().collect());
        self
    }

    /// Overrides the attachment limits (`THEHIDEOUT_MAX_UPLOAD_BYTES`, tests).
    pub fn with_attachment_settings(mut self, attachments: AttachmentSettings) -> Self {
        self.attachments = attachments;
        self
    }

    /// Overrides the embed settings (`THEHIDEOUT_EMBEDS`, tests).
    pub fn with_embed_settings(mut self, embeds: EmbedSettings) -> Self {
        self.embeds = embeds;
        self
    }

    /// Enables media storage.
    pub fn with_storage(mut self, storage: Storage) -> Self {
        self.storage = Some(storage);
        self
    }
}

/// Largest Connect request body of the media RPCs: the image as protobuf
/// bytes, or base64 inside Connect JSON (4/3 of the size), plus the other
/// fields.
const MEDIA_REQUEST_LIMIT: usize = MAX_INPUT_BYTES / 3 * 4 + 64 * 1024;

/// Builds the HTTP app: all Connect services, `GET /health`, `GET /media/*`,
/// `POST /attachments`, the passkey ceremony page (`/auth/passkey*`), the
/// invite landing page (`GET /invite/{token}`), the LiveKit webhook
/// (`POST /livekit/webhook`), and CORS for the configured public origin only.
pub fn build_app(state: AppState) -> Router {
    crate::voice::spawn_sweeper(&state);
    let public_url = state.public_url.clone();
    let connect = ConnectRouter::new()
        .add_service(Arc::new(AuthSvc::new(state.clone())))
        .add_service(Arc::new(GuildSvc::new(state.clone())))
        .add_service(Arc::new(MessageSvc::new(state.clone())))
        .add_service(Arc::new(DmSvc::new(state.clone())))
        .add_service(Arc::new(RoleSvc::new(state.clone())))
        .add_service(Arc::new(AdminSvc::new(state.clone())))
        .add_service(Arc::new(InviteSvc::new(state.clone())))
        .add_service(Arc::new(EmojiSvc::new(state.clone())))
        .add_service(Arc::new(VoiceSvc::new(state.clone())))
        .add_service(Arc::new(GatewaySvc::new(state.clone())));

    // Media RPCs get their own router so only they accept large bodies; the
    // library default (4 MB) keeps applying to everything else. A body over
    // the limit is refused with a Connect error.
    let media_limits = connectrpc::Limits::default()
        .with_max_request_body_size(MEDIA_REQUEST_LIMIT)
        .with_max_message_size(MEDIA_REQUEST_LIMIT);
    let media_connect = ConnectRouter::new()
        .add_service(Arc::new(MediaSvc::new(state.clone())))
        .into_axum_service()
        .with_limits(media_limits);

    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route_service("/thehideout.v1.MediaService/{method}", media_connect)
        .merge(media::media_route().with_state(state.clone()))
        .merge(media::attachments::attachments_route().with_state(state.clone()))
        .merge(crate::passkeys::routes().with_state(state.clone()))
        .merge(crate::services::voice::routes().with_state(state.clone()))
        .merge(crate::services::invite::routes().with_state(state))
        .fallback_service(connect.into_axum_service())
        .layer(cors(&public_url))
}

/// CORS for `public_url`'s origin only (the app's own pages, e.g. the passkey
/// ceremony page, are served from it). Native clients send no `Origin` header,
/// so this restricts browsers without affecting them; every other origin is
/// refused instead of mirrored back.
fn cors(public_url: &Url) -> CorsLayer {
    let headers = [
        "content-type",
        "authorization",
        "connect-protocol-version",
        "connect-timeout-ms",
        "connect-content-encoding",
        "connect-accept-encoding",
        "grpc-timeout",
        "x-grpc-web",
        "x-user-agent",
    ]
    .map(HeaderName::from_static);
    let exposed = [
        "grpc-status",
        "grpc-message",
        "grpc-status-details-bin",
        "connect-content-encoding",
        "connect-accept-encoding",
    ]
    .map(HeaderName::from_static);
    let origin = HeaderValue::from_str(&public_url.origin().ascii_serialization())
        .expect("an origin serialises to a valid header value");
    CorsLayer::new()
        .allow_origin(AllowOrigin::exact(origin))
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(headers)
        .expose_headers(exposed)
        .max_age(Duration::from_secs(600))
}
