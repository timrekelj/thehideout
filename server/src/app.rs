use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use connectrpc::Router as ConnectRouter;
use scylla::client::caching_session::CachingSession;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;
use url::Url;

fn default_public_url() -> Url {
    Url::parse("http://localhost:8080").expect("valid default URL")
}

use crate::auth::SeenThrottle;
use crate::clientip::TrustedProxies;
use crate::shutdown::Shutdown;
use crate::streams::StreamGate;
use crate::email::Mailer;
use crate::embeds::EmbedSettings;
use crate::events::EventBus;
use crate::media::attachments::AttachmentSettings;
use crate::media::{self, MAX_INPUT_BYTES};
use crate::ratelimit::RateLimiter;
use crate::services::auth::AuthMemory;
use crate::services::{AdminSvc, AuthSvc, DmSvc, EmojiSvc, GatewaySvc, GuildSvc, InviteSvc, MediaSvc, MessageSvc, NotificationSvc, RoleSvc, VoiceSvc};
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
    /// Proxies whose `X-Forwarded-For` is believed (`THEHIDEOUT_TRUSTED_PROXIES`).
    pub trusted_proxies: Arc<TrustedProxies>,
    /// Triggered when the process is asked to stop; ends gateway streams and sweepers.
    pub shutdown: Shutdown,
    /// Caps on concurrent gateway streams.
    pub streams: Arc<StreamGate>,
    /// Operator name and contact (legal pages) and the minimum client version.
    pub policy: Arc<crate::policy::Policy>,
    /// Push notification dispatch (off without a transport).
    pub push: Arc<crate::push::Pusher>,
    /// Folder of the web client bundle, served at `/app/` (`THEHIDEOUT_WEB_DIR`); `None` = not served.
    pub web_dir: Option<std::path::PathBuf>,
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
            trusted_proxies: Arc::default(),
            shutdown: Shutdown::default(),
            streams: Arc::default(),
            policy: Arc::default(),
            push: Arc::new(crate::push::Pusher::disabled()),
            web_dir: None,
        }
        .with_legal_footer()
    }

    /// Delivers pushes through `transport` (APNs / FCM, or a fake in tests);
    /// without one push stays off and tokens are only stored.
    pub fn with_push(mut self, transport: Option<Arc<dyn crate::push::Transport>>, tuning: crate::push::Tuning) -> Self {
        self.push = Arc::new(crate::push::Pusher::new(transport, tuning));
        self
    }

    /// Serves the web client bundle in `dir` at `/app/` (`THEHIDEOUT_WEB_DIR`, tests).
    pub fn with_web_dir(mut self, dir: Option<std::path::PathBuf>) -> Self {
        self.web_dir = dir;
        self
    }

    /// Sets operator contact details and the minimum client version.
    pub fn with_policy(mut self, policy: crate::policy::Policy) -> Self {
        self.policy = Arc::new(policy);
        self
    }

    fn with_legal_footer(mut self) -> Self {
        let base = self.public_base();
        self.mailer = self.mailer.clone().with_legal_links(&base);
        self
    }

    /// Sets the trusted reverse proxies (`THEHIDEOUT_TRUSTED_PROXIES`).
    pub fn with_trusted_proxies(mut self, proxies: TrustedProxies) -> Self {
        self.trusted_proxies = Arc::new(proxies);
        self
    }

    /// Sets the gateway stream caps (`THEHIDEOUT_MAX_STREAMS_PER_USER`, `THEHIDEOUT_MAX_STREAMS`).
    pub fn with_stream_limits(mut self, per_user: usize, total: usize) -> Self {
        self.streams = Arc::new(StreamGate::new(per_user, total));
        self
    }

    /// Uses `shutdown` as the process shutdown signal.
    pub fn with_shutdown(mut self, shutdown: Shutdown) -> Self {
        self.shutdown = shutdown;
        self
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
        self.with_legal_footer()
    }

    /// Sets the email sender.
    pub fn with_mailer(mut self, mailer: Mailer) -> Self {
        self.mailer = mailer;
        self.with_legal_footer()
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

/// Time limit of ordinary requests (unary RPCs and pages).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Time limit of requests that move large bodies: uploads (50 MiB by default)
/// and media reads.
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(600);
/// How long `/ready` waits for each dependency.
const READY_PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// The time limit of a request path; `None` for the long-lived gateway stream.
fn request_timeout(path: &str) -> Option<Duration> {
    if path.starts_with("/thehideout.v1.GatewayService/") {
        None
    } else if path == "/attachments" || path.starts_with("/app/") || path.starts_with("/media/") || path.starts_with("/thehideout.v1.MediaService/") {
        Some(TRANSFER_TIMEOUT)
    } else {
        Some(REQUEST_TIMEOUT)
    }
}

async fn timeout_middleware(request: Request, next: Next) -> Response {
    match request_timeout(request.uri().path()) {
        None => next.run(request).await,
        Some(limit) => match tokio::time::timeout(limit, next.run(request)).await {
            Ok(response) => response,
            Err(_) => (StatusCode::REQUEST_TIMEOUT, "request timed out").into_response(),
        },
    }
}

/// `GET /ready`: 200 when ScyllaDB and object storage both answer within
/// [`READY_PROBE_TIMEOUT`], else 503 naming what failed. No auth.
async fn ready(State(state): State<AppState>) -> Response {
    let scylla = async {
        matches!(
            tokio::time::timeout(READY_PROBE_TIMEOUT, state.session.execute_unpaged("SELECT key FROM system.local", ())).await,
            Ok(Ok(_))
        )
    };
    let s3 = async {
        match &state.storage {
            Some(storage) => matches!(tokio::time::timeout(READY_PROBE_TIMEOUT, storage.check()).await, Ok(Ok(()))),
            None => true,
        }
    };
    let (scylla_ok, s3_ok) = tokio::join!(scylla, s3);
    let mut failed = Vec::new();
    if !scylla_ok {
        failed.push("scylla");
    }
    if !s3_ok {
        failed.push("s3");
    }
    if failed.is_empty() {
        return (StatusCode::OK, axum::Json(serde_json::json!({ "status": "ok" }))).into_response();
    }
    tracing::warn!(?failed, "readiness check failed");
    (StatusCode::SERVICE_UNAVAILABLE, axum::Json(serde_json::json!({ "status": "unavailable", "failed": failed }))).into_response()
}

/// Builds the HTTP app: all Connect services, `GET /health`, `GET /ready`, `GET /media/*`,
/// `POST /attachments`, the passkey ceremony page (`/auth/passkey*`), the
/// invite landing page (`GET /invite/{token}`), the public landing page (`GET /`), the LiveKit webhook
/// (`POST /livekit/webhook`), and CORS for the configured public origin only.
pub fn build_app(state: AppState) -> Router {
    crate::voice::spawn_sweeper(&state);
    crate::lifecycle::spawn_background(&state);
    crate::push::dispatch::spawn(&state);
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
        .add_service(Arc::new(NotificationSvc::new(state.clone())))
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

    let connect_service = connect.into_axum_service();
    let trusted = state.trusted_proxies.clone();
    let livekit_url = state.livekit.config.as_ref().map(|c| c.url.as_str());
    let web = state.web_dir.as_deref().and_then(|dir| crate::web::routes(dir, livekit_url));
    let mut router = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/ready", get(ready).with_state(state.clone()))
        .route_service("/thehideout.v1.MediaService/{method}", media_connect)
        .merge(media::media_route().with_state(state.clone()))
        .merge(media::attachments::attachments_route().with_state(state.clone()))
        .merge(crate::passkeys::routes().with_state(state.clone()))
        .merge(crate::services::voice::routes().with_state(state.clone()))
        .merge(crate::services::invite::routes().with_state(state.clone()))
        .route("/", crate::landing::root().fallback_service(connect_service.clone()).with_state(state.clone()))
        .merge(crate::landing::routes().with_state(state.clone()))
        .merge(crate::legal::routes().with_state(state.clone()))
        .merge(crate::lifecycle::routes().with_state(state.clone()));
    if let Some(web) = web {
        router = router.merge(web);
    }
    router
        .fallback_service(connect_service)
        .layer(middleware::from_fn_with_state(state.policy.min_client.clone(), crate::policy::client_version_middleware))
        .layer(cors(&public_url))
        .layer(middleware::from_fn(timeout_middleware))
        .layer(
            // Method, path (never the query), status, latency and client IP; no headers.
            TraceLayer::new_for_http()
                .make_span_with(|request: &axum::http::Request<Body>| {
                    let ip = crate::clientip::from_extensions(request.extensions()).map_or_else(|| "unknown".to_owned(), |ip| ip.to_string());
                    tracing::info_span!("http", method = %request.method(), path = %request.uri().path(), client_ip = %ip)
                })
                .on_request(())
                .on_response(|response: &axum::http::Response<Body>, latency: Duration, _span: &tracing::Span| {
                    let (status, latency_ms) = (response.status().as_u16(), latency.as_millis() as u64);
                    if status >= 500 {
                        tracing::warn!(status, latency_ms, "request");
                    } else {
                        tracing::info!(status, latency_ms, "request");
                    }
                })
                .on_failure(()),
        )
        // Outermost: the trace layer and everything below read the client IP.
        .layer(middleware::from_fn_with_state(trusted, crate::clientip::middleware))
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
        "x-thehideout-client",
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
