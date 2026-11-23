//! `GET /`: the public landing page, plus its stylesheet, script, icon and the
//! favicons. Everything is embedded in the binary (no runtime file reads), and
//! served with a tight CSP: scripts and styles only from this origin, and the
//! only network access is the GitHub releases API for the download links.

use axum::Router;
use axum::http::header::{self, HeaderName, HeaderValue};
use axum::response::IntoResponse;
use axum::routing::get;

use crate::AppState;

const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; \
                   connect-src https://api.github.com; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";
const PAGE_CACHE: &str = "public, max-age=300";
const ASSET_CACHE: &str = "public, max-age=86400";

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
    index,
    "../assets/landing/index.html",
    "text/html; charset=utf-8",
    PAGE_CACHE
);
text_asset!(
    style,
    "../assets/landing/site.css",
    "text/css; charset=utf-8",
    ASSET_CACHE
);
text_asset!(
    script,
    "../assets/landing/site.js",
    "text/javascript; charset=utf-8",
    ASSET_CACHE
);
bin_asset!(icon, "../assets/landing/icon.png", "image/png");
bin_asset!(favicon_ico, "../assets/landing/favicon.ico", "image/x-icon");
bin_asset!(favicon_16, "../assets/landing/favicon-16.png", "image/png");
bin_asset!(favicon_32, "../assets/landing/favicon-32.png", "image/png");
bin_asset!(
    apple_touch_icon,
    "../assets/landing/apple-touch-icon.png",
    "image/png"
);

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(index))
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
