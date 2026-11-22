#![allow(dead_code)]
//! A minimal raw HTTP/1.1 client for the non-Connect routes.

use std::collections::HashMap;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::TestApp;

pub struct Reply {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn header(&self, name: &str) -> &str {
        self.headers.get(name).unwrap_or_else(|| panic!("no {name} header in {:?}", self.headers))
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// `method path` with the given headers and body.
pub async fn request(app: &TestApp, method: &str, path: &str, headers: &[(&str, String)], body: &[u8]) -> Reply {
    let addr = app.base_url.trim_start_matches("http://");
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
    for (k, v) in headers {
        request.push_str(&format!("{k}: {v}\r\n"));
    }
    request.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    stream.write_all(request.as_bytes()).await.unwrap();
    let _ = stream.write_all(body).await;
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw).await;
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").expect("response header end");
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.lines();
    let status = lines.next().unwrap().split(' ').nth(1).unwrap().parse().unwrap();
    let headers: HashMap<String, String> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let mut body = raw[split + 4..].to_vec();
    // `Connection: close` responses may still be chunked.
    if headers.get("transfer-encoding").is_some_and(|v| v.contains("chunked")) {
        body = dechunk(&body);
    }
    Reply { status, headers, body }
}

pub async fn get(app: &TestApp, path: &str) -> Reply {
    request(app, "GET", path, &[], &[]).await
}

fn dechunk(mut data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(eol) = data.windows(2).position(|w| w == b"\r\n") {
        let size = usize::from_str_radix(String::from_utf8_lossy(&data[..eol]).trim(), 16).unwrap_or(0);
        if size == 0 {
            break;
        }
        out.extend_from_slice(&data[eol + 2..eol + 2 + size]);
        data = &data[(eol + 2 + size + 2).min(data.len())..];
    }
    out
}

/// A Connect error detail of type `thehideout.v1.<name>`, decoded.
pub fn detail<T: buffa::Message>(err: &connectrpc::ConnectError, name: &str) -> T {
    use base64::Engine as _;
    let d = err
        .details
        .iter()
        .find(|d| d.type_url.ends_with(&format!("thehideout.v1.{name}")))
        .unwrap_or_else(|| panic!("no {name} detail in {err:?}"));
    let raw = d.value.as_deref().unwrap_or_default();
    let bytes = base64::engine::general_purpose::STANDARD_NO_PAD.decode(raw.trim_end_matches('=')).expect("base64 detail");
    T::decode_from_slice(&bytes).expect("decode detail")
}
