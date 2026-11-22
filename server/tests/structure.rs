//! Deleting channels and guilds, rotating the invite code, transferring ownership.
mod common;

use std::time::Duration;

use common::TestApp;
use common::fixtures::*;
use connectrpc::client::HttpClient;
use connectrpc::{ConnectError, ErrorCode};
use thehideout_server::proto::thehideout::v1::event::Payload;
use thehideout_server::proto::thehideout::v1::*;
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(5);

macro_rules! subscribe {
    ($app:expr, $who:expr) => {
        $app.client(GatewayServiceClient::new, Some(&$who.token))
            .subscribe(SubscribeRequest::default())
            .await
            .expect("subscribe")
    };
}

macro_rules! next_payload {
    ($stream:expr) => {{
        loop {
            let msg = timeout(WAIT, $stream.message())
                .await
                .expect("timed out waiting for an event")
                .expect("stream error")
                .expect("stream ended");
            let event = msg.to_owned_message().event.into_option().expect("event");
            match event.payload {
                Some(Payload::Heartbeat(_)) | None => continue,
                Some(p) => break p,
            }
        }
    }};
}

macro_rules! payloads_within {
    ($stream:expr, $window:expr) => {{
        let mut out = Vec::new();
        let deadline = tokio::time::Instant::now() + $window;
        while let Ok(res) = tokio::time::timeout_at(deadline, $stream.message()).await {
            let event = res.unwrap().unwrap().to_owned_message().event.into_option().unwrap();
            if !matches!(event.payload, Some(Payload::Heartbeat(_)) | None) {
                out.push(event.payload.unwrap());
            }
        }
        out
    }};
}

fn code<T: std::fmt::Debug>(result: Result<T, ConnectError>) -> ErrorCode {
    result.expect_err("expected an error").code
}

fn messages(app: &TestApp, who: &Account) -> MessageServiceClient<HttpClient> {
    app.client(MessageServiceClient::new, Some(&who.token))
}

async fn send(app: &TestApp, who: &Account, channel_id: u64, content: &str) -> Result<Message, ConnectError> {
    Ok(messages(app, who)
        .send_message(SendMessageRequest { channel_id, content: content.into(), ..Default::default() })
        .await?
        .into_owned()
        .message
        .unwrap())
}

async fn list(app: &TestApp, who: &Account, channel_id: u64) -> Result<Vec<Message>, ConnectError> {
    Ok(messages(app, who)
        .list_messages(ListMessagesRequest { channel_id, ..Default::default() })
        .await?
        .into_owned()
        .messages)
}

async fn delete_channel(app: &TestApp, who: &Account, guild_id: u64, channel_id: u64) -> Result<GuildLayout, ConnectError> {
    Ok(guilds(app, who)
        .delete_channel(DeleteChannelRequest { guild_id, channel_id, ..Default::default() })
        .await?
        .into_owned()
        .layout
        .unwrap())
}

async fn delete_guild(app: &TestApp, who: &Account, guild_id: u64, confirm: &str) -> Result<(), ConnectError> {
    guilds(app, who)
        .delete_guild(DeleteGuildRequest { guild_id, confirm_name: confirm.into(), ..Default::default() })
        .await?;
    Ok(())
}

async fn rotate(app: &TestApp, who: &Account, guild_id: u64) -> Result<String, ConnectError> {
    Ok(guilds(app, who)
        .rotate_invite_code(RotateInviteCodeRequest { guild_id, ..Default::default() })
        .await?
        .into_owned()
        .invite_code)
}

async fn transfer(app: &TestApp, who: &Account, guild_id: u64, user_id: u64, confirm: &str) -> Result<Guild, ConnectError> {
    Ok(guilds(app, who)
        .transfer_ownership(TransferOwnershipRequest { guild_id, user_id, confirm_name: confirm.into() , ..Default::default() })
        .await?
        .into_owned()
        .guild
        .unwrap())
}

async fn my_guild_ids(app: &TestApp, who: &Account) -> Vec<u64> {
    guilds(app, who)
        .list_my_guilds(ListMyGuildsRequest::default())
        .await
        .unwrap()
        .into_owned()
        .guilds
        .iter()
        .map(|g| g.id)
        .collect()
}

async fn rows(app: &TestApp, cql: &str, values: impl scylla::serialize::row::SerializeRow) -> usize {
    app.state.session.execute_unpaged(cql.to_owned(), values).await.unwrap().into_rows_result().unwrap().rows_num()
}

/// Polls until `check` holds (background purges have no completion signal).
async fn eventually<F: std::future::Future<Output = bool>>(what: &str, mut check: impl FnMut() -> F) {
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while !check().await {
        assert!(std::time::Instant::now() < deadline, "timed out waiting for: {what}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn start() -> TestApp {
    TestApp::builder().livekit().storage().heartbeat(Duration::from_millis(200)).start().await
}

struct World {
    app: TestApp,
    alice: Account,
    bob: Account,
    guild: Guild,
    general: u64,
}

async fn world() -> World {
    let app = start().await;
    let (alice, bob) = (user(&app, "alice").await, user(&app, "bob").await);
    let guild = guild(&app, &alice).await;
    join(&app, &bob, &guild).await;
    let general = channels(&app, &alice, guild.id).await.unwrap().channels[0].id;
    World { app, alice, bob, guild, general }
}

// ---------------------------------------------------------- delete channel

#[tokio::test]
async fn delete_channel_needs_manage_channels_and_an_existing_channel() {
    let w = world().await;
    let extra = channel(&w.app, &w.alice, w.guild.id, "extra").await.unwrap();
    assert_eq!(code(delete_channel(&w.app, &w.bob, w.guild.id, extra.id).await), ErrorCode::PermissionDenied);
    let outsider = user(&w.app, "dave").await;
    assert_eq!(code(delete_channel(&w.app, &outsider, w.guild.id, extra.id).await), ErrorCode::NotFound);
    assert_eq!(code(delete_channel(&w.app, &w.alice, w.guild.id, 424_242).await), ErrorCode::NotFound);

    // MANAGE_CHANNELS holders may.
    let managers = role(&w.app, &w.alice, w.guild.id, "builders", MANAGE_CHANNELS).await.unwrap();
    set_roles(&w.app, &w.alice, w.guild.id, w.bob.id, &[managers.id]).await.unwrap();
    delete_channel(&w.app, &w.bob, w.guild.id, extra.id).await.unwrap();
    assert_eq!(code(delete_channel(&w.app, &w.bob, w.guild.id, extra.id).await), ErrorCode::NotFound);
    // Another guild's channel id is not found in this guild.
    let other = guild(&w.app, &w.alice).await;
    let theirs = channels(&w.app, &w.alice, other.id).await.unwrap().channels[0].id;
    assert_eq!(code(delete_channel(&w.app, &w.alice, w.guild.id, theirs).await), ErrorCode::NotFound);
    w.app.cleanup().await;
}

#[tokio::test]
async fn delete_channel_updates_listings_events_and_makes_reads_fail() {
    let w = world().await;
    let doomed = channel(&w.app, &w.alice, w.guild.id, "doomed").await.unwrap();
    let keep = channel(&w.app, &w.alice, w.guild.id, "keep").await.unwrap();
    let before = channels(&w.app, &w.alice, w.guild.id).await.unwrap();
    let m = send(&w.app, &w.alice, doomed.id, "bye").await.unwrap();
    let mut stream = subscribe!(w.app, w.bob);

    let layout = delete_channel(&w.app, &w.alice, w.guild.id, doomed.id).await.unwrap();
    assert_eq!(layout.version, before.layout.as_option().unwrap().version + 1);
    assert!(layout.entries.iter().all(|e| !e.channel_ids.contains(&doomed.id)));
    assert!(layout.entries.iter().any(|e| e.channel_ids.contains(&keep.id)));

    // Members hear channel_deleted, then the new layout.
    let Payload::ChannelDeleted(d) = next_payload!(stream) else { panic!("expected ChannelDeleted") };
    assert_eq!((d.guild_id, d.channel_id), (w.guild.id, doomed.id));
    let Payload::LayoutUpdated(u) = next_payload!(stream) else { panic!("expected LayoutUpdated") };
    assert_eq!(u.layout.unwrap().version, layout.version);

    // Gone from the listing; reads and writes fail at once.
    let after = channels(&w.app, &w.bob, w.guild.id).await.unwrap();
    assert!(after.channels.iter().all(|c| c.id != doomed.id));
    assert_eq!(after.channels.len(), before.channels.len() - 1);
    assert_eq!(code(list(&w.app, &w.bob, doomed.id).await), ErrorCode::NotFound);
    assert_eq!(code(send(&w.app, &w.alice, doomed.id, "x").await), ErrorCode::NotFound);

    // The messages are purged in the background.
    let (channel, bucket) = (doomed.id as i64, thehideout_server::snowflake::bucket_of(m.id));
    eventually("messages purged", || async {
        rows(&w.app, "SELECT id FROM messages WHERE channel_id = ? AND bucket = ?", (channel, bucket)).await == 0
    })
    .await;
    // Channel quota is given back: the guild can create it again.
    channel_again(&w).await;
    w.app.cleanup().await;
}

async fn channel_again(w: &World) {
    channel(&w.app, &w.alice, w.guild.id, "doomed").await.expect("name is free again");
}

#[tokio::test]
async fn delete_channel_removes_attachment_objects_and_clears_voice() {
    let w = world().await;
    let voice_ch = guilds(&w.app, &w.alice)
        .create_channel(CreateChannelRequest {
            guild_id: w.guild.id,
            name: "lounge".into(),
            kind: ChannelKind::CHANNEL_KIND_VOICE.into(),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_owned()
        .channel
        .unwrap();
    w.app
        .client(VoiceServiceClient::new, Some(&w.bob.token))
        .join_voice(JoinVoiceRequest { channel_id: voice_ch.id, ..Default::default() })
        .await
        .unwrap();
    assert!(w.app.state.voice.get(w.bob.id).is_some());
    let mut stream = subscribe!(w.app, w.alice);
    let _ = payloads_within!(stream, Duration::from_millis(300));

    delete_channel(&w.app, &w.alice, w.guild.id, voice_ch.id).await.unwrap();
    assert!(w.app.state.voice.get(w.bob.id).is_none(), "voice state cleared");
    assert!(w.app.livekit.as_ref().unwrap().removed(&format!("voice-{}", voice_ch.id), w.bob.id));
    let events = payloads_within!(stream, Duration::from_millis(500));
    assert!(events.iter().any(|p| matches!(p, Payload::VoiceStateUpdate(v) if v.user_id == w.bob.id && v.state.as_option().is_none())));
    assert!(events.iter().any(|p| matches!(p, Payload::ChannelDeleted(_))));

    // A text channel with a file.
    let text = channel(&w.app, &w.alice, w.guild.id, "files").await.unwrap();
    let upload = reqwest::Client::new()
        .post(format!("{}/attachments?channel_id={}&filename=a.txt", w.app.base_url, text.id))
        .header("authorization", format!("Bearer {}", w.alice.token))
        .header("content-type", "text/plain")
        .body("some file")
        .send()
        .await
        .unwrap();
    assert_eq!(upload.status(), 200);
    let id: u64 = upload.json::<serde_json::Value>().await.unwrap()["id"].as_str().unwrap().parse().unwrap();
    let m = messages(&w.app, &w.alice)
        .send_message(SendMessageRequest { channel_id: text.id, content: "f".into(), attachment_ids: vec![id], ..Default::default() })
        .await
        .unwrap()
        .into_owned()
        .message
        .unwrap();
    let key = m.attachments[0].file.clone();
    assert!(w.app.state.storage.as_ref().unwrap().head(&key).await.unwrap().is_some());
    delete_channel(&w.app, &w.alice, w.guild.id, text.id).await.unwrap();
    eventually("attachment object deleted", || async { w.app.state.storage.as_ref().unwrap().head(&key).await.unwrap().is_none() }).await;
    assert_eq!(reqwest::get(format!("{}/media/{key}", w.app.base_url)).await.unwrap().status(), 404);
    w.app.cleanup().await;
}

// ------------------------------------------------------------ delete guild

#[tokio::test]
async fn delete_guild_is_owner_only_and_needs_the_name() {
    let w = world().await;
    assert_eq!(code(delete_guild(&w.app, &w.bob, w.guild.id, "guild").await), ErrorCode::PermissionDenied);
    let outsider = user(&w.app, "dave").await;
    assert_eq!(code(delete_guild(&w.app, &outsider, w.guild.id, "guild").await), ErrorCode::NotFound);
    // Even ADMINISTRATOR is not the owner.
    let admins = role(&w.app, &w.alice, w.guild.id, "admins", ADMINISTRATOR).await.unwrap();
    set_roles(&w.app, &w.alice, w.guild.id, w.bob.id, &[admins.id]).await.unwrap();
    assert_eq!(code(delete_guild(&w.app, &w.bob, w.guild.id, "guild").await), ErrorCode::PermissionDenied);
    // Wrong or missing confirmation.
    assert_eq!(code(delete_guild(&w.app, &w.alice, w.guild.id, "Guild").await), ErrorCode::InvalidArgument);
    assert_eq!(code(delete_guild(&w.app, &w.alice, w.guild.id, "").await), ErrorCode::InvalidArgument);
    assert!(my_guild_ids(&w.app, &w.alice).await.contains(&w.guild.id));
    w.app.cleanup().await;
}

#[tokio::test]
async fn delete_guild_removes_everything() {
    let w = world().await;
    let carol = user(&w.app, "carol").await;
    join(&w.app, &carol, &w.guild).await;
    let second = channel(&w.app, &w.alice, w.guild.id, "second").await.unwrap();
    let r = role(&w.app, &w.alice, w.guild.id, "r", 0).await.unwrap();
    set_roles(&w.app, &w.alice, w.guild.id, w.bob.id, &[r.id]).await.unwrap();
    section(&w.app, &w.alice, w.guild.id, "sec").await.unwrap();
    let m = send(&w.app, &w.bob, w.general, "hello").await.unwrap();
    // A ban to be cleaned up, and a stored file counted against the guild.
    let dave = user(&w.app, "dave").await;
    guilds(&w.app, &w.alice)
        .ban_member(BanMemberRequest { guild_id: w.guild.id, user_id: dave.id, ..Default::default() })
        .await
        .unwrap();
    let upload = reqwest::Client::new()
        .post(format!("{}/attachments?channel_id={}&filename=a.txt", w.app.base_url, second.id))
        .header("authorization", format!("Bearer {}", w.alice.token))
        .header("content-type", "text/plain")
        .body("some file")
        .send()
        .await
        .unwrap();
    let id: u64 = upload.json::<serde_json::Value>().await.unwrap()["id"].as_str().unwrap().parse().unwrap();
    let with_file = messages(&w.app, &w.alice)
        .send_message(SendMessageRequest { channel_id: second.id, content: "f".into(), attachment_ids: vec![id], ..Default::default() })
        .await
        .unwrap()
        .into_owned()
        .message
        .unwrap();
    let key = with_file.attachments[0].file.clone();

    let mut streams = vec![subscribe!(w.app, w.alice), subscribe!(w.app, w.bob), subscribe!(w.app, carol)];
    let _ = payloads_within!(streams[0], Duration::from_millis(300));
    delete_guild(&w.app, &w.alice, w.guild.id, "guild").await.unwrap();

    // Every member's streams are told, and stop following the guild.
    for stream in &mut streams {
        loop {
            if let Payload::GuildRemoved(r) = next_payload!(stream) {
                assert_eq!(r.guild_id, w.guild.id);
                break;
            }
        }
    }
    // Listings: no guild, no channels, no members, no messages, the old code is dead.
    for who in [&w.alice, &w.bob] {
        assert!(!my_guild_ids(&w.app, who).await.contains(&w.guild.id));
    }
    assert_eq!(code(channels(&w.app, &w.alice, w.guild.id).await), ErrorCode::NotFound);
    assert_eq!(code(list(&w.app, &w.bob, w.general).await), ErrorCode::NotFound);
    assert_eq!(code(send(&w.app, &w.bob, second.id, "x").await), ErrorCode::NotFound);
    let eve = user(&w.app, "eve").await;
    let join = guilds(&w.app, &eve).join_guild(JoinGuildRequest { invite_code: w.guild.invite_code.clone(), ..Default::default() }).await;
    assert_eq!(code(join), ErrorCode::NotFound);
    // Nothing follows the guild on the open streams any more.
    assert!(payloads_within!(streams[1], Duration::from_millis(500)).iter().all(|p| !matches!(p, Payload::MessageCreated(_) | Payload::ChannelCreated(_))));

    // Background cleanup.
    let guild_db = w.guild.id as i64;
    let bucket = thehideout_server::snowflake::bucket_of(m.id);
    let (general, second_id) = (w.general as i64, second.id as i64);
    eventually("guild rows purged", || async {
        rows(&w.app, "SELECT id FROM channels WHERE guild_id = ?", (guild_db,)).await == 0
            && rows(&w.app, "SELECT id FROM roles WHERE guild_id = ?", (guild_db,)).await == 0
            && rows(&w.app, "SELECT id FROM sections WHERE guild_id = ?", (guild_db,)).await == 0
            && rows(&w.app, "SELECT user_id FROM member_roles WHERE guild_id = ?", (guild_db,)).await == 0
            && rows(&w.app, "SELECT user_id FROM guild_bans WHERE guild_id = ?", (guild_db,)).await == 0
            && rows(&w.app, "SELECT id FROM messages WHERE channel_id = ? AND bucket = ?", (general, bucket)).await == 0
            && rows(&w.app, "SELECT id FROM messages WHERE channel_id = ? AND bucket = ?", (second_id, thehideout_server::snowflake::bucket_of(with_file.id))).await == 0
    })
    .await;
    eventually("file deleted", || async { w.app.state.storage.as_ref().unwrap().head(&key).await.unwrap().is_none() }).await;
    assert_eq!(rows(&w.app, "SELECT key FROM media_objects WHERE key = ?", (key.as_str(),)).await, 0);
    w.app.cleanup().await;
}

// ------------------------------------------------------------ rotate invite

#[tokio::test]
async fn rotate_invite_code_replaces_the_code() {
    let w = world().await;
    assert_eq!(code(rotate(&w.app, &w.bob, w.guild.id).await), ErrorCode::PermissionDenied);
    let outsider = user(&w.app, "dave").await;
    assert_eq!(code(rotate(&w.app, &outsider, w.guild.id).await), ErrorCode::NotFound);
    let mut stream = subscribe!(w.app, w.bob);

    let new = rotate(&w.app, &w.alice, w.guild.id).await.unwrap();
    assert_ne!(new, w.guild.invite_code);
    assert_eq!(new.len(), 8);
    let Payload::GuildUpdated(g) = next_payload!(stream) else { panic!("expected GuildUpdated") };
    assert_eq!((g.id, g.invite_code.as_str()), (w.guild.id, new.as_str()));
    let listed = guilds(&w.app, &w.alice).list_my_guilds(ListMyGuildsRequest::default()).await.unwrap().into_owned().guilds;
    assert_eq!(listed.iter().find(|g| g.id == w.guild.id).unwrap().invite_code, new);

    // The old code is dead, the new one works.
    let join = |who: &Account, invite_code: String| {
        let client = guilds(&w.app, who);
        async move { client.join_guild(JoinGuildRequest { invite_code, ..Default::default() }).await.map(|_| ()) }
    };
    assert_eq!(code(join(&outsider, w.guild.invite_code.clone()).await), ErrorCode::NotFound);
    join(&outsider, new.clone()).await.unwrap();

    // MANAGE_SERVER holders may rotate (and the code changes each time).
    let r = role(&w.app, &w.alice, w.guild.id, "managers", MANAGE_SERVER).await.unwrap();
    set_roles(&w.app, &w.alice, w.guild.id, w.bob.id, &[r.id]).await.unwrap();
    let newer = rotate(&w.app, &w.bob, w.guild.id).await.unwrap();
    assert_ne!(newer, new);
    let eve = user(&w.app, "eve").await;
    assert_eq!(code(join(&eve, new).await), ErrorCode::NotFound);
    w.app.cleanup().await;
}

// ------------------------------------------------------------ transfer

#[tokio::test]
async fn transfer_ownership_moves_the_crown() {
    let w = world().await;
    let admin = site_admin(&w.app, "root").await;
    let outsider = user(&w.app, "dave").await;
    // Owner only; the name must match; the new owner must be another member.
    assert_eq!(code(transfer(&w.app, &w.bob, w.guild.id, w.bob.id, "guild").await), ErrorCode::PermissionDenied);
    assert_eq!(code(transfer(&w.app, &outsider, w.guild.id, w.bob.id, "guild").await), ErrorCode::NotFound);
    assert_eq!(code(transfer(&w.app, &w.alice, w.guild.id, w.bob.id, "nope").await), ErrorCode::InvalidArgument);
    assert_eq!(code(transfer(&w.app, &w.alice, w.guild.id, w.alice.id, "guild").await), ErrorCode::InvalidArgument);
    assert_eq!(code(transfer(&w.app, &w.alice, w.guild.id, outsider.id, "guild").await), ErrorCode::NotFound);
    assert_eq!(code(transfer(&w.app, &w.alice, w.guild.id, 31337, "guild").await), ErrorCode::NotFound);

    let mut stream = subscribe!(w.app, w.bob);
    let guild = transfer(&w.app, &w.alice, w.guild.id, w.bob.id, "guild").await.unwrap();
    assert_eq!(guild.owner_id, w.bob.id);
    let Payload::GuildUpdated(g) = next_payload!(stream) else { panic!("expected GuildUpdated") };
    assert_eq!(g.owner_id, w.bob.id);

    // Bob now has the powers, Alice is an ordinary member (and may leave).
    assert_eq!(code(rotate(&w.app, &w.alice, w.guild.id).await), ErrorCode::PermissionDenied);
    assert_eq!(code(delete_guild(&w.app, &w.alice, w.guild.id, "guild").await), ErrorCode::PermissionDenied);
    assert!(rotate(&w.app, &w.bob, w.guild.id).await.is_ok());
    guilds(&w.app, &w.bob)
        .kick_member(KickMemberRequest { guild_id: w.guild.id, user_id: w.alice.id, ..Default::default() })
        .await
        .unwrap();
    let entries: Vec<String> = app_audit(&w, &admin).await;
    assert!(entries.iter().any(|a| a == "guild.transfer"), "{entries:?}");
    w.app.cleanup().await;
}

async fn app_audit(w: &World, admin: &Account) -> Vec<String> {
    w.app
        .client(AdminServiceClient::new, Some(&admin.token))
        .list_audit_log(ListAuditLogRequest { limit: 200, ..Default::default() })
        .await
        .unwrap()
        .into_owned()
        .entries
        .into_iter()
        .filter(|e| e.target.contains(&format!("guild:{}", w.guild.id)))
        .map(|e| e.action)
        .collect()
}

#[tokio::test]
async fn structure_actions_are_audit_logged() {
    let w = world().await;
    let admin = site_admin(&w.app, "root").await;
    let extra = channel(&w.app, &w.alice, w.guild.id, "extra").await.unwrap();
    delete_channel(&w.app, &w.alice, w.guild.id, extra.id).await.unwrap();
    rotate(&w.app, &w.alice, w.guild.id).await.unwrap();
    transfer(&w.app, &w.alice, w.guild.id, w.bob.id, "guild").await.unwrap();
    delete_guild(&w.app, &w.bob, w.guild.id, "guild").await.unwrap();
    let actions = app_audit(&w, &admin).await;
    for expected in ["channel.delete", "guild.rotate_invite", "guild.transfer", "guild.delete"] {
        assert_eq!(actions.iter().filter(|a| *a == expected).count(), 1, "{expected} in {actions:?}");
    }
    w.app.cleanup().await;
}
