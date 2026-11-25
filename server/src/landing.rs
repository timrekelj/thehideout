//! `GET /`: the public landing page, plus its stylesheet, script, icon and the
//! favicons. Everything is embedded in the binary (no runtime file reads), and
//! served with a tight CSP: scripts and styles only from this origin, and the
//! only network access is the GitHub releases API for the download links.

use std::sync::OnceLock;

use axum::Router;
use axum::http::header::{self, HeaderName, HeaderValue};
use axum::response::IntoResponse;
use axum::routing::{MethodRouter, get};

use crate::AppState;

const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; \
                   connect-src https://api.github.com; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";
const PAGE_CACHE: &str = "public, max-age=300";
const ASSET_CACHE: &str = "public, max-age=86400";
/// For URLs that carry a content hash.
const VERSIONED_CACHE: &str = "public, max-age=31536000, immutable";

macro_rules! text_asset {
    ($name:ident, $path:literal, $mime:literal, $cache:expr) => {
        async fn $name() -> impl IntoResponse {
            asset(include_str!($path).as_bytes(), $mime, $cache)
        }
    };
}
macro_rules! bin_asset {
    ($name:ident, $path:literal, $mime:literal) => {
        async fn $name() -> impl IntoResponse {
            asset(include_bytes!($path), $mime, ASSET_CACHE)
        }
    };
}

text_asset!(
    style,
    "../assets/landing/site.css",
    "text/css; charset=utf-8",
    VERSIONED_CACHE
);
text_asset!(
    script,
    "../assets/landing/site.js",
    "text/javascript; charset=utf-8",
    VERSIONED_CACHE
);

/// FNV-1a over `bytes`, evaluated at compile time: a content hash for cache busting.
const fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    hash
}

const CSS_HASH: u64 = fnv1a(include_bytes!("../assets/landing/site.css"));
const JS_HASH: u64 = fnv1a(include_bytes!("../assets/landing/site.js"));

/// The landing page with its stylesheet and script URLs carrying a content
/// hash, built once. A changed asset gets a new URL, so the long-cached old
/// one is never used with a new page (or the other way round).
fn page() -> &'static str {
    static PAGE: OnceLock<String> = OnceLock::new();
    PAGE.get_or_init(|| {
        include_str!("../assets/landing/index.html")
            .replace("@CSS_VERSION@", &format!("{CSS_HASH:016x}"))
            .replace("@JS_VERSION@", &format!("{JS_HASH:016x}"))
    })
}

async fn index() -> impl IntoResponse {
    asset(page().as_bytes(), "text/html; charset=utf-8", PAGE_CACHE)
}

bin_asset!(icon, "../assets/landing/icon.png", "image/png");
bin_asset!(favicon_ico, "../assets/landing/favicon.ico", "image/x-icon");
bin_asset!(favicon_16, "../assets/landing/favicon-16.png", "image/png");
bin_asset!(favicon_32, "../assets/landing/favicon-32.png", "image/png");
bin_asset!(
    apple_touch_icon,
    "../assets/landing/apple-touch-icon.png",
    "image/png"
);

/// The `GET /` handler. The caller adds the Connect service as its fallback so
/// other methods on `/` behave as before the landing page existed (a bare `get`
/// route would answer them with 405).
pub fn root() -> MethodRouter<AppState> {
    get(index)
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/landing/site.css", get(style))
        .route("/landing/site.js", get(script))
        .route("/landing/icon.png", get(icon))
        .route("/favicon.ico", get(favicon_ico))
        .route("/favicon-16.png", get(favicon_16))
        .route("/favicon-32.png", get(favicon_32))
        .route("/apple-touch-icon.png", get(apple_touch_icon))
}

/// An embedded file with the shared security headers. The CSP only matters on
/// the HTML document, but is sent on every response for uniformity.
fn asset(
    body: &'static [u8],
    mime: &'static str,
    cache: &'static str,
) -> impl IntoResponse + use<> {
    (
        [
            (header::CONTENT_TYPE, HeaderValue::from_static(mime)),
            (header::CACHE_CONTROL, HeaderValue::from_static(cache)),
            (
                HeaderName::from_static("content-security-policy"),
                HeaderValue::from_static(CSP),
            ),
            (
                HeaderName::from_static("x-frame-options"),
                HeaderValue::from_static("DENY"),
            ),
            (
                HeaderName::from_static("x-content-type-options"),
                HeaderValue::from_static("nosniff"),
            ),
            // The page links to other origins only as plain links; send no referrer.
            (
                HeaderName::from_static("referrer-policy"),
                HeaderValue::from_static("no-referrer"),
            ),
        ],
        body,
    )
}
