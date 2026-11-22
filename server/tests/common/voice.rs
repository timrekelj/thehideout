//! LiveKit stand-ins for the voice tests.
#![allow(dead_code)]

use std::sync::Mutex;

use async_trait::async_trait;
use base64::Engine;
use sha2::{Digest, Sha256};
use thehideout_server::config::LiveKitConfig;
use thehideout_server::voice::{LiveKitAdmin, PublishSource};

pub const API_KEY: &str = "testkey";
pub const API_SECRET: &str = "test-secret-that-is-long-enough-for-hs256";
pub const URL: &str = "ws://livekit.test:7880";

pub fn config() -> LiveKitConfig {
    LiveKitConfig::new(URL, None, API_KEY.into(), API_SECRET.into()).expect("valid test config")
}

/// A LiveKit server-API call the server made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Remove { room: String, identity: String },
    Sources { room: String, identity: String, sources: Vec<PublishSource> },
}

/// Records the server-API calls instead of making them.
#[derive(Default)]
pub struct RecordingLiveKit {
    calls: Mutex<Vec<Call>>,
}

impl RecordingLiveKit {
    pub fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }

    pub fn clear(&self) {
        self.calls.lock().unwrap().clear();
    }

    pub fn removed(&self, room: &str, identity: u64) -> bool {
        self.calls().contains(&Call::Remove { room: room.into(), identity: identity.to_string() })
    }
}

#[async_trait]
impl LiveKitAdmin for RecordingLiveKit {
    async fn remove_participant(&self, room: &str, identity: &str) -> Result<(), String> {
        self.calls.lock().unwrap().push(Call::Remove { room: room.into(), identity: identity.into() });
        Ok(())
    }

    async fn set_publish_sources(&self, room: &str, identity: &str, sources: &[PublishSource]) -> Result<(), String> {
        self.calls.lock().unwrap().push(Call::Sources {
            room: room.into(),
            identity: identity.into(),
            sources: sources.to_vec(),
        });
        Ok(())
    }
}

/// A LiveKit webhook request body and its `Authorization` header, signed the
/// way LiveKit does (a JWT carrying the base64 SHA-256 of the body).
pub fn signed_webhook(secret: &str, event: &str, room: &str, identity: &str) -> (String, String) {
    signed_webhook_sid(secret, event, room, identity, "")
}

/// [`signed_webhook`] for the participant connection `sid` (empty = none sent).
pub fn signed_webhook_sid(secret: &str, event: &str, room: &str, identity: &str, sid: &str) -> (String, String) {
    let participant = if sid.is_empty() {
        serde_json::json!({ "identity": identity })
    } else {
        serde_json::json!({ "identity": identity, "sid": sid })
    };
    let body = serde_json::json!({
        "event": event,
        "room": { "name": room },
        "participant": participant,
        "id": "EV_test",
        "createdAt": 1,
    })
    .to_string();
    let hash = base64::engine::general_purpose::STANDARD.encode(Sha256::digest(body.as_bytes()));
    let token = livekit_api::access_token::AccessToken::with_api_key(API_KEY, secret)
        .with_sha256(&hash)
        .to_jwt()
        .expect("sign webhook token");
    (body, token)
}
