#![allow(dead_code)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;

use connectrpc::client::{ClientConfig, HttpClient};
use rand::Rng;
use scylla::client::caching_session::CachingSession;
use scylla::client::execution_profile::ExecutionProfile;
use scylla::client::session::Session;
use scylla::client::session_builder::SessionBuilder;
use sha2::{Digest, Sha256};
use thehideout_server::config::S3Config;
use thehideout_server::embeds::EmbedSettings;
use thehideout_server::media::attachments::AttachmentSettings;
use thehideout_server::snowflake::Snowflake;
use thehideout_server::storage::Storage;
use thehideout_server::{AppState, build_app, migrations};
use tokio::sync::{OnceCell, Semaphore};

/// Fresh (isolated) keyspaces that may be created at the same time. Scylla DDL
/// is slow (every CREATE KEYSPACE / TABLE is a schema change the node has to
/// agree on); past a handful the DDL queues up and migrations time out.
const CONCURRENT_STARTUPS: usize = 3;
static STARTUPS: Semaphore = Semaphore::const_new(CONCURRENT_STARTUPS);
/// Attempts at creating a keyspace and running the migrations.
const MIGRATION_ATTEMPTS: usize = 4;

/// The migrated keyspace shared by every test of the process (and, being
/// named after the migrations' hash, by every later run until a migration
/// changes). Creating a keyspace takes ~10 s, so tests share it and isolate
/// themselves with unique (Snowflake / random) ids instead.
static SHARED_KEYSPACE: OnceCell<String> = OnceCell::const_new();

/// Worker ids handed out to apps, so two apps writing to the shared keyspace
/// in the same millisecond never generate the same Snowflake.
static NEXT_WORKER: AtomicU16 = AtomicU16::new(0);

fn next_worker_id() -> u16 {
    NEXT_WORKER.fetch_add(1, Ordering::Relaxed) % (thehideout_server::snowflake::MAX_WORKER_ID + 1)
}

/// Prefix of the shared keyspace's name.
const SHARED_PREFIX: &str = "test_shared_";

/// A running app backed by a Scylla keyspace: the shared one by default, or a
/// throwaway one for tests that assert on global state (site-wide counters,
/// the pending-upload sweeper, ...); see [`Builder::isolated`].
pub struct TestApp {
    pub base_url: String,
    pub state: AppState,
    /// The recording LiveKit stand-in when the app was built with [`Builder::livekit`].
    pub livekit: Option<Arc<voice::RecordingLiveKit>>,
    /// Whether `state.keyspace` is this app's own throwaway keyspace.
    isolated: bool,
    /// Random suffix that makes this app's usernames unique in the shared keyspace.
    tag: String,
}

/// Options of a [`TestApp`]; `TestApp::builder().storage().start().await`.
pub struct Builder {
    heartbeat: Duration,
    storage: bool,
    attachments: AttachmentSettings,
    embeds: EmbedSettings,
    isolated: bool,
    admins: Vec<String>,
    livekit: bool,
    connect_grace: Option<Duration>,
    message_rate: (u32, f64),
}

impl Builder {
    /// The per-user message rate (a burst, then per second). Tests default to
    /// an effectively unlimited rate so they can send freely; the rate-limit
    /// tests set the production one.
    pub fn message_rate(mut self, burst: u32, refill_per_sec: f64) -> Self {
        self.message_rate = (burst, refill_per_sec);
        self
    }

    /// Voice on, with a recording fake instead of a LiveKit server
    /// ([`voice::API_KEY`] / [`voice::API_SECRET`], room API calls recorded in
    /// `TestApp::livekit`).
    pub fn livekit(mut self) -> Self {
        self.livekit = true;
        self
    }

    /// How long a joined user may take to connect to LiveKit (default 30 s).
    pub fn connect_grace(mut self, grace: Duration) -> Self {
        self.connect_grace = Some(grace);
        self
    }

    /// Usernames (as passed to `TestApp::name`) promoted to site admin at
    /// startup and sign-up, like `THEHIDEOUT_ADMINS`.
    pub fn admins(mut self, names: &[&str]) -> Self {
        self.admins = names.iter().map(|n| n.to_string()).collect();
        self
    }

    /// Gateway heartbeat interval.
    pub fn heartbeat(mut self, interval: Duration) -> Self {
        self.heartbeat = interval;
        self
    }

    /// Media storage on the real S3-compatible service from `docker-compose.yml`
    /// (127.0.0.1:8333, or the `THEHIDEOUT_S3_*` env vars), under a prefix
    /// private to the app that [`TestApp::cleanup`] deletes.
    pub fn storage(mut self) -> Self {
        self.storage = true;
        self
    }

    /// Custom attachment limits and timings (implies [`Builder::storage`]).
    pub fn attachments(mut self, settings: AttachmentSettings) -> Self {
        self.storage = true;
        self.attachments = settings;
        self
    }

    /// Custom embed settings (implies [`Builder::storage`]).
    pub fn embeds(mut self, embeds: EmbedSettings) -> Self {
        self.storage = true;
        self.embeds = embeds;
        self
    }

    /// Use a fresh keyspace nobody else writes to. Needed by tests that read
    /// site-wide state: `media_totals` scopes `global*`, `pending_sweep`,
    /// whole-table scans, overview counters.
    pub fn isolated(mut self) -> Self {
        self.isolated = true;
        self
    }

    pub async fn start(self) -> TestApp {
        TestApp::start_inner(self).await
    }
}

impl TestApp {
    pub fn builder() -> Builder {
        Builder {
            heartbeat: Duration::from_secs(30),
            storage: false,
            attachments: AttachmentSettings::default(),
            embeds: EmbedSettings::default(),
            isolated: false,
            admins: Vec::new(),
            livekit: false,
            connect_grace: None,
            message_rate: (100_000, 1_000.0),
        }
    }

    /// Connects to Scylla at 127.0.0.1:9042 and serves the app on 127.0.0.1:0
    /// over the shared migrated keyspace.
    pub async fn start() -> Self {
        Self::builder().start().await
    }

    /// Like `start`, with a custom gateway heartbeat interval.
    pub async fn start_with_heartbeat(heartbeat_interval: Duration) -> Self {
        Self::builder().heartbeat(heartbeat_interval).start().await
    }

    /// Like `start`, with media storage (see [`Builder::storage`]).
    pub async fn start_with_storage() -> Self {
        Self::builder().storage().start().await
    }

    /// Like `start_with_storage`, with custom attachment limits and timings.
    pub async fn start_with_attachments(settings: AttachmentSettings) -> Self {
        Self::builder().attachments(settings).start().await
    }

    /// Like `start_with_storage`, with custom embed settings (loopback allowed
    /// for the in-test sites, short timeouts, ...).
    pub async fn start_with_embeds(embeds: EmbedSettings) -> Self {
        Self::builder().embeds(embeds).start().await
    }

    async fn start_inner(options: Builder) -> Self {
        // DDL can take far longer than the driver's 30 s default request
        // timeout when many keyspaces are being created.
        let profile = ExecutionProfile::builder().request_timeout(Some(Duration::from_secs(180))).build();
        let session = SessionBuilder::new()
            .known_node("127.0.0.1:9042")
            .default_execution_profile_handle(profile.into_handle())
            .schema_agreement_timeout(Duration::from_secs(120))
            .build()
            .await
            .expect("connect to ScyllaDB at 127.0.0.1:9042 (is `docker compose up -d` running?)");
        let keyspace = if options.isolated {
            Self::create_keyspace(&session).await
        } else {
            let keyspace = SHARED_KEYSPACE.get_or_init(|| Self::prepare_shared_keyspace(&session)).await.clone();
            session.use_keyspace(&keyspace, false).await.expect("select the shared test keyspace");
            keyspace
        };

        let storage = options.storage.then(|| {
            let config = S3Config::from_env().expect("S3 config");
            let storage = Storage::s3(&config).expect("configure S3 storage");
            (config, storage.with_prefix(&format!("t{:016x}", rand::rng().random::<u64>())))
        });
        let storage = match storage {
            Some((config, storage)) => {
                storage.check().await.unwrap_or_else(|e| {
                    panic!(
                        "object storage at {} is not reachable ({e:#}); run `docker compose up -d` and wait for the bucket to be created",
                        config.endpoint
                    )
                });
                Some(storage)
            }
            None => None,
        };

        let tag = format!("{:06x}", rand::rng().random::<u32>() & 0xff_ffff);
        let session = Arc::new(CachingSession::from(session, 64));
        let mut state = AppState::new(session, Arc::new(Snowflake::new(next_worker_id())), keyspace, options.heartbeat);
        state.storage = storage;
        state.attachments = options.attachments;
        state.embeds = options.embeds;
        state.embeds.cache_namespace = format!("{tag}:");
        state.limiter = Arc::new(thehideout_server::ratelimit::RateLimiter::new().with_message_rate(options.message_rate.0, options.message_rate.1));
        state.admins = Arc::new(options.admins.iter().map(|n| format!("{}_{tag}", thehideout_server::admin::normalize_username(n))).collect());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        // `localhost` (not the IP): WebAuthn needs a domain name as RP id.
        let state = state.with_public_url(format!("http://localhost:{}", addr.port()).parse().unwrap());
        let livekit = options.livekit.then(|| Arc::new(voice::RecordingLiveKit::default()));
        let state = match &livekit {
            Some(fake) => {
                let mut livekit = thehideout_server::voice::LiveKit::with_admin(voice::config(), fake.clone());
                if let Some(grace) = options.connect_grace {
                    livekit.connect_grace = grace;
                }
                state.with_livekit(livekit)
            }
            None => state,
        };
        let app = build_app(state.clone());
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
            .unwrap();
        });

        Self {
            base_url: format!("http://{addr}"),
            state,
            livekit,
            isolated: options.isolated,
            tag,
        }
    }

    /// A username unique to this app (`alice` -> `alice_1a2b3c`): the shared
    /// keyspace is used by many tests at once and usernames are global.
    pub fn name(&self, base: &str) -> String {
        format!("{base}_{}", self.tag)
    }

    /// Creates (once per migration set, ever) and migrates the shared keyspace
    /// and drops the shared keyspaces of older migration sets.
    async fn prepare_shared_keyspace(session: &Session) -> String {
        let digest = Sha256::digest(migrations::MIGRATIONS.iter().map(|(_, _, cql)| *cql).collect::<String>());
        let hash: String = digest.iter().take(6).map(|b| format!("{b:02x}")).collect();
        let keyspace = format!("{SHARED_PREFIX}{hash}");
        let mut last_error = None;
        for attempt in 0..MIGRATION_ATTEMPTS {
            let _permit = STARTUPS.acquire().await.expect("startup semaphore is never closed");
            match migrations::run(session, &keyspace).await {
                Ok(()) => {
                    Self::drop_stale_shared_keyspaces(session, &keyspace).await;
                    return keyspace;
                }
                Err(e) => {
                    eprintln!("shared test keyspace {keyspace}: migrations failed (attempt {}): {e:#}", attempt + 1);
                    last_error = Some(e);
                    tokio::time::sleep(Duration::from_millis(500 << attempt)).await;
                }
            }
        }
        panic!("run migrations: {:#}", last_error.expect("at least one attempt"));
    }

    async fn drop_stale_shared_keyspaces(session: &Session, current: &str) {
        let Ok(result) = session.query_unpaged("SELECT keyspace_name FROM system_schema.keyspaces", ()).await else {
            return;
        };
        let Ok(rows) = result.into_rows_result() else { return };
        let Ok(rows) = rows.rows::<(String,)>() else { return };
        for (name,) in rows.flatten() {
            if name.starts_with(SHARED_PREFIX) && name != current {
                let _ = session.query_unpaged(format!("DROP KEYSPACE IF EXISTS {name}"), ()).await;
            }
        }
    }

    /// Creates a fresh `test_<random>` keyspace and runs the migrations in it.
    /// Startups are limited to [`CONCURRENT_STARTUPS`] at a time and a failed
    /// attempt (a timeout under load leaves the keyspace half-built) is
    /// dropped and retried under a new name with a growing pause.
    async fn create_keyspace(session: &Session) -> String {
        let _permit = STARTUPS.acquire().await.expect("startup semaphore is never closed");
        let mut last_error = None;
        for attempt in 0..MIGRATION_ATTEMPTS {
            let keyspace = format!("test_{:016x}", rand::rng().random::<u64>());
            match migrations::run(session, &keyspace).await {
                Ok(()) => return keyspace,
                Err(e) => {
                    eprintln!("test keyspace {keyspace}: migrations failed (attempt {}): {e:#}", attempt + 1);
                    let _ = session.query_unpaged(format!("DROP KEYSPACE IF EXISTS {keyspace}"), ()).await;
                    last_error = Some(e);
                    tokio::time::sleep(Duration::from_millis(500 << attempt)).await;
                }
            }
        }
        panic!("run migrations: {:#}", last_error.expect("at least one attempt"));
    }

    /// Client config for generated Connect clients, optionally with a bearer token.
    pub fn client_config(&self, token: Option<&str>) -> ClientConfig {
        let mut config = ClientConfig::new(self.base_url.parse().unwrap());
        if let Some(token) = token {
            config = config.with_default_header("authorization", format!("Bearer {token}"));
        }
        config
    }

    /// A generated client, e.g. `app.client(AuthServiceClient::new, None)`.
    pub fn client<C>(&self, new: impl FnOnce(HttpClient, ClientConfig) -> C, token: Option<&str>) -> C {
        new(HttpClient::plaintext(), self.client_config(token))
    }

    /// Deletes the media objects the test stored (its private storage
    /// prefix) and, for an isolated app, drops its keyspace.
    pub async fn cleanup(self) {
        if let Some(storage) = &self.state.storage {
            let _ = storage.delete_all().await;
        }
        if self.isolated {
            let _ = self
                .state
                .session
                .get_session()
                .query_unpaged(format!("DROP KEYSPACE IF EXISTS {}", self.state.keyspace), ())
                .await;
        }
    }
}

pub mod fixtures;
pub mod voice;
