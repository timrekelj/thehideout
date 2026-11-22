//! Passkeys (WebAuthn): the relying party, stored credentials and the
//! browser ceremony page for native clients.
//!
//! The RP id is the host of `THEHIDEOUT_PUBLIC_URL` (dev: `localhost`) and
//! the only accepted origin is that URL's origin. Native apps that cannot run
//! WebAuthn themselves open `GET {PUBLIC_URL}/auth/passkey?ceremony=<id>` in
//! the system browser: the page fetches the options, calls
//! `navigator.credentials`, posts the result back and (optionally) redirects
//! to `http://127.0.0.1:<port>/done`; the app polls Finish* with an empty
//! credential, which picks up the stored result.

use std::sync::Arc;

use axum::Router;
use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use chrono::{DateTime, Utc};
use connectrpc::ConnectError;
use serde::Deserialize;
use webauthn_rs::prelude::{AuthenticationResult, Passkey, PasskeyAuthentication, PasskeyRegistration, Url, Uuid};
use webauthn_rs::{Webauthn, WebauthnBuilder};

use crate::AppState;
use crate::auth::internal;
use crate::convert::{datetime_to_proto, id_from_db, id_to_db};
use crate::proto::thehideout::v1 as pb;

/// Builds the relying party for `public_url`; `None` (passkeys disabled) when
/// its host is not a domain name (WebAuthn needs one; `localhost` works).
pub fn relying_party(public_url: &Url) -> Option<Arc<Webauthn>> {
    let rp_id = public_url.domain()?;
    let origin = Url::parse(&public_url.origin().ascii_serialization()).ok()?;
    match WebauthnBuilder::new(rp_id, &origin).and_then(|b| b.rp_name("thehideout").build()) {
        Ok(webauthn) => Some(Arc::new(webauthn)),
        Err(e) => {
            tracing::warn!(error = %e, %public_url, "passkeys disabled: invalid relying party");
            None
        }
    }
}

/// The WebAuthn user handle of an account.
pub fn user_handle(user_id: u64) -> Uuid {
    Uuid::from_u64_pair(0x7468_6568_6964_656f, user_id)
}

/// What a ceremony is for.
#[derive(Clone)]
pub enum CeremonyKind {
    Register { user_id: u64, name: String, state: PasskeyRegistration },
    Login { user_id: u64, device_name: String, state: PasskeyAuthentication },
    /// Answers the second-factor challenge `challenge_id`.
    SecondFactor { challenge_id: String, user_id: u64, state: PasskeyAuthentication },
}

/// A running ceremony (kept in memory for 5 minutes).
#[derive(Clone)]
pub struct Ceremony {
    pub kind: CeremonyKind,
    /// The browser options, as JSON.
    pub options_json: String,
    /// Credential JSON posted by the ceremony page.
    pub result: Option<String>,
    pub redirect_port: Option<u16>,
}

pub fn ceremony_url(state: &AppState, id: &str) -> String {
    format!("{}/auth/passkey?ceremony={id}", state.public_base())
}

/// The `PasskeyCeremony` message of a new ceremony.
pub fn ceremony_proto(state: &AppState, id: &str, options_json: &str) -> pb::PasskeyCeremony {
    pb::PasskeyCeremony {
        ceremony_id: id.to_owned(),
        ceremony_url: ceremony_url(state, id),
        options_json: options_json.to_owned(),
        ..Default::default()
    }
}

pub fn webauthn(state: &AppState) -> Result<&Webauthn, ConnectError> {
    state
        .webauthn
        .as_deref()
        .ok_or_else(|| ConnectError::failed_precondition("passkeys need THEHIDEOUT_PUBLIC_URL with a domain name"))
}

/// A stored passkey.
pub struct StoredPasskey {
    pub id: u64,
    pub name: String,
    pub credential: Passkey,
    pub created_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
}

impl StoredPasskey {
    pub fn to_proto(&self) -> pb::Passkey {
        pb::Passkey {
            id: self.id,
            name: self.name.clone(),
            created_at: self.created_at.map(datetime_to_proto).into(),
            last_used_at: self.last_used_at.map(datetime_to_proto).into(),
            ..Default::default()
        }
    }
}

/// The user's passkeys, oldest first.
pub async fn list(state: &AppState, user_id: u64) -> Result<Vec<StoredPasskey>, ConnectError> {
    let rows = state
        .session
        .execute_unpaged(
            "SELECT id, name, credential, created_at, last_used_at FROM passkeys WHERE user_id = ?",
            (id_to_db(user_id),),
        )
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .rows::<(i64, Option<String>, Option<String>, Option<DateTime<Utc>>, Option<DateTime<Utc>>)>()
        .map_err(internal)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?;
    let mut out = Vec::with_capacity(rows.len());
    for (id, name, credential, created_at, last_used_at) in rows {
        let Some(credential) = credential.and_then(|c| serde_json::from_str::<Passkey>(&c).ok()) else {
            tracing::warn!(passkey_id = id, "skipping unreadable passkey");
            continue;
        };
        out.push(StoredPasskey { id: id_from_db(id), name: name.unwrap_or_default(), credential, created_at, last_used_at });
    }
    out.sort_by_key(|p| p.id);
    Ok(out)
}

/// Writes (inserts or replaces) a passkey.
pub async fn save(state: &AppState, user_id: u64, passkey: &StoredPasskey) -> Result<(), ConnectError> {
    let json = serde_json::to_string(&passkey.credential).map_err(internal)?;
    state
        .session
        .execute_unpaged(
            "INSERT INTO passkeys (user_id, id, name, credential, created_at, last_used_at) VALUES (?, ?, ?, ?, ?, ?)",
            (id_to_db(user_id), id_to_db(passkey.id), &passkey.name, json, passkey.created_at, passkey.last_used_at),
        )
        .await
        .map_err(internal)?;
    Ok(())
}

/// Records a successful sign-in with one of the user's passkeys (counter, last used).
pub async fn mark_used(state: &AppState, user_id: u64, result: &AuthenticationResult) -> Result<(), ConnectError> {
    if let Some(mut passkey) = list(state, user_id).await?.into_iter().find(|p| p.credential.cred_id() == result.cred_id()) {
        passkey.credential.update_credential(result);
        passkey.last_used_at = Some(Utc::now());
        save(state, user_id, &passkey).await?;
    }
    Ok(())
}

pub async fn delete(state: &AppState, user_id: u64, id: u64) -> Result<(), ConnectError> {
    state
        .session
        .execute_unpaged("DELETE FROM passkeys WHERE user_id = ? AND id = ?", (id_to_db(user_id), id_to_db(id)))
        .await
        .map_err(internal)?;
    Ok(())
}

/// Routes of the ceremony page.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/passkey", get(page))
        .route("/auth/passkey/options", get(options))
        .route("/auth/passkey/result", axum::routing::post(result))
}

#[derive(Deserialize)]
struct CeremonyQuery {
    ceremony: String,
}

async fn page() -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
            (header::HeaderName::from_static("content-security-policy"), "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'; form-action 'none'; frame-ancestors 'none'"),
            (header::HeaderName::from_static("x-frame-options"), "DENY"),
            (header::HeaderName::from_static("x-content-type-options"), "nosniff"),
        ],
        PAGE,
    )
        .into_response()
}

async fn options(State(state): State<AppState>, Query(q): Query<CeremonyQuery>) -> Response {
    let Some(ceremony) = state.auth.ceremonies.get(&q.ceremony) else {
        return (StatusCode::NOT_FOUND, "ceremony not found or expired").into_response();
    };
    let kind = match ceremony.kind {
        CeremonyKind::Register { .. } => "register",
        CeremonyKind::Login { .. } | CeremonyKind::SecondFactor { .. } => "login",
    };
    let body = serde_json::json!({
        "kind": kind,
        "options": serde_json::from_str::<serde_json::Value>(&ceremony.options_json).unwrap_or_default(),
        "redirect": ceremony.redirect_port.map(|p| format!("http://127.0.0.1:{p}/done")),
    });
    ([(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-store")], body.to_string()).into_response()
}

/// Largest credential JSON the page may post.
const MAX_RESULT_BYTES: usize = 32 * 1024;

async fn result(State(state): State<AppState>, Query(q): Query<CeremonyQuery>, body: String) -> Response {
    if body.len() > MAX_RESULT_BYTES || serde_json::from_str::<serde_json::Value>(&body).is_err() {
        return (StatusCode::BAD_REQUEST, "invalid credential").into_response();
    }
    match state.auth.ceremonies.update(&q.ceremony, |c| c.result = Some(body)) {
        Some(()) => StatusCode::NO_CONTENT.into_response(),
        None => (StatusCode::NOT_FOUND, "ceremony not found or expired").into_response(),
    }
}

const PAGE: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width">
<title>thehideout passkey</title>
<style>body{font-family:-apple-system,Segoe UI,sans-serif;max-width:420px;margin:15vh auto;padding:0 16px;color:#1b1b1f}button{font:inherit;padding:8px 16px}</style>
</head><body>
<h2 id="title">Passkey</h2><p id="status">Loading…</p><button id="go" hidden>Continue</button>
<script>
const id = new URLSearchParams(location.search).get("ceremony");
const st = (t) => document.getElementById("status").textContent = t;
const b64 = (buf) => btoa(String.fromCharCode(...new Uint8Array(buf))).replace(/\+/g,"-").replace(/\//g,"_").replace(/=+$/,"");
const unb64 = (s) => Uint8Array.from(atob(s.replace(/-/g,"+").replace(/_/g,"/")), c => c.charCodeAt(0));
async function run(cfg) {
  const pk = cfg.options.publicKey;
  pk.challenge = unb64(pk.challenge);
  let cred, out;
  if (cfg.kind === "register") {
    pk.user.id = unb64(pk.user.id);
    (pk.excludeCredentials || []).forEach(c => c.id = unb64(c.id));
    cred = await navigator.credentials.create({ publicKey: pk });
    out = { id: cred.id, rawId: b64(cred.rawId), type: cred.type, extensions: cred.getClientExtensionResults(),
      response: { attestationObject: b64(cred.response.attestationObject), clientDataJSON: b64(cred.response.clientDataJSON),
        transports: cred.response.getTransports ? cred.response.getTransports() : [] } };
  } else {
    (pk.allowCredentials || []).forEach(c => c.id = unb64(c.id));
    cred = await navigator.credentials.get({ publicKey: pk });
    out = { id: cred.id, rawId: b64(cred.rawId), type: cred.type, extensions: cred.getClientExtensionResults(),
      response: { authenticatorData: b64(cred.response.authenticatorData), clientDataJSON: b64(cred.response.clientDataJSON),
        signature: b64(cred.response.signature), userHandle: cred.response.userHandle ? b64(cred.response.userHandle) : null } };
  }
  const r = await fetch("/auth/passkey/result?ceremony=" + encodeURIComponent(id), { method: "POST", body: JSON.stringify(out) });
  if (!r.ok) throw new Error(await r.text());
  st("Done. You can close this tab and return to thehideout.");
  if (cfg.redirect) location.href = cfg.redirect;
}
(async () => {
  try {
    const r = await fetch("/auth/passkey/options?ceremony=" + encodeURIComponent(id));
    if (!r.ok) throw new Error(await r.text());
    const cfg = await r.json();
    document.getElementById("title").textContent = cfg.kind === "register" ? "Add a passkey" : "Sign in with a passkey";
    st("Click continue and follow your browser's prompt.");
    const go = document.getElementById("go");
    go.hidden = false;
    go.onclick = () => { go.hidden = true; st("Waiting for your passkey…"); run(cfg).catch(e => { st("Failed: " + e.message); go.hidden = false; }); };
  } catch (e) { st("Failed: " + e.message); }
})();
</script></body></html>"#;
