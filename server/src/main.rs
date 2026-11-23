use std::sync::Arc;

use anyhow::{Context, Result};
use thehideout_server::config::{Config, LogFormat};
use thehideout_server::email::{self, EmailSender, Mailer, ResendSender, SmtpSender};
use thehideout_server::services::invite;
use thehideout_server::embeds::EmbedSettings;
use thehideout_server::media::{self, attachments::AttachmentSettings};
use thehideout_server::snowflake::Snowflake;
use thehideout_server::storage::Storage;
use thehideout_server::{AppState, build_app, migrations, scylla_conn};
use scylla::client::caching_session::CachingSession;
use thehideout_server::server;
use thehideout_server::shutdown::Shutdown;
use tracing_subscriber::EnvFilter;

/// Number of prepared statements kept by the session cache.
const PREPARED_STATEMENT_CACHE_SIZE: usize = 256;

const USAGE: &str = "usage: thehideout-server [invite <email> [--admin]]";

/// A command-line request other than "run the server".
enum Command {
    /// `invite <email> [--admin]`: create and email an invite, print it, exit.
    Invite { email: String, admin: bool },
}

fn parse_args() -> Result<Option<Command>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => Ok(None),
        Some("invite") => {
            let mut email = None;
            let mut admin = false;
            for arg in &args[1..] {
                match arg.as_str() {
                    "--admin" => admin = true,
                    a if a.starts_with('-') => anyhow::bail!("unknown option {a}\n{USAGE}"),
                    a if email.is_none() => email = Some(a.to_owned()),
                    _ => anyhow::bail!("{USAGE}"),
                }
            }
            let email = email.ok_or_else(|| anyhow::anyhow!("{USAGE}"))?;
            Ok(Some(Command::Invite { email, admin }))
        }
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            std::process::exit(0);
        }
        Some(other) => anyhow::bail!("unknown command {other:?}\n{USAGE}"),
    }
}

fn mailer(config: &Config) -> Result<Mailer> {
    let resend_key = config.resend_api_key.as_ref().filter(|_| {
        let allowed = email::resend_allowed(&config.public_url, config.email_force_resend);
        if !allowed {
            tracing::warn!(
                public_url = %config.public_url,
                smtp = %config.smtp_url,
                "THEHIDEOUT_RESEND_API_KEY is set but THEHIDEOUT_PUBLIC_URL is a development host: \
                 NOT using Resend, sending over SMTP instead (set THEHIDEOUT_EMAIL_FORCE_RESEND=true to override)"
            );
        }
        allowed
    });
    let sender: Arc<dyn EmailSender> = match resend_key {
        Some(key) => {
            tracing::info!("email: Resend");
            Arc::new(ResendSender::new(key.0.clone()))
        }
        None => {
            tracing::info!(smtp = %config.smtp_url, "email: SMTP");
            Arc::new(SmtpSender::new(&config.smtp_url)?)
        }
    };
    Ok(Mailer::new(sender, config.email_from.clone()))
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load `.env` (current directory or a parent) before anything reads the
    // environment, including RUST_LOG. Variables already set in the real
    // environment win over the file. Reported once logging is up.
    let dotenv = dotenvy::dotenv();
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    match LogFormat::from_env()? {
        LogFormat::Text => tracing_subscriber::fmt().with_env_filter(filter).init(),
        LogFormat::Json => tracing_subscriber::fmt().json().with_env_filter(filter).init(),
    }
    match dotenv {
        Ok(path) => tracing::info!(path = %path.display(), "loaded environment file"),
        Err(e) if e.not_found() => {}
        Err(e) => tracing::warn!(error = %e, "could not load .env file; continuing without it"),
    }

    let command = parse_args()?;
    let config = Config::from_env()?;
    if command.is_none() {
        tracing::info!(?config, "starting thehideout-server");
    }

    if config.scylla_auth.is_none() {
        tracing::warn!("ScyllaDB credentials are not set (THEHIDEOUT_SCYLLA_USERNAME/PASSWORD)");
    }
    if config.scylla_tls_ca.is_none() {
        tracing::warn!("connecting to ScyllaDB without TLS (set THEHIDEOUT_SCYLLA_TLS_CA to enable it)");
    }
    let session = scylla_conn::connect(&config.scylla_nodes, config.scylla_auth.as_ref(), config.scylla_tls_ca.as_deref()).await?;
    migrations::run_with(&session, &config.keyspace, &config.scylla_replication).await?;

    let storage = Storage::s3(&config.s3)?;
    let storage_ok = match storage.check().await {
        Ok(()) => true,
        Err(e) => {
            // Degrade instead of failing: everything but media keeps working,
            // and media starts working once the store is reachable.
            tracing::warn!(error = %format!("{e:#}"), s3 = ?config.s3, "object storage is not reachable; media uploads will fail until it is");
            false
        }
    };
    if storage_ok {
        tracing::info!(bucket = %config.s3.bucket, "object storage ready");
    }

    let shutdown = Shutdown::default();
    let state = AppState::new(
        Arc::new(CachingSession::from(session, PREPARED_STATEMENT_CACHE_SIZE)),
        Arc::new(Snowflake::new(config.worker_id)),
        config.keyspace.clone(),
        config.heartbeat_interval,
    )
    .with_shutdown(shutdown.clone())
    .with_trusted_proxies(config.trusted_proxies.clone())
    .with_stream_limits(config.max_streams_per_user, config.max_streams)
    .with_admins(config.admins.clone())
    .with_public_url(config.public_url.clone())
    .with_policy(config.policy.clone())
    .with_web_dir(config.web_dir.clone(), config.livekit.as_ref().map(|l| l.url.clone()))
    .with_mailer(mailer(&config)?)
    .with_storage(storage)
    .with_push(
        thehideout_server::push::Providers::from_config(&config.push)?
            .map(|p| Arc::new(p) as Arc<dyn thehideout_server::push::Transport>),
        Default::default(),
    )
    .with_attachment_settings(AttachmentSettings {
        max_upload_bytes: config.max_upload_bytes,
        ..Default::default()
    })
    .with_embed_settings({
        let mut embeds = EmbedSettings::default();
        embeds.enabled = config.embeds_enabled;
        embeds.contact = config.embeds_contact.clone();
        embeds.mirrors = config.embed_mirrors.clone();
        embeds.max_video_bytes = config.embed_max_video_bytes;
        embeds
    });
    if state.webauthn.is_none() {
        tracing::warn!(public_url = %config.public_url, "passkeys are disabled: THEHIDEOUT_PUBLIC_URL needs a domain name (e.g. localhost)");
    }
    let fail = |e: connectrpc::ConnectError| anyhow::anyhow!("{}", e.message.as_deref().unwrap_or("?"));
    if let Some(Command::Invite { email, admin }) = command {
        let (invite, token, code) = invite::create(
            &state,
            invite::NewInvite { email, inviter_id: None, guild_id: None, guild_ids: Vec::new(), make_admin: admin },
        )
        .await
        .map_err(fail)?;
        state.mailer.flush().await;
        println!("invite {} for {}{}", invite.id, invite.email, if admin { " (site admin)" } else { "" });
        println!("code: {}", thehideout_server::codes::format_invite_code(&code));
        println!("link: {}", invite::invite_link(&state, &token));
        return Ok(());
    }
    invite::bootstrap(&state, config.bootstrap_admin_email.as_deref()).await.map_err(fail)?;
    thehideout_server::admin::startup(&state)
        .await
        .map_err(|e| anyhow::anyhow!("startup jobs failed: {}", e.message.as_deref().unwrap_or("?")))?;
    media::pending::spawn_sweeper(state.clone());
    let mailer = state.mailer.clone();
    let app = build_app(state);

    let listener = tokio::net::TcpListener::bind(config.listen_addr)
        .await
        .with_context(|| format!("binding {}", config.listen_addr))?;
    tracing::info!(addr = %config.listen_addr, "listening");
    tokio::spawn({
        let shutdown = shutdown.clone();
        async move {
            wait_for_signal().await;
            tracing::info!("shutting down: ending event streams and draining connections");
            shutdown.trigger();
        }
    });
    let started = std::time::Instant::now();
    server::serve(listener, app, shutdown.clone(), server::HEADER_READ_TIMEOUT, server::DRAIN_TIMEOUT).await;
    // Mail still being sent or retried gets what is left of the drain budget.
    let left = server::DRAIN_TIMEOUT.saturating_sub(started.elapsed());
    if tokio::time::timeout(left, mailer.flush()).await.is_err() {
        tracing::warn!("some queued email was not sent before shutdown");
    }
    tracing::info!("stopped");
    Ok(())
}

/// Resolves on SIGTERM or SIGINT (Ctrl-C).
async fn wait_for_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let (Ok(mut term), Ok(mut int)) = (signal(SignalKind::terminate()), signal(SignalKind::interrupt())) else {
            tracing::error!("could not install signal handlers");
            return std::future::pending().await;
        };
        tokio::select! {
            _ = term.recv() => tracing::info!("received SIGTERM"),
            _ = int.recv() => tracing::info!("received SIGINT"),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
