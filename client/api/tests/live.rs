//! End-to-end tests against the real server running in-process on a throwaway
//! ScyllaDB keyspace (needs ScyllaDB at 127.0.0.1:9042: `docker compose up -d`).

use std::sync::Arc;
use std::time::Duration;

use rand::Rng;
use scylla::client::caching_session::CachingSession;
use scylla::client::session_builder::SessionBuilder;
mod common;

use common::PASSWORD;
use thehideout_api::{
    ApiError, Client, Event, Gateway, GatewayConfig, GatewayUpdate, InviteStatus, LoginOutcome, NewInvite, SecondFactor,
};
use thehideout_server::snowflake::Snowflake;
use thehideout_server::{AppState, build_app, migrations};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tokio::task::JoinSet;
use tokio::time::timeout;

const HEARTBEAT: Duration = Duration::from_millis(200);
const WAIT: Duration = Duration::from_secs(10);

struct Server {
    url: String,
    state: AppState,
}

impl Server {
    async fn start() -> Self {
        let session = SessionBuilder::new()
            .known_node("127.0.0.1:9042")
            .build()
            .await
            .expect("connect to ScyllaDB at 127.0.0.1:9042 (is `docker compose up -d` running?)");
        let keyspace = format!("test_{:016x}", rand::rng().random::<u64>());
        migrations::run(&session, &keyspace).await.expect("run migrations");
        let session = Arc::new(CachingSession::from(session, 64));
        let state = AppState::new(session, Arc::new(Snowflake::new(0)), keyspace, HEARTBEAT);
        let app = build_app(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>())
                .await
                .unwrap();
        });
        Self { url: format!("http://{addr}"), state }
    }

    async fn cleanup(self) {
        let _ = self
            .state
            .session
            .get_session()
            .query_unpaged(format!("DROP KEYSPACE IF EXISTS {}", self.state.keyspace), ())
            .await;
    }
}

/// A TCP forwarder that can be killed (cutting every open connection, like a
/// server restart would) and started again on the same port.
struct Proxy {
    addr: std::net::SocketAddr,
    target: std::net::SocketAddr,
    tasks: JoinSet<()>,
}

impl Proxy {
    async fn start(target: std::net::SocketAddr) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        Self::serve(listener, addr, target)
    }

    fn serve(listener: TcpListener, addr: std::net::SocketAddr, target: std::net::SocketAddr) -> Self {
        let mut tasks = JoinSet::new();
        tasks.spawn(async move {
            let mut conns = JoinSet::new();
            while let Ok((mut inbound, _)) = listener.accept().await {
                conns.spawn(async move {
                    if let Ok(mut outbound) = TcpStream::connect(target).await {
                        let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
                    }
                });
            }
        });
        Self { addr, target, tasks }
    }

    /// Kills the listener and every connection through it.
    async fn kill(&mut self) {
        self.tasks.abort_all();
        while self.tasks.join_next().await.is_some() {}
    }

    async fn restart(&mut self) {
        let listener = TcpListener::bind(self.addr).await.expect("rebind proxy port");
        *self = Self::serve(listener, self.addr, self.target);
    }
}

fn host(url: &str) -> std::net::SocketAddr {
    url.trim_start_matches("http://").parse().unwrap()
}

fn spawn_gateway(client: Client) -> (thehideout_api::GatewayHandle, UnboundedReceiver<GatewayUpdate>) {
    let (tx, rx) = unbounded_channel();
    let config = GatewayConfig {
        initial_backoff: Duration::from_millis(100),
        max_backoff: Duration::from_millis(500),
        heartbeat_interval: HEARTBEAT,
        ..Default::default()
    };
    let handle = Gateway::spawn_with(client, config, move |u| {
        let _ = tx.send(u);
    });
    (handle, rx)
}

/// Waits for the first update matching `pred`, skipping others.
async fn expect_update(rx: &mut UnboundedReceiver<GatewayUpdate>, what: &str, pred: impl Fn(&GatewayUpdate) -> bool) -> GatewayUpdate {
    timeout(WAIT, async {
        loop {
            let update = rx.recv().await.expect("gateway stopped");
            if pred(&update) {
                return update;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
}

#[tokio::test]
async fn rpc_round_trip() {
    let server = Server::start().await;
    let alice = Client::new(&server.url);
    let bob = Client::new(&server.url);

    let (token, me) = common::sign_up(&server.state, &alice, "alice").await;
    assert_eq!(alice.token().as_deref(), Some(token.as_str()));
    assert_eq!(me.username, "alice");
    assert!(me.created_at > std::time::UNIX_EPOCH);
    assert_eq!(alice.sign_up("alice2", PASSWORD).await.unwrap_err(), ApiError::InviteRequired);
    assert_eq!(alice.login("alice", "wrong password!").await.unwrap_err(), ApiError::Unauthenticated);

    let fresh = Client::new(&server.url);
    let LoginOutcome::Session { user: logged_in, .. } = fresh.login("alice", PASSWORD).await.expect("login") else {
        panic!("no second factor expected");
    };
    assert_eq!(logged_in.id, me.id);
    assert_eq!(fresh.get_me().await.unwrap(), me);
    assert_eq!(Client::new(&server.url).get_me().await.unwrap_err(), ApiError::Unauthenticated);

    let guild = alice.create_guild("friends").await.expect("create guild");
    assert_eq!(guild.owner_id, me.id);
    assert_eq!(alice.list_guilds().await.unwrap(), vec![guild.clone()]);
    let general = alice.create_channel(guild.id, "random").await.expect("create channel");
    let channels = alice.list_channels(guild.id).await.unwrap();
    assert!(channels.contains(&general));
    let channel_id = channels[0].id;

    let (_, bob_user) = common::sign_up(&server.state, &bob, "bob").await;
    assert_eq!(bob.join_guild("nope-nope").await.unwrap_err(), ApiError::NotFound);
    assert_eq!(bob.join_guild(&guild.invite_code).await.unwrap().id, guild.id);
    let members = alice.list_guild_members(guild.id).await.unwrap();
    assert_eq!(members.len(), 2);
    assert_eq!(bob.create_channel(guild.id, "nope").await.unwrap_err(), ApiError::PermissionDenied);

    // Sending and pagination.
    assert!(matches!(alice.send_message(channel_id, "", None).await, Err(ApiError::InvalidArgument(_))));
    let too_long = "x".repeat(thehideout_api::MAX_MESSAGE_CHARS + 1);
    assert!(matches!(alice.send_message(channel_id, &too_long, None).await, Err(ApiError::InvalidArgument(_))));
    let first = alice.send_message(channel_id, "m0", Some("n0")).await.unwrap();
    assert_eq!((first.content.as_str(), first.nonce.as_str()), ("m0", "n0"));
    assert_eq!(first.guild_id, Some(guild.id));
    assert_eq!(first.edited_at, None);
    for i in 1..5 {
        bob.send_message(channel_id, &format!("m{i}"), None).await.unwrap();
    }
    let page1 = alice.list_messages(channel_id, None, 2).await.unwrap();
    assert_eq!(contents(&page1.items), ["m4", "m3"]);
    assert!(page1.has_more);
    let page2 = alice.list_messages(channel_id, page1.next_before, 2).await.unwrap();
    assert_eq!(contents(&page2.items), ["m2", "m1"]);
    let page3 = alice.list_messages(channel_id, page2.next_before, 2).await.unwrap();
    assert_eq!(contents(&page3.items), ["m0"]);
    assert!(!page3.has_more);
    assert_eq!(page3.next_before, None);

    // Direct messages.
    let dm = alice.open_dm(bob_user.id).await.expect("open dm");
    assert_eq!(dm.participants.len(), 2);
    assert_eq!(bob.open_dm(me.id).await.unwrap().id, dm.id, "opening is idempotent");
    assert_eq!(alice.open_dm(u64::MAX).await.unwrap_err(), ApiError::NotFound);
    assert_eq!(alice.list_dms().await.unwrap().len(), 1);
    let sent = bob.send_message(dm.id, "psst", None).await.unwrap();
    assert_eq!(sent.guild_id, None);
    let history = alice.list_messages(dm.id, None, 10).await.unwrap();
    assert_eq!(contents(&history.items), ["psst"]);

    server.cleanup().await;
}

fn contents(messages: &[thehideout_api::Message]) -> Vec<&str> {
    messages.iter().map(|m| m.content.as_str()).collect()
}

#[tokio::test]
async fn gateway_delivers_events() {
    let server = Server::start().await;
    let alice = Client::new(&server.url);
    let bob = Client::new(&server.url);
    common::sign_up(&server.state, &alice, "alice").await;
    let (_, bob_user) = common::sign_up(&server.state, &bob, "bob").await;
    let guild = alice.create_guild("friends").await.unwrap();
    bob.join_guild(&guild.invite_code).await.unwrap();
    let channel_id = alice.list_channels(guild.id).await.unwrap()[0].id;

    let (_handle, mut rx) = spawn_gateway(bob.clone());
    assert_eq!(rx.recv().await, Some(GatewayUpdate::Connecting));
    expect_update(&mut rx, "Connected", |u| *u == GatewayUpdate::Connected).await;
    expect_update(&mut rx, "heartbeat", |u| *u == GatewayUpdate::Event(Event::Heartbeat)).await;

    alice.send_message(channel_id, "hello bob", Some("n-1")).await.unwrap();
    let update = expect_update(&mut rx, "MessageCreated", |u| matches!(u, GatewayUpdate::Event(Event::MessageCreated(_)))).await;
    let GatewayUpdate::Event(Event::MessageCreated(m)) = update else { unreachable!() };
    assert_eq!((m.content.as_str(), m.nonce.as_str(), m.channel_id), ("hello bob", "n-1", channel_id));

    alice.create_channel(guild.id, "extra").await.unwrap();
    expect_update(&mut rx, "ChannelCreated", |u| matches!(u, GatewayUpdate::Event(Event::ChannelCreated(c)) if c.name == "extra")).await;

    alice.open_dm(bob_user.id).await.unwrap();
    expect_update(&mut rx, "DmOpened", |u| matches!(u, GatewayUpdate::Event(Event::DmOpened(_)))).await;

    server.cleanup().await;
}

#[tokio::test]
async fn gateway_reconnects_after_connection_loss() {
    let server = Server::start().await;
    let mut proxy = Proxy::start(host(&server.url)).await;
    let url = format!("http://{}", proxy.addr);
    let alice = Client::new(&url);
    let bob = Client::new(&url);
    common::sign_up(&server.state, &alice, "alice").await;
    common::sign_up(&server.state, &bob, "bob").await;
    let guild = alice.create_guild("friends").await.unwrap();
    bob.join_guild(&guild.invite_code).await.unwrap();
    let channel_id = alice.list_channels(guild.id).await.unwrap()[0].id;

    let (_handle, mut rx) = spawn_gateway(bob.clone());
    expect_update(&mut rx, "Connected", |u| *u == GatewayUpdate::Connected).await;

    proxy.kill().await;
    let down = expect_update(&mut rx, "Disconnected", |u| matches!(u, GatewayUpdate::Disconnected { .. })).await;
    let GatewayUpdate::Disconnected { retry_in, .. } = down else { unreachable!() };
    assert!(retry_in <= Duration::from_millis(500));

    proxy.restart().await;
    expect_update(&mut rx, "Connected again", |u| *u == GatewayUpdate::Connected).await;
    alice.send_message(channel_id, "after restart", None).await.unwrap();
    expect_update(&mut rx, "MessageCreated after reconnect", |u| {
        matches!(u, GatewayUpdate::Event(Event::MessageCreated(m)) if m.content == "after restart")
    })
    .await;

    proxy.kill().await;
    server.cleanup().await;
}

#[tokio::test]
async fn gateway_stops_when_token_is_rejected() {
    let server = Server::start().await;
    let client = Client::new(&server.url).with_token("not-a-real-token");
    let (handle, mut rx) = spawn_gateway(client);
    expect_update(&mut rx, "AuthExpired", |u| *u == GatewayUpdate::AuthExpired).await;
    timeout(WAIT, async {
        while !handle.is_stopped() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("gateway task ends after AuthExpired");
    server.cleanup().await;
}

#[tokio::test]
async fn gateway_declares_silent_connection_dead() {
    let server = Server::start().await;
    let client = Client::new(&server.url);
    common::sign_up(&server.state, &client, "alice").await;
    // The server heartbeats every 200ms; a watchdog window far below that
    // must trip and reconnect.
    let (tx, mut rx) = unbounded_channel();
    let config = GatewayConfig {
        initial_backoff: Duration::from_millis(50),
        heartbeat_interval: Duration::from_millis(1),
        silence_factor: 2.5,
        ..Default::default()
    };
    let _handle = Gateway::spawn_with(client, config, move |u| {
        let _ = tx.send(u);
    });
    expect_update(&mut rx, "Disconnected", |u| matches!(u, GatewayUpdate::Disconnected { .. })).await;
    server.cleanup().await;
}

/// The first run of 6 digits in `text`.
fn six_digits(text: &str) -> String {
    let b = text.as_bytes();
    (0..b.len() - 5).find(|&i| b[i..i + 6].iter().all(u8::is_ascii_digit)).map(|i| text[i..i + 6].to_owned()).expect("code")
}

#[tokio::test]
async fn invite_accept_and_sign_in() {
    let server = Server::start().await;
    let admin = Client::new(&server.url);
    let (_, admin_user) = common::sign_up(&server.state, &admin, "admin").await;
    thehideout_server::admin::set_flag(&server.state, admin_user.id, true).await.unwrap();
    let guild = admin.create_guild("friends").await.unwrap();

    let invite = admin
        .create_invite(&NewInvite { email: "Zoe@Example.test".into(), guild_ids: vec![guild.id], ..Default::default() })
        .await
        .expect("create invite");
    assert_eq!(invite.email, "zoe@example.test");
    assert_eq!(invite.status, InviteStatus::Pending);
    let code = invite.code.clone().expect("code");
    // The emailed link (test hook: the server's in-memory outbox) and the code both work; the
    // creator gets both back.
    let mail = common::mail(&server.state, "zoe@example.test", "invited").await;
    assert!(mail.text.contains(&code));
    let link = invite.link.clone().expect("link");
    assert!(link.contains("/invite/") && mail.text.contains(&link), "{link}");

    let zoe = Client::new(&server.url);
    let preview = zoe.lookup_invite(&code).await.unwrap();
    assert_eq!((preview.masked_email.as_str(), preview.guild_names.clone()), ("z***@example.test", vec!["friends".to_owned()]));
    let preview_terms = preview.terms_version.clone();
    assert_eq!(preview.minimum_age, 16);
    // The terms and the age confirmation are required.
    assert!(matches!(
        zoe.accept_invite(&code, "zoe", Some(PASSWORD), "laptop", thehideout_api::InviteTerms { version: "", age_confirmed: true }).await,
        Err(ApiError::TermsUpdateRequired)
    ));
    assert!(matches!(
        zoe.accept_invite(&code, "zoe", Some(PASSWORD), "laptop", thehideout_api::InviteTerms { version: &preview_terms, age_confirmed: false }).await,
        Err(ApiError::InvalidArgument(_))
    ));
    let (token, me) = zoe.accept_invite(&code, "zoe", Some(PASSWORD), "laptop", thehideout_api::InviteTerms { version: &preview_terms, age_confirmed: true }).await.expect("accept");
    let (_, terms) = zoe.get_me_with_terms().await.unwrap();
    assert_eq!((terms.update_required, terms.current), (None, preview_terms.clone()));
    assert_eq!(zoe.token().as_deref(), Some(token.as_str()));
    assert_eq!(me.email.as_deref(), Some("zoe@example.test"));
    assert_eq!(zoe.list_guilds().await.unwrap()[0].id, guild.id);
    assert!(matches!(zoe.accept_invite(&code, "zoe2", None, "", thehideout_api::InviteTerms { version: &preview_terms, age_confirmed: true }).await, Err(ApiError::Precondition(_))));
    let listed = admin.list_invites(None, None, 10).await.unwrap();
    assert_eq!(listed.items[0].status, InviteStatus::Accepted);

    // Password sign-in by email.
    let fresh = Client::new(&server.url);
    match fresh.login_on("zoe@example.test", PASSWORD, "phone").await.unwrap() {
        LoginOutcome::Session { user, .. } => assert_eq!(user.id, me.id),
        other => panic!("unexpected {other:?}"),
    }
    let sessions = fresh.list_sessions().await.unwrap();
    assert!(sessions.iter().any(|s| s.current && s.device_name == "phone"));
    assert!(sessions.iter().any(|s| s.device_name == "laptop"));
    server.cleanup().await;
}

#[tokio::test]
async fn email_code_sign_in_with_second_factor() {
    let server = Server::start().await;
    let ann = Client::new(&server.url);
    common::sign_up(&server.state, &ann, "ann").await;
    let email = common::email("ann");

    // Email code without two-factor: straight to a session.
    let fresh = Client::new(&server.url);
    fresh.request_email_code(&email).await.unwrap();
    let code = six_digits(&common::mail(&server.state, &email, "sign-in code").await.text);
    assert!(matches!(fresh.verify_email_code(&email, &code, "tablet").await.unwrap(), LoginOutcome::Session { .. }));
    assert!(fresh.token().is_some());

    // With an authenticator app the code leads to a second-factor challenge. A wrong code while
    // signed in is a `WrongCode` with the tries left, never `Unauthenticated`.
    let security = ann.get_security().await.unwrap();
    assert!(!security.totp_enabled && security.has_password && security.email_verified);
    assert_eq!(ann.confirm_totp("000000").await, Err(ApiError::CodeExpired), "no setup started");
    let setup = ann.begin_totp_setup().await.unwrap();
    assert_eq!(ann.confirm_totp("000000").await, Err(ApiError::WrongCode { attempts_remaining: 8 }));
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let recovery = ann.confirm_totp(&thehideout_server::codes::totp_now(&setup.secret, now)).await.unwrap();
    assert_eq!(recovery.len(), 10);
    let security = ann.get_security().await.unwrap();
    assert!(security.totp_enabled);
    assert_eq!((security.recovery_codes_remaining, security.passkey_count), (10, 0));
    let other = Client::new(&server.url);
    other.request_email_code(&email).await.unwrap();
    let mail = loop {
        let m = common::mail(&server.state, &email, "sign-in code").await;
        if six_digits(&m.text) != code {
            break m;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let LoginOutcome::SecondFactor { challenge, methods } = other.verify_email_code(&email, &six_digits(&mail.text), "").await.unwrap()
    else {
        panic!("second factor expected");
    };
    assert_eq!(methods.len(), 2);
    assert!(other.token().is_none());
    assert_eq!(
        other.complete_second_factor(&challenge, SecondFactor::Totp("000000")).await,
        Err(ApiError::WrongCode { attempts_remaining: 4 })
    );
    let (_, me) = other.complete_second_factor(&challenge, SecondFactor::RecoveryCode(&recovery[0])).await.unwrap();
    assert_eq!(me.username, "ann");
    assert!(other.token().is_some());
    assert_eq!(
        other.complete_second_factor(&challenge, SecondFactor::RecoveryCode(&recovery[1])).await,
        Err(ApiError::CodeExpired),
        "the challenge is used"
    );
    assert_eq!(ann.get_security().await.unwrap().recovery_codes_remaining, 9);
    server.cleanup().await;
}

#[tokio::test]
async fn passkey_answers_the_second_factor_and_too_many_wrong_codes_end_it() {
    use webauthn_authenticator_rs::WebauthnAuthenticator;
    use webauthn_authenticator_rs::prelude::Url;
    use webauthn_authenticator_rs::softpasskey::SoftPasskey;

    let server = Server::start().await;
    let bea = Client::new(&server.url);
    common::sign_up(&server.state, &bea, "bea").await;
    let origin = Url::parse(&server.state.public_base()).unwrap();
    let mut key = SoftPasskey::new(true);
    let ceremony = bea.begin_passkey_registration("laptop", None).await.unwrap();
    let credential = key.do_registration(origin.clone(), serde_json::from_str(&ceremony.options_json).unwrap()).unwrap();
    bea.finish_passkey_registration(&ceremony.id, Some(&serde_json::to_string(&credential).unwrap())).await.unwrap().unwrap();
    let setup = bea.begin_totp_setup().await.unwrap();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    bea.confirm_totp(&thehideout_server::codes::totp_now(&setup.secret, now)).await.unwrap();
    assert_eq!(bea.get_security().await.unwrap().passkey_count, 1);

    let challenge = |c: &Client| {
        let c = c.clone();
        async move {
            match c.login("bea", PASSWORD).await.unwrap() {
                LoginOutcome::SecondFactor { challenge, methods } => (challenge, methods),
                other => panic!("second factor expected, got {other:?}"),
            }
        }
    };
    let other = Client::new(&server.url);
    let (id, methods) = challenge(&other).await;
    assert!(methods.contains(&thehideout_api::SecondFactorMethod::Passkey));
    let ceremony = other.begin_second_factor_passkey(&id, None).await.unwrap();
    assert_eq!(other.finish_second_factor_passkey(&ceremony.id, None).await, Ok(None), "pending");
    let credential = key.do_authentication(origin, serde_json::from_str(&ceremony.options_json).unwrap()).unwrap();
    let (_, me) = other
        .finish_second_factor_passkey(&ceremony.id, Some(&serde_json::to_string(&credential).unwrap()))
        .await
        .unwrap()
        .expect("signed in");
    assert_eq!(me.username, "bea");
    assert!(other.token().is_some());

    // Five wrong answers end the challenge.
    let third = Client::new(&server.url);
    let (id, _) = challenge(&third).await;
    for left in (1..=4).rev() {
        assert_eq!(third.complete_second_factor(&id, SecondFactor::Totp("000000")).await, Err(ApiError::WrongCode { attempts_remaining: left }));
    }
    assert_eq!(third.complete_second_factor(&id, SecondFactor::Totp("000000")).await, Err(ApiError::TooManyAttempts));
    assert_eq!(third.begin_second_factor_passkey(&id, None).await, Err(ApiError::CodeExpired));
    server.cleanup().await;
}
