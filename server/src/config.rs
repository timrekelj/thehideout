use std::env;
use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{Context, Result, bail};

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
        let embed_mirrors = match env::var("THEHIDEOUT_EMBED_MIRRORS").ok().filter(|s| !s.trim().is_empty()) {
            Some(json) => crate::embeds::providers::Mirrors::default()
                .with_override(&json)
                .map_err(|e| anyhow::anyhow!("invalid THEHIDEOUT_EMBED_MIRRORS: {e}"))?,
            None => crate::embeds::providers::Mirrors::default(),
        };
        let embed_max_video_bytes: usize = var("THEHIDEOUT_EMBED_MAX_VIDEO_BYTES", &crate::embeds::MAX_VIDEO_BYTES.to_string())
            .parse()
            .context("invalid THEHIDEOUT_EMBED_MAX_VIDEO_BYTES")?;
        if embed_max_video_bytes == 0 {
            bail!("THEHIDEOUT_EMBED_MAX_VIDEO_BYTES must be at least 1");
        }
        let public_url = public_url(&listen_addr)?;
        let email_force_resend = match var("THEHIDEOUT_EMAIL_FORCE_RESEND", "false").trim().to_lowercase().as_str() {
            "true" | "1" | "yes" => true,
            "" | "false" | "0" | "no" => false,
            other => bail!("invalid THEHIDEOUT_EMAIL_FORCE_RESEND {other:?}, expected true or false"),
        };
        let non_empty = |name: &str| env::var(name).ok().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
        let s3 = S3Config::from_env()?;
        check_s3_credentials(&s3, &public_url)?;
        Ok(Self {
            public_url,
            email_from: non_empty("THEHIDEOUT_EMAIL_FROM").unwrap_or_else(|| crate::email::DEFAULT_FROM.to_owned()),
            resend_api_key: non_empty("THEHIDEOUT_RESEND_API_KEY").map(Redacted),
            email_force_resend,
            smtp_url: non_empty("THEHIDEOUT_SMTP_URL").unwrap_or_else(|| "smtp://127.0.0.1:1025".to_owned()),
            bootstrap_admin_email: non_empty("THEHIDEOUT_BOOTSTRAP_ADMIN_EMAIL").map(|e| crate::accounts::normalize_email(&e)),
            listen_addr,
            scylla_nodes,
            keyspace,
            worker_id,
            heartbeat_interval: Duration::from_secs(heartbeat_secs),
            s3,
            max_upload_bytes,
            embeds_enabled,
            embeds_contact: env::var("THEHIDEOUT_EMBEDS_CONTACT").ok().filter(|s| !s.trim().is_empty()),
            embed_mirrors,
            embed_max_video_bytes,
            admins: crate::admin::parse_admins(&var("THEHIDEOUT_ADMINS", "")),
        })
    }
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
        let path_style = match var("THEHIDEOUT_S3_PATH_STYLE", "true").to_lowercase().as_str() {
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
            public_base: env::var("THEHIDEOUT_S3_PUBLIC_BASE").ok().filter(|s| !s.is_empty()),
        })
    }
}

/// `THEHIDEOUT_PUBLIC_URL` without a trailing slash, or `http://localhost:<port>`.
fn public_url(listen_addr: &SocketAddr) -> Result<url::Url> {
    let raw = env::var("THEHIDEOUT_PUBLIC_URL").ok().map(|s| s.trim().trim_end_matches('/').to_owned()).filter(|s| !s.is_empty());
    let raw = raw.unwrap_or_else(|| format!("http://localhost:{}", listen_addr.port()));
    let url = url::Url::parse(&raw).with_context(|| format!("invalid THEHIDEOUT_PUBLIC_URL {raw:?}"))?;
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
    if s3.uses_dev_credentials() && public_url.host().is_some_and(|h| !crate::email::is_dev_host(&h)) {
        bail!(
            "THEHIDEOUT_S3_ACCESS_KEY/THEHIDEOUT_S3_SECRET_KEY are the built-in development defaults, \
             which are public; set real credentials for a non-local deployment (THEHIDEOUT_PUBLIC_URL={public_url})"
        );
    }
    Ok(())
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
        assert!(check_s3_credentials(&dev, &url::Url::parse("http://localhost:8080").unwrap()).is_ok());
        assert!(check_s3_credentials(&dev, &url::Url::parse("http://127.0.0.1:8080").unwrap()).is_ok());
        assert!(check_s3_credentials(&dev, &url::Url::parse("https://hideout.example.com").unwrap()).is_err());
    }

    #[test]
    fn real_credentials_are_allowed_anywhere() {
        let real = s3("AKIAEXAMPLE", "not-the-dev-secret");
        assert!(check_s3_credentials(&real, &url::Url::parse("https://hideout.example.com").unwrap()).is_ok());
    }
}
