//! Legal pages, terms acceptance and the minimum client version.
mod common;

use common::TestApp;
use common::fixtures::*;
use common::http;
use connectrpc::ErrorCode;
use thehideout_server::policy::{LEGAL_VERSION, MinClient, Policy, Version};
use thehideout_server::proto::thehideout::v1::*;

fn policy() -> Policy {
    Policy {
        contact_email: Some("privacy@hideout.example".into()),
        operator_name: Some("Hideout Collective".into()),
        ..Default::default()
    }
}

#[tokio::test]
async fn legal_pages_are_served_with_headers_and_contact_info() {
    let app = TestApp::builder().policy(policy()).start().await;
    for (path, title) in [("/privacy", "Privacy notice"), ("/terms", "Terms of use")] {
        let r = http::get(&app, path).await;
        assert_eq!(r.status, 200, "{path}");
        assert!(r.header("content-type").starts_with("text/html"));
        assert_eq!(r.header("x-frame-options"), "DENY");
        assert_eq!(r.header("x-content-type-options"), "nosniff");
        let csp = r.header("content-security-policy");
        assert!(csp.contains("default-src 'none'") && csp.contains("frame-ancestors 'none'"), "{csp}");
        let body = r.text();
        assert!(body.contains(title) && body.contains(LEGAL_VERSION), "{path}");
        assert!(body.contains("privacy@hideout.example") && body.contains("Hideout Collective"), "{path}");
        assert!(body.contains("16"), "{path}: minimum age");
        assert!(!body.contains("<script"), "{path}");
    }
    let privacy = http::get(&app, "/privacy").await.text();
    for needle in ["not recorded", "Resend", "30 days", "7 days", "Deleted user", "cookies"] {
        assert!(privacy.contains(needle), "privacy notice mentions {needle:?}");
    }
    app.cleanup().await;
}

#[tokio::test]
async fn placeholder_is_marked_when_no_contact_is_configured() {
    let app = TestApp::start().await;
    let body = http::get(&app, "/terms").await.text();
    assert!(body.contains("contact email not configured"), "{body}");
    assert!(body.contains("operator name not configured"));
}

#[tokio::test]
async fn invite_page_and_every_mail_link_the_legal_pages() {
    let app = TestApp::start().await;
    let landing = http::get(&app, "/invite/ABCD-EFGH").await.text();
    assert!(landing.contains("/privacy") && landing.contains("/terms"));

    let admin = site_admin(&app, "admin").await;
    let email = format!("{}@invitee.test", app.name("legal"));
    invites(&app, Some(&admin.token))
        .create_invite(CreateInviteRequest { email: email.clone(), ..Default::default() })
        .await
        .unwrap();
    let mail = wait_mail(&app, &email, 0, "invited").await;
    let base = app.state.public_base();
    for part in [&mail.text, &mail.html] {
        assert!(part.contains(&format!("{base}/privacy")) && part.contains(&format!("{base}/terms")), "{part}");
    }
}

#[tokio::test]
async fn terms_and_age_are_required_to_accept_an_invite() {
    let app = TestApp::start().await;
    let admin = site_admin(&app, "admin").await;
    let email = format!("{}@invitee.test", app.name("newbie"));
    let token = invites(&app, Some(&admin.token))
        .create_invite(CreateInviteRequest { email, ..Default::default() })
        .await
        .unwrap()
        .into_owned()
        .invite
        .unwrap()
        .link
        .unwrap();
    let token = token.rsplit('/').next().unwrap().to_owned();
    let lookup = invites(&app, None)
        .lookup_invite(LookupInviteRequest { token_or_code: token.clone(), ..Default::default() })
        .await
        .unwrap()
        .into_owned();
    assert_eq!((lookup.terms_version.as_str(), lookup.minimum_age), (LEGAL_VERSION, 16));

    let accept = |version: &str, age: bool| {
        let client = invites(&app, None);
        let req = AcceptInviteRequest {
            token_or_code: token.clone(),
            username: app.name("newbie"),
            password: Some(PASSWORD.into()),
            device_name: "laptop".into(),
            accepted_terms_version: version.into(),
            age_confirmed: age,
            ..Default::default()
        };
        async move { client.accept_invite(req).await.map(|r| r.into_owned()) }
    };
    for (version, age) in [("", true), ("2020-01-01", true), (LEGAL_VERSION, false), ("", false)] {
        let err = accept(version, age).await.err().unwrap_or_else(|| panic!("refused"));
        assert_eq!(err.code, ErrorCode::InvalidArgument, "{version:?} {age}");
    }
    // The invite is still usable after the refusals.
    let ok = accept(LEGAL_VERSION, true).await.unwrap();
    let me = auth(&app, Some(&ok.token)).get_me(GetMeRequest::default()).await.unwrap().into_owned();
    assert_eq!(me.terms_update_required, None);
    assert_eq!(me.terms_version, LEGAL_VERSION);
    let stored = thehideout_server::accounts::load(&app.state, ok.user.as_option().unwrap().id).await.unwrap().unwrap();
    assert_eq!(stored.accepted_terms_version.as_deref(), Some(LEGAL_VERSION));
}

#[tokio::test]
async fn existing_users_must_accept_the_current_terms() {
    let app = TestApp::start().await;
    let alice = user(&app, "alice").await;
    let client = auth(&app, Some(&alice.token));
    let me = client.get_me(GetMeRequest::default()).await.unwrap().into_owned();
    assert_eq!(me.terms_update_required.as_deref(), Some(LEGAL_VERSION));

    let err = client.accept_terms(AcceptTermsRequest { version: "1999-01-01".into(), ..Default::default() }).await.err().unwrap();
    assert_eq!(err.code, ErrorCode::FailedPrecondition);
    client.accept_terms(AcceptTermsRequest { version: LEGAL_VERSION.into(), ..Default::default() }).await.unwrap();
    let me = client.get_me(GetMeRequest::default()).await.unwrap().into_owned();
    assert_eq!(me.terms_update_required, None);
    // An older accepted version counts as not accepted.
    app.state
        .session
        .execute_unpaged("UPDATE users SET accepted_terms_version = '2020-01-01' WHERE id = ?", (alice.id as i64,))
        .await
        .unwrap();
    let me = client.get_me(GetMeRequest::default()).await.unwrap().into_owned();
    assert_eq!(me.terms_update_required.as_deref(), Some(LEGAL_VERSION));
}

fn min_policy(min: &str) -> Policy {
    Policy { min_client: MinClient { default: Version::parse(min), ..Default::default() }, ..Default::default() }
}

#[tokio::test]
async fn too_old_clients_are_refused_and_others_pass() {
    let app = TestApp::builder().policy(min_policy("1.2.0")).start().await;
    let call = |header: Option<&str>| {
        let mut config = app.client_config(None);
        if let Some(h) = header {
            config = config.with_default_header("x-thehideout-client", h.to_owned());
        }
        let client = AuthServiceClient::new(connectrpc::client::HttpClient::plaintext(), config);
        async move {
            client
                .login(LoginRequest { username: "nobody".into(), password: "x".into(), ..Default::default() })
                .await
                .map(|_| ())
        }
    };
    for old in ["desktop/1.1.9 (macos)", "desktop/0.9.0 (linux)"] {
        let err = call(Some(old)).await.err().unwrap_or_else(|| panic!("refused"));
        assert_eq!(err.code, ErrorCode::FailedPrecondition, "{old}");
        assert!(err.message.as_deref().unwrap_or("").starts_with("client too old"), "{err:?}");
    }
    // Equal, newer, missing and unparseable headers reach the handler (bad credentials).
    for ok in [Some("desktop/1.2.0 (macos)"), Some("desktop/2.0.0 (macos)"), Some("ios/1.2.1"), None, Some("curl-ish")] {
        let err = call(ok).await.err().unwrap_or_else(|| panic!("bad credentials"));
        assert_eq!(err.code, ErrorCode::Unauthenticated, "{ok:?}");
    }
    // Exempt paths ignore the header.
    let old = [("x-thehideout-client", "desktop/0.1.0 (macos)".to_owned())];
    for path in ["/health", "/privacy", "/terms", "/invite/abc"] {
        assert_eq!(http::request(&app, "GET", path, &old, &[]).await.status, 200, "{path}");
    }
    assert_eq!(http::request(&app, "GET", "/media/nothing", &old, &[]).await.status, 404);
    assert_eq!(http::request(&app, "GET", "/ready", &old, &[]).await.status, 200);
    // A refusal is plain Connect JSON; streams get an end-of-stream frame.
    let reply = http::request(
        &app,
        "POST",
        "/thehideout.v1.AuthService/GetMe",
        &[("x-thehideout-client", "desktop/1.0.0 (macos)".to_owned()), ("content-type", "application/json".to_owned())],
        b"{}",
    )
    .await;
    assert_eq!(reply.status, 400);
    assert!(reply.text().contains("\"failed_precondition\"") && reply.text().contains("client too old"), "{}", reply.text());
    let reply = http::request(
        &app,
        "POST",
        "/thehideout.v1.GatewayService/Subscribe",
        &[("x-thehideout-client", "desktop/1.0.0 (macos)".to_owned()), ("content-type", "application/connect+proto".to_owned())],
        b"",
    )
    .await;
    assert_eq!(reply.status, 200);
    assert_eq!(reply.body[0], 0x02, "end-of-stream frame");
    assert!(String::from_utf8_lossy(&reply.body[5..]).contains("client too old"));
}

#[tokio::test]
async fn platform_minimums_override_the_default() {
    let mut min = MinClient { default: Version::parse("1.0.0"), ..Default::default() };
    min.platforms.insert("ios".into(), Version(3, 0, 0));
    let app = TestApp::builder().policy(Policy { min_client: min, ..Default::default() }).start().await;
    let status = |h: &'static str| {
        let app = &app;
        async move {
            http::request(
                app,
                "POST",
                "/thehideout.v1.AuthService/GetMe",
                &[("x-thehideout-client", h.to_owned()), ("content-type", "application/json".to_owned())],
                b"{}",
            )
            .await
            .status
        }
    };
    assert_eq!(status("ios/2.9.0").await, 400);
    assert_eq!(status("ios/3.0.0").await, 401);
    assert_eq!(status("desktop/1.0.0 (macos)").await, 401);
    assert_eq!(status("android/0.5.0").await, 400);
}
