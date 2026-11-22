use std::env;
use std::net::SocketAddr;
use std::time::Duration;

use std::path::PathBuf;

use anyhow::{bail, Context, Result};

use crate::clientip::TrustedProxies;
use crate::migrations::Replication;

#[derive(Debug, Clone)]
pub struct Config {
    pub listen_addr: SocketAddr,
    pub scylla_nodes: Vec<String>,
    pub keyspace: String,
    pub worker_id: u16,
    pub heartbeat_interval: Duration,
    pub s3: S3Config,
    /// Largest attachment upload in bytes (`THEHIDEOUT_MAX_UPLOAD_BYTES`).
    pub max_upload_bytes: u64,
    /// Link embeds (`THEHIDEOUT_EMBEDS=on|off`, default on).
    pub embeds_enabled: bool,
    /// Contact shown in the embed fetcher's user agent (`THEHIDEOUT_EMBEDS_CONTACT`).
    pub embeds_contact: Option<String>,
    /// Mirror lists of the provider embeds (`THEHIDEOUT_EMBED_MIRRORS`, a JSON object of role → base URLs).
    pub embed_mirrors: crate::embeds::providers::Mirrors,
    /// Largest video stored in an embed (`THEHIDEOUT_EMBED_MAX_VIDEO_BYTES`).
    pub embed_max_video_bytes: usize,
    /// Usernames promoted to site admin at startup and sign-up (`THEHIDEOUT_ADMINS`).
    pub admins: Vec<String>,
    /// Public base URL used in email links and as the WebAuthn origin
    /// (`THEHIDEOUT_PUBLIC_URL`; default `http://localhost:<listen port>`).
    pub public_url: url::Url,
    /// Sender of outgoing mail (`THEHIDEOUT_EMAIL_FROM`).
    pub email_from: String,
    /// Resend API key (`THEHIDEOUT_RESEND_API_KEY`); `None` = SMTP.
    pub resend_api_key: Option<Redacted>,
    /// Use Resend even when `public_url` is a development host
    /// (`THEHIDEOUT_EMAIL_FORCE_RESEND`, default false).
    pub email_force_resend: bool,
    /// SMTP sink used without a Resend key (`THEHIDEOUT_SMTP_URL`, Mailpit in dev).
    pub smtp_url: String,
    /// Email that gets the first admin invite on an empty install
    /// (`THEHIDEOUT_BOOTSTRAP_ADMIN_EMAIL`).
    pub bootstrap_admin_email: Option<String>,
    /// Operator contact, name and minimum client version (legal pages, update gate).
    pub policy: crate::policy::Policy,
    /// LiveKit (voice channels); `None` = voice is not configured and
    /// `JoinVoice` answers UNAVAILABLE.
    pub livekit: Option<LiveKitConfig>,
    /// ScyllaDB credentials (`THEHIDEOUT_SCYLLA_USERNAME` / `THEHIDEOUT_SCYLLA_PASSWORD`).
    pub scylla_auth: Option<ScyllaAuth>,
    /// CA bundle (PEM) that enables TLS to ScyllaDB (`THEHIDEOUT_SCYLLA_TLS_CA`).
    pub scylla_tls_ca: Option<PathBuf>,
    /// Replication of the keyspace when it is created (`THEHIDEOUT_SCYLLA_REPLICATION`).
    pub scylla_replication: Replication,
    /// Reverse proxies whose `X-Forwarded-For` is trusted (`THEHIDEOUT_TRUSTED_PROXIES`).
    pub trusted_proxies: TrustedProxies,
    /// Most gateway streams one user may hold open (`THEHIDEOUT_MAX_STREAMS_PER_USER`).
    pub max_streams_per_user: usize,
    /// Most gateway streams open at once (`THEHIDEOUT_MAX_STREAMS`).
    pub max_streams: usize,
    /// Push providers (`THEHIDEOUT_APNS_*`, `THEHIDEOUT_FCM_SERVICE_ACCOUNT`); both off by default.
    pub push: crate::push::config::PushConfig,
}

/// ScyllaDB login.
#[derive(Debug, Clone)]
pub struct ScyllaAuth {
    pub username: String,
    pub password: Redacted,
}

/// Log output format (`THEHIDEOUT_LOG_FORMAT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogFormat {
    #[default]
    Text,
    Json,
}

impl LogFormat {
    /// `text` (default) or `json`.
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "" | "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            other => bail!("invalid THEHIDEOUT_LOG_FORMAT {other:?}, expected text or json"),
        }
    }

    pub fn from_env() -> Result<Self> {
        Self::parse(&var("THEHIDEOUT_LOG_FORMAT", "text"))
    }
}

/// Connection settings of the LiveKit SFU that carries voice channel media.
#[derive(Debug, Clone)]
pub struct LiveKitConfig {
    /// URL clients connect to (`LIVEKIT_URL`, e.g. `ws://127.0.0.1:7880`).
    pub url: String,
    /// URL of the LiveKit server API the server calls (`LIVEKIT_API_URL`,
    /// default: `url` with ws -> http and wss -> https).
    pub api_url: String,
    pub api_key: String,
    pub api_secret: Redacted,
}

impl LiveKitConfig {
    /// The public development API key of `docker-compose.yml` / `.env.example`.
    pub const DEV_API_KEY: &str = "devkey";
    /// The public development API secret of `docker-compose.yml` / `.env.example`.
    pub const DEV_API_SECRET: &str = "secret";
    /// Shortest API secret accepted for a non-local deployment (HS256 needs 32 bytes).
    pub const MIN_SECRET_LEN: usize = 32;

    /// Whether the credentials are the public development defaults or a
    /// secret too short to be one.
    fn uses_dev_credentials(&self) -> bool {
        self.api_key == Self::DEV_API_KEY
            || self.api_secret.0 == Self::DEV_API_SECRET
            || self.api_secret.0.len() < Self::MIN_SECRET_LEN
    }

    /// `LIVEKIT_URL`, `LIVEKIT_API_URL` (optional), `LIVEKIT_API_KEY` and
    /// `LIVEKIT_API_SECRET`. None of the first, third and fourth set = `None`;
    /// some but not all = an error.
    pub fn from_env() -> Result<Option<Self>> {
        Self::from_lookup(|name| env::var(name).ok())
    }

    /// [`from_env`](Self::from_env) over any variable lookup (tests).
    fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Option<Self>> {
        let get = |name: &str| {
            lookup(name)
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
        };
        let (url, key, secret) = (
            get("LIVEKIT_URL"),
            get("LIVEKIT_API_KEY"),
            get("LIVEKIT_API_SECRET"),
        );
        let (Some(url), Some(api_key), Some(secret)) = (url.clone(), key.clone(), secret.clone())
        else {
            if url.is_some() || key.is_some() || secret.is_some() {
                bail!("LIVEKIT_URL, LIVEKIT_API_KEY and LIVEKIT_API_SECRET must all be set (or none, to disable voice)");
            }
            return Ok(None);
        };
        Self::new(&url, get("LIVEKIT_API_URL").as_deref(), api_key, secret).map(Some)
    }

    pub fn new(
        url: &str,
        api_url: Option<&str>,
        api_key: String,
        api_secret: String,
    ) -> Result<Self> {
        let url = url.trim().trim_end_matches('/').to_owned();
        let parsed =
            url::Url::parse(&url).with_context(|| format!("invalid LIVEKIT_URL {url:?}"))?;
        if !matches!(parsed.scheme(), "ws" | "wss" | "http" | "https")
            || parsed.host_str().is_none()
        {
            bail!("LIVEKIT_URL must be a ws(s):// or http(s):// URL with a host");
        }
        let api_url = match api_url {
            Some(u) => u.trim().trim_end_matches('/').to_owned(),
            None => match (url.strip_prefix("wss://"), url.strip_prefix("ws://")) {
                (Some(rest), _) => format!("https://{rest}"),
                (_, Some(rest)) => format!("http://{rest}"),
                _ => url.clone(),
            },
        };
        let parsed_api = url::Url::parse(&api_url)
            .with_context(|| format!("invalid LIVEKIT_API_URL {api_url:?}"))?;
        if !matches!(parsed_api.scheme(), "http" | "https") || parsed_api.host_str().is_none() {
            bail!("LIVEKIT_API_URL must be an http(s):// URL with a host");
        }
        Ok(Self {
            url,
            api_url,
            api_key,
            api_secret: Redacted(api_secret),
        })
    }
}

/// A secret config value that never shows up in logs.
#[derive(Clone)]
pub struct Redacted(pub String);

impl std::fmt::Debug for Redacted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

/// Connection settings of the S3-compatible object store holding media.
/// Defaults match the SeaweedFS service in `docker-compose.yml`.
#[derive(Clone)]
pub struct S3Config {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: String,
    /// Path-style addressing (`endpoint/bucket/key`); `false` for AWS S3 and
    /// other providers that use virtual-hosted style.
    pub path_style: bool,
    /// Reserved for a CDN or public bucket URL in front of `/media`; currently
    /// only logged.
    pub public_base: Option<String>,
}

impl std::fmt::Debug for S3Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3Config")
            .field("endpoint", &self.endpoint)
            .field("region", &self.region)
            .field("bucket", &self.bucket)
            .field("access_key", &self.access_key)
            .field("secret_key", &"<redacted>")
            .field("path_style", &self.path_style)
            .field("public_base", &self.public_base)
            .finish()
    }
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let listen_addr = var("THEHIDEOUT_LISTEN_ADDR", "0.0.0.0:8080")
            .parse()
            .context("invalid THEHIDEOUT_LISTEN_ADDR")?;
        let scylla_nodes = var("THEHIDEOUT_SCYLLA_NODES", "127.0.0.1:9042")
            .split(',')
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect();
        let keyspace = var("THEHIDEOUT_KEYSPACE", "thehideout");
        let worker_id = var("THEHIDEOUT_WORKER_ID", "0")
            .parse()
            .context("invalid THEHIDEOUT_WORKER_ID")?;
        let heartbeat_secs: u64 = var("THEHIDEOUT_HEARTBEAT_SECS", "30")
            .parse()
            .context("invalid THEHIDEOUT_HEARTBEAT_SECS")?;
        if heartbeat_secs == 0 {
            bail!("THEHIDEOUT_HEARTBEAT_SECS must be at least 1");
        }
        let max_upload_bytes: u64 = var("THEHIDEOUT_MAX_UPLOAD_BYTES", "52428800")
            .parse()
            .context("invalid THEHIDEOUT_MAX_UPLOAD_BYTES")?;
        if max_upload_bytes == 0 {
            bail!("THEHIDEOUT_MAX_UPLOAD_BYTES must be at least 1");
        }
        let embeds_enabled = match var("THEHIDEOUT_EMBEDS", "on").to_lowercase().as_str() {
            "on" | "true" | "1" | "yes" => true,
            "off" | "false" | "0" | "no" => false,
            other => bail!("invalid THEHIDEOUT_EMBEDS {other:?}, expected on or off"),
        };
        let embed_mirrors = match env::var("THEHIDEOUT_EMBED_MIRRORS")
            .ok()
            .filter(|s| !s.trim().is_empty())
        {
            Some(json) => crate::embeds::providers::Mirrors::default()
                .with_override(&json)
                .map_err(|e| anyhow::anyhow!("invalid THEHIDEOUT_EMBED_MIRRORS: {e}"))?,
            None => crate::embeds::providers::Mirrors::default(),
        };
        let embed_max_video_bytes: usize = var(
            "THEHIDEOUT_EMBED_MAX_VIDEO_BYTES",
            &crate::embeds::MAX_VIDEO_BYTES.to_string(),
        )
        .parse()
        .context("invalid THEHIDEOUT_EMBED_MAX_VIDEO_BYTES")?;
        if embed_max_video_bytes == 0 {
            bail!("THEHIDEOUT_EMBED_MAX_VIDEO_BYTES must be at least 1");
        }
        let public_url = public_url(&listen_addr)?;
        let email_force_resend = match var("THEHIDEOUT_EMAIL_FORCE_RESEND", "false")
            .trim()
            .to_lowercase()
            .as_str()
        {
            "true" | "1" | "yes" => true,
            "" | "false" | "0" | "no" => false,
            other => {
                bail!("invalid THEHIDEOUT_EMAIL_FORCE_RESEND {other:?}, expected true or false")
            }
        };
        let non_empty = |name: &str| {
            env::var(name)
                .ok()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
        };
        let s3 = S3Config::from_env()?;
        check_s3_credentials(&s3, &public_url)?;
        let livekit = LiveKitConfig::from_env()?;
        check_livekit_credentials(livekit.as_ref(), &public_url)?;
        let scylla_auth = match (non_empty("THEHIDEOUT_SCYLLA_USERNAME"), non_empty("THEHIDEOUT_SCYLLA_PASSWORD")) {
            (Some(username), Some(password)) => Some(ScyllaAuth { username, password: Redacted(password) }),
            (None, None) => None,
            _ => bail!("THEHIDEOUT_SCYLLA_USERNAME and THEHIDEOUT_SCYLLA_PASSWORD must both be set (or neither)"),
        };
        let allow_no_auth = match var("THEHIDEOUT_SCYLLA_ALLOW_NO_AUTH", "").trim().to_lowercase().as_str() {
            "1" | "true" | "yes" => true,
            "" | "0" | "false" | "no" => false,
            other => bail!("invalid THEHIDEOUT_SCYLLA_ALLOW_NO_AUTH {other:?}, expected 1 or 0"),
        };
        check_scylla_auth(scylla_auth.as_ref(), allow_no_auth, &public_url)?;
        let scylla_replication = Replication::parse(&non_empty("THEHIDEOUT_SCYLLA_REPLICATION").unwrap_or_else(|| "SimpleStrategy:1".into()))?;
        let trusted_proxies = TrustedProxies::parse(&var("THEHIDEOUT_TRUSTED_PROXIES", ""))?;
        let max_streams_per_user: usize = non_empty("THEHIDEOUT_MAX_STREAMS_PER_USER")
            .unwrap_or_else(|| crate::streams::DEFAULT_MAX_PER_USER.to_string())
            .parse()
            .context("invalid THEHIDEOUT_MAX_STREAMS_PER_USER")?;
        let max_streams: usize = non_empty("THEHIDEOUT_MAX_STREAMS")
            .unwrap_or_else(|| crate::streams::DEFAULT_MAX_TOTAL.to_string())
            .parse()
            .context("invalid THEHIDEOUT_MAX_STREAMS")?;
        if max_streams_per_user == 0 || max_streams == 0 {
            bail!("THEHIDEOUT_MAX_STREAMS_PER_USER and THEHIDEOUT_MAX_STREAMS must be at least 1");
        }
        let policy = policy_from_env(&public_url, &non_empty)?;
        Ok(Self {
            policy: crate::policy::Policy {
                bootstrap_admin_email: non_empty("THEHIDEOUT_BOOTSTRAP_ADMIN_EMAIL").map(|e| crate::accounts::normalize_email(&e)),
                ..policy
            },
            public_url,
            email_from: non_empty("THEHIDEOUT_EMAIL_FROM")
                .unwrap_or_else(|| crate::email::DEFAULT_FROM.to_owned()),
            resend_api_key: non_empty("THEHIDEOUT_RESEND_API_KEY").map(Redacted),
            email_force_resend,
            smtp_url: non_empty("THEHIDEOUT_SMTP_URL")
                .unwrap_or_else(|| "smtp://127.0.0.1:1025".to_owned()),
            bootstrap_admin_email: non_empty("THEHIDEOUT_BOOTSTRAP_ADMIN_EMAIL")
                .map(|e| crate::accounts::normalize_email(&e)),
            listen_addr,
            scylla_nodes,
            keyspace,
            worker_id,
            heartbeat_interval: Duration::from_secs(heartbeat_secs),
            s3,
            max_upload_bytes,
            embeds_enabled,
            embeds_contact: env::var("THEHIDEOUT_EMBEDS_CONTACT")
                .ok()
                .filter(|s| !s.trim().is_empty()),
            embed_mirrors,
            embed_max_video_bytes,
            admins: crate::admin::parse_admins(&var("THEHIDEOUT_ADMINS", "")),
            livekit,
            scylla_auth,
            scylla_tls_ca: non_empty("THEHIDEOUT_SCYLLA_TLS_CA").map(PathBuf::from),
            scylla_replication,
            trusted_proxies,
            max_streams_per_user,
            max_streams,
            push: crate::push::config::PushConfig::from_env()?,
        })
    }
}

/// `THEHIDEOUT_CONTACT_EMAIL` (required away from development hosts),
/// `THEHIDEOUT_OPERATOR_NAME` and `THEHIDEOUT_MIN_CLIENT_VERSION[_DESKTOP|_IOS|_ANDROID]`.
fn policy_from_env(public_url: &url::Url, non_empty: &dyn Fn(&str) -> Option<String>) -> Result<crate::policy::Policy> {
    use crate::policy::{MinClient, Policy};
    let contact_email = non_empty("THEHIDEOUT_CONTACT_EMAIL").map(|e| crate::accounts::normalize_email(&e));
    match &contact_email {
        Some(email) if !crate::accounts::valid_email(email) => bail!("invalid THEHIDEOUT_CONTACT_EMAIL {email:?}"),
        None if !public_url.host().is_some_and(|h| crate::email::is_dev_host(&h)) => bail!(
            "THEHIDEOUT_CONTACT_EMAIL is required for a non-development deployment: the privacy notice and terms show it \
             (THEHIDEOUT_PUBLIC_URL={public_url})"
        ),
        _ => {}
    }
    let version = |name: &str| MinClient::parse_var(name, non_empty(name).as_deref()).map_err(anyhow::Error::msg);
    let mut min_client = MinClient { default: version("THEHIDEOUT_MIN_CLIENT_VERSION")?, ..Default::default() };
    for platform in ["desktop", "ios", "android"] {
        if let Some(v) = version(&format!("THEHIDEOUT_MIN_CLIENT_VERSION_{}", platform.to_uppercase()))? {
            min_client.platforms.insert(platform.to_owned(), v);
        }
    }
    Ok(Policy { contact_email, operator_name: non_empty("THEHIDEOUT_OPERATOR_NAME"), min_client, bootstrap_admin_email: None })
}

impl S3Config {
    /// The public development access key (matches `docker-compose.yml`).
    pub const DEV_ACCESS_KEY: &str = "thehideout-dev";
    /// The public development secret key (matches `docker-compose.yml`).
    pub const DEV_SECRET_KEY: &str = "thehideout-dev-secret";

    /// Whether the credentials are the public built-in development defaults.
    fn uses_dev_credentials(&self) -> bool {
        self.access_key == Self::DEV_ACCESS_KEY && self.secret_key == Self::DEV_SECRET_KEY
    }

    pub fn from_env() -> Result<Self> {
        let path_style = match var("THEHIDEOUT_S3_PATH_STYLE", "true")
            .to_lowercase()
            .as_str()
        {
            "true" | "1" | "yes" => true,
            "false" | "0" | "no" => false,
            other => bail!("invalid THEHIDEOUT_S3_PATH_STYLE {other:?}, expected true or false"),
        };
        Ok(Self {
            endpoint: var("THEHIDEOUT_S3_ENDPOINT", "http://127.0.0.1:8333"),
            region: var("THEHIDEOUT_S3_REGION", "us-east-1"),
            bucket: var("THEHIDEOUT_S3_BUCKET", "thehideout-media"),
            access_key: var("THEHIDEOUT_S3_ACCESS_KEY", Self::DEV_ACCESS_KEY),
            secret_key: var("THEHIDEOUT_S3_SECRET_KEY", Self::DEV_SECRET_KEY),
            path_style,
            public_base: env::var("THEHIDEOUT_S3_PUBLIC_BASE")
                .ok()
                .filter(|s| !s.is_empty()),
        })
    }
}

/// `THEHIDEOUT_PUBLIC_URL` without a trailing slash, or `http://localhost:<port>`.
fn public_url(listen_addr: &SocketAddr) -> Result<url::Url> {
    let raw = env::var("THEHIDEOUT_PUBLIC_URL")
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_owned())
        .filter(|s| !s.is_empty());
    let raw = raw.unwrap_or_else(|| format!("http://localhost:{}", listen_addr.port()));
    let url =
        url::Url::parse(&raw).with_context(|| format!("invalid THEHIDEOUT_PUBLIC_URL {raw:?}"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        bail!("THEHIDEOUT_PUBLIC_URL must be an http(s) URL with a host");
    }
    Ok(url)
}

fn var(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_owned())
}

/// The built-in dev credentials are public (docker-compose.yml, .env.example):
/// refuse to run a non-local deployment with them, so a changed endpoint does not
/// silently leave every media object guarded by a known secret.
fn check_s3_credentials(s3: &S3Config, public_url: &url::Url) -> Result<()> {
    if s3.uses_dev_credentials()
        && public_url
            .host()
            .is_some_and(|h| !crate::email::is_dev_host(&h))
    {
        bail!(
            "THEHIDEOUT_S3_ACCESS_KEY/THEHIDEOUT_S3_SECRET_KEY are the built-in development defaults, \
             which are public; set real credentials for a non-local deployment (THEHIDEOUT_PUBLIC_URL={public_url})"
        );
    }
    Ok(())
}

/// The LiveKit dev key / secret are public (docker-compose.yml, .env.example):
/// refuse to start a non-local deployment with them (or with a secret under 32
/// characters), as for the S3 credentials, since they sign every voice token.
fn check_livekit_credentials(livekit: Option<&LiveKitConfig>, public_url: &url::Url) -> Result<()> {
    let Some(livekit) = livekit else {
        return Ok(());
    };
    if livekit.uses_dev_credentials()
        && public_url
            .host()
            .is_some_and(|h| !crate::email::is_dev_host(&h))
    {
        bail!(
            "LIVEKIT_API_KEY/LIVEKIT_API_SECRET are the built-in development defaults or a secret shorter than {} \
             characters, which is not safe; set a real key and a secret of at least {} characters for a non-local \
             deployment, or unset all LIVEKIT_* to disable voice (THEHIDEOUT_PUBLIC_URL={public_url})",
            LiveKitConfig::MIN_SECRET_LEN,
            LiveKitConfig::MIN_SECRET_LEN
        );
    }
    Ok(())
}

/// ScyllaDB without a login is only for development or a private network:
/// refuse to start a non-local deployment (`THEHIDEOUT_PUBLIC_URL` is not a
/// development host) without `THEHIDEOUT_SCYLLA_USERNAME`/`PASSWORD`, unless
/// `THEHIDEOUT_SCYLLA_ALLOW_NO_AUTH=1` says the database is not reachable by anyone else.
fn check_scylla_auth(auth: Option<&ScyllaAuth>, allow_no_auth: bool, public_url: &url::Url) -> Result<()> {
    if auth.is_none()
        && !allow_no_auth
        && public_url
            .host()
            .is_some_and(|h| !crate::email::is_dev_host(&h))
    {
        bail!(
            "THEHIDEOUT_SCYLLA_USERNAME/THEHIDEOUT_SCYLLA_PASSWORD are not set for a non-local deployment \
             (THEHIDEOUT_PUBLIC_URL={public_url}); set them, or set THEHIDEOUT_SCYLLA_ALLOW_NO_AUTH=1 if \
             ScyllaDB is only reachable on a private network"
        );
    }
    Ok(())
}

#[cfg(test)]
mod scylla_auth_tests {
    use super::*;

    fn page(url: &str) -> url::Url {
        url::Url::parse(url).unwrap()
    }

    fn auth() -> ScyllaAuth {
        ScyllaAuth { username: "app".into(), password: Redacted("pw".into()) }
    }

    #[test]
    fn no_credentials_are_fine_on_dev_hosts_only() {
        for host in ["http://localhost:8080", "http://127.0.0.1:8080", "https://hideout.test"] {
            assert!(check_scylla_auth(None, false, &page(host)).is_ok(), "{host}");
        }
        let err = check_scylla_auth(None, false, &page("https://hideout.example.com")).unwrap_err();
        assert!(err.to_string().contains("THEHIDEOUT_SCYLLA_USERNAME"), "{err}");
        assert!(err.to_string().contains("ALLOW_NO_AUTH"), "{err}");
    }

    #[test]
    fn credentials_or_the_explicit_opt_out_allow_production() {
        let prod = page("https://hideout.example.com");
        assert!(check_scylla_auth(Some(&auth()), false, &prod).is_ok());
        assert!(check_scylla_auth(None, true, &prod).is_ok());
    }

    #[test]
    fn the_password_is_not_debug_printed() {
        assert!(!format!("{:?}", auth()).contains("pw"));
    }

    #[test]
    fn log_format_parses() {
        assert_eq!(LogFormat::parse("").unwrap(), LogFormat::Text);
        assert_eq!(LogFormat::parse("TEXT").unwrap(), LogFormat::Text);
        assert_eq!(LogFormat::parse(" json ").unwrap(), LogFormat::Json);
        assert!(LogFormat::parse("xml").is_err());
    }
}

#[cfg(test)]
mod livekit_config_tests {
    use super::*;
    use std::collections::HashMap;

    const LONG_SECRET: &str = "a-real-secret-of-at-least-32-characters";

    fn from(vars: &[(&str, &str)]) -> Result<Option<LiveKitConfig>> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        LiveKitConfig::from_lookup(|name| vars.get(name).cloned())
    }

    fn full(url: &str, key: &str, secret: &str) -> Result<Option<LiveKitConfig>> {
        from(&[
            ("LIVEKIT_URL", url),
            ("LIVEKIT_API_KEY", key),
            ("LIVEKIT_API_SECRET", secret),
        ])
    }

    fn page(url: &str) -> url::Url {
        url::Url::parse(url).unwrap()
    }

    #[test]
    fn nothing_set_disables_voice_and_partial_settings_are_an_error() {
        assert!(from(&[]).unwrap().is_none());
        // LIVEKIT_API_URL alone does not enable voice.
        assert!(from(&[("LIVEKIT_API_URL", "http://127.0.0.1:7880")])
            .unwrap()
            .is_none());
        assert!(from(&[("LIVEKIT_URL", "ws://127.0.0.1:7880")]).is_err());
        assert!(from(&[("LIVEKIT_API_KEY", "k")]).is_err());
        assert!(from(&[("LIVEKIT_URL", "ws://h:1"), ("LIVEKIT_API_KEY", "k")]).is_err());
        assert!(from(&[
            ("LIVEKIT_API_KEY", "k"),
            ("LIVEKIT_API_SECRET", LONG_SECRET)
        ])
        .is_err());
        // Blank values count as unset.
        assert!(from(&[
            ("LIVEKIT_URL", ""),
            ("LIVEKIT_API_KEY", " "),
            ("LIVEKIT_API_SECRET", "")
        ])
        .unwrap()
        .is_none());
    }

    #[test]
    fn ws_maps_to_http_for_the_api_url() {
        let cfg = |url: &str| full(url, "k", LONG_SECRET).unwrap().unwrap();
        assert_eq!(cfg("ws://127.0.0.1:7880").api_url, "http://127.0.0.1:7880");
        assert_eq!(
            cfg("wss://lk.example.test/").api_url,
            "https://lk.example.test"
        );
        assert_eq!(
            cfg("https://lk.example.test").api_url,
            "https://lk.example.test"
        );
        let with_api = from(&[
            ("LIVEKIT_URL", "wss://lk.example.test"),
            ("LIVEKIT_API_URL", "http://livekit:7880/"),
            ("LIVEKIT_API_KEY", "k"),
            ("LIVEKIT_API_SECRET", LONG_SECRET),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(
            (with_api.url.as_str(), with_api.api_url.as_str()),
            ("wss://lk.example.test", "http://livekit:7880")
        );
    }

    #[test]
    fn urls_must_use_an_accepted_scheme() {
        assert!(full("ftp://lk.example.test", "k", LONG_SECRET).is_err());
        assert!(full("nonsense", "k", LONG_SECRET).is_err());
        assert!(full("ws://", "k", LONG_SECRET).is_err());
        let api = |api_url: &str| {
            from(&[
                ("LIVEKIT_URL", "ws://127.0.0.1:7880"),
                ("LIVEKIT_API_URL", api_url),
                ("LIVEKIT_API_KEY", "k"),
                ("LIVEKIT_API_SECRET", LONG_SECRET),
            ])
        };
        assert!(api("http://127.0.0.1:7880").is_ok() && api("https://lk.example.test").is_ok());
        // The API URL is called over HTTP: websocket and other schemes are refused.
        assert!(api("ws://127.0.0.1:7880").is_err());
        assert!(api("wss://lk.example.test").is_err());
        assert!(api("ftp://lk.example.test").is_err());
        assert!(api("not a url").is_err());
    }

    #[test]
    fn dev_credentials_are_allowed_on_dev_hosts_only() {
        let dev = full("ws://127.0.0.1:7880", "devkey", "secret").unwrap();
        for host in [
            "http://localhost:8080",
            "http://127.0.0.1:8080",
            "https://hideout.test",
            "https://x.example",
        ] {
            assert!(
                check_livekit_credentials(dev.as_ref(), &page(host)).is_ok(),
                "{host}"
            );
        }
        let err = check_livekit_credentials(dev.as_ref(), &page("https://hideout.example.com"))
            .unwrap_err();
        assert!(err.to_string().contains("LIVEKIT_API_SECRET"));
        // Each weak part alone is enough to refuse: the dev key, the dev secret, a short secret.
        let prod = page("https://hideout.example.com");
        for (key, secret) in [
            ("devkey", LONG_SECRET),
            ("realkey", "secret"),
            ("realkey", "short-secret-under-32"),
            ("realkey", ""),
        ] {
            let cfg = LiveKitConfig::new("wss://lk.example.com", None, key.into(), secret.into())
                .unwrap();
            assert!(
                check_livekit_credentials(Some(&cfg), &prod).is_err(),
                "{key}/{secret}"
            );
            assert!(check_livekit_credentials(Some(&cfg), &page("http://localhost:8080")).is_ok());
        }
    }

    #[test]
    fn real_credentials_and_disabled_voice_are_allowed_anywhere() {
        let prod = page("https://hideout.example.com");
        let real = full("wss://lk.example.com", "APIrealkey", LONG_SECRET).unwrap();
        assert!(check_livekit_credentials(real.as_ref(), &prod).is_ok());
        assert!(check_livekit_credentials(None, &prod).is_ok());
        // Exactly 32 characters is long enough.
        let edge = full("wss://lk.example.com", "k", &"x".repeat(32)).unwrap();
        assert!(check_livekit_credentials(edge.as_ref(), &prod).is_ok());
    }
}

#[cfg(test)]
mod s3_credentials_tests {
    use super::*;

    fn s3(access_key: &str, secret_key: &str) -> S3Config {
        S3Config {
            endpoint: "http://127.0.0.1:8333".into(),
            region: "us-east-1".into(),
            bucket: "thehideout-media".into(),
            access_key: access_key.into(),
            secret_key: secret_key.into(),
            path_style: true,
            public_base: None,
        }
    }

    #[test]
    fn dev_credentials_are_allowed_on_dev_hosts_only() {
        let dev = s3(S3Config::DEV_ACCESS_KEY, S3Config::DEV_SECRET_KEY);
        assert!(
            check_s3_credentials(&dev, &url::Url::parse("http://localhost:8080").unwrap()).is_ok()
        );
        assert!(
            check_s3_credentials(&dev, &url::Url::parse("http://127.0.0.1:8080").unwrap()).is_ok()
        );
        assert!(check_s3_credentials(
            &dev,
            &url::Url::parse("https://hideout.example.com").unwrap()
        )
        .is_err());
    }

    #[test]
    fn real_credentials_are_allowed_anywhere() {
        let real = s3("AKIAEXAMPLE", "not-the-dev-secret");
        assert!(check_s3_credentials(
            &real,
            &url::Url::parse("https://hideout.example.com").unwrap()
        )
        .is_ok());
    }
}

#[cfg(test)]
mod policy_tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| pairs.iter().find(|(k, _)| *k == name).map(|(_, v)| (*v).to_owned())
    }

    fn url(s: &str) -> url::Url {
        s.parse().unwrap()
    }

    #[test]
    fn the_contact_email_is_required_away_from_development() {
        let none = env(&[]);
        assert!(policy_from_env(&url("https://chat.example.org"), &none).is_err());
        let ok = policy_from_env(&url("https://chat.example.org"), &env(&[("THEHIDEOUT_CONTACT_EMAIL", " Privacy@Example.org ")])).unwrap();
        assert_eq!(ok.contact_email.as_deref(), Some("privacy@example.org"));
        // Development hosts fall back to the marked placeholder.
        for dev in ["http://localhost:8080", "http://127.0.0.1:8080", "https://hideout.test"] {
            assert!(policy_from_env(&url(dev), &none).unwrap().contact_email.is_none(), "{dev}");
        }
        assert!(policy_from_env(&url("http://localhost"), &env(&[("THEHIDEOUT_CONTACT_EMAIL", "nope")])).is_err());
    }

    #[test]
    fn minimum_versions_parse_with_platform_overrides() {
        let p = policy_from_env(
            &url("http://localhost"),
            &env(&[("THEHIDEOUT_MIN_CLIENT_VERSION", "1.2.0"), ("THEHIDEOUT_MIN_CLIENT_VERSION_IOS", "2.0.1"), ("THEHIDEOUT_OPERATOR_NAME", "Us")]),
        )
        .unwrap();
        assert_eq!(p.operator_name.as_deref(), Some("Us"));
        assert_eq!(p.min_client.minimum("desktop"), Some(crate::policy::Version(1, 2, 0)));
        assert_eq!(p.min_client.minimum("ios"), Some(crate::policy::Version(2, 0, 1)));
        assert!(policy_from_env(&url("http://localhost"), &env(&[("THEHIDEOUT_MIN_CLIENT_VERSION", "soon")])).is_err());
        assert_eq!(policy_from_env(&url("http://localhost"), &env(&[])).unwrap().min_client.minimum("desktop"), None);
    }
}
