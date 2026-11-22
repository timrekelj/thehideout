//! Operational behaviour: readiness, gateway stream caps, shutdown, client IPs.

mod common;

use std::time::Duration;

use common::TestApp;
use connectrpc::ErrorCode;
use thehideout_server::proto::thehideout::v1::{GatewayServiceClient, SubscribeRequest};
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(5);

async fn get(app: &TestApp, path: &str, headers: &[(&str, &str)]) -> (u16, String) {
    let mut request = reqwest::Client::new().get(format!("{}{path}", app.base_url));
    for (k, v) in headers {
        request = request.header(*k, *v);
    }
    let response = request.send().await.expect("request");
    (response.status().as_u16(), response.text().await.unwrap_or_default())
}

#[tokio::test]
async fn health_is_ok_and_ready_checks_scylla_and_storage() {
    let app = TestApp::start_with_storage().await;
    assert_eq!(get(&app, "/health", &[]).await, (200, "ok".to_owned()));
    let (status, body) = get(&app, "/ready", &[]).await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("\"ok\""), "{body}");
    app.cleanup().await;
}

#[tokio::test]
async fn ready_answers_without_storage_configured() {
    let app = TestApp::start().await;
    assert_eq!(get(&app, "/ready", &[]).await.0, 200);
    app.cleanup().await;
}

#[tokio::test]
async fn the_ninth_stream_of_a_user_is_refused() {
    let app = TestApp::start().await;
    let alice = common::fixtures::user(&app, "alice").await.token;
    let bob = common::fixtures::user(&app, "bob").await.token;
    let subscribe = |token: String| {
        let client = app.client(GatewayServiceClient::new, Some(&token));
        async move { client.subscribe(SubscribeRequest::default()).await }
    };

    // Eight streams, each delivering its first event, are held open.
    let mut open = Vec::new();
    for _ in 0..8 {
        let mut stream = subscribe(alice.clone()).await.expect("stream within the cap");
        timeout(WAIT, stream.message()).await.expect("first event").expect("no error").expect("event");
        open.push(stream);
    }
    // The ninth fails, either when opening or on its first message.
    let refused = match subscribe(alice.clone()).await {
        Err(e) => e,
        Ok(mut stream) => timeout(WAIT, stream.message()).await.expect("answer").expect_err("ninth stream must be refused"),
    };
    assert_eq!(refused.code, ErrorCode::ResourceExhausted, "{refused:?}");

    // Another user is unaffected.
    let mut other = subscribe(bob).await.expect("another user's stream");
    timeout(WAIT, other.message()).await.expect("first event").expect("no error").expect("event");

    // Closing one frees a slot.
    drop(open.pop());
    let mut again = None;
    for _ in 0..50 {
        if let Ok(mut stream) = subscribe(alice.clone()).await
            && timeout(WAIT, stream.message()).await.is_ok_and(|m| matches!(m, Ok(Some(_))))
        {
            again = Some(stream);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(again.is_some(), "a slot should free up once a stream is dropped");
    app.cleanup().await;
}

#[tokio::test]
async fn shutdown_ends_open_streams_and_refuses_new_ones() {
    let app = TestApp::start().await;
    let token = common::fixtures::user(&app, "alice").await.token;
    let client = app.client(GatewayServiceClient::new, Some(&token));
    let mut stream = client.subscribe(SubscribeRequest::default()).await.expect("subscribe");
    timeout(WAIT, stream.message()).await.expect("first event").expect("no error").expect("event");

    app.state.shutdown.trigger();

    // The stream ends (with an error status or cleanly) well before the next heartbeat (30 s).
    let ended = timeout(WAIT, async {
        while let Ok(Some(_)) = stream.message().await {}
    })
    .await;
    assert!(ended.is_ok(), "the stream should end promptly after shutdown");

    let refused = match client.subscribe(SubscribeRequest::default()).await {
        Err(e) => Some(e),
        Ok(mut s) => timeout(WAIT, s.message()).await.expect("answer").err(),
    };
    assert_eq!(refused.map(|e| e.code), Some(ErrorCode::Unavailable));
    app.cleanup().await;
}
