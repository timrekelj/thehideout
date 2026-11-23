//! `ClientTransport` over the browser Fetch API (wasm32 only).
//!
//! Request bodies are buffered (Connect unary and server-streaming requests are one small
//! message), but response bodies are streamed chunk by chunk from `response.body()`, so
//! server-streaming RPCs such as the gateway's `Subscribe` deliver messages as they arrive.
//!
//! The JS types are `!Send`; [`SendWrapper`] satisfies the `Send` bounds of
//! [`ClientTransport`]. That is sound here because wasm32-unknown-unknown is
//! single-threaded: the wrapped values are never touched from another thread.

use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use connectrpc::ConnectError;
use connectrpc::client::{ClientBody, ClientTransport};
use futures::future::BoxFuture;
use futures::stream::{Stream, StreamExt};
use http::{Request, Response};
use http_body::{Body, Frame};
use http_body_util::BodyExt;
use send_wrapper::SendWrapper;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{AbortController, ReadableStreamDefaultReader};

/// Fetch-based transport for browser environments.
#[derive(Clone, Copy, Debug, Default)]
pub struct FetchTransport;

/// Browser failures (network down, CORS, aborted) surface as `unavailable`, like the native
/// transport's connection errors. `JsValue` is neither `Send` nor `Error`, so it is stringified.
fn js_err(val: JsValue) -> ConnectError {
    let text = val
        .as_string()
        .or_else(|| val.dyn_ref::<js_sys::Error>().map(|e| String::from(e.message())))
        .unwrap_or_else(|| format!("{val:?}"));
    ConnectError::unavailable_from_transport("fetch failed", text)
}

/// Aborts the fetch (and its response stream) when dropped, so dropping a request future
/// or an open stream cancels the network request.
struct AbortOnDrop(AbortController);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

type ChunkStream = Pin<Box<dyn Stream<Item = Result<Frame<Bytes>, ConnectError>>>>;

/// A response body streaming from a `ReadableStream`.
pub struct ResponseBody {
    // Field order matters: the stream (and its reader) drops before the abort fires.
    chunks: SendWrapper<ChunkStream>,
    _abort: SendWrapper<AbortOnDrop>,
}

impl Body for ResponseBody {
    type Data = Bytes;
    type Error = ConnectError;

    fn poll_frame(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, ConnectError>>> {
        self.get_mut().chunks.as_mut().poll_next(cx)
    }
}

impl ClientTransport for FetchTransport {
    type ResponseBody = ResponseBody;
    type Error = ConnectError;

    fn send(&self, request: Request<ClientBody>) -> BoxFuture<'static, Result<Response<ResponseBody>, ConnectError>> {
        Box::pin(SendWrapper::new(fetch(request)))
    }
}

async fn fetch(request: Request<ClientBody>) -> Result<Response<ResponseBody>, ConnectError> {
    let (parts, body) = request.into_parts();
    let body_bytes = body.collect().await?.to_bytes();

    let headers = web_sys::Headers::new().map_err(js_err)?;
    for (name, value) in &parts.headers {
        let value = value.to_str().map_err(|e| ConnectError::invalid_argument(format!("bad header {name}: {e}")))?;
        headers.append(name.as_str(), value).map_err(js_err)?;
    }

    let abort = AbortOnDrop(AbortController::new().map_err(js_err)?);
    let init = web_sys::RequestInit::new();
    init.set_method(parts.method.as_str());
    init.set_headers(&headers);
    init.set_signal(Some(&abort.0.signal()));
    if !body_bytes.is_empty() || parts.method != http::Method::GET {
        init.set_body(&js_sys::Uint8Array::from(body_bytes.as_ref()));
    }
    let js_req = web_sys::Request::new_with_str_and_init(&parts.uri.to_string(), &init).map_err(js_err)?;

    // The global `fetch` works in window and worker scopes alike.
    let fetch_fn: js_sys::Function = js_sys::Reflect::get(&js_sys::global(), &"fetch".into())
        .map_err(js_err)?
        .dyn_into()
        .map_err(js_err)?;
    let js_resp: web_sys::Response =
        JsFuture::from(fetch_fn.call1(&JsValue::undefined(), &js_req).map_err(js_err)?.dyn_into::<js_sys::Promise>().map_err(js_err)?)
            .await
            .map_err(js_err)?
            .dyn_into()
            .map_err(js_err)?;

    let mut builder = Response::builder().status(http::StatusCode::from_u16(js_resp.status()).map_err(|e| ConnectError::internal(format!("bad status: {e}")))?);
    if let Some(iter) = js_sys::try_iter(&js_resp.headers()).map_err(js_err)? {
        for entry in iter {
            let pair: js_sys::Array = entry.map_err(js_err)?.into();
            let (Some(key), Some(val)) = (pair.get(0).as_string(), pair.get(1).as_string()) else {
                continue;
            };
            // The browser decodes the body per Content-Encoding but leaves these two headers
            // describing the encoded bytes; keeping them would make the client decompress
            // twice and trust a wrong length.
            if key.eq_ignore_ascii_case("content-encoding") || key.eq_ignore_ascii_case("content-length") {
                continue;
            }
            builder = builder.header(key, val);
        }
    }

    let chunks: ChunkStream = match js_resp.body() {
        Some(stream) => {
            let reader: ReadableStreamDefaultReader = stream.get_reader().unchecked_into();
            Box::pin(futures::stream::unfold(Some(reader), |reader| async move {
                let reader = reader?;
                match read_chunk(&reader).await {
                    Ok(Some(bytes)) => Some((Ok(Frame::data(bytes)), Some(reader))),
                    Ok(None) => None,
                    Err(e) => Some((Err(e), None)),
                }
            }))
        }
        None => futures::stream::empty().boxed_local(),
    };
    builder
        .body(ResponseBody { chunks: SendWrapper::new(chunks), _abort: SendWrapper::new(abort) })
        .map_err(|e| ConnectError::internal(e.to_string()))
}

/// The next chunk of the stream, `None` at its end.
async fn read_chunk(reader: &ReadableStreamDefaultReader) -> Result<Option<Bytes>, ConnectError> {
    let result = JsFuture::from(reader.read()).await.map_err(js_err)?;
    if js_sys::Reflect::get(&result, &"done".into()).map_err(js_err)?.as_bool().unwrap_or(true) {
        return Ok(None);
    }
    let value = js_sys::Reflect::get(&result, &"value".into()).map_err(js_err)?;
    Ok(Some(Bytes::from(js_sys::Uint8Array::new(&value).to_vec())))
}
