//! Apple Push Notification service over HTTP/2 with token-based auth.

use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::config::ApnsConfig;
use super::jwt::Es256Key;
use super::payload::Notification;
use super::{Device, Outcome};

pub const PRODUCTION_URL: &str = "https://api.push.apple.com";
pub const SANDBOX_URL: &str = "https://api.sandbox.push.apple.com";
/// Apple rejects provider tokens older than an hour and dislikes refreshes
/// more often than every 20 minutes; 50 minutes sits between.
const JWT_LIFETIME: Duration = Duration::from_secs(50 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

pub struct Apns {
    key: Es256Key,
    key_id: String,
    team_id: String,
    topic: String,
    sandbox: bool,
    production_url: String,
    sandbox_url: String,
    client: reqwest::Client,
    jwt: Mutex<Option<(String, Instant)>>,
}

impl Apns {
    pub fn new(config: &ApnsConfig) -> anyhow::Result<Self> {
        Ok(Self {
            key: Es256Key::from_pem(&config.key_pem.0)?,
            key_id: config.key_id.clone(),
            team_id: config.team_id.clone(),
            topic: config.topic.clone(),
            sandbox: config.sandbox,
            production_url: PRODUCTION_URL.to_owned(),
            sandbox_url: SANDBOX_URL.to_owned(),
            client: reqwest::Client::builder().timeout(REQUEST_TIMEOUT).build()?,
            jwt: Mutex::new(None),
        })
    }

    /// Points both environments at `url` (tests with a local server).
    pub fn with_base_url(mut self, url: &str) -> Self {
        self.production_url = url.trim_end_matches('/').to_owned();
        self.sandbox_url = self.production_url.clone();
        self
    }

    /// The provider token, signed again after [`JWT_LIFETIME`].
    pub fn provider_token(&self) -> anyhow::Result<String> {
        let mut cached = self.jwt.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((token, made)) = cached.as_ref()
            && made.elapsed() < JWT_LIFETIME
        {
            return Ok(token.clone());
        }
        let iat = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let token = self.key.sign(&self.key_id, &serde_json::json!({ "iss": self.team_id, "iat": iat }))?;
        *cached = Some((token.clone(), Instant::now()));
        Ok(token)
    }

    fn forget_token(&self) {
        *self.jwt.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    pub async fn send(&self, device: &Device, notification: &Notification) -> Outcome {
        let token = match self.provider_token() {
            Ok(t) => t,
            Err(e) => return Outcome::Failed(format!("APNs token: {e}")),
        };
        let base = if self.sandbox || device.sandbox { &self.sandbox_url } else { &self.production_url };
        let response = self
            .client
            .post(format!("{base}/3/device/{}", device.token))
            .header("authorization", format!("bearer {token}"))
            .header("apns-topic", &self.topic)
            .header("apns-push-type", "alert")
            .header("apns-priority", "10")
            .header("apns-collapse-id", truncate_bytes(&notification.collapse_id(), 64))
            .body(payload(notification).to_string())
            .send()
            .await;
        let response = match response {
            Ok(r) => r,
            Err(e) => return Outcome::Retry(None, format!("APNs request: {}", e.without_url())),
        };
        let status = response.status().as_u16();
        let retry_after = response.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()).map(Duration::from_secs);
        let body = response.text().await.unwrap_or_default();
        let outcome = classify(status, &body, retry_after);
        if matches!(&outcome, Outcome::Retry(..)) && body.contains("ExpiredProviderToken") {
            self.forget_token();
        }
        outcome
    }
}

fn truncate_bytes(s: &str, max: usize) -> &str {
    if s.len() <= max { s } else { &s[..max] }
}

/// The APNs JSON body: an alert, `mutable-content` (for a notification service
/// extension), the channel as `thread-id`, and the deep-link data at the top level.
pub fn payload(n: &Notification) -> serde_json::Value {
    let mut body = serde_json::json!({
        "aps": {
            "alert": { "title": n.title, "body": n.body },
            "sound": "default",
            "mutable-content": 1,
            "thread-id": n.collapse_id(),
        }
    });
    for (key, value) in n.data() {
        body[key] = serde_json::Value::String(value);
    }
    body
}

/// Maps an APNs response to what the dispatcher does next.
pub fn classify(status: u16, body: &str, retry_after: Option<Duration>) -> Outcome {
    if status == 200 {
        return Outcome::Delivered;
    }
    let reason = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("reason").and_then(|r| r.as_str().map(str::to_owned)))
        .unwrap_or_default();
    match (status, reason.as_str()) {
        (410, _) | (_, "Unregistered" | "BadDeviceToken" | "DeviceTokenNotForTopic") => Outcome::InvalidToken,
        (403, "ExpiredProviderToken") => Outcome::Retry(None, "APNs provider token expired".into()),
        (429, _) | (500..=599, _) => Outcome::Retry(retry_after, format!("APNs {status} {reason}")),
        _ => Outcome::Failed(format!("APNs {status} {reason}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Redacted;
    use crate::push::jwt::decode;
    use crate::push::payload::Kind;
    use crate::push::testkeys::EC_P8;

    fn config() -> ApnsConfig {
        ApnsConfig { key_pem: Redacted(EC_P8.into()), key_id: "ABC123DEFG".into(), team_id: "TEAM123456".into(), topic: "si.zabit.thehideout".into(), sandbox: false }
    }

    fn note() -> Notification {
        Notification { kind: Kind::Mention, title: "Ana in #general · Hideout".into(), body: "hi @you".into(), guild_id: 7, channel_id: 8, message_id: 9 }
    }

    #[test]
    fn provider_token_is_es256_with_team_and_key_id_and_is_cached() {
        let apns = Apns::new(&config()).unwrap();
        let jwt = apns.provider_token().unwrap();
        let (header, claims, _, _) = decode(&jwt);
        assert_eq!(header, serde_json::json!({ "alg": "ES256", "kid": "ABC123DEFG" }));
        assert_eq!(claims["iss"], "TEAM123456");
        let iat = claims["iat"].as_u64().unwrap();
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        assert!(now.abs_diff(iat) < 5);
        assert_eq!(apns.provider_token().unwrap(), jwt, "cached");
        apns.forget_token();
        // ECDSA signatures are randomised, so a fresh signing differs.
        assert_ne!(apns.provider_token().unwrap(), jwt);
    }

    #[test]
    fn payload_has_alert_thread_mutable_content_and_deep_link_data() {
        let p = payload(&note());
        assert_eq!(p["aps"]["alert"], serde_json::json!({ "title": "Ana in #general · Hideout", "body": "hi @you" }));
        assert_eq!(p["aps"]["mutable-content"], 1);
        assert_eq!(p["aps"]["thread-id"], "8");
        assert!(p["aps"].get("badge").is_none(), "no read state, no badge");
        assert_eq!((p["kind"].as_str(), p["guild_id"].as_str(), p["channel_id"].as_str(), p["message_id"].as_str()), (Some("mention"), Some("7"), Some("8"), Some("9")));
    }

    #[test]
    fn responses_map_to_outcomes() {
        assert_eq!(classify(200, "", None), Outcome::Delivered);
        assert_eq!(classify(410, r#"{"reason":"Unregistered"}"#, None), Outcome::InvalidToken);
        assert_eq!(classify(400, r#"{"reason":"BadDeviceToken"}"#, None), Outcome::InvalidToken);
        assert!(matches!(classify(400, r#"{"reason":"BadTopic"}"#, None), Outcome::Failed(_)));
        assert!(matches!(classify(503, "", Some(Duration::from_secs(3))), Outcome::Retry(Some(d), _) if d == Duration::from_secs(3)));
        assert!(matches!(classify(429, r#"{"reason":"TooManyRequests"}"#, None), Outcome::Retry(None, _)));
        assert!(matches!(classify(403, r#"{"reason":"ExpiredProviderToken"}"#, None), Outcome::Retry(..)));
        assert!(matches!(classify(403, r#"{"reason":"InvalidProviderToken"}"#, None), Outcome::Failed(_)));
    }
}
