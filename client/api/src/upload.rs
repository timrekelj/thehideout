//! Plain-HTTP file transfer: streaming attachment uploads and downloads.

use std::io::{Read, Write};
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};

use bytes::Bytes;
use connectrpc::client::{ClientBody, ClientTransport};
use connectrpc::{ConnectError, ErrorCode};
use http_body::{Body, Frame, SizeHint};
use http_body_util::BodyExt;

use crate::client::Client;
use crate::content::PendingAttachment;
use crate::error::ApiError;
use crate::proto::thehideout::v1 as pb;

type Result<T> = std::result::Result<T, ApiError>;

/// Bytes handed to the transport per chunk (and the granularity of upload progress).
const CHUNK: usize = 64 * 1024;

/// A request body that reads `total` bytes from `reader` in [`CHUNK`]s and reports
/// the running total as each chunk is pulled by the connection.
struct ReaderBody {
    reader: Mutex<Box<dyn Read + Send>>,
    total: u64,
    sent: u64,
    progress: Box<dyn Fn(u64, u64) + Send + Sync>,
}

impl Body for ReaderBody {
    type Data = Bytes;
    type Error = ConnectError;

    fn poll_frame(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<std::result::Result<Frame<Bytes>, ConnectError>>> {
        let this = self.get_mut();
        if this.sent >= this.total {
            return Poll::Ready(None);
        }
        let want = CHUNK.min((this.total - this.sent) as usize);
        let mut buf = vec![0u8; want];
        let mut filled = 0;
        {
            // Local files read fast; a chunk is read here without yielding.
            let mut reader = this.reader.lock().unwrap();
            while filled < want {
                match reader.read(&mut buf[filled..]) {
                    Ok(0) => break,
                    Ok(n) => filled += n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(e) => return Poll::Ready(Some(Err(ConnectError::new(ErrorCode::Internal, format!("read failed: {e}"))))),
                }
            }
        }
        if filled < want {
            return Poll::Ready(Some(Err(ConnectError::new(
                ErrorCode::Internal,
                "the file ended before its announced size",
            ))));
        }
        this.sent += filled as u64;
        (this.progress)(this.sent, this.total);
        Poll::Ready(Some(Ok(Frame::data(Bytes::from(buf)))))
    }

    fn is_end_stream(&self) -> bool {
        self.sent >= self.total
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(self.total - self.sent)
    }
}

/// Percent-encodes everything except RFC 3986 unreserved characters.
fn encode_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn parse_pending(body: &[u8]) -> Result<PendingAttachment> {
    serde_json::from_slice::<pb::PendingAttachment>(body)
        .map(Into::into)
        .map_err(|e| ApiError::Other(format!("bad upload response: {e}")))
}

impl Client {
    /// URL of the attachment upload endpoint for a channel.
    pub(crate) fn upload_url(&self, channel_id: u64, filename: &str) -> String {
        format!(
            "{}/attachments?channel_id={channel_id}&filename={}",
            self.base_url.trim_end_matches('/'),
            encode_component(filename)
        )
    }

    /// Uploads `data` as a pending attachment of `channel_id` (plain HTTP, streamed in chunks).
    ///
    /// `progress(sent, total)` runs as chunks are handed to the connection. Link the
    /// result with [`send_message_with`](Self::send_message_with) within an hour. Fails
    /// locally with [`ApiError::TooLarge`] above the client's limit
    /// ([`with_max_upload_bytes`](Self::with_max_upload_bytes)); the server answers 401
    /// [`Unauthenticated`](ApiError::Unauthenticated), 403 [`PermissionDenied`](ApiError::PermissionDenied),
    /// 404 [`NotFound`](ApiError::NotFound), 413 `TooLarge`, 429 [`RateLimited`](ApiError::RateLimited).
    pub async fn upload_attachment(
        &self,
        channel_id: u64,
        filename: &str,
        content_type: &str,
        data: Vec<u8>,
        progress: impl Fn(u64, u64) + Send + Sync + 'static,
    ) -> Result<PendingAttachment> {
        let size = data.len() as u64;
        self.upload_attachment_reader(channel_id, filename, content_type, size, std::io::Cursor::new(data), progress)
            .await
    }

    /// Like [`upload_attachment`](Self::upload_attachment) but streams `size` bytes
    /// from `reader` (e.g. a `std::fs::File`) without loading them into memory. Reads
    /// are blocking, 64 KiB at a time.
    pub async fn upload_attachment_reader(
        &self,
        channel_id: u64,
        filename: &str,
        content_type: &str,
        size: u64,
        reader: impl Read + Send + 'static,
        progress: impl Fn(u64, u64) + Send + Sync + 'static,
    ) -> Result<PendingAttachment> {
        if size > self.max_upload_bytes {
            return Err(ApiError::TooLarge);
        }
        let uri: hyper::Uri = self
            .upload_url(channel_id, filename)
            .parse()
            .map_err(|e| ApiError::InvalidArgument(format!("bad upload url: {e}")))?;
        let body: ClientBody =
            ReaderBody { reader: Mutex::new(Box::new(reader)), total: size, sent: 0, progress: Box::new(progress) }.boxed();
        let mut request = hyper::Request::post(uri)
            .header(hyper::header::CONTENT_TYPE, content_type)
            .header(hyper::header::CONTENT_LENGTH, size)
            .header(crate::client::CLIENT_HEADER, crate::client::client_header_value())
            .body(body)
            .map_err(|e| ApiError::InvalidArgument(format!("bad upload request: {e}")))?;
        if let Some(token) = self.token() {
            let value = format!("Bearer {token}")
                .parse()
                .map_err(|_| ApiError::InvalidArgument("session token is not a valid header value".into()))?;
            request.headers_mut().insert(hyper::header::AUTHORIZATION, value);
        }
        let response = self.http.send(request).await?;
        let status = response.status();
        let bytes = response.into_body().collect().await.map_err(|e| ApiError::Network(e.to_string()))?.to_bytes();
        if !status.is_success() {
            return Err(ApiError::from_status(status.as_u16(), &String::from_utf8_lossy(&bytes)));
        }
        parse_pending(&bytes)
    }

    /// Streams a stored file (`GET /media/{key}`) into `out` without buffering it.
    ///
    /// `progress(received, total)` runs per chunk; `total` is the `Content-Length` if
    /// the server sent one. Returns the bytes written. [`ApiError::NotFound`] for an unknown key.
    pub async fn download(
        &self,
        key: &str,
        out: &mut impl Write,
        mut progress: impl FnMut(u64, Option<u64>),
    ) -> Result<u64> {
        let uri: hyper::Uri = self
            .media_url(key)
            .parse()
            .map_err(|e| ApiError::InvalidArgument(format!("bad media url: {e}")))?;
        let request = hyper::Request::get(uri)
            .header(crate::client::CLIENT_HEADER, crate::client::client_header_value())
            .body(connectrpc::client::full_body(Bytes::new()))
            .map_err(|e| ApiError::Other(e.to_string()))?;
        let response = self.http.send(request).await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.into_body().collect().await.map(|b| b.to_bytes()).unwrap_or_default();
            return Err(ApiError::from_status(status.as_u16(), &String::from_utf8_lossy(&body)));
        }
        let total = response
            .headers()
            .get(hyper::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok());
        let mut body = std::pin::pin!(response.into_body());
        let mut received = 0u64;
        while let Some(frame) = body.frame().await {
            let frame = frame.map_err(|e| ApiError::Network(e.to_string()))?;
            if let Some(chunk) = frame.data_ref() {
                out.write_all(chunk).map_err(|e| ApiError::Other(format!("write failed: {e}")))?;
                received += chunk.len() as u64;
                progress(received, total);
            }
        }
        out.flush().map_err(|e| ApiError::Other(format!("write failed: {e}")))?;
        Ok(received)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn upload_url_encodes_the_filename() {
        let c = Client::new("http://h:1/");
        assert_eq!(c.upload_url(7, "a b/ü.png"), "http://h:1/attachments?channel_id=7&filename=a%20b%2F%C3%BC.png");
        assert_eq!(c.upload_url(7, "ok-1_2.~x"), "http://h:1/attachments?channel_id=7&filename=ok-1_2.~x");
    }

    #[test]
    fn pending_attachment_parses_protobuf_json() {
        // 64-bit integers are strings in the proto-JSON mapping; camelCase names.
        let p = parse_pending(
            br#"{"id":"123","filename":"a.txt","contentType":"text/plain","size":"5","expiresAt":"2026-01-02T03:04:05Z"}"#,
        )
        .unwrap();
        assert_eq!((p.id, p.filename.as_str(), p.content_type.as_str(), p.size), (123, "a.txt", "text/plain", 5));
        assert_eq!(p.expires_at.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(), 1767323045);
        // Numbers and snake_case are accepted too.
        let p = parse_pending(br#"{"id":9,"content_type":"x/y","size":1}"#).unwrap();
        assert_eq!((p.id, p.content_type.as_str(), p.size), (9, "x/y", 1));
        assert!(matches!(parse_pending(b"nope"), Err(ApiError::Other(_))));
    }

    #[tokio::test]
    async fn body_streams_in_chunks_and_reports_progress() {
        let data = vec![7u8; CHUNK * 3 + 100];
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let body = ReaderBody {
            reader: Mutex::new(Box::new(std::io::Cursor::new(data.clone()))),
            total: data.len() as u64,
            sent: 0,
            progress: Box::new(move |sent, total| log.lock().unwrap().push((sent, total))),
        };
        assert_eq!(body.size_hint().exact(), Some(data.len() as u64));
        let collected = body.collect().await.unwrap().to_bytes();
        assert_eq!(collected.len(), data.len());
        let total = data.len() as u64;
        let c = CHUNK as u64;
        assert_eq!(*seen.lock().unwrap(), vec![(c, total), (2 * c, total), (3 * c, total), (total, total)]);
    }

    #[tokio::test]
    async fn short_reader_fails_the_body() {
        let body = ReaderBody {
            reader: Mutex::new(Box::new(std::io::Cursor::new(vec![0u8; 10]))),
            total: 20,
            sent: 0,
            progress: Box::new(|_, _| {}),
        };
        assert!(body.collect().await.is_err());
    }

    #[tokio::test]
    async fn oversized_uploads_fail_locally() {
        // Unroutable port: reaching the network would yield `Network`, not `TooLarge`.
        let client = Client::new("http://127.0.0.1:1").with_max_upload_bytes(10);
        let err = client.upload_attachment(1, "a", "text/plain", vec![0; 11], |_, _| {}).await.unwrap_err();
        assert_eq!(err, ApiError::TooLarge);
        let default = Client::new("http://127.0.0.1:1");
        let err = default
            .upload_attachment_reader(1, "a", "x/y", crate::DEFAULT_MAX_UPLOAD_BYTES + 1, std::io::empty(), |_, _| {})
            .await
            .unwrap_err();
        assert_eq!(err, ApiError::TooLarge);
        let err = default.upload_attachment(1, "a", "x/y", vec![1], |_, _| {}).await.unwrap_err();
        assert!(matches!(err, ApiError::Network(_)), "{err:?}");
    }
}
