//! Push notifications: recipients, settings, devices and dispatch, against the
//! real database with a recording fake instead of APNs / FCM.
mod common;

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use common::TestApp;
use common::fixtures::*;
use connectrpc::client::HttpClient;
use connectrpc::{ConnectError, ErrorCode};
use thehideout_server::convert::datetime_to_proto;
use thehideout_server::lifecycle::{self, STEPS};
use thehideout_server::proto::thehideout::v1::event::Payload;
use thehideout_server::proto::thehideout::v1::*;
use thehideout_server::push::payload::Kind;
use thehideout_server::push::{Device, Notification, Outcome, Transport, Tuning, store};

/// A transport that records what it was asked to send and answers from a script.
#[derive(Default)]
struct Fake {
    delivered: Mutex<Vec<(String, Notification)>>,
    calls: Mutex<Vec<String>>,
    script: Mutex<HashMap<String, VecDeque<Outcome>>>,
}

#[async_trait]
impl Transport for Fake {
    async fn send(&self, device: &Device, notification: &Notification) -> Outcome {
        self.calls.lock().unwrap().push(device.token.clone());
        let scripted = self.script.lock().unwrap().get_mut(&device.token).and_then(|q| q.pop_front());
        let outcome = scripted.unwrap_or(Outcome::Delivered);
        if outcome == Outcome::Delivered {
            self.delivered.lock().unwrap().push((device.token.clone(), notification.clone()));
        }
        outcome
    }
}

impl Fake {
    fn to(&self, token: &str) -> Vec<Notification> {
        self.delivered.lock().unwrap().iter().filter(|(t, _)| t == token).map(|(_, n)| n.clone()).collect()
    }

    fn calls_to(&self, token: &str) -> usize {
        self.calls.lock().unwrap().iter().filter(|t| *t == token).count()
    }

    fn script(&self, token: &str, outcomes: Vec<Outcome>) {
        self.script.lock().unwrap().insert(token.to_owned(), outcomes.into());
    }

    /// Waits until `token` has received `n` pushes.
    async fn wait(&self, token: &str, n: usize) -> Vec<Notification> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let got = self.to(token);
            if got.len() >= n {
                return got;
            }
            assert!(tokio::time::Instant::now() < deadline, "timed out waiting for {n} push(es) to {token}, got {got:?}");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
}

/// Time the dispatcher gets to (not) deliver before an absence is asserted.
const QUIET: Duration = Duration::from_millis(700);

async fn start() -> (TestApp, Arc<Fake>) {
    start_with(Tuning { coalesce_window: Duration::ZERO, retry_backoff: Duration::from_millis(10), ..Tuning::default() }).await
}

async fn start_with(tuning: Tuning) -> (TestApp, Arc<Fake>) {
    let fake = Arc::new(Fake::default());
    let app = TestApp::builder().push(fake.clone(), tuning).start().await;
    (app, fake)
}

fn notifications(app: &TestApp, who: &Account) -> NotificationServiceClient<HttpClient> {
    app.client(NotificationServiceClient::new, Some(&who.token))
}

fn token_for(app: &TestApp, name: &str) -> String {
    format!("{}-fcm-token-0123456789", app.name(name))
}

/// Registers an Android device with a token unique to the user.
async fn register(app: &TestApp, who: &Account, name: &str) -> (String, u64) {
    let token = token_for(app, name);
    let id = register_token(app, who, &token).await.unwrap();
    (token, id)
}

async fn register_token(app: &TestApp, who: &Account, token: &str) -> Result<u64, ConnectError> {
    Ok(notifications(app, who)
        .register_device(RegisterDeviceRequest {
            platform: PushPlatform::PUSH_PLATFORM_ANDROID.into(),
            token: token.into(),
            app_version: "1.0.0".into(),
            device_name: "Pixel".into(),
            ..Default::default()
        })
        .await?
        .into_owned()
        .device_id)
}

async fn update(app: &TestApp, who: &Account, req: UpdateNotificationSettingsRequest) -> Result<NotificationSettings, ConnectError> {
    Ok(notifications(app, who).update_notification_settings(req).await?.into_owned().settings.unwrap())
}

fn ov(id: u64, level: NotificationLevel) -> NotificationOverride {
    NotificationOverride { id, level: level.into(), ..Default::default() }
}

fn muted(id: u64, level: NotificationLevel, secs: i64) -> NotificationOverride {
    NotificationOverride { muted_until: datetime_to_proto(Utc::now() + chrono::Duration::seconds(secs)).into(), ..ov(id, level) }
}

async fn guild_level(app: &TestApp, who: &Account, level: NotificationLevel) {
    update(app, who, UpdateNotificationSettingsRequest { guild_default: Some(level.into()), ..Default::default() }).await.unwrap();
}

async fn say(app: &TestApp, who: &Account, channel_id: u64, text: &str) -> Message {
    app.client(MessageServiceClient::new, Some(&who.token))
        .send_message(SendMessageRequest { channel_id, content: text.into(), ..Default::default() })
        .await
        .unwrap()
        .into_owned()
        .message
        .unwrap()
}

/// A guild with its owner, a text channel and the given members.
async fn community(app: &TestApp, names: &[&str]) -> (Account, Guild, Channel, Vec<Account>) {
    let owner = user(app, "owner").await;
    let g = guild(app, &owner).await;
    let general = channel(app, &owner, g.id, "general").await.unwrap();
    let mut members = Vec::new();
    for name in names {
        let m = user(app, name).await;
        join(app, &m, &g).await;
        members.push(m);
    }
    (owner, g, general, members)
}

macro_rules! assert_silent {
    ($fake:expr, $($token:expr),+) => {
        tokio::time::sleep(QUIET).await;
        $(assert!($fake.to($token).is_empty(), "{} must not be pushed: {:?}", $token, $fake.to($token));)+
    };
}

#[tokio::test]
async fn dm_pushes_the_other_participant_but_not_the_author() {
    let (app, fake) = start().await;
    let (ana, bob) = (user(&app, "ana").await, user(&app, "bob").await);
    let (ana_tok, _) = register(&app, &ana, "ana").await;
    let (bob_tok, _) = register(&app, &bob, "bob").await;
    let dm = app
        .client(DmServiceClient::new, Some(&ana.token))
        .open_dm(OpenDmRequest { user_id: bob.id, ..Default::default() })
        .await
        .unwrap()
        .into_owned()
        .channel
        .unwrap();

    let sent = say(&app, &ana, dm.id, "hey **bob**, lunch?").await;
    let got = fake.wait(&bob_tok, 1).await;
    assert_eq!(got[0].kind, Kind::Dm);
    assert_eq!(got[0].title, app.name("ana"));
    assert_eq!(got[0].body, "hey bob, lunch?");
    assert_eq!((got[0].guild_id, got[0].channel_id, got[0].message_id), (0, dm.id, sent.id));
    assert_eq!(got[0].collapse_id(), dm.id.to_string());
    assert_silent!(fake, &ana_tok);

    // DM pushes can be turned off globally.
    update(&app, &bob, UpdateNotificationSettingsRequest { dms_enabled: Some(false), ..Default::default() }).await.unwrap();
    say(&app, &ana, dm.id, "still there?").await;
    assert_silent!(fake, "never");
    assert_eq!(fake.to(&bob_tok).len(), 1);
    // A per-channel override beats the global switch.
    update(&app, &bob, UpdateNotificationSettingsRequest { channels: vec![ov(dm.id, NotificationLevel::NOTIFICATION_LEVEL_ALL)], ..Default::default() }).await.unwrap();
    say(&app, &ana, dm.id, "please?").await;
    fake.wait(&bob_tok, 2).await;
}

#[tokio::test]
async fn guild_messages_follow_the_levels_and_mentions() {
    let (app, fake) = start().await;
    let (owner, g, general, members) = community(&app, &["bob", "cara", "dan"]).await;
    let (bob, cara, dan) = (&members[0], &members[1], &members[2]);
    let (otok, _) = register(&app, &owner, "owner").await;
    let (btok, _) = register(&app, bob, "bob").await;
    let (ctok, _) = register(&app, cara, "cara").await;
    let (dtok, _) = register(&app, dan, "dan").await;
    // bob keeps the default (mentions), cara wants all, dan nothing.
    guild_level(&app, cara, NotificationLevel::NOTIFICATION_LEVEL_ALL).await;
    guild_level(&app, dan, NotificationLevel::NOTIFICATION_LEVEL_NOTHING).await;

    say(&app, &owner, general.id, "plain message").await;
    let got = fake.wait(&ctok, 1).await;
    assert_eq!(got[0].kind, Kind::Message);
    assert_eq!(got[0].title, format!("{} in #general · guild", app.name("owner")));
    assert_eq!(got[0].guild_id, g.id);
    assert_silent!(fake, &btok, &dtok, &otok);

    // A mention reaches bob (as a mention) and still cara; case does not matter; dan stays silent.
    let name = app.name("bob").to_uppercase();
    say(&app, &owner, general.id, &format!("ping @{name}, look")).await;
    let got = fake.wait(&btok, 1).await;
    assert_eq!(got[0].kind, Kind::Mention);
    assert_eq!(fake.wait(&ctok, 2).await[1].kind, Kind::Message, "cara was not mentioned herself");
    assert_silent!(fake, &dtok, &otok);
}

#[tokio::test]
async fn mention_parsing_matches_the_client_syntax() {
    let (app, fake) = start().await;
    let (owner, _g, general, members) = community(&app, &["bob"]).await;
    let bob = &members[0];
    let (btok, _) = register(&app, bob, "bob").await;
    let name = app.name("bob");

    // Not mentions: inside an e-mail address, inside code, a different name.
    for text in [format!("mail me@{name}.test"), format!("run `@{name}` now"), format!("```\n@{name}\n```"), format!("@{name}x is someone else")] {
        say(&app, &owner, general.id, &text).await;
    }
    assert_silent!(fake, &btok);
    // Mentions: inside emphasis, with trailing punctuation, in parentheses.
    for (i, text) in [format!("**hi @{name}**"), format!("thanks @{name}."), format!("(@{name})"), format!("__@{name}__")].into_iter().enumerate() {
        say(&app, &owner, general.id, &text).await;
        fake.wait(&btok, i + 1).await;
    }
}

#[tokio::test]
async fn channel_override_beats_guild_beats_global_and_mutes_win() {
    let (app, fake) = start().await;
    let (owner, g, general, members) = community(&app, &["bob"]).await;
    let bob = &members[0];
    let other = channel(&app, &owner, g.id, "other").await.unwrap();
    let (btok, _) = register(&app, bob, "bob").await;
    let put = |req: UpdateNotificationSettingsRequest| update(&app, bob, req);

    // Global NOTHING, guild ALL: the guild wins.
    guild_level(&app, bob, NotificationLevel::NOTIFICATION_LEVEL_NOTHING).await;
    say(&app, &owner, general.id, "one").await;
    assert_silent!(fake, &btok);
    put(UpdateNotificationSettingsRequest { guilds: vec![ov(g.id, NotificationLevel::NOTIFICATION_LEVEL_ALL)], ..Default::default() }).await.unwrap();
    say(&app, &owner, general.id, "two").await;
    fake.wait(&btok, 1).await;

    // Channel NOTHING beats guild ALL, in that channel only.
    put(UpdateNotificationSettingsRequest { channels: vec![ov(general.id, NotificationLevel::NOTIFICATION_LEVEL_NOTHING)], ..Default::default() }).await.unwrap();
    say(&app, &owner, general.id, "three").await;
    say(&app, &owner, other.id, "four").await;
    let got = fake.wait(&btok, 2).await;
    assert_eq!(got[1].body, "four");
    assert_silent!(fake, "never");
    assert_eq!(fake.to(&btok).len(), 2);

    // Inheriting from the guild again.
    put(UpdateNotificationSettingsRequest { channels: vec![ov(general.id, NotificationLevel::NOTIFICATION_LEVEL_INHERIT)], ..Default::default() }).await.unwrap();
    say(&app, &owner, general.id, "five").await;
    fake.wait(&btok, 3).await;

    // A muted guild is silent; a lapsed mute is not.
    put(UpdateNotificationSettingsRequest { guilds: vec![muted(g.id, NotificationLevel::NOTIFICATION_LEVEL_ALL, 3600)], ..Default::default() }).await.unwrap();
    say(&app, &owner, general.id, "six").await;
    assert_silent!(fake, "never");
    assert_eq!(fake.to(&btok).len(), 3);
    // A muted channel stays silent even when its level is ALL.
    put(UpdateNotificationSettingsRequest {
        guilds: vec![ov(g.id, NotificationLevel::NOTIFICATION_LEVEL_ALL)],
        channels: vec![muted(other.id, NotificationLevel::NOTIFICATION_LEVEL_ALL, 3600)],
        ..Default::default()
    })
    .await
    .unwrap();
    say(&app, &owner, other.id, "seven").await;
    say(&app, &owner, general.id, "eight").await;
    let got = fake.wait(&btok, 4).await;
    assert_eq!(got[3].body, "eight");
    // The mute lapses (muted_until in the past counts as not muted).
    put(UpdateNotificationSettingsRequest { channels: vec![muted(other.id, NotificationLevel::NOTIFICATION_LEVEL_ALL, -5)], ..Default::default() }).await.unwrap();
    say(&app, &owner, other.id, "nine").await;
    assert_eq!(fake.wait(&btok, 5).await[4].body, "nine");
}

#[tokio::test]
async fn no_push_for_people_outside_the_channel() {
    let (app, fake) = start().await;
    let (owner, g, general, members) = community(&app, &["bob", "leaver"]).await;
    let (bob, leaver) = (&members[0], &members[1]);
    let outsider = user(&app, "outsider").await;
    let (btok, _) = register(&app, bob, "bob").await;
    let (ltok, _) = register(&app, leaver, "leaver").await;
    let (xtok, _) = register(&app, &outsider, "outsider").await;
    for who in [bob, leaver, &outsider] {
        guild_level(&app, who, NotificationLevel::NOTIFICATION_LEVEL_ALL).await;
    }
    // The outsider cannot even set an override on a channel / guild they cannot see.
    let err = update(&app, &outsider, UpdateNotificationSettingsRequest { channels: vec![ov(general.id, NotificationLevel::NOTIFICATION_LEVEL_ALL)], ..Default::default() }).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    let err = update(&app, &outsider, UpdateNotificationSettingsRequest { guilds: vec![ov(g.id, NotificationLevel::NOTIFICATION_LEVEL_ALL)], ..Default::default() }).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);

    app.client(GuildServiceClient::new, Some(&leaver.token)).leave_guild(LeaveGuildRequest { guild_id: g.id, ..Default::default() }).await.unwrap();
    say(&app, &owner, general.id, "members only").await;
    fake.wait(&btok, 1).await;
    assert_silent!(fake, &ltok, &xtok);
}

#[tokio::test]
async fn previews_can_be_turned_off() {
    let (app, fake) = start().await;
    let (owner, _g, general, members) = community(&app, &["bob", "cara"]).await;
    let (bob, cara) = (&members[0], &members[1]);
    let (btok, _) = register(&app, bob, "bob").await;
    let (ctok, _) = register(&app, cara, "cara").await;
    for who in [bob, cara] {
        guild_level(&app, who, NotificationLevel::NOTIFICATION_LEVEL_ALL).await;
    }
    let settings = update(&app, bob, UpdateNotificationSettingsRequest { show_previews: Some(false), ..Default::default() }).await.unwrap();
    assert!(!settings.show_previews);

    say(&app, &owner, general.id, "the **secret** plan").await;
    assert_eq!(fake.wait(&btok, 1).await[0].body, "New message");
    assert_eq!(fake.wait(&ctok, 1).await[0].body, "the secret plan");
    // The title still names the sender and place, and long text is cut for those who want it.
    update(&app, bob, UpdateNotificationSettingsRequest { show_previews: Some(true), ..Default::default() }).await.unwrap();
    say(&app, &owner, general.id, &"x".repeat(500)).await;
    let body = fake.wait(&btok, 2).await[1].body.clone();
    assert_eq!(body.chars().count(), 180);
    assert!(body.ends_with('…'));
}

#[tokio::test]
async fn bursts_are_coalesced_into_a_leading_and_a_trailing_push() {
    let (app, fake) = start_with(Tuning { coalesce_window: Duration::from_millis(1500), ..Tuning::default() }).await;
    let (owner, g, general, members) = community(&app, &["bob"]).await;
    let bob = &members[0];
    let other = channel(&app, &owner, g.id, "other").await.unwrap();
    let (btok, _) = register(&app, bob, "bob").await;
    guild_level(&app, bob, NotificationLevel::NOTIFICATION_LEVEL_ALL).await;

    for text in ["one", "two", "three"] {
        say(&app, &owner, general.id, text).await;
    }
    // Another channel is not held back.
    say(&app, &owner, other.id, "elsewhere").await;
    let early = fake.wait(&btok, 2).await;
    assert_eq!(early.iter().map(|n| n.body.as_str()).collect::<Vec<_>>().len(), 2);
    assert!(early.iter().any(|n| n.body == "one") && early.iter().any(|n| n.body == "elsewhere"));
    // The rest arrive as one push, carrying the latest message and the same collapse id.
    let all = fake.wait(&btok, 3).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let all2 = fake.to(&btok);
    assert_eq!(all2.len(), 3, "two and three collapse into one: {all2:?}");
    let trailing = all.iter().find(|n| n.body == "three").expect("the latest message is the trailing push");
    assert_eq!(trailing.collapse_id(), general.id.to_string());
    assert!(!all2.iter().any(|n| n.body == "two"));
}

#[tokio::test]
async fn dead_tokens_remove_the_device_and_transient_errors_are_retried() {
    let (app, fake) = start().await;
    let (owner, _g, general, members) = community(&app, &["bob"]).await;
    let bob = &members[0];
    let (dead, dead_id) = register(&app, bob, "dead").await;
    let (flaky, _) = {
        // A second device on another session of bob's.
        let second = thehideout_server::auth::create_session(&app.state, bob.id, "tablet").await.unwrap();
        let acct = Account { token: second, id: bob.id };
        register(&app, &acct, "flaky").await
    };
    guild_level(&app, bob, NotificationLevel::NOTIFICATION_LEVEL_ALL).await;
    fake.script(&dead, vec![Outcome::InvalidToken]);
    fake.script(&flaky, vec![Outcome::Retry(Some(Duration::from_millis(10)), "503".into()), Outcome::Retry(None, "429".into())]);

    say(&app, &owner, general.id, "hello").await;
    fake.wait(&flaky, 1).await;
    assert_eq!(fake.calls_to(&flaky), 3, "two retries, then delivered");
    // The dead device is gone; the healthy one stays.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let devices = store::list(&app.state, bob.id).await.unwrap();
        if !devices.iter().any(|d| d.id == dead_id) {
            assert_eq!(devices.len(), 1);
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "the dead device was not removed");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    say(&app, &owner, general.id, "again").await;
    fake.wait(&flaky, 2).await;
    assert_eq!(fake.calls_to(&dead), 1, "a removed device is not tried again");
}

#[tokio::test]
async fn signing_out_and_account_deletion_remove_devices() {
    let (app, _fake) = start().await;
    let (bob, cara) = (user(&app, "bob").await, user(&app, "cara").await);
    let (_t, bob_device) = register(&app, &bob, "bob").await;
    // A second session with its own device.
    let second = Account { token: thehideout_server::auth::create_session(&app.state, bob.id, "phone").await.unwrap(), id: bob.id };
    let (_t2, second_device) = register(&app, &second, "bob2").await;
    assert_eq!(store::list(&app.state, bob.id).await.unwrap().len(), 2);

    // Revoking one session removes that session's device only.
    let sessions = auth(&app, Some(&bob.token)).list_sessions(ListSessionsRequest::default()).await.unwrap().into_owned().sessions;
    let phone = sessions.iter().find(|s| s.device_name == "phone").unwrap().id;
    auth(&app, Some(&bob.token)).revoke_session(RevokeSessionRequest { session_id: phone, ..Default::default() }).await.unwrap();
    let left = store::list(&app.state, bob.id).await.unwrap();
    assert_eq!(left.iter().map(|d| d.id).collect::<Vec<_>>(), [bob_device]);
    assert!(!left.iter().any(|d| d.id == second_device));

    // Signing out everywhere else keeps the current session's device.
    let third = Account { token: thehideout_server::auth::create_session(&app.state, bob.id, "tv").await.unwrap(), id: bob.id };
    register(&app, &third, "bob3").await;
    auth(&app, Some(&bob.token)).revoke_other_sessions(RevokeOtherSessionsRequest::default()).await.unwrap();
    assert_eq!(store::list(&app.state, bob.id).await.unwrap().iter().map(|d| d.id).collect::<Vec<_>>(), [bob_device]);

    // Suspension revokes the sessions and with them the devices.
    register(&app, &cara, "cara").await;
    update(&app, &cara, UpdateNotificationSettingsRequest { show_previews: Some(false), ..Default::default() }).await.unwrap();
    lifecycle::suspend(&app.state, bob.id, cara.id, "test", None).await.unwrap();
    assert!(store::list(&app.state, cara.id).await.unwrap().is_empty());

    // Account deletion removes every device and the settings.
    let dave = user(&app, "dave").await;
    register(&app, &dave, "dave").await;
    update(&app, &dave, UpdateNotificationSettingsRequest { show_previews: Some(false), ..Default::default() }).await.unwrap();
    let second = Account { token: thehideout_server::auth::create_session(&app.state, dave.id, "second").await.unwrap(), id: dave.id };
    register(&app, &second, "dave2").await;
    assert_eq!(store::list(&app.state, dave.id).await.unwrap().len(), 2);
    let account = thehideout_server::accounts::load(&app.state, dave.id).await.unwrap().unwrap();
    lifecycle::begin_deletion(&app.state, &account, bob.id, "").await.unwrap();
    lifecycle::run_steps(&app.state, dave.id, STEPS.len()).await.unwrap();
    assert!(store::list(&app.state, dave.id).await.unwrap().is_empty());
    let settings = thehideout_server::push::settings::load(&app.state, dave.id).await.unwrap();
    assert!(settings.show_previews, "settings are back to defaults after deletion");
}

#[tokio::test]
async fn sessions_that_expired_do_not_get_pushes() {
    let (app, fake) = start().await;
    let (owner, _g, general, members) = community(&app, &["bob"]).await;
    let bob = &members[0];
    let (btok, id) = register(&app, bob, "bob").await;
    guild_level(&app, bob, NotificationLevel::NOTIFICATION_LEVEL_ALL).await;
    // The session row disappears (TTL expiry), without anything revoking it.
    let hash = thehideout_server::auth::hash_token(&bob.token);
    app.state.session.execute_unpaged("DELETE FROM sessions WHERE token_hash = ?", (&hash,)).await.unwrap();

    say(&app, &owner, general.id, "anyone?").await;
    assert_silent!(fake, &btok);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while store::list(&app.state, bob.id).await.unwrap().iter().any(|d| d.id == id) {
        assert!(tokio::time::Instant::now() < deadline, "the stale device was not removed");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test]
async fn users_watching_the_app_are_not_pushed() {
    let (app, fake) = start().await;
    let (owner, _g, general, members) = community(&app, &["bob"]).await;
    let bob = &members[0];
    let (btok, _) = register(&app, bob, "bob").await;
    guild_level(&app, bob, NotificationLevel::NOTIFICATION_LEVEL_ALL).await;

    let mut stream = app.client(GatewayServiceClient::new, Some(&bob.token)).subscribe(SubscribeRequest::default()).await.unwrap();
    assert!(matches!(stream.message().await.unwrap().unwrap().to_owned_message().event.unwrap().payload, Some(Payload::Heartbeat(_))));
    // A live stream that never reported its state counts as foreground.
    say(&app, &owner, general.id, "while watching").await;
    assert_silent!(fake, &btok);

    // The app goes to the background: pushes resume although the stream is still open.
    notifications(&app, bob).set_client_state(SetClientStateRequest { foreground: false, ..Default::default() }).await.unwrap();
    say(&app, &owner, general.id, "while in the background").await;
    assert_eq!(fake.wait(&btok, 1).await[0].body, "while in the background");

    // Back in the foreground: suppressed again.
    notifications(&app, bob).set_client_state(SetClientStateRequest { foreground: true, ..Default::default() }).await.unwrap();
    say(&app, &owner, general.id, "foreground again").await;
    tokio::time::sleep(QUIET).await;
    assert_eq!(fake.to(&btok).len(), 1);

    // Closing the stream makes the user pushable again.
    drop(stream);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while app.state.push.presence.is_active(bob.id) {
        assert!(tokio::time::Instant::now() < deadline, "the stream was never released");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    say(&app, &owner, general.id, "offline").await;
    assert_eq!(fake.wait(&btok, 2).await[1].body, "offline");
}

#[tokio::test]
async fn being_added_to_a_guild_is_pushed_unless_guild_pushes_are_off() {
    let (app, fake) = start().await;
    let owner = user(&app, "owner").await;
    let g = guild(&app, &owner).await;
    let (bob, cara) = (user(&app, "bob").await, user(&app, "cara").await);
    let (btok, _) = register(&app, &bob, "bob").await;
    let (ctok, _) = register(&app, &cara, "cara").await;
    guild_level(&app, &cara, NotificationLevel::NOTIFICATION_LEVEL_NOTHING).await;

    join(&app, &bob, &g).await;
    join(&app, &cara, &g).await;
    let got = fake.wait(&btok, 1).await;
    assert_eq!(got[0].kind, Kind::GuildAdded);
    assert_eq!(got[0].guild_id, g.id);
    assert_eq!(got[0].collapse_id(), g.id.to_string());
    assert!(got[0].body.contains("guild"));
    assert_silent!(fake, &ctok);
}

#[tokio::test]
async fn registering_devices() {
    // Works without any push provider configured (the default app).
    let app = TestApp::start().await;
    let (bob, cara) = (user(&app, "bob").await, user(&app, "cara").await);
    let client = notifications(&app, &bob);
    let register = |platform: PushPlatform, token: &str| {
        let client = notifications(&app, &bob);
        let token = token.to_owned();
        async move {
            client
                .register_device(RegisterDeviceRequest { platform: platform.into(), token, app_version: "1".into(), device_name: "d".into(), ..Default::default() })
                .await
                .map(|r| r.into_owned().device_id)
        }
    };
    let ios = "ab".repeat(32);

    // Validation.
    assert_eq!(register(PushPlatform::PUSH_PLATFORM_UNSPECIFIED, &ios).await.unwrap_err().code, ErrorCode::InvalidArgument);
    assert_eq!(register(PushPlatform::PUSH_PLATFORM_IOS, "not hex at all, sorry").await.unwrap_err().code, ErrorCode::InvalidArgument);
    assert_eq!(register(PushPlatform::PUSH_PLATFORM_ANDROID, "short").await.unwrap_err().code, ErrorCode::InvalidArgument);
    assert_eq!(register(PushPlatform::PUSH_PLATFORM_ANDROID, "").await.unwrap_err().code, ErrorCode::InvalidArgument);
    let err = app.client(NotificationServiceClient::new, None).get_notification_settings(GetNotificationSettingsRequest::default()).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Unauthenticated);

    // Re-registering the same token keeps the id.
    let tok = format!("{}-token-aaaaaaaaaaaaaaaa", app.name("t"));
    let id = register(PushPlatform::PUSH_PLATFORM_ANDROID, &tok).await.unwrap();
    assert_eq!(register(PushPlatform::PUSH_PLATFORM_ANDROID, &tok).await.unwrap(), id);
    assert_eq!(store::list(&app.state, bob.id).await.unwrap().len(), 1);
    // A rotated token of the same session replaces the old device.
    let rotated = format!("{}-token-bbbbbbbbbbbbbbbb", app.name("t"));
    let id2 = register(PushPlatform::PUSH_PLATFORM_ANDROID, &rotated).await.unwrap();
    assert_ne!(id2, id);
    let devices = store::list(&app.state, bob.id).await.unwrap();
    assert_eq!(devices.iter().map(|d| d.token.as_str()).collect::<Vec<_>>(), [rotated.as_str()]);
    // iOS and Android can coexist in one session.
    register(PushPlatform::PUSH_PLATFORM_IOS, &ios).await.unwrap();
    assert_eq!(store::list(&app.state, bob.id).await.unwrap().len(), 2);

    // A token moves to the account that registers it next (a shared phone).
    let moved = register_token(&app, &cara, &rotated).await.unwrap();
    assert!(store::list(&app.state, bob.id).await.unwrap().iter().all(|d| d.token != rotated));
    assert_eq!(store::list(&app.state, cara.id).await.unwrap()[0].id, moved);

    // Unregister by id and by token; unknown ones and other people's ids are no-ops.
    let ios_id = store::list(&app.state, bob.id).await.unwrap()[0].id;
    client.unregister_device(UnregisterDeviceRequest { device_id: moved, ..Default::default() }).await.unwrap();
    assert_eq!(store::list(&app.state, cara.id).await.unwrap().len(), 1, "bob cannot remove cara's device");
    client.unregister_device(UnregisterDeviceRequest { device_id: ios_id, ..Default::default() }).await.unwrap();
    assert!(store::list(&app.state, bob.id).await.unwrap().is_empty());
    notifications(&app, &cara).unregister_device(UnregisterDeviceRequest { token: rotated.clone(), ..Default::default() }).await.unwrap();
    assert!(store::list(&app.state, cara.id).await.unwrap().is_empty());
    client.unregister_device(UnregisterDeviceRequest { token: "unknown".into(), ..Default::default() }).await.unwrap();
}

#[tokio::test]
async fn at_most_ten_devices_per_user() {
    let app = TestApp::start().await;
    let bob = user(&app, "bob").await;
    for i in 0..10 {
        let session = Account { token: thehideout_server::auth::create_session(&app.state, bob.id, "d").await.unwrap(), id: bob.id };
        register_token(&app, &session, &format!("{}-token-{i:016}", app.name("t"))).await.unwrap();
    }
    let session = Account { token: thehideout_server::auth::create_session(&app.state, bob.id, "d").await.unwrap(), id: bob.id };
    let err = register_token(&app, &session, &format!("{}-token-extra-0000000", app.name("t"))).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::ResourceExhausted);
}

#[tokio::test]
async fn settings_are_validated_stored_and_mirrored_to_other_sessions() {
    let app = TestApp::start().await;
    let (owner, g, general, members) = community(&app, &["bob"]).await;
    let _ = owner;
    let bob = &members[0];
    let got = notifications(&app, bob).get_notification_settings(GetNotificationSettingsRequest::default()).await.unwrap().into_owned().settings.unwrap();
    assert!(got.dms_enabled && got.show_previews);
    assert_eq!(got.guild_default.as_known(), Some(NotificationLevel::NOTIFICATION_LEVEL_MENTIONS));
    assert!(got.guilds.is_empty() && got.channels.is_empty());

    // The other session listens.
    let other = thehideout_server::auth::create_session(&app.state, bob.id, "desktop").await.unwrap();
    let mut stream = app.client(GatewayServiceClient::new, Some(&other)).subscribe(SubscribeRequest::default()).await.unwrap();
    stream.message().await.unwrap().unwrap(); // heartbeat

    // Validation.
    for bad in [NotificationLevel::NOTIFICATION_LEVEL_INHERIT, NotificationLevel::NOTIFICATION_LEVEL_UNSPECIFIED] {
        let err = update(&app, bob, UpdateNotificationSettingsRequest { guild_default: Some(bad.into()), ..Default::default() }).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidArgument);
    }
    let too_many: Vec<_> = (1..=101).map(|i| ov(i, NotificationLevel::NOTIFICATION_LEVEL_ALL)).collect();
    let err = update(&app, bob, UpdateNotificationSettingsRequest { channels: too_many, ..Default::default() }).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidArgument);
    let err = update(&app, bob, UpdateNotificationSettingsRequest { channels: vec![ov(1, NotificationLevel::NOTIFICATION_LEVEL_ALL)], ..Default::default() }).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);

    let saved = update(
        &app,
        bob,
        UpdateNotificationSettingsRequest {
            dms_enabled: Some(false),
            guild_default: Some(NotificationLevel::NOTIFICATION_LEVEL_ALL.into()),
            guilds: vec![muted(g.id, NotificationLevel::NOTIFICATION_LEVEL_NOTHING, 3600)],
            channels: vec![ov(general.id, NotificationLevel::NOTIFICATION_LEVEL_MENTIONS)],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(!saved.dms_enabled && saved.show_previews, "unset fields are unchanged");
    assert_eq!(saved.guild_default.as_known(), Some(NotificationLevel::NOTIFICATION_LEVEL_ALL));
    assert_eq!((saved.guilds.len(), saved.channels.len()), (1, 1));
    assert!(saved.guilds[0].muted_until.is_set());

    // The other session gets the full settings as an event.
    let mut event = None;
    for _ in 0..5 {
        let e = tokio::time::timeout(Duration::from_secs(5), stream.message()).await.unwrap().unwrap().unwrap().to_owned_message().event.unwrap();
        if let Some(Payload::NotificationSettingsUpdated(s)) = e.payload {
            event = Some(s);
            break;
        }
    }
    assert_eq!(*event.expect("notification_settings_updated"), saved);
    // And they persist.
    let again = notifications(&app, bob).get_notification_settings(GetNotificationSettingsRequest::default()).await.unwrap().into_owned().settings.unwrap();
    assert_eq!(again, saved);

    // Resetting an override removes it (INHERIT without a mute).
    let reset = update(&app, bob, UpdateNotificationSettingsRequest { guilds: vec![ov(g.id, NotificationLevel::NOTIFICATION_LEVEL_INHERIT)], channels: vec![ov(general.id, NotificationLevel::NOTIFICATION_LEVEL_INHERIT)], ..Default::default() }).await.unwrap();
    assert!(reset.guilds.is_empty() && reset.channels.is_empty());
}
