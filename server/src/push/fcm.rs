//! Firebase Cloud Messaging (HTTP v1) with a service-account OAuth2 token.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::Deserialize;
use tokio::sync::Mutex;

use super::config::FcmConfig;
use super::jwt::Rs256Key;
use super::payload::Notification;
use super::{Device, Outcome};

pub const API_URL: &str = "https://fcm.googleapis.com";
const DEFAULT_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";
const SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// An access token is renewed this long before it expires.
const EXPIRY_MARGIN: Duration = Duration::from_secs(60);

/// The fields of a Google service-account key file that matter here.
#[derive(Clone, Deserialize)]
pub struct ServiceAccount {
    pub client_email: String,
    pub private_key: String,
    pub project_id: String,
    #[serde(default)]
    pub token_uri: Option<String>,
}

impl ServiceAccount {
    pub fn parse(json: &str) -> Result<Self> {
        let account: Self = serde_json::from_str(json).context("not a service-account JSON key")?;
        if account.client_email.is_empty() || account.project_id.is_empty() {
            anyhow::bail!("client_email and project_id are required");
        }
        Rs256Key::from_pem(&account.private_key)?;
        Ok(account)
    }

    fn token_uri(&self) -> &str {
        self.token_uri.as_deref().filter(|u| !u.is_empty()).unwrap_or(DEFAULT_TOKEN_URI)
    }

    /// The signed JWT assertion exchanged for an access token.
    pub fn assertion(&self, key: &Rs256Key, now: u64) -> Result<String> {
        key.sign(&serde_json::json!({
            "iss": self.client_email,
            "scope": SCOPE,
            "aud": self.token_uri(),
            "iat": now,
            "exp": now + 3600,
        }))
    }
}

pub struct Fcm {
    account: ServiceAccount,
    key: Rs256Key,
    client: reqwest::Client,
    api_url: String,
    token_url: String,
    access: Mutex<Option<(String, Instant)>>,
}

impl Fcm {
    pub fn new(config: &FcmConfig) -> Result<Self> {
        let account = ServiceAccount::parse(&config.service_account_json.0)?;
        Ok(Self {
            key: Rs256Key::from_pem(&account.private_key)?,
            token_url: account.token_uri().to_owned(),
            account,
            client: reqwest::Client::builder().timeout(REQUEST_TIMEOUT).build()?,
            api_url: API_URL.to_owned(),
            access: Mutex::new(None),
        })
    }

    /// Points the API and the OAuth endpoint at `url` (tests with a local server).
    pub fn with_base_url(mut self, url: &str) -> Self {
        self.api_url = url.trim_end_matches('/').to_owned();
        self.token_url = format!("{}/token", self.api_url);
        self
    }

    async fn access_token(&self) -> Result<String> {
        let mut cached = self.access.lock().await;
        if let Some((token, expires)) = cached.as_ref()
            && Instant::now() < *expires
        {
            return Ok(token.clone());
        }
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let assertion = self.account.assertion(&self.key, now)?;
        let form = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer")
            .append_pair("assertion", &assertion)
            .finish();
        let response = self
            .client
            .post(&self.token_url)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(form)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("{}", e.without_url()))?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!("OAuth token request failed with {status}");
        }
        #[derive(Deserialize)]
        struct Token {
            access_token: String,
            #[serde(default)]
            expires_in: u64,
        }
        let token: Token = serde_json::from_str(&body).context("OAuth response")?;
        let lifetime = Duration::from_secs(token.expires_in.max(120)).saturating_sub(EXPIRY_MARGIN);
        *cached = Some((token.access_token.clone(), Instant::now() + lifetime));
        Ok(token.access_token)
    }

    pub async fn send(&self, device: &Device, notification: &Notification) -> Outcome {
        let token = match self.access_token().await {
            Ok(t) => t,
            Err(e) => return Outcome::Retry(None, format!("FCM auth: {e}")),
        };
        let response = self
            .client
            .post(format!("{}/v1/projects/{}/messages:send", self.api_url, self.account.project_id))
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(message(&device.token, notification).to_string())
            .send()
            .await;
        let response = match response {
            Ok(r) => r,
            Err(e) => return Outcome::Retry(None, format!("FCM request: {}", e.without_url())),
        };
        let status = response.status().as_u16();
        let retry_after = response.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()).map(Duration::from_secs);
        let body = response.text().await.unwrap_or_default();
        let outcome = classify(status, &body, retry_after);
        if status == 401 {
            *self.access.lock().await = None;
        }
        outcome
    }
}

/// The FCM v1 request body: a notification plus the deep-link data, with the
/// channel as collapse key and `tag`, and the Android notification channel.
pub fn message(token: &str, n: &Notification) -> serde_json::Value {
    let data: serde_json::Map<String, serde_json::Value> = n.data().into_iter().map(|(k, v)| (k.to_owned(), v.into())).collect();
    serde_json::json!({
        "message": {
            "token": token,
            "notification": { "title": n.title, "body": n.body },
            "data": data,
            "android": {
                "collapse_key": n.collapse_id(),
                "priority": "HIGH",
                "notification": { "channel_id": n.kind.android_channel(), "tag": n.collapse_id() },
            },
        }
    })
}

/// Maps an FCM response to what the dispatcher does next.
pub fn classify(status: u16, body: &str, retry_after: Option<Duration>) -> Outcome {
    if (200..300).contains(&status) {
        return Outcome::Delivered;
    }
    let error = serde_json::from_str::<serde_json::Value>(body).ok().and_then(|v| v.get("error").cloned()).unwrap_or_default();
    let state = error.get("status").and_then(|s| s.as_str()).unwrap_or_default();
    let message = error.get("message").and_then(|s| s.as_str()).unwrap_or_default();
    let code = error
        .get("details")
        .and_then(|d| d.as_array())
        .and_then(|d| d.iter().find_map(|x| x.get("errorCode").and_then(|c| c.as_str())))
        .unwrap_or_default();
    match status {
        _ if code == "UNREGISTERED" || state == "NOT_FOUND" || status == 404 => Outcome::InvalidToken,
        // INVALID_ARGUMENT also covers a bad payload; only a complaint about the token removes the device.
        400 if code == "INVALID_ARGUMENT" || state == "INVALID_ARGUMENT" => {
            if message.to_lowercase().contains("token") {
                Outcome::InvalidToken
            } else {
                Outcome::Failed(format!("FCM 400 INVALID_ARGUMENT: {message}"))
            }
        }
        401 => Outcome::Retry(None, "FCM access token rejected".into()),
        429 | 500..=599 => Outcome::Retry(retry_after, format!("FCM {status} {state}")),
        _ => Outcome::Failed(format!("FCM {status} {state}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Redacted;
    use crate::push::jwt::decode;
    use crate::push::payload::Kind;
    use crate::push::testkeys::RSA_PEM;

    fn account_json() -> String {
        serde_json::json!({ "client_email": "push@proj.iam.gserviceaccount.com", "private_key": RSA_PEM, "project_id": "proj" }).to_string()
    }

    #[test]
    fn oauth_assertion_has_the_service_account_shape() {
        let account = ServiceAccount::parse(&account_json()).unwrap();
        let key = Rs256Key::from_pem(&account.private_key).unwrap();
        let (header, claims, _, _) = decode(&account.assertion(&key, 1_700_000_000).unwrap());
        assert_eq!(header, serde_json::json!({ "alg": "RS256", "typ": "JWT" }));
        assert_eq!(
            claims,
            serde_json::json!({
                "iss": "push@proj.iam.gserviceaccount.com",
                "scope": "https://www.googleapis.com/auth/firebase.messaging",
                "aud": "https://oauth2.googleapis.com/token",
                "iat": 1_700_000_000u64,
                "exp": 1_700_003_600u64,
            })
        );
        assert!(Fcm::new(&FcmConfig { service_account_json: Redacted(account_json()) }).is_ok());
    }

    #[test]
    fn message_has_notification_data_channel_and_collapse_key() {
        let n = Notification { kind: Kind::Message, title: "T".into(), body: "B".into(), guild_id: 1, channel_id: 2, message_id: 3 };
        let m = message("tok", &n);
        assert_eq!(m["message"]["token"], "tok");
        assert_eq!(m["message"]["notification"]["body"], "B");
        assert_eq!(m["message"]["data"]["channel_id"], "2");
        assert_eq!(m["message"]["android"]["collapse_key"], "2");
        assert_eq!(m["message"]["android"]["notification"]["channel_id"], "messages");
        let mention = Notification { kind: Kind::Mention, ..n };
        assert_eq!(message("t", &mention)["message"]["android"]["notification"]["channel_id"], "mentions");
    }

    #[test]
    fn responses_map_to_outcomes() {
        assert_eq!(classify(200, "{}", None), Outcome::Delivered);
        let unregistered = r#"{"error":{"code":404,"status":"NOT_FOUND","details":[{"errorCode":"UNREGISTERED"}]}}"#;
        assert_eq!(classify(404, unregistered, None), Outcome::InvalidToken);
        let bad_token = r#"{"error":{"code":400,"status":"INVALID_ARGUMENT","message":"The registration token is not a valid FCM registration token"}}"#;
        assert_eq!(classify(400, bad_token, None), Outcome::InvalidToken);
        let bad_payload = r#"{"error":{"code":400,"status":"INVALID_ARGUMENT","message":"Invalid value at 'message.data'"}}"#;
        assert!(matches!(classify(400, bad_payload, None), Outcome::Failed(_)));
        assert!(matches!(classify(503, "{}", None), Outcome::Retry(..)));
        assert!(matches!(classify(429, "{}", Some(Duration::from_secs(2))), Outcome::Retry(Some(_), _)));
        assert!(matches!(classify(401, "{}", None), Outcome::Retry(..)));
        assert!(matches!(classify(403, "{}", None), Outcome::Failed(_)));
    }
}
