//! The web client bundle (`THEHIDEOUT_WEB_DIR`) served at `/app/`, same origin as the API.

use std::path::{Path, PathBuf};

use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderName, HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use tower::ServiceExt;
use tower_http::services::ServeDir;
use url::Url;

/// Files that change with every release under a fixed name: always revalidate.
const REVALIDATE: [&str; 2] = ["/index.html", "/boot.js"];
const REVALIDATE_HEADER: &str = "no-cache";
const STATIC_HEADER: &str = "public, max-age=3600";

#[derive(Clone)]
struct Web {
    files: ServeDir,
    csp: HeaderValue,
}

/// The CSP of the web client; `livekit` is the voice server's URL, if voice is on.
pub fn content_security_policy(livekit: Option<&str>) -> String {
    let mut connect = "'self'".to_owned();
    if let Some(origin) = livekit.and_then(|u| Url::parse(u).ok()).map(|u| u.origin()).filter(|o| o.is_tuple()) {
        let ws = origin.ascii_serialization();
        connect.push(' ');
        connect.push_str(&ws);
        // livekit-client probes `http(s)://<host>/validate` to classify a failed connection.
        let http = ws.replacen("wss://", "https://", 1).replacen("ws://", "http://", 1);
        if http != ws {
            connect.push(' ');
            connect.push_str(&http);
        }
    }
    format!(
        "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; \
         img-src 'self' blob: data:; font-src 'self'; connect-src {connect}; media-src 'self' blob:; \
         frame-ancestors 'none'; base-uri 'none'; form-action 'none'"
    )
}

/// Routes of `/app` and `/app/*` over the bundle in `dir`; `None` (with a
/// warning) when `dir` is not a directory.
pub fn routes<S: Clone + Send + Sync + 'static>(dir: &Path, livekit_url: Option<&str>) -> Option<Router<S>> {
    if !dir.is_dir() {
        tracing::warn!(dir = %dir.display(), "THEHIDEOUT_WEB_DIR is not a directory: the web client is not served");
        return None;
    }
    let csp = HeaderValue::from_str(&content_security_policy(livekit_url)).expect("the CSP is a valid header value");
    let web = Web { files: ServeDir::new(dir).precompressed_br().precompressed_gzip(), csp };
    tracing::info!(dir = %dir.display(), "serving the web client at /app/");
    Some(
        Router::new()
            .route("/app", get(|| async { Redirect::permanent("/app/") }))
            .route("/app/", get(serve))
            .route("/app/{*path}", get(serve))
            .with_state(web),
    )
}

/// The directory from `THEHIDEOUT_WEB_DIR` (empty = unset).
pub fn dir_from_env(value: Option<String>) -> Option<PathBuf> {
    value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn rewritten(parts: &axum::http::request::Parts, path: &str) -> Request {
    let mut builder = Request::builder().method(parts.method.clone()).uri(path.parse::<Uri>().expect("a valid path"));
    *builder.headers_mut().expect("a fresh builder") = parts.headers.clone();
    builder.body(Body::empty()).expect("a valid request")
}

async fn serve(State(web): State<Web>, request: Request) -> Response {
    let (parts, _) = request.into_parts();
    let path = parts.uri.path().strip_prefix("/app").unwrap_or("/").to_owned();
    let mut path = if path.is_empty() { "/".to_owned() } else { path };
    let mut response = web.files.clone().oneshot(rewritten(&parts, &path)).await.unwrap_or_else(|e| match e {}).into_response();
    // Client-side routes (no file extension) get the app shell; a missing file stays a 404.
    let last = path.rsplit('/').next().unwrap_or("");
    if response.status() == StatusCode::NOT_FOUND && !last.contains('.') {
        path = "/index.html".to_owned();
        response = web.files.clone().oneshot(rewritten(&parts, &path)).await.unwrap_or_else(|e| match e {}).into_response();
    }
    let cacheable = response.status().is_success() || response.status() == StatusCode::NOT_MODIFIED;
    let headers = response.headers_mut();
    if cacheable {
        let shell = path == "/" || path.ends_with('/') || REVALIDATE.contains(&path.as_str());
        let cache = if shell { REVALIDATE_HEADER } else { STATIC_HEADER };
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    }
    headers.insert(header::CONTENT_SECURITY_POLICY, web.csp);
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(HeaderName::from_static("referrer-policy"), HeaderValue::from_static("no-referrer"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csp_includes_the_livekit_origin_only_when_configured() {
        assert!(content_security_policy(Some("wss://voice.example.com/path")).contains("connect-src 'self' wss://voice.example.com https://voice.example.com;"));
        assert!(content_security_policy(None).contains("connect-src 'self';"));
        assert!(content_security_policy(Some("not a url")).contains("connect-src 'self';"));
    }
}
