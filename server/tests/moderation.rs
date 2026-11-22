//! Message edit/delete, the message rate limit, kick, ban, unban and leave.
mod common;

use std::time::Duration;

use common::TestApp;
use common::fixtures::*;
use connectrpc::client::HttpClient;
use connectrpc::{ConnectError, ErrorCode};
use thehideout_server::proto::thehideout::v1::event::Payload;
use thehideout_server::proto::thehideout::v1::*;
use tokio::time::{sleep, timeout};

const WAIT: Duration = Duration::from_secs(5);

const MANAGE_MESSAGES: u64 = 2048;
const KICK_MEMBERS: u64 = 4096;
const BAN_MEMBERS: u64 = 8192;

async fn start() -> TestApp {
    TestApp::builder().livekit().heartbeat(Duration::from_millis(200)).start().await
}

macro_rules! subscribe {
    ($app:expr, $who:expr) => {
        $app.client(GatewayServiceClient::new, Some(&$who.token))
            .subscribe(SubscribeRequest::default())
            .await
            .expect("subscribe")
    };
}

/// Next non-heartbeat event payload.
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

/// Every non-heartbeat payload arriving within `window`.
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

async fn edit(app: &TestApp, who: &Account, channel_id: u64, message_id: u64, content: &str) -> Result<Message, ConnectError> {
    Ok(messages(app, who)
        .edit_message(EditMessageRequest { channel_id, message_id, content: content.into(), ..Default::default() })
        .await?
        .into_owned()
        .message
        .unwrap())
}

async fn delete(app: &TestApp, who: &Account, channel_id: u64, message_id: u64) -> Result<(), ConnectError> {
    messages(app, who).delete_message(DeleteMessageRequest { channel_id, message_id, ..Default::default() }).await?;
    Ok(())
}

async fn list(app: &TestApp, who: &Account, channel_id: u64) -> Result<Vec<Message>, ConnectError> {
    Ok(messages(app, who)
        .list_messages(ListMessagesRequest { channel_id, ..Default::default() })
        .await?
        .into_owned()
        .messages)
}

fn code<T: std::fmt::Debug>(result: Result<T, ConnectError>) -> ErrorCode {
    result.expect_err("expected an error").code
}

async fn general(app: &TestApp, who: &Account, guild_id: u64) -> u64 {
    channels(app, who, guild_id).await.unwrap().channels[0].id
}

async fn kick(app: &TestApp, who: &Account, guild_id: u64, user_id: u64) -> Result<(), ConnectError> {
    guilds(app, who).kick_member(KickMemberRequest { guild_id, user_id, ..Default::default() }).await?;
    Ok(())
}

async fn ban(app: &TestApp, who: &Account, guild_id: u64, user_id: u64, reason: &str, purge: Option<u32>) -> Result<(), ConnectError> {
    guilds(app, who)
        .ban_member(BanMemberRequest { guild_id, user_id, reason: reason.into(), delete_message_seconds: purge, ..Default::default() })
        .await?;
    Ok(())
}

async fn try_join(app: &TestApp, who: &Account, guild: &Guild) -> Result<(), ConnectError> {
    guilds(app, who)
        .join_guild(JoinGuildRequest { invite_code: guild.invite_code.clone(), ..Default::default() })
        .await?;
    Ok(())
}

async fn member_ids(app: &TestApp, who: &Account, guild_id: u64) -> Vec<u64> {
    let mut ids: Vec<u64> = guilds(app, who)
        .list_guild_members(ListGuildMembersRequest { guild_id, ..Default::default() })
        .await
        .unwrap()
        .into_owned()
        .members
        .iter()
        .filter_map(|m| m.user.as_option().map(|u| u.id))
        .collect();
    ids.sort_unstable();
    ids
}

/// The audit entries (action, target, reason) whose target mentions `needle`.
async fn audit(app: &TestApp, admin: &Account, needle: &str) -> Vec<(String, String, String)> {
    app.client(AdminServiceClient::new, Some(&admin.token))
        .list_audit_log(ListAuditLogRequest { limit: 200, ..Default::default() })
        .await
        .unwrap()
        .into_owned()
        .entries
        .into_iter()
        .filter(|e| e.target.contains(needle))
        .map(|e| (e.action, e.target, e.reason))
        .collect()
}

/// A guild with `alice` (owner), `bob` and `carol` as plain members.
struct World {
    app: TestApp,
    alice: Account,
    bob: Account,
    carol: Account,
    guild: Guild,
    channel: u64,
}

async fn world() -> World {
    let app = start().await;
    let (alice, bob, carol) = (user(&app, "alice").await, user(&app, "bob").await, user(&app, "carol").await);
    let guild = guild(&app, &alice).await;
    join(&app, &bob, &guild).await;
    join(&app, &carol, &guild).await;
    let channel = general(&app, &alice, guild.id).await;
    World { app, alice, bob, carol, guild, channel }
}

// ------------------------------------------------------------ permissions

#[tokio::test]
async fn new_permission_bits_are_grantable_but_not_in_the_baseline() {
    let w = world().await;
    let role = role(&w.app, &w.alice, w.guild.id, "mods", MANAGE_MESSAGES | KICK_MEMBERS | BAN_MEMBERS).await.unwrap();
    assert_eq!(role.permissions, 2048 | 4096 | 8192);
    // Not held by default: a plain member can do none of it.
    let m = send(&w.app, &w.alice, w.channel, "hello").await.unwrap();
    assert_eq!(code(delete(&w.app, &w.bob, w.channel, m.id).await), ErrorCode::PermissionDenied);
    assert_eq!(code(kick(&w.app, &w.bob, w.guild.id, w.carol.id).await), ErrorCode::PermissionDenied);
    assert_eq!(code(ban(&w.app, &w.bob, w.guild.id, w.carol.id, "", None).await), ErrorCode::PermissionDenied);
    // Unknown bits stay invalid.
    assert!(self::role(&w.app, &w.alice, w.guild.id, "bad", 1 << 40).await.is_err());
    w.app.cleanup().await;
}

// ---------------------------------------------------------------- editing

#[tokio::test]
async fn edit_is_author_only_validated_and_published() {
    let w = world().await;
    let mut stream = subscribe!(w.app, w.bob);
    let m = send(&w.app, &w.alice, w.channel, "first").await.unwrap();
    assert!(matches!(next_payload!(stream), Payload::MessageCreated(_)));

    let edited = edit(&w.app, &w.alice, w.channel, m.id, "second").await.unwrap();
    assert_eq!((edited.content.as_str(), edited.id), ("second", m.id));
    assert!(edited.edited_at.is_set());
    let Payload::MessageUpdated(u) = next_payload!(stream) else { panic!("expected MessageUpdated") };
    let updated = u.message.unwrap();
    assert_eq!((updated.id, updated.content.as_str()), (m.id, "second"));
    assert!(updated.edited_at.is_set());
    let listed = list(&w.app, &w.bob, w.channel).await.unwrap();
    assert_eq!(listed[0].content, "second");
    assert!(listed[0].edited_at.is_set());

    // Same content: nothing changes, nothing is published.
    edit(&w.app, &w.alice, w.channel, m.id, "second").await.unwrap();
    assert!(payloads_within!(stream, Duration::from_millis(500)).is_empty());

    // Others, even the owner of the guild, cannot edit it.
    assert_eq!(code(edit(&w.app, &w.bob, w.channel, m.id, "mine now").await), ErrorCode::PermissionDenied);
    let bobs = send(&w.app, &w.bob, w.channel, "bob's").await.unwrap();
    assert_eq!(code(edit(&w.app, &w.alice, w.channel, bobs.id, "x").await), ErrorCode::PermissionDenied);

    // Validation is send's.
    assert_eq!(code(edit(&w.app, &w.alice, w.channel, m.id, "   ").await), ErrorCode::InvalidArgument);
    assert_eq!(code(edit(&w.app, &w.alice, w.channel, m.id, &"x".repeat(4001)).await), ErrorCode::InvalidArgument);
    assert!(edit(&w.app, &w.alice, w.channel, m.id, &"x".repeat(4000)).await.is_ok());
    // Unknown message / outsider.
    assert_eq!(code(edit(&w.app, &w.alice, w.channel, 12345, "x").await), ErrorCode::NotFound);
    let outsider = user(&w.app, "dave").await;
    assert_eq!(code(edit(&w.app, &outsider, w.channel, m.id, "x").await), ErrorCode::NotFound);
    w.app.cleanup().await;
}

#[tokio::test]
async fn editing_a_deleted_message_does_not_bring_it_back() {
    let w = world().await;
    let m = send(&w.app, &w.alice, w.channel, "gone soon").await.unwrap();
    delete(&w.app, &w.alice, w.channel, m.id).await.unwrap();
    assert_eq!(code(edit(&w.app, &w.alice, w.channel, m.id, "back").await), ErrorCode::NotFound);
    assert!(list(&w.app, &w.alice, w.channel).await.unwrap().is_empty());
    w.app.cleanup().await;
}

#[tokio::test]
async fn editing_in_a_dm_works_for_the_author_only() {
    let app = start().await;
    let (alice, bob) = (user(&app, "alice").await, user(&app, "bob").await);
    let dm = app
        .client(DmServiceClient::new, Some(&alice.token))
        .open_dm(OpenDmRequest { user_id: bob.id, ..Default::default() })
        .await
        .unwrap()
        .into_owned()
        .channel
        .unwrap();
    let m = send(&app, &alice, dm.id, "hi").await.unwrap();
    assert_eq!(edit(&app, &alice, dm.id, m.id, "hello").await.unwrap().content, "hello");
    assert_eq!(code(edit(&app, &bob, dm.id, m.id, "x").await), ErrorCode::PermissionDenied);
    // Deleting: the author only, in DMs.
    assert_eq!(code(delete(&app, &bob, dm.id, m.id).await), ErrorCode::PermissionDenied);
    delete(&app, &alice, dm.id, m.id).await.unwrap();
    assert!(list(&app, &bob, dm.id).await.unwrap().is_empty());
    app.cleanup().await;
}

#[tokio::test]
async fn editing_re_resolves_embeds() {
    // Embeds are off in the default test app: the old ones must still be dropped
    // and the edit published exactly once.
    let w = world().await;
    let m = send(&w.app, &w.alice, w.channel, "see https://example.invalid/a").await.unwrap();
    // A stored embed row (as the pipeline would write it).
    w.app
        .state
        .session
        .execute_unpaged(
            "INSERT INTO message_embeds (channel_id, bucket, message_id, position, url, title, description, site_name, kind) \
             VALUES (?, ?, ?, 0, 'https://example.invalid/a', 't', 'd', 's', 'link')",
            (m.channel_id as i64, thehideout_server::snowflake::bucket_of(m.id), m.id as i64),
        )
        .await
        .unwrap();
    assert_eq!(list(&w.app, &w.alice, w.channel).await.unwrap()[0].embeds.len(), 1);
    edit(&w.app, &w.alice, w.channel, m.id, "no link any more").await.unwrap();
    assert!(list(&w.app, &w.alice, w.channel).await.unwrap()[0].embeds.is_empty());
    w.app.cleanup().await;
}

// --------------------------------------------------------------- deleting

#[tokio::test]
async fn author_and_moderators_can_delete_and_it_is_published() {
    let w = world().await;
    let admin = site_admin(&w.app, "root").await;
    let mods = role(&w.app, &w.alice, w.guild.id, "janitor", MANAGE_MESSAGES).await.unwrap();
    set_roles(&w.app, &w.alice, w.guild.id, w.carol.id, &[mods.id]).await.unwrap();
    let mut stream = subscribe!(w.app, w.bob);

    // Author.
    let m = send(&w.app, &w.bob, w.channel, "mine").await.unwrap();
    assert!(matches!(next_payload!(stream), Payload::MessageCreated(_)));
    delete(&w.app, &w.bob, w.channel, m.id).await.unwrap();
    let Payload::MessageDeleted(d) = next_payload!(stream) else { panic!("expected MessageDeleted") };
    assert_eq!((d.channel_id, d.message_id), (w.channel, m.id));
    assert!(list(&w.app, &w.bob, w.channel).await.unwrap().is_empty());
    assert_eq!(code(delete(&w.app, &w.bob, w.channel, m.id).await), ErrorCode::NotFound);

    // A plain member cannot delete someone else's; MANAGE_MESSAGES and the owner can.
    let m = send(&w.app, &w.alice, w.channel, "alice's").await.unwrap();
    assert_eq!(code(delete(&w.app, &w.bob, w.channel, m.id).await), ErrorCode::PermissionDenied);
    delete(&w.app, &w.carol, w.channel, m.id).await.unwrap();
    let m2 = send(&w.app, &w.bob, w.channel, "bob again").await.unwrap();
    delete(&w.app, &w.alice, w.channel, m2.id).await.unwrap();
    assert!(list(&w.app, &w.alice, w.channel).await.unwrap().is_empty());

    // Outsiders see NotFound; moderator deletes are audit-logged, author deletes are not.
    let outsider = user(&w.app, "dave").await;
    assert_eq!(code(delete(&w.app, &outsider, w.channel, m2.id).await), ErrorCode::NotFound);
    let entries = audit(&w.app, &admin, &format!("guild:{}/channel", w.guild.id)).await;
    let deletes: Vec<_> = entries.iter().filter(|e| e.0 == "message.delete").collect();
    assert_eq!(deletes.len(), 2, "{entries:?}");
    assert!(deletes.iter().any(|e| e.1.ends_with(&format!("message:{}", m.id)) && e.2 == format!("author:{}", w.alice.id)));
    w.app.cleanup().await;
}

#[tokio::test]
async fn deleting_removes_reactions_embeds_and_attachments() {
    let app = TestApp::builder().storage().heartbeat(Duration::from_millis(200)).start().await;
    let (alice, bob) = (user(&app, "alice").await, user(&app, "bob").await);
    let g = guild(&app, &alice).await;
    join(&app, &bob, &g).await;
    let channel = general(&app, &alice, g.id).await;

    // An attachment through the real upload path.
    let data = b"hello attachment".to_vec();
    let upload = reqwest::Client::new()
        .post(format!("{}/attachments?channel_id={channel}&filename=note.txt", app.base_url))
        .header("authorization", format!("Bearer {}", alice.token))
        .header("content-type", "text/plain")
        .body(data)
        .send()
        .await
        .unwrap();
    assert_eq!(upload.status(), 200);
    let pending: serde_json::Value = upload.json().await.unwrap();
    let attachment_id: u64 = pending["id"].as_str().unwrap().parse().unwrap();
    let m = messages(&app, &alice)
        .send_message(SendMessageRequest { channel_id: channel, content: "file".into(), attachment_ids: vec![attachment_id], ..Default::default() })
        .await
        .unwrap()
        .into_owned()
        .message
        .unwrap();
    let key = m.attachments[0].file.clone();
    assert!(app.state.storage.as_ref().unwrap().head(&key).await.unwrap().is_some());

    // A reaction and a stored embed.
    messages(&app, &bob)
        .add_reaction(AddReactionRequest {
            channel_id: channel,
            message_id: m.id,
            emoji: ReactionEmoji { kind: Some(reaction_emoji::Kind::Unicode("👍".into())), ..Default::default() }.into(),
            ..Default::default()
        })
        .await
        .unwrap();
    let bucket = thehideout_server::snowflake::bucket_of(m.id);
    app.state
        .session
        .execute_unpaged(
            "INSERT INTO message_embeds (channel_id, bucket, message_id, position, url, title, kind) VALUES (?, ?, ?, 0, 'https://x.invalid', 't', 'link')",
            (channel as i64, bucket, m.id as i64),
        )
        .await
        .unwrap();

    delete(&app, &alice, channel, m.id).await.unwrap();

    // Rows and objects are gone.
    let count = |table: &'static str| {
        let app = &app;
        async move {
            app.state
                .session
                .execute_unpaged(
                    format!("SELECT message_id FROM {table} WHERE channel_id = ? AND bucket = ? AND message_id = ?"),
                    (channel as i64, bucket, m.id as i64),
                )
                .await
                .unwrap()
                .into_rows_result()
                .unwrap()
                .rows_num()
        }
    };
    for table in ["reactions", "message_embeds", "message_attachments"] {
        assert_eq!(count(table).await, 0, "{table}");
    }
    assert!(app.state.storage.as_ref().unwrap().head(&key).await.unwrap().is_none(), "object deleted");
    let media = reqwest::get(format!("{}/media/{key}", app.base_url)).await.unwrap();
    assert_eq!(media.status(), 404);
    app.cleanup().await;
}

// ------------------------------------------------------------- rate limit

#[tokio::test]
async fn message_rate_limit_has_a_burst_and_a_retry_hint() {
    let app = TestApp::builder().message_rate(3, 0.5).start().await;
    let alice = user(&app, "alice").await;
    let g = guild(&app, &alice).await;
    let channel = general(&app, &alice, g.id).await;
    let m = send(&app, &alice, channel, "1").await.unwrap();
    send(&app, &alice, channel, "2").await.unwrap();
    send(&app, &alice, channel, "3").await.unwrap();
    let err = send(&app, &alice, channel, "4").await.unwrap_err();
    assert_eq!(err.code, ErrorCode::ResourceExhausted);
    let retry: u64 = err.response_headers().get("retry-after").expect("retry-after header").to_str().unwrap().parse().unwrap();
    assert!((1..=3).contains(&retry), "retry-after {retry}");
    assert!(err.message.as_deref().unwrap().contains("try again in"));
    // Edits count against the same limit.
    assert_eq!(edit(&app, &alice, channel, m.id, "edited").await.unwrap_err().code, ErrorCode::ResourceExhausted);
    // Other users are unaffected.
    let bob = user(&app, "bob").await;
    join(&app, &bob, &g).await;
    send(&app, &bob, channel, "mine").await.unwrap();
    // The bucket refills (0.5/s: two seconds buy one message).
    sleep(Duration::from_millis(2100)).await;
    send(&app, &alice, channel, "5").await.unwrap();
    app.cleanup().await;
}

#[tokio::test]
async fn a_retried_send_with_the_same_nonce_returns_the_stored_message() {
    let app = TestApp::builder().message_rate(2, 0.01).start().await;
    let alice = user(&app, "alice").await;
    let g = guild(&app, &alice).await;
    let channel = general(&app, &alice, g.id).await;
    let send_with = |nonce: &str, content: &str| {
        let client = messages(&app, &alice);
        let req = SendMessageRequest { channel_id: channel, content: content.into(), nonce: Some(nonce.into()), ..Default::default() };
        async move { client.send_message(req).await.map(|r| r.into_owned().message.unwrap()) }
    };
    let first = send_with("n-1", "hello").await.unwrap();
    // The retries cost no rate-limit tokens and post nothing.
    for _ in 0..5 {
        let again = send_with("n-1", "hello").await.unwrap();
        assert_eq!((again.id, again.nonce.as_str()), (first.id, "n-1"));
    }
    assert_eq!(list(&app, &alice, channel).await.unwrap().len(), 1);
    // A different nonce is a new message (and the second token).
    let second = send_with("n-2", "hello").await.unwrap();
    assert_ne!(second.id, first.id);
    app.cleanup().await;
}

// ------------------------------------------------------------------ kick

#[tokio::test]
async fn kick_checks_permission_and_hierarchy() {
    let w = world().await;
    // Roles: "mod" (kicks) above "junior".
    let moderator = role(&w.app, &w.alice, w.guild.id, "mod", KICK_MEMBERS).await.unwrap();
    let junior = role(&w.app, &w.alice, w.guild.id, "junior", 0).await.unwrap();
    let ordered = list_roles(&w.app, &w.alice, w.guild.id).await.unwrap().roles;
    assert_eq!(ordered.iter().map(|r| r.id).collect::<Vec<_>>(), vec![moderator.id, junior.id]);
    set_roles(&w.app, &w.alice, w.guild.id, w.bob.id, &[moderator.id]).await.unwrap();
    set_roles(&w.app, &w.alice, w.guild.id, w.carol.id, &[junior.id]).await.unwrap();

    // Without KICK_MEMBERS: denied. Outsiders: not found.
    assert_eq!(code(kick(&w.app, &w.carol, w.guild.id, w.bob.id).await), ErrorCode::PermissionDenied);
    let outsider = user(&w.app, "dave").await;
    assert_eq!(code(kick(&w.app, &outsider, w.guild.id, w.bob.id).await), ErrorCode::NotFound);
    // Nobody but the owner acts on the owner; nobody kicks themself.
    assert_eq!(code(kick(&w.app, &w.bob, w.guild.id, w.alice.id).await), ErrorCode::PermissionDenied);
    assert_eq!(code(kick(&w.app, &w.bob, w.guild.id, w.bob.id).await), ErrorCode::InvalidArgument);
    // Peers (same top role) are protected; non-members are not found.
    let eve = user(&w.app, "eve").await;
    join(&w.app, &eve, &w.guild).await;
    set_roles(&w.app, &w.alice, w.guild.id, eve.id, &[moderator.id]).await.unwrap();
    assert_eq!(code(kick(&w.app, &w.bob, w.guild.id, eve.id).await), ErrorCode::PermissionDenied);
    assert_eq!(code(kick(&w.app, &w.bob, w.guild.id, outsider.id).await), ErrorCode::NotFound);

    // A moderator kicks someone below, who can rejoin.
    kick(&w.app, &w.bob, w.guild.id, w.carol.id).await.unwrap();
    assert!(!member_ids(&w.app, &w.alice, w.guild.id).await.contains(&w.carol.id));
    assert_eq!(code(channels(&w.app, &w.carol, w.guild.id).await), ErrorCode::NotFound);
    try_join(&w.app, &w.carol, &w.guild).await.expect("kicked members may rejoin");
    // Roles are gone with the membership.
    let carol_member = guilds(&w.app, &w.alice)
        .list_guild_members(ListGuildMembersRequest { guild_id: w.guild.id, ..Default::default() })
        .await
        .unwrap()
        .into_owned()
        .members
        .into_iter()
        .find(|m| m.user.as_option().is_some_and(|u| u.id == w.carol.id))
        .unwrap();
    assert!(carol_member.role_ids.is_empty());

    // The owner can kick a moderator; ADMINISTRATOR implies the permission.
    let admins = role(&w.app, &w.alice, w.guild.id, "admins", ADMINISTRATOR).await.unwrap();
    set_roles(&w.app, &w.alice, w.guild.id, w.carol.id, &[admins.id]).await.unwrap();
    // `admins` was created last, so it sits below `mod`: carol outranks nobody with `mod`.
    assert_eq!(code(kick(&w.app, &w.carol, w.guild.id, eve.id).await), ErrorCode::PermissionDenied);
    kick(&w.app, &w.alice, w.guild.id, eve.id).await.unwrap();
    w.app.cleanup().await;
}

#[tokio::test]
async fn kick_publishes_to_the_right_audiences_and_closes_the_guild_for_the_removed() {
    let w = world().await;
    let mut alice_stream = subscribe!(w.app, w.alice);
    let mut carol_stream = subscribe!(w.app, w.carol);
    let mut bob_stream = subscribe!(w.app, w.bob);

    kick(&w.app, &w.alice, w.guild.id, w.carol.id).await.unwrap();
    // Remaining members hear who left and why.
    for stream in [&mut alice_stream, &mut bob_stream] {
        let Payload::GuildMemberRemoved(r) = next_payload!(stream) else { panic!("expected GuildMemberRemoved") };
        assert_eq!((r.guild_id, r.user_id), (w.guild.id, w.carol.id));
        assert_eq!(r.reason.as_known(), Some(MemberRemovalReason::MEMBER_REMOVAL_REASON_KICKED));
    }
    // The removed user is told the guild is gone (possibly after seeing the removal itself).
    let mut got_removed = false;
    for _ in 0..2 {
        match next_payload!(carol_stream) {
            Payload::GuildRemoved(r) => {
                assert_eq!(r.guild_id, w.guild.id);
                got_removed = true;
                break;
            }
            Payload::GuildMemberRemoved(_) => {}
            other => panic!("unexpected {other:?}"),
        }
    }
    assert!(got_removed, "guild_removed reaches the removed user");

    // From now on the removed user's open stream hears nothing of the guild.
    send(&w.app, &w.alice, w.channel, "after the kick").await.unwrap();
    channel(&w.app, &w.alice, w.guild.id, "new").await.unwrap();
    assert!(payloads_within!(carol_stream, Duration::from_millis(700)).is_empty(), "removed member still receives guild events");
    // ...while the others do.
    let kinds: Vec<_> = payloads_within!(bob_stream, Duration::from_millis(500));
    assert!(kinds.iter().any(|p| matches!(p, Payload::MessageCreated(_))));
    // Other guilds' events still arrive on the removed user's stream.
    guild(&w.app, &w.carol).await;
    assert!(payloads_within!(carol_stream, Duration::from_millis(500)).iter().any(|p| matches!(p, Payload::GuildAdded(_))));
    w.app.cleanup().await;
}

#[tokio::test]
async fn kick_clears_voice_state() {
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
        .client(VoiceServiceClient::new, Some(&w.carol.token))
        .join_voice(JoinVoiceRequest { channel_id: voice_ch.id, ..Default::default() })
        .await
        .unwrap();
    assert!(w.app.state.voice.get(w.carol.id).is_some());
    let mut stream = subscribe!(w.app, w.bob);
    let _ = payloads_within!(stream, Duration::from_millis(300));

    kick(&w.app, &w.alice, w.guild.id, w.carol.id).await.unwrap();
    assert!(w.app.state.voice.get(w.carol.id).is_none());
    assert!(w.app.livekit.as_ref().unwrap().removed(&format!("voice-{}", voice_ch.id), w.carol.id));
    let events = payloads_within!(stream, Duration::from_millis(500));
    assert!(events.iter().any(|p| matches!(p, Payload::VoiceStateUpdate(v) if v.user_id == w.carol.id && v.state.as_option().is_none())));
    w.app.cleanup().await;
}

#[tokio::test]
async fn kick_ban_unban_and_leave_are_audit_logged_where_specified() {
    let w = world().await;
    let admin = site_admin(&w.app, "root").await;
    kick(&w.app, &w.alice, w.guild.id, w.bob.id).await.unwrap();
    ban(&w.app, &w.alice, w.guild.id, w.carol.id, "spam", None).await.unwrap();
    guilds(&w.app, &w.alice)
        .unban_member(UnbanMemberRequest { guild_id: w.guild.id, user_id: w.carol.id, ..Default::default() })
        .await
        .unwrap();
    let entries = audit(&w.app, &admin, &format!("guild:{}/user", w.guild.id)).await;
    let actions: Vec<&str> = entries.iter().map(|e| e.0.as_str()).collect();
    assert_eq!(actions.iter().filter(|a| **a == "guild.kick").count(), 1, "{entries:?}");
    assert_eq!(actions.iter().filter(|a| **a == "guild.ban").count(), 1);
    assert_eq!(actions.iter().filter(|a| **a == "guild.unban").count(), 1);
    let ban = entries.iter().find(|e| e.0 == "guild.ban").unwrap();
    assert_eq!((ban.1.as_str(), ban.2.as_str()), (format!("guild:{}/user:{}", w.guild.id, w.carol.id).as_str(), "spam"));
    w.app.cleanup().await;
}

// ------------------------------------------------------------------- ban

#[tokio::test]
async fn ban_blocks_rejoining_until_unbanned() {
    let w = world().await;
    let mods = role(&w.app, &w.alice, w.guild.id, "bouncer", BAN_MEMBERS).await.unwrap();
    set_roles(&w.app, &w.alice, w.guild.id, w.bob.id, &[mods.id]).await.unwrap();

    // Permission (BAN, not just KICK), hierarchy and self.
    let kickers = role(&w.app, &w.alice, w.guild.id, "kickers", KICK_MEMBERS).await.unwrap();
    set_roles(&w.app, &w.alice, w.guild.id, w.carol.id, &[kickers.id]).await.unwrap();
    assert_eq!(code(ban(&w.app, &w.carol, w.guild.id, w.bob.id, "", None).await), ErrorCode::PermissionDenied);
    assert_eq!(code(ban(&w.app, &w.bob, w.guild.id, w.alice.id, "", None).await), ErrorCode::PermissionDenied);
    assert_eq!(code(ban(&w.app, &w.bob, w.guild.id, w.bob.id, "", None).await), ErrorCode::InvalidArgument);
    assert_eq!(code(ban(&w.app, &w.bob, w.guild.id, 999_999, "", None).await), ErrorCode::NotFound);
    assert_eq!(code(ban(&w.app, &w.alice, w.guild.id, w.carol.id, &"x".repeat(513), None).await), ErrorCode::InvalidArgument);
    assert_eq!(code(ban(&w.app, &w.alice, w.guild.id, w.carol.id, "", Some(604_801)).await), ErrorCode::InvalidArgument);

    let mut carol_stream = subscribe!(w.app, w.carol);
    let mut alice_stream = subscribe!(w.app, w.alice);
    ban(&w.app, &w.bob, w.guild.id, w.carol.id, "being rude", None).await.unwrap();
    let Payload::GuildMemberRemoved(r) = next_payload!(alice_stream) else { panic!("expected GuildMemberRemoved") };
    assert_eq!(r.reason.as_known(), Some(MemberRemovalReason::MEMBER_REMOVAL_REASON_BANNED));
    loop {
        if matches!(next_payload!(carol_stream), Payload::GuildRemoved(_)) {
            break;
        }
    }

    // Rejoining with the code fails, naming the ban; the guild stays closed.
    assert_eq!(code(try_join(&w.app, &w.carol, &w.guild).await), ErrorCode::PermissionDenied);
    assert_eq!(code(channels(&w.app, &w.carol, w.guild.id).await), ErrorCode::NotFound);
    // The email-invite path goes through the same admission and refuses too.
    let svc = thehideout_server::services::GuildSvc::new(w.app.state.clone());
    assert_eq!(svc.admit(&w.guild, w.carol.id).await.unwrap_err().code, ErrorCode::PermissionDenied);
    // An invite for the banned user's email cannot be made either (they have an account).
    let invite = invites(&w.app, Some(&w.alice.token))
        .create_invite(CreateInviteRequest { email: email_of(&w.app, "carol"), guild_id: Some(w.guild.id), ..Default::default() })
        .await;
    assert_eq!(code(invite), ErrorCode::AlreadyExists);
    assert!(!member_ids(&w.app, &w.alice, w.guild.id).await.contains(&w.carol.id));

    // The ban list shows it (BAN_MEMBERS only), newest first.
    let bans = guilds(&w.app, &w.bob).list_bans(ListBansRequest { guild_id: w.guild.id, ..Default::default() }).await.unwrap().into_owned().bans;
    assert_eq!(bans.len(), 1);
    assert_eq!(
        (bans[0].user.as_option().unwrap().id, bans[0].banned_by, bans[0].reason.as_str()),
        (w.carol.id, w.bob.id, "being rude")
    );
    assert!(bans[0].created_at.is_set());
    let denied = guilds(&w.app, &w.carol).list_bans(ListBansRequest { guild_id: w.guild.id, ..Default::default() }).await;
    assert_eq!(code(denied), ErrorCode::NotFound);

    // Unban: not-found for the unbanned, then rejoin works.
    let unban = |who: &Account, user_id: u64| {
        let client = guilds(&w.app, who);
        let guild_id = w.guild.id;
        async move { client.unban_member(UnbanMemberRequest { guild_id, user_id, ..Default::default() }).await.map(|_| ()) }
    };
    assert_eq!(code(unban(&w.carol, w.carol.id).await), ErrorCode::NotFound);
    unban(&w.bob, w.carol.id).await.unwrap();
    assert_eq!(code(unban(&w.bob, w.carol.id).await), ErrorCode::NotFound);
    try_join(&w.app, &w.carol, &w.guild).await.expect("rejoin after unban");
    assert!(guilds(&w.app, &w.bob).list_bans(ListBansRequest { guild_id: w.guild.id, ..Default::default() }).await.unwrap().into_owned().bans.is_empty());
    w.app.cleanup().await;
}

#[tokio::test]
async fn banning_a_non_member_pre_bans_them() {
    let w = world().await;
    let dave = user(&w.app, "dave").await;
    ban(&w.app, &w.alice, w.guild.id, dave.id, "", None).await.unwrap();
    assert_eq!(code(try_join(&w.app, &dave, &w.guild).await), ErrorCode::PermissionDenied);
    w.app.cleanup().await;
}

#[tokio::test]
async fn ban_can_delete_recent_messages_in_the_background() {
    let w = world().await;
    let second = channel(&w.app, &w.alice, w.guild.id, "second").await.unwrap();
    let old = send(&w.app, &w.carol, w.channel, "recent 0").await.unwrap();
    // Backdate `old`: re-key it to a time well before the purge window.
    let long_ago = thehideout_server::snowflake::make_id(
        thehideout_server::snowflake::millis_of(old.id) - 3 * 24 * 3600 * 1000,
        1,
        0,
    );
    w.app
        .state
        .session
        .execute_unpaged(
            "INSERT INTO messages (channel_id, bucket, id, guild_id, author_id, content, nonce) VALUES (?, ?, ?, ?, ?, 'ancient', '')",
            (w.channel as i64, thehideout_server::snowflake::bucket_of(long_ago), long_ago as i64, w.guild.id as i64, w.carol.id as i64),
        )
        .await
        .unwrap();
    send(&w.app, &w.carol, w.channel, "recent 1").await.unwrap();
    send(&w.app, &w.carol, second.id, "recent 2").await.unwrap();
    let keep = send(&w.app, &w.bob, w.channel, "bob stays").await.unwrap();
    let mut stream = subscribe!(w.app, w.alice);

    ban(&w.app, &w.alice, w.guild.id, w.carol.id, "", Some(3600)).await.unwrap();

    // The recent messages go (the events say so); the ancient one and bob's stay.
    let mut deleted = 0;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while deleted < 3 && tokio::time::Instant::now() < deadline {
        deleted += payloads_within!(stream, Duration::from_millis(300)).iter().filter(|p| matches!(p, Payload::MessageDeleted(_))).count();
    }
    assert_eq!(deleted, 3, "recent 0 + recent 1 + recent 2");
    let remaining = list(&w.app, &w.alice, w.channel).await.unwrap();
    let contents: Vec<&str> = remaining.iter().map(|m| m.content.as_str()).collect();
    assert_eq!(contents, vec!["bob stays", "ancient"]);
    assert_eq!(remaining[0].id, keep.id);
    assert!(list(&w.app, &w.alice, second.id).await.unwrap().is_empty());
    w.app.cleanup().await;
}

// ----------------------------------------------------------------- leave

#[tokio::test]
async fn leaving_works_for_members_but_not_for_the_owner() {
    let w = world().await;
    let mut alice_stream = subscribe!(w.app, w.alice);
    let mut bob_stream = subscribe!(w.app, w.bob);

    assert_eq!(
        code(guilds(&w.app, &w.alice).leave_guild(LeaveGuildRequest { guild_id: w.guild.id, ..Default::default() }).await),
        ErrorCode::FailedPrecondition
    );
    let outsider = user(&w.app, "dave").await;
    assert_eq!(
        code(guilds(&w.app, &outsider).leave_guild(LeaveGuildRequest { guild_id: w.guild.id, ..Default::default() }).await),
        ErrorCode::NotFound
    );

    guilds(&w.app, &w.bob).leave_guild(LeaveGuildRequest { guild_id: w.guild.id, ..Default::default() }).await.unwrap();
    let Payload::GuildMemberRemoved(r) = next_payload!(alice_stream) else { panic!("expected GuildMemberRemoved") };
    assert_eq!((r.user_id, r.reason.as_known()), (w.bob.id, Some(MemberRemovalReason::MEMBER_REMOVAL_REASON_LEFT)));
    loop {
        if let Payload::GuildRemoved(r) = next_payload!(bob_stream) {
            assert_eq!(r.guild_id, w.guild.id);
            break;
        }
    }
    let mine = guilds(&w.app, &w.bob).list_my_guilds(ListMyGuildsRequest::default()).await.unwrap().into_owned().guilds;
    assert!(mine.iter().all(|g| g.id != w.guild.id));
    assert_eq!(code(send(&w.app, &w.bob, w.channel, "x").await), ErrorCode::NotFound);
    // Leaving again is not found; the invite code still works.
    assert_eq!(
        code(guilds(&w.app, &w.bob).leave_guild(LeaveGuildRequest { guild_id: w.guild.id, ..Default::default() }).await),
        ErrorCode::NotFound
    );
    try_join(&w.app, &w.bob, &w.guild).await.unwrap();
    // The stream, which stopped following the guild, follows it again after rejoining.
    let _ = payloads_within!(bob_stream, Duration::from_millis(300));
    send(&w.app, &w.alice, w.channel, "welcome back").await.unwrap();
    let events = payloads_within!(bob_stream, Duration::from_millis(500));
    assert!(events.iter().any(|p| matches!(p, Payload::MessageCreated(_))));
    w.app.cleanup().await;
}
