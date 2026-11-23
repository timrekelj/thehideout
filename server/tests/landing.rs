//! The public landing page and its embedded assets.
mod common;

use common::TestApp;
use common::http;

#[tokio::test]
async fn landing_page_is_served_with_a_tight_csp() {
    let app = TestApp::start().await;
    let r = http::get(&app, "/").await;
    assert_eq!(r.status, 200);
    assert!(r.header("content-type").starts_with("text/html"));
    assert_eq!(r.header("x-frame-options"), "DENY");
    assert_eq!(r.header("x-content-type-options"), "nosniff");
    assert_eq!(r.header("referrer-policy"), "no-referrer");
    assert!(r.header("cache-control").contains("max-age"));
    let csp = r.header("content-security-policy");
    for needle in [
        "script-src 'self'",
        "connect-src https://api.github.com",
        "img-src 'self' data:",
        "frame-ancestors 'none'",
    ] {
        assert!(csp.contains(needle), "{csp}");
    }
    assert!(
        !csp.contains("unsafe-inline") && !csp.contains("unsafe-eval"),
        "{csp}"
    );
    let body = r.text();
    assert!(body.contains("thehideout") && body.contains("/privacy") && body.contains("/terms"));
    for prefix in ["/landing/site.css?v=", "/landing/site.js?v="] {
        let at = body.find(prefix).unwrap_or_else(|| panic!("{prefix} not referenced"));
        let version: String = body[at + prefix.len()..].chars().take_while(char::is_ascii_hexdigit).collect();
        assert_eq!(version.len(), 16, "{prefix}");
        // The versioned URL resolves and is cached long-term.
        let asset = http::get(&app, &format!("{}{version}", &prefix[..prefix.len()])).await;
        assert_eq!(asset.status, 200);
        assert!(asset.header("cache-control").contains("immutable"));
    }
    assert!(!body.contains("@CSS_VERSION@") && !body.contains("@JS_VERSION@"));
    assert!(body.contains("brew install --cask timrekelj/tap/thehideout"));
    assert!(
        !body.contains("<script>") && !body.contains("style=\""),
        "no inline script or style"
    );
    app.cleanup().await;
}

#[tokio::test]
async fn landing_assets_are_served_with_the_right_types() {
    let app = TestApp::start().await;
    for (path, mime) in [
        ("/landing/site.css", "text/css"),
        ("/landing/site.js", "text/javascript"),
        ("/landing/icon.png", "image/png"),
        ("/favicon.ico", "image/x-icon"),
        ("/favicon-16.png", "image/png"),
        ("/favicon-32.png", "image/png"),
        ("/apple-touch-icon.png", "image/png"),
    ] {
        let r = http::get(&app, path).await;
        assert_eq!(r.status, 200, "{path}");
        assert!(r.header("content-type").starts_with(mime), "{path}");
        assert_eq!(r.header("x-content-type-options"), "nosniff", "{path}");
        assert!(!r.body.is_empty(), "{path}");
    }
    assert!(
        http::get(&app, "/landing/site.js")
            .await
            .text()
            .contains("api.github.com/repos/timrekelj/thehideout")
    );
    // Existing routes are unaffected.
    assert_eq!(http::get(&app, "/health").await.status, 200);
    assert_eq!(http::get(&app, "/privacy").await.status, 200);
    assert_eq!(http::get(&app, "/nope").await.status, 404);
    app.cleanup().await;
}

#[tokio::test]
async fn non_get_methods_on_root_keep_reaching_the_connect_fallback() {
    let app = TestApp::start().await;
    // `POST /` must behave like any other unmatched path (the Connect service),
    // not like a 405 from the landing route.
    let root = http::request(&app, "POST", "/", &[], b"").await;
    let other = http::request(&app, "POST", "/no-such-route", &[], b"").await;
    assert_ne!(root.status, 405);
    assert_eq!(root.status, other.status);
    app.cleanup().await;
}
