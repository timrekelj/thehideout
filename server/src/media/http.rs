//! `GET /media/{key}`: public, immutable serving of avatars, icons, emojis
//! (content-addressed) and message attachments (random 128-bit keys, which are
//! the access control), with single-range support.

use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;

use super::{content_type_of_key, is_attachment_key, parse_key, sniff};
use crate::AppState;
use crate::storage::{ObjectBody, parse_range_header, resolve_range};

/// Requests per peer IP per [`MEDIA_WINDOW`]. Generous: clients load many
/// avatars at once, and responses are immutable so browsers cache them.
const MEDIA_IP_MAX: usize = 1200;
const MEDIA_WINDOW: Duration = Duration::from_secs(60);
const CACHE_CONTROL: &str = "public, max-age=31536000, immutable";

pub fn media_route() -> Router<AppState> {
    Router::new().route("/media/{*key}", get(serve))
}

async fn serve(State(state): State<AppState>, Path(key): Path<String>, request: Request) -> Response {
    let peer = crate::clientip::from_extensions(request.extensions()).map_or_else(|| "unknown".to_owned(), |ip| ip.to_string());
    if state.limiter.check(&format!("media:{peer}"), MEDIA_IP_MAX, MEDIA_WINDOW).is_err() {
        return (StatusCode::TOO_MANY_REQUESTS, "too many requests").into_response();
    }
    // Anything that is not exactly a key we would have produced is a 404;
    // this also rules out traversal before the key reaches the store.
    let Some(hash) = parse_key(&key) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let etag = if key.ends_with("-thumb.webp") { format!("\"{hash}-t\"") } else { format!("\"{hash}\"") };
    let etag_matches = request
        .headers()
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim().trim_start_matches("W/") == etag));
    // A conditional range (`If-Range`) only applies if the validator still matches.
    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_range_header)
        .filter(|_| {
            request
                .headers()
                .get(header::IF_RANGE)
                .and_then(|v| v.to_str().ok())
                .is_none_or(|v| v.trim() == etag)
        });

    let Some(storage) = &state.storage else {
        return (StatusCode::SERVICE_UNAVAILABLE, "media storage is not configured").into_response();
    };
    let unavailable = |e: anyhow::Error| {
        tracing::warn!(error = %format!("{e:#}"), "media read failed");
        (StatusCode::SERVICE_UNAVAILABLE, "media storage unavailable").into_response()
    };
    // Ranges need the size up front to be validated before the read.
    let mut byte_range = None;
    if let Some(range) = range.filter(|_| !etag_matches) {
        let size = match storage.head(&key).await {
            Ok(Some(info)) => info.size,
            Ok(None) => return StatusCode::NOT_FOUND.into_response(),
            Err(e) => return unavailable(e),
        };
        match resolve_range(range, size) {
            Some(r) => byte_range = Some(r),
            None => {
                let mut r = StatusCode::RANGE_NOT_SATISFIABLE.into_response();
                r.headers_mut().insert(header::CONTENT_RANGE, HeaderValue::from_str(&format!("bytes */{size}")).expect("ascii"));
                return r;
            }
        }
    }
    let object = match storage.get_range(&key, byte_range).await {
        Ok(Some(object)) => object,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return unavailable(e),
    };
    let attachment = is_attachment_key(&key);
    let mut content_type = object.info.content_type.clone().unwrap_or_else(|| content_type_of_key(&key).to_owned());
    // Attachments carry the disposition chosen at upload. Re-check it here: a
    // type that must not render inline is never served inline, whatever was stored.
    let disposition = attachment.then(|| {
        if key.ends_with("-thumb.webp") {
            return "inline".to_owned();
        }
        let safe = sniff::serving(&content_type, true);
        content_type = safe.content_type;
        match object.info.content_disposition.clone() {
            Some(d) if safe.inline || !d.starts_with("inline") => d,
            _ => "attachment".to_owned(),
        }
    });
    let mut response = if etag_matches {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        let partial = byte_range_of(&object);
        let mut r = Response::new(Body::from_stream(object.stream));
        if let Ok(v) = HeaderValue::from_str(&content_type) {
            r.headers_mut().insert(header::CONTENT_TYPE, v);
        }
        if let Some(range) = partial {
            *r.status_mut() = StatusCode::PARTIAL_CONTENT;
            let value = format!("bytes {}-{}/{}", range.start, range.end - 1, object.info.size);
            r.headers_mut().insert(header::CONTENT_RANGE, HeaderValue::from_str(&value).expect("ascii"));
            r.headers_mut().insert(header::CONTENT_LENGTH, (range.end - range.start).into());
        } else {
            r.headers_mut().insert(header::CONTENT_LENGTH, object.info.size.into());
        }
        r
    };
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(CACHE_CONTROL));
    headers.insert(header::ETAG, HeaderValue::from_str(&etag).expect("hex etag"));
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    if let Some(d) = disposition
        && let Ok(v) = HeaderValue::from_str(&d)
    {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    response
}

/// The range a partial response covers, `None` if the whole object is sent.
fn byte_range_of(object: &ObjectBody) -> Option<std::ops::Range<u64>> {
    (object.range != (0..object.info.size)).then(|| object.range.clone())
}
