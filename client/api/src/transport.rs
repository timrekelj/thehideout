//! The HTTP transport the generated clients run on: hyper natively, `fetch` in the browser.

#[cfg(not(target_arch = "wasm32"))]
pub(crate) type Transport = connectrpc::client::HttpClient;

#[cfg(target_arch = "wasm32")]
pub(crate) type Transport = crate::fetch::FetchTransport;
