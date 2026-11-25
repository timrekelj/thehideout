//! Operator policy: who runs the server (shown on the legal pages), the legal
//! document version and the minimum client version.

use std::collections::HashMap;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// Version (date) of the privacy notice and terms; users accept it at sign-up
/// and again whenever it changes.
pub const LEGAL_VERSION: &str = "2026-11-23";
/// Youngest age allowed to use the service.
pub const MIN_AGE: u32 = 16;
/// Header every first-party client sends: `desktop/1.2.3 (macos)`.
pub const CLIENT_HEADER: &str = "x-thehideout-client";
/// Shown on the legal pages when no contact address is configured (development only).
pub const CONTACT_PLACEHOLDER: &str = "[contact email not configured: set THEHIDEOUT_CONTACT_EMAIL]";
pub const OPERATOR_PLACEHOLDER: &str = "[operator name not configured: set THEHIDEOUT_OPERATOR_NAME]";

/// A parsed `major.minor.patch` (a pre-release or build suffix is ignored).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u64, pub u64, pub u64);

impl Version {
    pub fn parse(raw: &str) -> Option<Version> {
        let core = raw.trim().trim_start_matches('v');
        let core = core.split(['-', '+']).next()?;
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next().map_or(Some(0), |p| p.parse().ok())?;
        let patch = parts.next().map_or(Some(0), |p| p.parse().ok())?;
        if parts.next().is_some() {
            return None;
        }
        Some(Version(major, minor, patch))
    }
}

/// Minimum client versions: one for everybody, optionally overridden per
/// platform (`desktop`, `ios`, `android`, `web`; `web` has no generic fallback).
#[derive(Debug, Clone, Default)]
pub struct MinClient {
    pub default: Option<Version>,
    pub platforms: HashMap<String, Version>,
}

impl MinClient {
    /// Parses a semver env value; `Ok(None)` for unset or empty.
    pub fn parse_var(name: &str, raw: Option<&str>) -> Result<Option<Version>, String> {
        match raw.map(str::trim).filter(|s| !s.is_empty()) {
            None => Ok(None),
            Some(v) => Version::parse(v).map(Some).ok_or_else(|| format!("invalid {name} {v:?}, expected a version like 1.2.3")),
        }
    }

    /// The minimum for `platform`.
    pub fn minimum(&self, platform: &str) -> Option<Version> {
        // The web bundle ships with the server, so it never inherits the generic default.
        if platform == "web" {
            return self.platforms.get(platform).copied();
        }
        self.platforms.get(platform).copied().or(self.default)
    }

    /// `Some(minimum)` when the `x-thehideout-client` value names a client
    /// older than the minimum for its platform. Unparseable values pass.
    pub fn too_old(&self, header: &str) -> Option<Version> {
        let (platform, rest) = header.trim().split_once('/')?;
        let version = Version::parse(rest.split_whitespace().next()?)?;
        let min = self.minimum(&platform.to_ascii_lowercase())?;
        (version < min).then_some(min)
    }
}

/// Settings from the environment that shape policy.
#[derive(Debug, Clone, Default)]
pub struct Policy {
    /// `THEHIDEOUT_CONTACT_EMAIL`.
    pub contact_email: Option<String>,
    /// `THEHIDEOUT_OPERATOR_NAME`.
    pub operator_name: Option<String>,
    pub min_client: MinClient,
    /// `THEHIDEOUT_BOOTSTRAP_ADMIN_EMAIL` (that account cannot be deleted by an admin).
    pub bootstrap_admin_email: Option<String>,
}

/// Paths that never get the client-version check.
fn exempt(path: &str) -> bool {
    ["/health", "/ready", "/privacy", "/terms", "/invite", "/media", "/app"]
        .iter()
        .any(|p| path == *p || path.strip_prefix(p).is_some_and(|rest| rest.starts_with('/')))
}

/// The refusal, shaped for Connect: a JSON error for unary calls, an
/// end-of-stream frame for streaming ones.
fn too_old_response(content_type: &str, min: Version) -> Response {
    let message = format!("client too old: update to {}.{}.{} or newer", min.0, min.1, min.2);
    let error = serde_json::json!({ "code": "failed_precondition", "message": message });
    if content_type.starts_with("application/connect+") {
        let body = serde_json::to_vec(&serde_json::json!({ "error": error })).unwrap_or_default();
        let mut frame = Vec::with_capacity(body.len() + 5);
        frame.push(0x02);
        frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
        frame.extend_from_slice(&body);
        let mut response = Response::new(Body::from(frame));
        *response.status_mut() = StatusCode::OK;
        if let Ok(value) = HeaderValue::from_str(content_type) {
            response.headers_mut().insert(header::CONTENT_TYPE, value);
        }
        return response;
    }
    (StatusCode::BAD_REQUEST, [(header::CONTENT_TYPE, "application/json")], error.to_string()).into_response()
}

/// Refuses requests from clients older than the configured minimum.
pub async fn client_version_middleware(State(min): State<MinClient>, request: Request, next: Next) -> Response {
    if min.default.is_some() || !min.platforms.is_empty() {
        let value = request.headers().get(CLIENT_HEADER).and_then(|v| v.to_str().ok());
        if let Some(value) = value
            && !exempt(request.uri().path())
            && let Some(minimum) = min.too_old(value)
        {
            let content_type = request.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_owned();
            return too_old_response(&content_type, minimum);
        }
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_and_compare() {
        assert_eq!(Version::parse("1.2.3"), Some(Version(1, 2, 3)));
        assert_eq!(Version::parse("v1.2"), Some(Version(1, 2, 0)));
        assert_eq!(Version::parse("1.2.3-beta.1+7"), Some(Version(1, 2, 3)));
        assert_eq!(Version::parse("x"), None);
        assert!(Version(1, 10, 0) > Version(1, 9, 9));
    }

    #[test]
    fn too_old_uses_the_platform_minimum() {
        let mut min = MinClient { default: Version::parse("1.2.0"), ..Default::default() };
        min.platforms.insert("ios".into(), Version(2, 0, 0));
        assert!(min.too_old("desktop/1.1.9 (macos)").is_some());
        assert!(min.too_old("desktop/1.2.0 (macos)").is_none());
        assert!(min.too_old("desktop/1.3.0 (linux)").is_none());
        assert!(min.too_old("ios/1.9.0").is_some());
        assert!(min.too_old("ios/2.0.0").is_none());
        assert!(min.too_old("garbage").is_none());
    }

    #[test]
    fn web_never_inherits_the_default_minimum() {
        let mut min = MinClient { default: Version::parse("1.2.0"), ..Default::default() };
        assert!(min.too_old("web/0.1.0").is_none());
        min.platforms.insert("web".into(), Version(2, 0, 0));
        assert!(min.too_old("web/1.9.0").is_some());
        assert!(min.too_old("web/2.0.0").is_none());
    }

    #[test]
    fn exempt_paths() {
        for p in ["/health", "/ready", "/privacy", "/terms", "/invite/abc", "/media/k", "/app", "/app/", "/app/app_bg.wasm"] {
            assert!(exempt(p), "{p}");
        }
        assert!(!exempt("/thehideout.v1.AuthService/Login"));
        assert!(!exempt("/privacy-x"));
        assert!(!exempt("/application"));
    }
}
