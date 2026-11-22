//! Outgoing email: Resend's HTTP API in production, SMTP (Mailpit) in
//! development, and an in-memory outbox for tests.
//!
//! Mail is sent from a spawned task ([`Mailer::send`]) so request latency
//! does not reveal whether a message was sent (enumeration safety) and a slow
//! provider never stalls an RPC. The task retries transient failures (network
//! errors, 5xx, 429, SMTP 4xx) after [`RETRY_BACKOFF`]; the idempotency key
//! makes a retry of a send that actually went through harmless.

pub mod templates;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;

/// A rendered message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Email {
    /// The template: `invite`, `sign_in_code`, ... (safe to log, unlike the subject).
    pub kind: &'static str,
    pub to: String,
    pub subject: String,
    pub text: String,
    pub html: String,
    /// Stable key so a retried send is delivered once (Resend `Idempotency-Key`).
    pub idempotency_key: String,
}

/// Marker context on a send error that may succeed if tried again.
#[derive(Debug)]
pub struct Transient;

impl std::fmt::Display for Transient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("transient failure")
    }
}

/// Delays before each retry of a failed send: the first attempt, then up to
/// three more at +2 s, +10 s and +30 s.
pub const RETRY_BACKOFF: [Duration; 3] = [Duration::from_secs(2), Duration::from_secs(10), Duration::from_secs(30)];

/// Whether an HTTP status from a mail API means "try again": server errors,
/// 429 (rate limited) and 408 (timeout).
pub fn is_transient_status(status: u16) -> bool {
    status >= 500 || status == 429 || status == 408
}

/// Whether a failed send is worth retrying (see [`Transient`]).
pub fn is_transient(err: &anyhow::Error) -> bool {
    err.downcast_ref::<Transient>().is_some()
}

/// The part of an address after the last `@` (the only part logged).
pub fn recipient_domain(to: &str) -> &str {
    to.trim().rsplit_once('@').map_or("?", |(_, domain)| domain.trim_end_matches('>'))
}

#[async_trait]
pub trait EmailSender: Send + Sync {
    async fn send(&self, from: &str, email: &Email) -> Result<()>;
}

/// Resend HTTP API (`THEHIDEOUT_RESEND_API_KEY`).
pub struct ResendSender {
    api_key: String,
    http: reqwest::Client,
    endpoint: String,
}

impl ResendSender {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .expect("build reqwest client"),
            endpoint: "https://api.resend.com/emails".to_owned(),
        }
    }
}

#[async_trait]
impl EmailSender for ResendSender {
    async fn send(&self, from: &str, email: &Email) -> Result<()> {
        if is_reserved_recipient(&email.to) {
            // Test and documentation addresses never reach a real provider;
            // succeed so the calling flow carries on as if it was sent.
            tracing::warn!(kind = email.kind, domain = recipient_domain(&email.to), "not sending to a reserved test/example domain via Resend; dropped");
            return Ok(());
        }
        let body = serde_json::json!({
            "from": from,
            "to": [email.to],
            "subject": email.subject,
            "text": email.text,
            "html": email.html,
        });
        let resp = self
            .http
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .header("content-type", "application/json")
            .header("idempotency-key", &email.idempotency_key)
            .body(serde_json::to_vec(&body)?)
            .send()
            .await
            .map_err(|e| anyhow::Error::new(e).context("sending to Resend").context(Transient))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            let err = anyhow::anyhow!("Resend answered {status}: {}", text.chars().take(300).collect::<String>());
            return Err(if is_transient_status(status.as_u16()) { err.context(Transient) } else { err });
        }
        Ok(())
    }
}

/// Whether `host` is a development host: `localhost`, `*.localhost`,
/// `*.test`, `*.example` or an IP literal.
pub fn is_dev_host(host: &url::Host<&str>) -> bool {
    match host {
        url::Host::Ipv4(_) | url::Host::Ipv6(_) => true,
        url::Host::Domain(domain) => {
            let domain = domain.trim_end_matches('.').to_ascii_lowercase();
            ["localhost", "test", "example"]
                .iter()
                .any(|tld| domain == *tld || domain.ends_with(&format!(".{tld}")))
        }
    }
}

/// Whether Resend may be used for a server reachable at `public_url`: never
/// for a development host unless `force` (`THEHIDEOUT_EMAIL_FORCE_RESEND`),
/// so a developer `.env` with a real key cannot send real mail from a local
/// or test server.
pub fn resend_allowed(public_url: &url::Url, force: bool) -> bool {
    force || !public_url.host().is_some_and(|host| is_dev_host(&host))
}

/// Whether `to` is at a domain reserved for tests and documentation
/// (`example.com`, `example.net`, `example.org` and their subdomains,
/// `*.test`, `*.example`, `*.invalid`, `localhost`, `*.localhost`).
/// Such mail is never handed to Resend.
pub fn is_reserved_recipient(to: &str) -> bool {
    let Some((_, domain)) = to.trim().rsplit_once('@') else {
        return false;
    };
    let domain = domain.trim_end_matches(['.', '>']).to_ascii_lowercase();
    let within = |zone: &str| domain == zone || domain.ends_with(&format!(".{zone}"));
    ["example.com", "example.net", "example.org", "test", "example", "invalid", "localhost"]
        .iter()
        .any(|zone| within(zone))
}

/// Plain SMTP without auth or TLS, for the local Mailpit sink (`THEHIDEOUT_SMTP_URL`).
pub struct SmtpSender {
    transport: lettre::AsyncSmtpTransport<lettre::Tokio1Executor>,
}

impl SmtpSender {
    /// `smtp://host:port`.
    pub fn new(url: &str) -> Result<Self> {
        let parsed = url::Url::parse(url).with_context(|| format!("invalid SMTP URL {url:?}"))?;
        if parsed.scheme() != "smtp" {
            bail!("SMTP URL must start with smtp:// (got {url:?})");
        }
        let host = parsed.host_str().context("SMTP URL has no host")?.to_owned();
        let port = parsed.port().unwrap_or(25);
        let transport = lettre::AsyncSmtpTransport::<lettre::Tokio1Executor>::builder_dangerous(host)
            .port(port)
            .timeout(Some(std::time::Duration::from_secs(10)))
            .build();
        Ok(Self { transport })
    }
}

#[async_trait]
impl EmailSender for SmtpSender {
    async fn send(&self, from: &str, email: &Email) -> Result<()> {
        use lettre::AsyncTransport;
        use lettre::message::{Message, MultiPart};
        let message = Message::builder()
            .from(from.parse().context("invalid THEHIDEOUT_EMAIL_FROM")?)
            .to(email.to.parse().context("invalid recipient")?)
            .subject(&email.subject)
            .message_id(Some(format!("<{}@thehideout>", email.idempotency_key)))
            .multipart(MultiPart::alternative_plain_html(email.text.clone(), email.html.clone()))?;
        if let Err(e) = self.transport.send(message).await {
            // Only a permanent (5xx) SMTP reply is final; connection and 4xx errors may pass.
            let transient = !e.is_permanent();
            let err = anyhow::Error::new(e).context("sending over SMTP");
            return Err(if transient { err.context(Transient) } else { err });
        }
        Ok(())
    }
}

/// Keeps every message in memory; the default (tests read [`MemorySender::messages`]).
#[derive(Clone, Default)]
pub struct MemorySender {
    messages: Arc<Mutex<Vec<Email>>>,
}

impl MemorySender {
    pub fn messages(&self) -> Vec<Email> {
        self.messages.lock().unwrap().clone()
    }

    /// Messages sent to `to` (case-insensitive), oldest first.
    pub fn to(&self, to: &str) -> Vec<Email> {
        let to = to.to_lowercase();
        self.messages().into_iter().filter(|m| m.to.to_lowercase() == to).collect()
    }
}

#[async_trait]
impl EmailSender for MemorySender {
    async fn send(&self, _from: &str, email: &Email) -> Result<()> {
        self.messages.lock().unwrap().push(email.clone());
        Ok(())
    }
}

/// The configured sender plus the sender address and the public base URL
/// used in links.
#[derive(Clone)]
pub struct Mailer {
    sender: Arc<dyn EmailSender>,
    pub from: String,
    /// Set when `sender` is the in-memory one, so tests can read it.
    pub outbox: Option<MemorySender>,
    /// Sends still running (see [`Mailer::flush`]).
    pending: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
    /// Delays before each retry of a transient failure.
    retry: Arc<[Duration]>,
}

impl Mailer {
    pub fn memory() -> Self {
        let outbox = MemorySender::default();
        Self {
            sender: Arc::new(outbox.clone()),
            from: DEFAULT_FROM.to_owned(),
            outbox: Some(outbox),
            pending: Arc::default(),
            retry: Arc::new(RETRY_BACKOFF),
        }
    }

    pub fn new(sender: Arc<dyn EmailSender>, from: String) -> Self {
        Self { sender, from, outbox: None, pending: Arc::default(), retry: Arc::new(RETRY_BACKOFF) }
    }

    /// Replaces the retry delays (tests).
    pub fn with_retry_schedule(mut self, delays: &[Duration]) -> Self {
        self.retry = delays.into();
        self
    }

    /// Sends in the background, retrying transient failures; the final failure is logged.
    pub fn send(&self, email: Email) {
        let mailer = self.clone();
        let handle = tokio::spawn(async move { mailer.deliver(&email).await });
        let mut pending = self.pending.lock().unwrap();
        pending.retain(|h| !h.is_finished());
        pending.push(handle);
    }

    /// Waits for every background send (the CLI, before exiting).
    pub async fn flush(&self) {
        let handles: Vec<_> = std::mem::take(&mut *self.pending.lock().unwrap());
        for handle in handles {
            let _ = handle.await;
        }
    }

    /// Tries to send, retrying transient failures on the [`retry`](Self::with_retry_schedule)
    /// schedule. Never logs the recipient or subject beyond the template kind and domain.
    async fn deliver(&self, email: &Email) {
        let mut attempt = 0;
        loop {
            let err = match self.send_now(email).await {
                Ok(()) => return,
                Err(e) => e,
            };
            let reason = format!("{err:#}").replace(&email.to, "<recipient>");
            match self.retry.get(attempt) {
                Some(delay) if is_transient(&err) => {
                    tracing::warn!(kind = email.kind, domain = recipient_domain(&email.to), attempt = attempt + 1, retry_in_secs = delay.as_secs_f32(), error = %reason, "sending email failed; will retry");
                    tokio::time::sleep(*delay).await;
                    attempt += 1;
                }
                _ => {
                    tracing::error!(kind = email.kind, domain = recipient_domain(&email.to), attempts = attempt + 1, transient = is_transient(&err), error = %reason, "sending email failed; giving up");
                    return;
                }
            }
        }
    }

    /// Sends once and waits (CLI and bootstrap).
    pub async fn send_now(&self, email: &Email) -> Result<()> {
        self.sender.send(&self.from, email).await?;
        tracing::info!(kind = email.kind, domain = recipient_domain(&email.to), "sent email");
        Ok(())
    }
}

pub const DEFAULT_FROM: &str = "thehideout <noreply@thehideout.zabit.si>";

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn memory_sender_records_messages() {
        let mailer = Mailer::memory();
        let email = Email {
            kind: "test",
            to: "A@x.org".into(),
            subject: "s".into(),
            text: "t".into(),
            html: "h".into(),
            idempotency_key: "k".into(),
        };
        mailer.send_now(&email).await.unwrap();
        assert_eq!(mailer.outbox.as_ref().unwrap().to("a@x.org"), vec![email]);
    }

    fn url(s: &str) -> url::Url {
        url::Url::parse(s).unwrap()
    }

    #[test]
    fn resend_is_not_used_for_dev_public_urls() {
        for dev in [
            "http://localhost:8080",
            "http://LOCALHOST",
            "http://app.localhost",
            "http://127.0.0.1:8080",
            "http://192.168.1.5",
            "http://[::1]:8080",
            "https://hideout.test",
            "https://hideout.example",
        ] {
            assert!(!resend_allowed(&url(dev), false), "{dev}");
            assert!(resend_allowed(&url(dev), true), "{dev} forced");
        }
        for prod in ["https://thehideout.zabit.si", "https://localhost.zabit.si", "https://test.com", "https://example.com"] {
            assert!(resend_allowed(&url(prod), false), "{prod}");
        }
    }

    #[test]
    fn reserved_recipients_are_detected() {
        for to in [
            "a@example.com",
            "a@EXAMPLE.ORG",
            "a@example.net",
            "a@mail.example.com",
            "a@example.test",
            "a@x.test",
            "a@x.example",
            "a@x.invalid",
            "a@localhost",
            "a@app.localhost",
        ] {
            assert!(is_reserved_recipient(to), "{to}");
        }
        for to in ["a@gmail.com", "a@thecalda.com", "a@example.co", "a@notexample.com", "a@test.com", "no-at-sign"] {
            assert!(!is_reserved_recipient(to), "{to}");
        }
    }

    #[tokio::test]
    async fn resend_drops_reserved_recipients_without_a_request() {
        // An unroutable endpoint: reaching it would fail the send.
        let mut sender = ResendSender::new("re_test".into());
        sender.endpoint = "http://127.0.0.1:9/emails".into();
        let mut email = Email {
            kind: "test",
            to: "a@example.com".into(),
            subject: "s".into(),
            text: "t".into(),
            html: "h".into(),
            idempotency_key: "k".into(),
        };
        sender.send(DEFAULT_FROM, &email).await.expect("dropped as success");
        email.to = "a@real-domain.org".into();
        assert!(sender.send(DEFAULT_FROM, &email).await.is_err());
    }

    /// Fails with the scripted results, then succeeds; counts the calls.
    struct Scripted {
        results: Mutex<Vec<bool>>, // true = transient failure, false = permanent
        calls: Mutex<usize>,
    }

    #[async_trait]
    impl EmailSender for Scripted {
        async fn send(&self, _from: &str, _email: &Email) -> Result<()> {
            *self.calls.lock().unwrap() += 1;
            let next = {
                let mut results = self.results.lock().unwrap();
                if results.is_empty() { None } else { Some(results.remove(0)) }
            };
            match next {
                None => Ok(()),
                Some(true) => Err(anyhow::anyhow!("boom for a@x.org").context(Transient)),
                Some(false) => Err(anyhow::anyhow!("rejected")),
            }
        }
    }

    fn scripted(results: &[bool]) -> (Arc<Scripted>, Mailer) {
        let sender = Arc::new(Scripted { results: Mutex::new(results.to_vec()), calls: Mutex::new(0) });
        let mailer = Mailer::new(sender.clone(), DEFAULT_FROM.into()).with_retry_schedule(&[Duration::from_millis(5); 3]);
        (sender, mailer)
    }

    fn test_email() -> Email {
        Email { kind: "test", to: "a@x.org".into(), subject: "s".into(), text: "t".into(), html: "h".into(), idempotency_key: "k".into() }
    }

    #[tokio::test]
    async fn transient_failures_are_retried_until_they_pass() {
        let (sender, mailer) = scripted(&[true, true]);
        mailer.send(test_email());
        mailer.flush().await;
        assert_eq!(*sender.calls.lock().unwrap(), 3);
    }

    #[tokio::test]
    async fn retries_stop_after_the_schedule() {
        let (sender, mailer) = scripted(&[true; 10]);
        mailer.send(test_email());
        mailer.flush().await;
        // The first attempt plus one per delay.
        assert_eq!(*sender.calls.lock().unwrap(), 1 + 3);
    }

    #[tokio::test]
    async fn permanent_failures_are_not_retried() {
        let (sender, mailer) = scripted(&[false, true]);
        mailer.send(test_email());
        mailer.flush().await;
        assert_eq!(*sender.calls.lock().unwrap(), 1);
    }

    #[test]
    fn status_classification() {
        for status in [500, 502, 503, 504, 429, 408] {
            assert!(is_transient_status(status), "{status}");
        }
        for status in [200, 400, 401, 403, 404, 409, 422] {
            assert!(!is_transient_status(status), "{status}");
        }
        assert!(is_transient(&anyhow::anyhow!("x").context(Transient)));
        assert!(is_transient(&anyhow::anyhow!("x").context(Transient).context("outer")));
        assert!(!is_transient(&anyhow::anyhow!("x")));
    }

    #[test]
    fn backoff_schedule_is_2_10_30_seconds() {
        assert_eq!(RETRY_BACKOFF, [Duration::from_secs(2), Duration::from_secs(10), Duration::from_secs(30)]);
    }

    #[test]
    fn only_the_domain_is_logged() {
        assert_eq!(recipient_domain("Someone@Mail.Example.org"), "Mail.Example.org");
        assert_eq!(recipient_domain("Name <a@x.org>"), "x.org");
        assert_eq!(recipient_domain("nonsense"), "?");
    }

    #[test]
    fn smtp_url_is_validated() {
        assert!(SmtpSender::new("smtp://127.0.0.1:1025").is_ok());
        assert!(SmtpSender::new("http://127.0.0.1:1025").is_err());
    }
}
