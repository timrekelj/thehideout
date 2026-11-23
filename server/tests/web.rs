//! The web client bundle at `/app/`, its headers, and how the version gate and CORS treat it.
mod common;

use std::path::PathBuf;

use common::TestApp;
use common::http;
use thehideout_server::policy::{MinClient, Policy, Version};

/// A tiny bundle in a fresh temp folder.
fn bundle(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("thehideout-web-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("fonts")).unwrap();
    std::fs::write(dir.join("index.html"), "<!doctype html><title>app shell</title>").unwrap();
    std::fs::write(dir.join("boot.js"), "console.log('boot');").unwrap();
    std::fs::write(dir.join("app.js"), "export default 1;").unwrap();
    std::fs::write(dir.join("app_bg.wasm"), b"\0asm\x01\0\0\0").unwrap();
    std::fs::write(dir.join("fonts/a.woff2"), b"wOF2").unwrap();
    dir
}

async fn web_app(tag: &str) -> TestApp {
    TestApp::builder().livekit().web_dir(bundle(tag)).start().await
}

#[tokio::test]
async fn index_is_served_with_security_headers() {
    let app = web_app("index").await;
    for path in ["/app/", "/app/index.html"] {
        let r = http::get(&app, path).await;
        assert_eq!(r.status, 200, "{path}");
        assert!(r.text().contains("app shell"), "{path}");
        assert!(r.header("content-type").starts_with("text/html"));
        assert_eq!(r.header("cache-control"), "no-cache");
        assert_eq!(r.header("x-frame-options"), "DENY");
        assert_eq!(r.header("x-content-type-options"), "nosniff");
        assert_eq!(r.header("referrer-policy"), "no-referrer");
        let csp = r.header("content-security-policy");
        for part in [
            "default-src 'self'",
            "script-src 'self' 'wasm-unsafe-eval'",
            "style-src 'self' 'unsafe-inline'",
            "img-src 'self' blob: data:",
            "font-src 'self'",
            "connect-src 'self' ws://livekit.test:7880;",
            "media-src 'self' blob:",
            "frame-ancestors 'none'",
            "base-uri 'none'",
            "form-action 'none'",
        ] {
            assert!(csp.contains(part), "{part} in {csp}");
        }
    }
}

#[tokio::test]
async fn assets_have_their_mime_type_and_cache_policy() {
    let app = web_app("assets").await;
    let wasm = http::get(&app, "/app/app_bg.wasm").await;
    assert_eq!(wasm.status, 200);
    assert_eq!(wasm.header("content-type"), "application/wasm");
    assert_eq!(wasm.header("cache-control"), "public, max-age=3600");
    assert_eq!(wasm.header("x-content-type-options"), "nosniff");
    assert!(wasm.header("content-security-policy").contains("default-src 'self'"));
    let boot = http::get(&app, "/app/boot.js").await;
    assert!(boot.header("content-type").contains("javascript"), "{:?}", boot.headers);
    assert_eq!(boot.header("cache-control"), "no-cache");
    assert_eq!(http::get(&app, "/app/app.js").await.header("cache-control"), "public, max-age=3600");
    assert_eq!(http::get(&app, "/app/fonts/a.woff2").await.header("cache-control"), "public, max-age=3600");
}

#[tokio::test]
async fn precompressed_files_are_preferred() {
    let dir = bundle("compressed");
    std::fs::write(dir.join("app.js.gz"), b"gzipped").unwrap();
    let app = TestApp::builder().web_dir(dir).start().await;
    let r = http::request(&app, "GET", "/app/app.js", &[("accept-encoding", "gzip".to_owned())], &[]).await;
    assert_eq!((r.status, r.header("content-encoding")), (200, "gzip"));
    assert_eq!(r.body, b"gzipped");
    assert!(r.header("content-type").contains("javascript"));
}

#[tokio::test]
async fn app_without_a_slash_redirects() {
    let app = web_app("redirect").await;
    let r = http::get(&app, "/app").await;
    assert!(matches!(r.status, 301 | 308), "{}", r.status);
    assert_eq!(r.header("location"), "/app/");
}

#[tokio::test]
async fn unknown_routes_fall_back_to_index_but_missing_files_are_404() {
    let app = web_app("fallback").await;
    for path in ["/app/foo", "/app/guilds/123/channels/456", "/app/fonts/"] {
        let r = http::get(&app, path).await;
        assert_eq!(r.status, 200, "{path}");
        assert!(r.text().contains("app shell"), "{path}");
        assert_eq!(r.header("cache-control"), "no-cache", "{path}");
        assert!(r.header("content-security-policy").contains("default-src 'self'"), "{path}");
    }
    for path in ["/app/missing.js", "/app/fonts/missing.woff2", "/app/..%2Fsecret.txt"] {
        let r = http::get(&app, path).await;
        assert_eq!(r.status, 404, "{path}");
        assert!(!r.text().contains("app shell"), "{path}");
    }
}

#[tokio::test]
async fn without_a_web_dir_app_is_not_served() {
    let app = TestApp::start().await;
    let r = http::get(&app, "/app/").await;
    assert_ne!(r.status, 200);
    assert!(!r.text().contains("app shell"));
    assert!(!r.headers.contains_key("content-security-policy"));
    // The Connect fallback answers, as for any unknown path.
    assert_eq!(http::get(&app, "/app/app_bg.wasm").await.status, r.status);
    // A directory that does not exist is not mounted either.
    let app = TestApp::builder().web_dir(PathBuf::from("/definitely/not/here")).start().await;
    assert!(!http::get(&app, "/app/").await.headers.contains_key("content-security-policy"));
}

#[tokio::test]
async fn csp_leaves_livekit_out_when_voice_is_off() {
    let app = TestApp::builder().web_dir(bundle("novoice")).start().await;
    let csp = http::get(&app, "/app/").await.header("content-security-policy").to_owned();
    assert!(csp.contains("connect-src 'self';"), "{csp}");
}

fn gate(default: Option<&str>, web: Option<&str>) -> Policy {
    let mut min = MinClient { default: default.and_then(Version::parse), ..Default::default() };
    if let Some(v) = web.and_then(Version::parse) {
        min.platforms.insert("web".into(), v);
    }
    Policy { min_client: min, ..Default::default() }
}

async fn get_me(app: &TestApp, client: &str) -> u16 {
    http::request(
        app,
        "POST",
        "/thehideout.v1.AuthService/GetMe",
        &[("x-thehideout-client", client.to_owned()), ("content-type", "application/json".to_owned())],
        b"{}",
    )
    .await
    .status
}

#[tokio::test]
async fn version_gate_ignores_app_and_web_does_not_inherit_the_default() {
    let app = TestApp::builder().policy(gate(Some("9.0.0"), None)).web_dir(bundle("gate")).start().await;
    let old = [("x-thehideout-client", "desktop/1.0.0 (macos)".to_owned())];
    for path in ["/app/", "/app/app_bg.wasm", "/app/foo"] {
        assert_eq!(http::request(&app, "GET", path, &old, &[]).await.status, 200, "{path}");
    }
    // The generic default gates desktop but not web.
    assert_eq!(get_me(&app, "desktop/1.0.0 (macos)").await, 400);
    assert_eq!(get_me(&app, "web/0.0.1").await, 401);
}

#[tokio::test]
async fn version_gate_honours_the_web_minimum() {
    let app = TestApp::builder().policy(gate(None, Some("2.0.0"))).start().await;
    assert_eq!(get_me(&app, "web/1.9.9").await, 400);
    assert_eq!(get_me(&app, "web/2.0.0").await, 401);
    assert_eq!(get_me(&app, "desktop/0.1.0 (macos)").await, 401);
}

#[tokio::test]
async fn cors_preflight_allows_the_client_header() {
    let app = TestApp::start().await;
    let origin = app.state.public_base();
    let r = http::request(
        &app,
        "OPTIONS",
        "/thehideout.v1.AuthService/GetMe",
        &[
            ("origin", origin.clone()),
            ("access-control-request-method", "POST".to_owned()),
            ("access-control-request-headers", "content-type,authorization,x-thehideout-client".to_owned()),
        ],
        &[],
    )
    .await;
    assert!(r.status == 200 || r.status == 204, "{}", r.status);
    assert!(r.header("access-control-allow-headers").contains("x-thehideout-client"), "{:?}", r.headers);
    assert_eq!(r.header("access-control-allow-origin"), origin);
    // Other origins are still refused.
    let r = http::request(
        &app,
        "OPTIONS",
        "/thehideout.v1.AuthService/GetMe",
        &[("origin", "https://evil.example".to_owned()), ("access-control-request-method", "POST".to_owned())],
        &[],
    )
    .await;
    assert_ne!(r.headers.get("access-control-allow-origin").map(String::as_str), Some("https://evil.example"));
}
