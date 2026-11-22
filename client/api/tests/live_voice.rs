//! End-to-end tests of voice channels against the real server running in-process on a throwaway
//! ScyllaDB keyspace (needs ScyllaDB at 127.0.0.1:9042: `docker compose up -d`). Voice is on with
//! the dev LiveKit keys of `.env.example`; no media flows, only the server's bookkeeping and the
//! tokens it signs (the LiveKit server API calls the server makes fail quietly when it is down).

use std::sync::Arc;
use std::time::Duration;

use rand::Rng;
use scylla::client::caching_session::CachingSession;
use scylla::client::session_builder::SessionBuilder;
mod common;

use thehideout_api::{
    ApiError, ChannelIconRef, ChannelKind, Client, Event, Gateway, GatewayConfig, GatewayUpdate, PaletteColor,
};
use thehideout_server::config::LiveKitConfig;
use thehideout_server::snowflake::Snowflake;
use thehideout_server::voice::LiveKit;
use thehideout_server::{AppState, build_app, migrations};
use tokio::net::TcpListener;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tokio::time::timeout;

const HEARTBEAT: Duration = Duration::from_millis(200);
const WAIT: Duration = Duration::from_secs(10);
const LIVEKIT_URL: &str = "ws://127.0.0.1:7880";

struct Server {
    url: String,
    state: AppState,
}

impl Server {
    async fn start(voice: bool) -> Self {
        let session = SessionBuilder::new()
            .known_node("127.0.0.1:9042")
            .build()
            .await
            .expect("connect to ScyllaDB at 127.0.0.1:9042 (is `docker compose up -d` running?)");
        let keyspace = format!("test_{:016x}", rand::rng().random::<u64>());
        migrations::run(&session, &keyspace).await.expect("run migrations");
        let session = Arc::new(CachingSession::from(session, 64));
        let mut state = AppState::new(session, Arc::new(Snowflake::new(0)), keyspace, HEARTBEAT);
        if voice {
            let config = LiveKitConfig::new(LIVEKIT_URL, None, "devkey".into(), "secret".into()).expect("livekit config");
            state = state.with_livekit(LiveKit::new(config));
        }
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

fn spawn_gateway(client: Client) -> (thehideout_api::GatewayHandle, UnboundedReceiver<GatewayUpdate>) {
    let (tx, rx) = unbounded_channel();
    let config = GatewayConfig { heartbeat_interval: HEARTBEAT, ..Default::default() };
    let handle = Gateway::spawn_with(client, config, move |u| {
        let _ = tx.send(u);
    });
    (handle, rx)
}

async fn connected(rx: &mut UnboundedReceiver<GatewayUpdate>) {
    timeout(WAIT, async {
        while rx.recv().await != Some(GatewayUpdate::Connected) {}
    })
    .await
    .expect("gateway connects");
}

/// Waits for the first event matching `pred`, skipping others.
async fn expect_event(rx: &mut UnboundedReceiver<GatewayUpdate>, what: &str, pred: impl Fn(&Event) -> bool) -> Event {
    timeout(WAIT, async {
        loop {
            if let GatewayUpdate::Event(event) = rx.recv().await.expect("gateway stopped")
                && pred(&event)
            {
                return event;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
}

#[tokio::test]
async fn voice_channels_list_but_joining_needs_a_configured_server() {
    let server = Server::start(false).await;
    let alice = Client::new(&server.url);
    common::sign_up(&server.state, &alice, "alice").await;
    let guild = alice.create_guild("friends").await.unwrap();
    let lounge = alice
        .create_channel_of_kind(guild.id, "lounge", ChannelKind::Voice, None, &ChannelIconRef::Default, PaletteColor::None)
        .await
        .expect("create voice channel");
    assert_eq!(lounge.kind, ChannelKind::Voice);
    let channels = alice.list_channels(guild.id).await.unwrap();
    assert!(channels.iter().any(|c| c.id == lounge.id && c.kind == ChannelKind::Voice));
    assert!(channels.iter().any(|c| c.kind == ChannelKind::Text), "the first channel is text");
    // Without LiveKit the server answers UNAVAILABLE, which the client reports as a network error.
    assert!(matches!(alice.join_voice(lounge.id).await, Err(ApiError::Network(m)) if m.contains("not configured")));
    server.cleanup().await;
}

#[tokio::test]
async fn joining_moderating_and_the_events_around_it() {
    let server = Server::start(true).await;
    let alice = Client::new(&server.url);
    let bob = Client::new(&server.url);
    common::sign_up(&server.state, &alice, "alice").await;
    let (_, bob_user) = common::sign_up(&server.state, &bob, "bob").await;
    let guild = alice.create_guild("friends").await.unwrap();
    bob.join_guild(&guild.invite_code).await.unwrap();
    let make = |name: &'static str, kind| {
        let alice = alice.clone();
        async move {
            alice
                .create_channel_of_kind(guild.id, name, kind, None, &ChannelIconRef::Default, PaletteColor::None)
                .await
                .unwrap()
        }
    };
    let lounge = make("lounge", ChannelKind::Voice).await;
    let games = make("games", ChannelKind::Voice).await;
    let text = alice.list_channels(guild.id).await.unwrap().into_iter().find(|c| c.kind == ChannelKind::Text).unwrap();

    let (_alice_gateway, mut alice_rx) = spawn_gateway(alice.clone());
    let (_bob_gateway, mut bob_rx) = spawn_gateway(bob.clone());
    connected(&mut alice_rx).await;
    connected(&mut bob_rx).await;

    // Text channels can't be joined; a voice channel returns what a LiveKit client needs.
    assert!(matches!(alice.join_voice(text.id).await, Err(ApiError::Precondition(_))));
    let join = alice.join_voice(lounge.id).await.expect("join voice");
    assert_eq!((join.url.as_str(), join.channel_id), (LIVEKIT_URL, lounge.id));
    assert!(!join.token.is_empty());
    let event = expect_event(&mut bob_rx, "alice joining", |e| matches!(e, Event::VoiceStateUpdate { state: Some(_), .. })).await;
    let Event::VoiceStateUpdate { guild_id, state: Some(state), .. } = event else { unreachable!() };
    assert_eq!((guild_id, state.channel_id, state.self_mute), (guild.id, lounge.id, false));

    // Own flags.
    let state = alice.update_voice_state(Some(true), Some(false), None, None).await.expect("update state");
    assert!(state.self_mute && !state.self_deaf);
    expect_event(&mut bob_rx, "alice muting", |e| matches!(e, Event::VoiceStateUpdate { state: Some(s), .. } if s.self_mute)).await;
    assert!(matches!(bob.update_voice_state(Some(true), None, None, None).await, Err(ApiError::Precondition(_))));

    // A client opening a stream while someone is in voice gets the snapshot.
    let carol = Client::new(&server.url);
    common::sign_up(&server.state, &carol, "carol").await;
    carol.join_guild(&guild.invite_code).await.unwrap();
    let (_carol_gateway, mut carol_rx) = spawn_gateway(carol.clone());
    let event = expect_event(&mut carol_rx, "the snapshot", |e| matches!(e, Event::VoiceSnapshot { .. })).await;
    let Event::VoiceSnapshot { guild_ids, states } = event else { unreachable!() };
    assert!(guild_ids.contains(&guild.id));
    assert_eq!(states.len(), 1);
    assert_eq!(states[0].channel_id, lounge.id);

    // Moderation: bob has no rights, alice (the owner) does.
    bob.join_voice(lounge.id).await.expect("bob joins");
    assert_eq!(bob.server_mute_member(guild.id, bob_user.id, true).await.unwrap_err(), ApiError::PermissionDenied);
    let muted = alice.server_mute_member(guild.id, bob_user.id, true).await.expect("server mute");
    assert!(muted.server_mute);
    expect_event(&mut bob_rx, "the server mute", |e| matches!(e, Event::VoiceStateUpdate { state: Some(s), .. } if s.server_mute)).await;

    alice.move_member(guild.id, bob_user.id, games.id).await.expect("move");
    let event = expect_event(&mut bob_rx, "voice_moved", |e| matches!(e, Event::VoiceMoved { .. })).await;
    assert_eq!(event, Event::VoiceMoved { guild_id: guild.id, channel_id: games.id });
    let rejoin = bob.join_voice(games.id).await.expect("rejoin");
    assert_eq!(rejoin.channel_id, games.id);

    alice.disconnect_member(guild.id, bob_user.id).await.expect("disconnect");
    expect_event(&mut bob_rx, "bob leaving", |e| matches!(e, Event::VoiceStateUpdate { user_id, state: None, .. } if *user_id == bob_user.id)).await;
    alice.leave_voice().await.expect("leave");
    alice.leave_voice().await.expect("leaving twice is fine");
    server.cleanup().await;
}
