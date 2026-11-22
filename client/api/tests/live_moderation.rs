//! End-to-end tests of editing and deleting messages, kick, ban, leaving and the guild structure
//! calls against the real server running in-process on a throwaway ScyllaDB keyspace (needs
//! ScyllaDB at 127.0.0.1:9042: `docker compose up -d`).

use std::sync::Arc;
use std::time::Duration;

use rand::Rng;
use scylla::client::caching_session::CachingSession;
use scylla::client::session_builder::SessionBuilder;
mod common;

use thehideout_api::{
    ApiError, Client, Event, Gateway, GatewayConfig, GatewayUpdate, MemberRemovalReason,
};
use thehideout_server::snowflake::Snowflake;
use thehideout_server::{AppState, build_app, migrations};
use tokio::net::TcpListener;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
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

fn spawn_gateway(client: Client) -> (thehideout_api::GatewayHandle, UnboundedReceiver<GatewayUpdate>) {
    let (tx, rx) = unbounded_channel();
    let config = GatewayConfig { heartbeat_interval: HEARTBEAT, ..Default::default() };
    let handle = Gateway::spawn_with(client, config, move |u| {
        let _ = tx.send(u);
    });
    (handle, rx)
}

/// Waits for the first update matching `pred`, skipping others.
async fn expect_update(rx: &mut UnboundedReceiver<GatewayUpdate>, what: &str, pred: impl Fn(&Event) -> bool) -> Event {
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

async fn expect_connected(rx: &mut UnboundedReceiver<GatewayUpdate>) {
    timeout(WAIT, async {
        loop {
            if matches!(rx.recv().await.expect("gateway stopped"), GatewayUpdate::Connected) {
                return;
            }
        }
    })
    .await
    .expect("gateway connected");
}

#[tokio::test]
async fn messages_members_and_structure() {
    let server = Server::start().await;
    let (alice, bob, carol) = (Client::new(&server.url), Client::new(&server.url), Client::new(&server.url));
    common::sign_up(&server.state, &alice, "alice").await;
    let (_, bob_user) = common::sign_up(&server.state, &bob, "bob").await;
    let (_, carol_user) = common::sign_up(&server.state, &carol, "carol").await;
    let guild = alice.create_guild("friends").await.unwrap();
    bob.join_guild(&guild.invite_code).await.unwrap();
    carol.join_guild(&guild.invite_code).await.unwrap();
    let general = alice.list_channels(guild.id).await.unwrap()[0].id;
    let (_alice_gateway, mut alice_rx) = spawn_gateway(alice.clone());
    let (_bob_gateway, mut bob_rx) = spawn_gateway(bob.clone());
    expect_connected(&mut alice_rx).await;
    expect_connected(&mut bob_rx).await;

    // Edit your own message; others cannot; the event carries the new text.
    let m = bob.send_message(general, "first", None).await.unwrap();
    let edited = bob.edit_message(general, m.id, "second").await.unwrap();
    assert_eq!(edited.content, "second");
    assert!(edited.edited_at.is_some());
    assert_eq!(alice.edit_message(general, m.id, "mine").await.unwrap_err(), ApiError::PermissionDenied);
    expect_update(&mut alice_rx, "MessageUpdated", |e| matches!(e, Event::MessageUpdated(u) if u.id == m.id && u.content == "second")).await;

    // The owner deletes it; everyone hears; a plain member cannot delete the owner's.
    let theirs = alice.send_message(general, "owner's", None).await.unwrap();
    assert_eq!(bob.delete_message(general, theirs.id).await.unwrap_err(), ApiError::PermissionDenied);
    alice.delete_message(general, m.id).await.unwrap();
    expect_update(&mut bob_rx, "MessageDeleted", |e| matches!(e, Event::MessageDeleted { message_id, .. } if *message_id == m.id)).await;
    let page = alice.list_messages(general, None, 50).await.unwrap();
    assert_eq!(page.items.iter().map(|i| i.id).collect::<Vec<_>>(), vec![theirs.id]);
    assert_eq!(alice.delete_message(general, m.id).await.unwrap_err(), ApiError::NotFound);

    // Rotate the invite code, then kick, ban, list bans, unban.
    let new_code = alice.rotate_invite_code(guild.id).await.unwrap();
    assert_ne!(new_code, guild.invite_code);
    let dave = Client::new(&server.url);
    common::sign_up(&server.state, &dave, "dave").await;
    assert_eq!(dave.join_guild(&guild.invite_code).await.unwrap_err(), ApiError::NotFound);
    assert_eq!(bob.rotate_invite_code(guild.id).await.unwrap_err(), ApiError::PermissionDenied);

    alice.kick_member(guild.id, bob_user.id).await.unwrap();
    expect_update(&mut bob_rx, "GuildRemoved", |e| matches!(e, Event::GuildRemoved { guild_id } if *guild_id == guild.id)).await;
    expect_update(&mut alice_rx, "GuildMemberRemoved", |e| {
        matches!(e, Event::GuildMemberRemoved { user_id, reason: MemberRemovalReason::Kicked, .. } if *user_id == bob_user.id)
    })
    .await;
    assert_eq!(bob.list_channels(guild.id).await.unwrap_err(), ApiError::NotFound);
    bob.join_guild(&new_code).await.expect("kicked members may rejoin");

    alice.ban_member(guild.id, carol_user.id, "spam", Some(3600)).await.unwrap();
    assert_eq!(carol.join_guild(&new_code).await.unwrap_err(), ApiError::PermissionDenied);
    let bans = alice.list_bans(guild.id).await.unwrap();
    assert_eq!((bans[0].user.id, bans[0].banned_by, bans[0].reason.as_str()), (carol_user.id, alice.get_me().await.unwrap().id, "spam"));
    assert_eq!(bob.list_bans(guild.id).await.unwrap_err(), ApiError::PermissionDenied);
    alice.unban_member(guild.id, carol_user.id).await.unwrap();
    assert_eq!(alice.unban_member(guild.id, carol_user.id).await.unwrap_err(), ApiError::NotFound);

    // Structure: delete a channel, the owner cannot leave, hand over, leave, delete.
    let extra = alice.create_channel(guild.id, "extra").await.unwrap();
    let layout = alice.delete_channel(guild.id, extra.id).await.unwrap();
    assert!(layout.entries.iter().all(|e| !e.channel_ids.contains(&extra.id)));
    assert_eq!(alice.list_messages(extra.id, None, 10).await.unwrap_err(), ApiError::NotFound);
    assert!(matches!(alice.leave_guild(guild.id).await.unwrap_err(), ApiError::Precondition(_)));
    assert_eq!(bob.transfer_ownership(guild.id, bob_user.id, "friends").await.unwrap_err(), ApiError::PermissionDenied);
    let handed = alice.transfer_ownership(guild.id, bob_user.id, "friends").await.unwrap();
    assert_eq!(handed.owner_id, bob_user.id);
    alice.leave_guild(guild.id).await.unwrap();
    assert!(alice.list_guilds().await.unwrap().is_empty());
    assert!(matches!(bob.delete_guild(guild.id, "wrong").await.unwrap_err(), ApiError::InvalidArgument(_)));
    bob.delete_guild(guild.id, "friends").await.unwrap();
    assert!(bob.list_guilds().await.unwrap().is_empty());
    assert_eq!(bob.list_channels(guild.id).await.unwrap_err(), ApiError::NotFound);
    server.cleanup().await;
}
