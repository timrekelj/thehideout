//! Account deletion (anonymizing), data export and sliding sessions, against
//! the real database and object store from `docker-compose.yml`.
mod common;

use std::time::Duration;

use chrono::Utc;
use common::TestApp;
use common::fixtures::*;
use common::http::{self, detail};
use connectrpc::client::HttpClient;
use connectrpc::{ConnectError, ErrorCode};
use thehideout_server::convert::id_to_db;
use thehideout_server::lifecycle::{self, STEPS, Step};
use thehideout_server::proto::thehideout::v1::event::Payload;
use thehideout_server::proto::thehideout::v1::*;
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(10);

fn code<T>(r: Result<T, ConnectError>) -> ErrorCode {
    r.err().expect("expected an error").code
}

fn messages(app: &TestApp, who: &Account) -> MessageServiceClient<HttpClient> {
    app.client(MessageServiceClient::new, Some(&who.token))
}

async fn send(app: &TestApp, who: &Account, channel_id: u64, text: &str, attachments: &[u64]) -> Message {
    messages(app, who)
        .send_message(SendMessageRequest {
            channel_id,
            content: text.into(),
            attachment_ids: attachments.to_vec(),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_owned()
        .message
        .unwrap()
}

async fn list(app: &TestApp, who: &Account, channel_id: u64) -> Vec<Message> {
    messages(app, who)
        .list_messages(ListMessagesRequest { channel_id, limit: 100, ..Default::default() })
        .await
        .unwrap()
        .into_owned()
        .messages
}

fn thumbs() -> ReactionEmoji {
    ReactionEmoji { kind: Some(reaction_emoji::Kind::Unicode("👍".into())), ..Default::default() }
}

async fn react(app: &TestApp, who: &Account, channel_id: u64, message_id: u64) {
    messages(app, who)
        .add_reaction(AddReactionRequest { channel_id, message_id, emoji: thumbs().into(), ..Default::default() })
        .await
        .unwrap();
}

async fn upload(app: &TestApp, who: &Account, channel_id: u64, name: &str) -> u64 {
    let reply = http::request(
        app,
        "POST",
        &format!("/attachments?channel_id={channel_id}&filename={name}"),
        &[("Content-Type", "application/octet-stream".to_owned()), ("Authorization", format!("Bearer {}", who.token))],
        b"some file contents",
    )
    .await;
    assert_eq!(reply.status, 200, "{}", reply.text());
    let json: serde_json::Value = serde_json::from_slice(&reply.body).unwrap();
    json["id"].as_str().unwrap().parse().unwrap()
}

async fn state_of(app: &TestApp, id: u64) -> Option<String> {
    thehideout_server::accounts::load_any(&app.state, id).await.unwrap().and_then(|a| a.deletion_state)
}

async fn wait_deleted(app: &TestApp, id: u64) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while state_of(app, id).await.as_deref() != Some("deleted") {
        assert!(std::time::Instant::now() < deadline, "deletion did not finish");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn count(app: &TestApp, cql: &str, id: u64) -> usize {
    app.state
        .session
        .execute_unpaged(cql.to_owned(), (id_to_db(id),))
        .await
        .unwrap()
        .into_rows_result()
        .unwrap()
        .rows_num()
}

fn delete_req(password: Option<&str>) -> DeleteAccountRequest {
    DeleteAccountRequest { password: password.map(str::to_owned), ..Default::default() }
}

#[tokio::test]
async fn deletion_needs_credentials_and_no_owned_guilds() {
    let app = TestApp::start().await;
    let owner = user(&app, "owner").await;
    let heir = user(&app, "heir").await;
    let g = guild(&app, &owner).await;
    join(&app, &heir, &g).await;
    let client = auth(&app, Some(&owner.token));

    // Owning a guild blocks deletion and says which.
    let err = client.delete_account(delete_req(Some(PASSWORD))).await.err().unwrap();
    assert_eq!(err.code, ErrorCode::FailedPrecondition);
    let owned: OwnedGuilds = detail(&err, "OwnedGuilds");
    assert_eq!((owned.guilds.len(), owned.guilds[0].id, owned.guilds[0].member_count), (1, g.id, 2));
    assert_eq!(state_of(&app, owner.id).await, None);

    guilds(&app, &owner)
        .transfer_ownership(TransferOwnershipRequest { guild_id: g.id, user_id: heir.id, confirm_name: g.name.clone(), ..Default::default() })
        .await
        .unwrap();
    // Wrong or missing credentials.
    assert_eq!(code(client.delete_account(delete_req(Some("wrong password"))).await), ErrorCode::PermissionDenied);
    assert_eq!(code(client.delete_account(delete_req(None)).await), ErrorCode::InvalidArgument);
    assert_eq!(state_of(&app, owner.id).await, None);
    client.delete_account(delete_req(Some(PASSWORD))).await.unwrap();
    // Unusable at once.
    assert_eq!(code(client.get_me(GetMeRequest::default()).await), ErrorCode::Unauthenticated);
    assert_eq!(
        code(auth(&app, None).login(login_req(&app.name("owner"), PASSWORD, "x")).await),
        ErrorCode::Unauthenticated
    );
    wait_deleted(&app, owner.id).await;
    // Rate limited: further attempts by the same (now gone) user do not get past authentication.
    assert_eq!(code(client.delete_account(delete_req(Some(PASSWORD))).await), ErrorCode::Unauthenticated);
}

#[tokio::test]
async fn passwordless_accounts_confirm_with_an_emailed_code_and_totp_is_required_when_on() {
    let app = TestApp::start().await;
    let name = app.name("nopass");
    let email = format!("{name}@example.test");
    let account = thehideout_server::accounts::create_user(
        &app.state,
        thehideout_server::accounts::NewUser { username: &name, password: None, email: Some(&email) },
    )
    .await
    .unwrap();
    let token = thehideout_server::auth::create_session(&app.state, account.id, "test").await.unwrap();
    let client = auth(&app, Some(&token));

    assert_eq!(
        code(client.delete_account(DeleteAccountRequest { email_code: None, ..Default::default() }).await),
        ErrorCode::InvalidArgument
    );
    client.request_account_deletion_code(RequestAccountDeletionCodeRequest::default()).await.unwrap();
    let mail = wait_mail(&app, &email, 0, "Confirm deleting").await;
    let good = six_digit_code(&mail.text);
    let wrong = if good == "000000" { "000001" } else { "000000" };
    let (c, detail) = code_error(client.delete_account(DeleteAccountRequest { email_code: Some(wrong.into()), ..Default::default() }).await);
    assert_eq!((c, detail.map(|d| d.0)), (ErrorCode::InvalidArgument, Some(CodeErrorReason::CODE_ERROR_REASON_WRONG)));
    assert_eq!(state_of(&app, account.id).await, None);

    // With an authenticator app, the code is needed as well.
    let totp_user = user(&app, "withtotp").await;
    let tc = auth(&app, Some(&totp_user.token));
    let secret = tc.begin_totp_setup(BeginTotpSetupRequest::default()).await.unwrap().into_owned().secret;
    let now = Utc::now().timestamp() as u64;
    let step = thehideout_server::codes::TOTP_STEP_SECS;
    tc.confirm_totp(ConfirmTotpRequest { code: thehideout_server::codes::totp_now(&secret, now - step), ..Default::default() })
        .await
        .unwrap();
    assert_eq!(code(tc.delete_account(delete_req(Some(PASSWORD))).await), ErrorCode::InvalidArgument);
    let (c, _) = code_error(
        tc.delete_account(DeleteAccountRequest { password: Some(PASSWORD.into()), totp_code: Some("000000".into()), ..Default::default() }).await,
    );
    assert_eq!(c, ErrorCode::InvalidArgument);
    assert_eq!(state_of(&app, totp_user.id).await, None);
    tc.delete_account(DeleteAccountRequest {
        password: Some(PASSWORD.into()),
        totp_code: Some(thehideout_server::codes::totp_now(&secret, now + step)),
        ..Default::default()
    })
    .await
    .unwrap();
    wait_deleted(&app, totp_user.id).await;

    // The emailed code finally deletes the passwordless account.
    client.delete_account(DeleteAccountRequest { email_code: Some(good), ..Default::default() }).await.unwrap();
    wait_deleted(&app, account.id).await;
}

#[tokio::test]
async fn deletion_anonymizes_everywhere() {
    let app = TestApp::builder().storage().livekit().heartbeat(Duration::from_millis(200)).start().await;
    let admin = site_admin(&app, "admin").await;
    let alice = user(&app, "alice").await;
    let bob = user(&app, "bob").await;
    let g = guild(&app, &bob).await;
    join(&app, &alice, &g).await;
    let ch = channel(&app, &bob, g.id, "talk").await.unwrap();

    // Alice's footprint: a message with an attachment, a reaction on bob's message, a DM,
    // a pending upload, a pending invite and a custom state in voice.
    let bobs = send(&app, &bob, ch.id, "bob's message", &[]).await;
    react(&app, &alice, ch.id, bobs.id).await;
    let file = upload(&app, &alice, ch.id, "doc.bin").await;
    let mine = send(&app, &alice, ch.id, "alice's message", &[file]).await;
    assert_eq!(mine.attachments.len(), 1);
    let file_key = mine.attachments[0].file.clone();
    assert!(app.state.storage.as_ref().unwrap().head(&file_key).await.unwrap().is_some());
    let dm = app
        .client(DmServiceClient::new, Some(&alice.token))
        .open_dm(OpenDmRequest { user_id: bob.id, ..Default::default() })
        .await
        .unwrap()
        .into_owned()
        .channel
        .unwrap();
    send(&app, &alice, dm.id, "hi bob", &[]).await;
    send(&app, &bob, dm.id, "hi alice", &[]).await;
    let pending_upload = upload(&app, &alice, ch.id, "never-sent.bin").await;
    let _ = pending_upload;
    let invite_email = format!("{}@invitee.test", app.name("friend"));
    let invite = invites(&app, Some(&alice.token))
        .create_invite(CreateInviteRequest { email: invite_email.clone(), guild_id: Some(g.id), ..Default::default() })
        .await;
    // Alice may not invite (no INVITE_PEOPLE); the admin invite is not hers either. Use bob's role grant.
    assert!(invite.is_err());
    let r = role(&app, &bob, g.id, "inviters", INVITE_PEOPLE).await.unwrap();
    set_roles(&app, &bob, g.id, alice.id, &[r.id]).await.unwrap();
    let invite = invites(&app, Some(&alice.token))
        .create_invite(CreateInviteRequest { email: invite_email, guild_id: Some(g.id), ..Default::default() })
        .await
        .unwrap()
        .into_owned()
        .invite
        .unwrap();
    app.livekit.as_ref().unwrap();

    let mut bob_stream = app
        .client(GatewayServiceClient::new, Some(&bob.token))
        .subscribe(SubscribeRequest::default())
        .await
        .unwrap();

    auth(&app, Some(&alice.token)).delete_account(delete_req(Some(PASSWORD))).await.unwrap();
    wait_deleted(&app, alice.id).await;

    // Bob's stream: the anonymized user, the departure, the reaction and attachment cleanup.
    let (mut saw_user, mut saw_removed) = (false, false);
    let deadline = tokio::time::Instant::now() + WAIT;
    while !(saw_user && saw_removed) && tokio::time::Instant::now() < deadline {
        let Ok(Ok(Some(msg))) = timeout(Duration::from_millis(500), bob_stream.message()).await else { continue };
        match msg.to_owned_message().event.into_option().and_then(|e| e.payload) {
            Some(Payload::UserUpdated(u)) if u.id == alice.id => {
                assert!(u.deleted && u.username == "Deleted user" && u.avatar.is_empty() && u.email.is_none());
                saw_user = true;
            }
            Some(Payload::GuildMemberRemoved(r)) if r.user_id == alice.id => {
                assert_eq!(r.reason, MemberRemovalReason::MEMBER_REMOVAL_REASON_LEFT);
                saw_removed = true;
            }
            _ => {}
        }
    }
    assert!(saw_user && saw_removed, "user_updated and guild_member_removed arrive");

    // Credentials and sessions are gone, the farewell mail went out.
    assert_eq!(code(auth(&app, Some(&alice.token)).get_me(GetMeRequest::default()).await), ErrorCode::Unauthenticated);
    assert_eq!(code(auth(&app, None).login(login_req(&app.name("alice"), PASSWORD, "x")).await), ErrorCode::Unauthenticated);
    wait_mail(&app, &email_of(&app, "alice"), 0, "deleted").await;
    for cql in [
        "SELECT session_id FROM sessions_by_user WHERE user_id = ?",
        "SELECT dm_id FROM dms_by_user WHERE user_id = ?",
        "SELECT guild_id FROM guilds_by_user WHERE user_id = ?",
        "SELECT key FROM pending_attachments WHERE uploader = ?",
        "SELECT code_hash FROM recovery_codes WHERE user_id = ?",
    ] {
        assert_eq!(count(&app, cql, alice.id).await, 0, "{cql}");
    }

    // The tombstone: no name, email, avatar or password; the username is free.
    let account = thehideout_server::accounts::load_any(&app.state, alice.id).await.unwrap().unwrap();
    assert_eq!(account.username, "Deleted user");
    assert!(account.email.is_none() && account.password_hash.is_none() && account.avatar.as_deref().unwrap_or("").is_empty());
    assert!(thehideout_server::accounts::id_by_username(&app.state, &app.name("alice")).await.unwrap().is_none());
    assert!(thehideout_server::accounts::id_by_email(&app.state, &email_of(&app, "alice")).await.unwrap().is_none());

    // Messages stay with their text; the attachment is removed, the file deleted; the author is "Deleted user".
    let seen = list(&app, &bob, ch.id).await;
    let kept = seen.iter().find(|m| m.id == mine.id).expect("message stays");
    assert_eq!((kept.content.as_str(), kept.author_id), ("alice's message", alice.id));
    assert!(kept.attachments.len() == 1 && kept.attachments[0].removed && kept.attachments[0].file.is_empty());
    assert!(app.state.storage.as_ref().unwrap().head(&file_key).await.unwrap().is_none());
    let bobs_now = seen.iter().find(|m| m.id == bobs.id).unwrap();
    assert!(bobs_now.reactions.is_empty(), "alice's reaction is gone");
    let author = thehideout_server::access::find_user(&app.state, id_to_db(kept.author_id)).await.unwrap().unwrap();
    assert!(author.deleted && author.username == "Deleted user");

    // Membership, DMs: bob keeps the conversation with "Deleted user".
    let members = guilds(&app, &bob)
        .list_guild_members(ListGuildMembersRequest { guild_id: g.id, ..Default::default() })
        .await
        .unwrap()
        .into_owned();
    assert!(!members.users.iter().any(|u| u.id == alice.id));
    let dms = app.client(DmServiceClient::new, Some(&bob.token)).list_dms(ListDmsRequest::default()).await.unwrap().into_owned().channels;
    let dm_now = dms.iter().find(|c| c.id == dm.id).expect("bob keeps the DM");
    let other = dm_now.participants.iter().find(|u| u.id == alice.id).expect("participant stays");
    assert!(other.deleted && other.username == "Deleted user");
    let dm_texts: Vec<String> = list(&app, &bob, dm.id).await.into_iter().map(|m| m.content).collect();
    assert!(dm_texts.contains(&"hi bob".to_owned()) && dm_texts.contains(&"hi alice".to_owned()));
    // Nobody can open a new DM with a deleted user.
    assert_eq!(
        code(app.client(DmServiceClient::new, Some(&bob.token)).open_dm(OpenDmRequest { user_id: alice.id, ..Default::default() }).await),
        ErrorCode::NotFound
    );

    // The pending invite was withdrawn.
    let listed = invites(&app, Some(&admin.token))
        .list_invites(ListInvitesRequest::default())
        .await
        .unwrap()
        .into_owned()
        .invites;
    assert_eq!(listed.iter().find(|i| i.id == invite.id).unwrap().status, InviteStatus::INVITE_STATUS_REVOKED);

    // The audit entry names only the user id.
    let entries = app
        .client(AdminServiceClient::new, Some(&admin.token))
        .list_audit_log(ListAuditLogRequest { limit: 200, ..Default::default() })
        .await
        .unwrap()
        .into_owned()
        .entries;
    let entry = entries.iter().find(|e| e.action == "account.delete" && e.target == format!("user:{}", alice.id)).expect("audited");
    assert_eq!(entry.reason, "");

    // The username can be registered again, by a new invite.
    let email = format!("{}@invitee.test", app.name("reborn"));
    let link = invites(&app, Some(&admin.token))
        .create_invite(CreateInviteRequest { email, ..Default::default() })
        .await
        .unwrap()
        .into_owned()
        .invite
        .unwrap()
        .link
        .unwrap();
    let accepted = invites(&app, None)
        .accept_invite(AcceptInviteRequest {
            token_or_code: link.rsplit('/').next().unwrap().to_owned(),
            username: app.name("alice"),
            password: Some(PASSWORD.into()),
            device_name: "x".into(),
            accepted_terms_version: thehideout_server::policy::LEGAL_VERSION.into(),
            age_confirmed: true,
            ..Default::default()
        })
        .await
        .unwrap()
        .into_owned();
    let new_alice = accepted.user.as_option().unwrap();
    assert_ne!(new_alice.id, alice.id);
    assert_eq!(new_alice.username, app.name("alice"));
    app.cleanup().await;
}

#[tokio::test]
async fn an_interrupted_deletion_resumes_and_steps_are_idempotent() {
    let app = TestApp::builder().isolated().storage().start().await;
    let alice = user(&app, "alice").await;
    let bob = user(&app, "bob").await;
    let g = guild(&app, &bob).await;
    join(&app, &alice, &g).await;
    let ch = channel(&app, &bob, g.id, "talk").await.unwrap();
    let file = upload(&app, &alice, ch.id, "doc.bin").await;
    let mine = send(&app, &alice, ch.id, "still here", &[file]).await;
    let key = mine.attachments[0].file.clone();

    // Not marked: the job refuses to touch the account.
    assert!(lifecycle::run_step(&app.state, alice.id, Step::RemoveContent).await.is_err());

    let account = thehideout_server::accounts::load(&app.state, alice.id).await.unwrap().unwrap();
    lifecycle::begin_deletion(&app.state, &account, alice.id, "").await.unwrap();
    assert_eq!(state_of(&app, alice.id).await.as_deref(), Some("deleting"));
    // Already unusable and shown as deleted, while the cleanup has not run.
    assert_eq!(code(auth(&app, Some(&alice.token)).get_me(GetMeRequest::default()).await), ErrorCode::Unauthenticated);
    assert!(thehideout_server::access::find_user(&app.state, id_to_db(alice.id)).await.unwrap().unwrap().deleted);

    // "Crash" after the first two steps.
    lifecycle::run_steps(&app.state, alice.id, 2).await.unwrap();
    assert_eq!(state_of(&app, alice.id).await.as_deref(), Some("deleting"));
    assert!(app.state.storage.as_ref().unwrap().head(&key).await.unwrap().is_none(), "content step ran");
    assert_eq!(count(&app, "SELECT guild_id FROM guilds_by_user WHERE user_id = ?", alice.id).await, 1, "still a member");
    assert_eq!(count(&app, "SELECT user_id FROM account_deletions WHERE kind = 'pending' AND user_id = ?", alice.id).await, 1);
    assert!(thehideout_server::accounts::id_by_username(&app.state, &app.name("alice")).await.unwrap().is_none(), "name already freed");

    // The sweep finishes it.
    assert!(lifecycle::resume_pending(&app.state).await.unwrap() >= 1);
    assert_eq!(state_of(&app, alice.id).await.as_deref(), Some("deleted"));
    assert_eq!(count(&app, "SELECT guild_id FROM guilds_by_user WHERE user_id = ?", alice.id).await, 0);
    assert_eq!(count(&app, "SELECT user_id FROM account_deletions WHERE kind = 'pending' AND user_id = ?", alice.id).await, 0);
    // Running everything again changes nothing and does not fail.
    lifecycle::run_steps(&app.state, alice.id, STEPS.len()).await.unwrap();
    assert_eq!(lifecycle::resume_pending(&app.state).await.unwrap(), 0);
    let kept = list(&app, &bob, ch.id).await;
    assert!(kept.iter().any(|m| m.content == "still here" && m.attachments[0].removed));
    app.cleanup().await;
}

#[tokio::test]
async fn a_departed_guild_is_still_cleaned_up() {
    let app = TestApp::start().await;
    let alice = user(&app, "alice").await;
    let bob = user(&app, "bob").await;
    let g = guild(&app, &bob).await;
    join(&app, &alice, &g).await;
    let ch = channel(&app, &bob, g.id, "talk").await.unwrap();
    let bobs = send(&app, &bob, ch.id, "bob", &[]).await;
    react(&app, &alice, ch.id, bobs.id).await;
    guilds(&app, &alice).leave_guild(LeaveGuildRequest { guild_id: g.id, ..Default::default() }).await.unwrap();
    auth(&app, Some(&alice.token)).delete_account(delete_req(Some(PASSWORD))).await.unwrap();
    wait_deleted(&app, alice.id).await;
    let after = list(&app, &bob, ch.id).await;
    assert!(after.iter().find(|m| m.id == bobs.id).unwrap().reactions.is_empty(), "the reaction in a guild she had left is removed");
}

#[tokio::test]
async fn export_contains_the_users_own_data_and_expires() {
    let app = TestApp::builder().storage().start().await;
    let alice = user(&app, "alice").await;
    let bob = user(&app, "bob").await;
    let g = guild(&app, &bob).await;
    join(&app, &alice, &g).await;
    let ch = channel(&app, &bob, g.id, "talk").await.unwrap();
    let r = role(&app, &bob, g.id, "regulars", 0).await.unwrap();
    set_roles(&app, &bob, g.id, alice.id, &[r.id]).await.unwrap();
    let bobs = send(&app, &bob, ch.id, "bob says secret-bob-text", &[]).await;
    let file = upload(&app, &alice, ch.id, "notes.bin").await;
    send(&app, &alice, ch.id, "alice says hello-guild", &[file]).await;
    react(&app, &alice, ch.id, bobs.id).await;
    let dm = app
        .client(DmServiceClient::new, Some(&alice.token))
        .open_dm(OpenDmRequest { user_id: bob.id, ..Default::default() })
        .await
        .unwrap()
        .into_owned()
        .channel
        .unwrap();
    send(&app, &alice, dm.id, "alice says hello-dm", &[]).await;
    send(&app, &bob, dm.id, "bob says secret-dm-text", &[]).await;

    let client = auth(&app, Some(&alice.token));
    let resp = client.export_data(ExportDataRequest::default()).await.unwrap().into_owned();
    assert!(resp.masked_email.contains("***@example.test"));
    // Once per 24 hours.
    assert_eq!(code(client.export_data(ExportDataRequest::default()).await), ErrorCode::ResourceExhausted);

    let mail = wait_mail(&app, &email_of(&app, "alice"), 0, "data export").await;
    let prefix = format!("{}/export/", app.state.public_base());
    let start = mail.text.find(&prefix).expect("link") + prefix.len();
    let token = mail.text[start..].split_whitespace().next().unwrap().to_owned();
    assert!(token.len() >= 43, "unguessable token");
    assert!(mail.text.contains("7 days"));

    let reply = http::get(&app, &format!("/export/{token}")).await;
    assert_eq!(reply.status, 200);
    assert_eq!(reply.header("content-type"), "application/json");
    assert!(reply.header("content-disposition").starts_with("attachment"));
    assert_eq!(reply.header("cache-control"), "no-store");
    let body = reply.text();
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["profile"]["username"], app.name("alice"));
    assert_eq!(json["profile"]["email"], email_of(&app, "alice"));
    let memberships = json["guild_memberships"].as_array().unwrap();
    assert!(memberships.iter().any(|m| m["guild_id"] == g.id && m["roles"][0] == "regulars"));
    let msgs = json["messages"].as_array().unwrap();
    let guild_msg = msgs.iter().find(|m| m["content"] == "alice says hello-guild").expect("guild message");
    assert_eq!(guild_msg["channel_name"], "talk");
    assert_eq!(guild_msg["guild_name"], "guild");
    assert!(guild_msg["attachments"][0]["url"].as_str().unwrap().contains("/media/"));
    assert!(json["direct_messages_you_wrote"].as_array().unwrap().iter().any(|m| m["content"] == "alice says hello-dm"));
    assert!(!json["reactions"].as_array().unwrap().is_empty());
    assert!(!json["sessions"].as_array().unwrap().is_empty());
    assert!(json["uploads"].as_array().unwrap().iter().any(|u| u["kind"] == "attachment"));
    assert!(!body.contains("secret-bob-text") && !body.contains("secret-dm-text"), "only the user's own messages");

    // Unknown tokens, and expiry.
    assert_eq!(http::get(&app, "/export/not-a-token").await.status, 404);
    let object_key: String = {
        let hash = thehideout_server::auth::hash_token(&token);
        let rows = app
            .state
            .session
            .execute_unpaged("SELECT object_key FROM data_exports WHERE token_hash = ?", (&hash,))
            .await
            .unwrap()
            .into_rows_result()
            .unwrap();
        rows.first_row::<(String,)>().unwrap().0
    };
    assert!(app.state.storage.as_ref().unwrap().head(&object_key).await.unwrap().is_some());
    let hash = thehideout_server::auth::hash_token(&token);
    app.state
        .session
        .execute_unpaged("UPDATE data_exports SET expires_at = ? WHERE token_hash = ?", (Utc::now() - chrono::Duration::hours(1), &hash))
        .await
        .unwrap();
    assert_eq!(http::get(&app, &format!("/export/{token}")).await.status, 404, "expired link");
    app.state
        .session
        .execute_unpaged(
            "UPDATE data_exports_index SET expires_at = ? WHERE kind = 'all' AND token_hash = ?",
            (Utc::now() - chrono::Duration::hours(1), &hash),
        )
        .await
        .unwrap();
    assert!(lifecycle::sweep_exports(&app.state).await.unwrap() >= 1);
    assert!(app.state.storage.as_ref().unwrap().head(&object_key).await.unwrap().is_none(), "the sweeper deleted the file");
    app.cleanup().await;
}

#[tokio::test]
async fn export_needs_an_email_address() {
    let app = TestApp::builder().storage().start().await;
    let legacy = legacy_user(&app, "legacy").await;
    assert_eq!(code(auth(&app, Some(&legacy.token)).export_data(ExportDataRequest::default()).await), ErrorCode::FailedPrecondition);
    app.cleanup().await;
}

async fn session_ttl(app: &TestApp, token: &str) -> Option<i32> {
    let hash = thehideout_server::auth::hash_token(token);
    app.state
        .session
        .execute_unpaged("SELECT TTL(user_id) FROM sessions WHERE token_hash = ?", (&hash,))
        .await
        .unwrap()
        .into_rows_result()
        .unwrap()
        .first_row::<(Option<i32>,)>()
        .unwrap()
        .0
}

/// Rewrites the session rows as if they were `age` old and about to expire in `ttl` seconds.
async fn age_session(app: &TestApp, who: &Account, age: chrono::Duration, ttl: i32) {
    let hash = thehideout_server::auth::hash_token(&who.token);
    let created = Utc::now() - age;
    app.state
        .session
        .execute_unpaged(
            "INSERT INTO sessions (token_hash, user_id, created_at, id, device_name, refreshed_at) VALUES (?, ?, ?, ?, 'test', ?) USING TTL ?",
            (&hash, id_to_db(who.id), created, 12345_i64, created, ttl),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn sessions_slide_but_at_most_once_a_day() {
    let app = TestApp::start().await;
    let alice = user(&app, "alice").await;
    let client = auth(&app, Some(&alice.token));
    let me = || client.get_me(GetMeRequest::default());
    let full = thehideout_server::auth::SESSION_TTL_SECS;

    // Used within the day: untouched.
    age_session(&app, &alice, chrono::Duration::hours(2), 1000).await;
    me().await.unwrap();
    assert!(session_ttl(&app, &alice.token).await.unwrap() <= 1000);

    // Older than a day: pushed out to a full TTL.
    age_session(&app, &alice, chrono::Duration::days(2), 1000).await;
    me().await.unwrap();
    let ttl = session_ttl(&app, &alice.token).await.unwrap();
    assert!(ttl > full - 60, "refreshed to ~30 days, got {ttl}");

    // Immediately again: throttled (the TTL we lower by hand stays low).
    app.state
        .session
        .execute_unpaged(
            "INSERT INTO sessions (token_hash, user_id, created_at, id, device_name, refreshed_at) VALUES (?, ?, ?, ?, 'test', ?) USING TTL 900",
            (thehideout_server::auth::hash_token(&alice.token), id_to_db(alice.id), Utc::now() - chrono::Duration::days(2), 12345_i64, Utc::now()),
        )
        .await
        .unwrap();
    me().await.unwrap();
    assert!(session_ttl(&app, &alice.token).await.unwrap() <= 900, "second call within a day does not write");

    // The listing row slides with it, and the session still works after the refresh.
    age_session(&app, &alice, chrono::Duration::days(3), 1000).await;
    me().await.unwrap();
    let sessions = auth(&app, Some(&alice.token)).list_sessions(ListSessionsRequest::default()).await.unwrap().into_owned().sessions;
    assert!(sessions.iter().any(|s| s.current));
}
