mod common;

use std::time::Duration;

use common::fixtures::{self, Account, MOVE_MEMBERS, MUTE_MEMBERS, guild, join, user};
use common::voice::{self as lk, Call};
use common::TestApp;
use connectrpc::ErrorCode;
use livekit_api::access_token::{Claims, TokenVerifier};
use thehideout_server::proto::thehideout::v1::event::Payload;
use thehideout_server::proto::thehideout::v1::*;
use thehideout_server::voice::PublishSource;
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(5);

async fn start() -> TestApp {
    TestApp::builder().livekit().heartbeat(Duration::from_millis(200)).start().await
}

fn voice(app: &TestApp, who: &Account) -> VoiceServiceClient<connectrpc::client::HttpClient> {
    app.client(VoiceServiceClient::new, Some(&who.token))
}

async fn make_channel(app: &TestApp, who: &Account, guild_id: u64, name: &str, kind: ChannelKind) -> Channel {
    fixtures::guilds(app, who)
        .create_channel(CreateChannelRequest { guild_id, name: name.into(), kind: kind.into(), ..Default::default() })
        .await
        .expect("create channel")
        .into_owned()
        .channel
        .unwrap()
}

async fn voice_channel(app: &TestApp, who: &Account, guild_id: u64, name: &str) -> Channel {
    make_channel(app, who, guild_id, name, ChannelKind::CHANNEL_KIND_VOICE).await
}

async fn join_voice(app: &TestApp, who: &Account, channel_id: u64) -> Result<JoinVoiceResponse, connectrpc::ConnectError> {
    Ok(voice(app, who).join_voice(JoinVoiceRequest { channel_id, ..Default::default() }).await?.into_owned())
}

/// Opens a Subscribe stream for `who`.
macro_rules! subscribe {
    ($app:expr, $who:expr) => {
        $app.client(GatewayServiceClient::new, Some(&$who.token))
            .subscribe(SubscribeRequest::default())
            .await
            .expect("subscribe")
    };
}

macro_rules! next_event {
    ($stream:expr) => {{
        let msg = timeout(WAIT, $stream.message())
            .await
            .expect("timed out waiting for an event")
            .expect("stream error")
            .expect("stream ended");
        let event: Event = msg.to_owned_message().event.into_option().expect("response carries an event");
        event
    }};
}

/// Next event that is not a heartbeat.
macro_rules! next_real_event {
    ($stream:expr) => {{
        loop {
            let event: Event = next_event!($stream);
            if !matches!(event.payload, Some(Payload::Heartbeat(_))) {
                break event;
            }
        }
    }};
}

/// Next voice state update, as `(user_id, guild_id, state)`.
macro_rules! next_voice_update {
    ($stream:expr) => {{
        let event = next_real_event!($stream);
        let Some(Payload::VoiceStateUpdate(update)) = event.payload else {
            panic!("expected VoiceStateUpdate, got {event:?}");
        };
        (update.user_id, update.guild_id, update.state.as_option().cloned())
    }};
}

/// Whether no non-heartbeat event shows up within `window`.
macro_rules! no_event {
    ($stream:expr, $window:expr) => {{
        let deadline = tokio::time::Instant::now() + $window;
        let mut quiet = true;
        while let Ok(res) = tokio::time::timeout_at(deadline, $stream.message()).await {
            let event = res.expect("stream error").expect("stream ended").to_owned_message().event.into_option().unwrap();
            if !matches!(event.payload, Some(Payload::Heartbeat(_))) {
                quiet = false;
            }
        }
        quiet
    }};
}

fn claims(token: &str) -> Claims {
    TokenVerifier::with_api_key(lk::API_KEY, lk::API_SECRET).verify(token).expect("token verifies with the API secret")
}

fn state_of(app: &TestApp, user_id: u64) -> Option<thehideout_server::voice::VoiceEntry> {
    app.state.voice.get(user_id)
}

// ---------------------------------------------------------------- channels

#[tokio::test]
async fn channel_kind_is_set_on_create_and_listed() {
    let app = start().await;
    let alice = user(&app, "alice").await;
    let g = guild(&app, &alice).await;
    let voice_ch = voice_channel(&app, &alice, g.id, "lounge").await;
    let text_ch = make_channel(&app, &alice, g.id, "chat", ChannelKind::CHANNEL_KIND_UNSPECIFIED).await;
    let explicit = make_channel(&app, &alice, g.id, "chat2", ChannelKind::CHANNEL_KIND_TEXT).await;
    assert_eq!(voice_ch.kind.as_known(), Some(ChannelKind::CHANNEL_KIND_VOICE));
    assert_eq!(text_ch.kind.as_known(), Some(ChannelKind::CHANNEL_KIND_TEXT));
    assert_eq!(explicit.kind.as_known(), Some(ChannelKind::CHANNEL_KIND_TEXT));

    let listed = fixtures::channels(&app, &alice, g.id).await.unwrap().channels;
    let kind_of = |id: u64| listed.iter().find(|c| c.id == id).unwrap().kind.as_known();
    assert_eq!(kind_of(voice_ch.id), Some(ChannelKind::CHANNEL_KIND_VOICE));
    assert_eq!(kind_of(text_ch.id), Some(ChannelKind::CHANNEL_KIND_TEXT));
    // The guild's default channel is text: only the one voice channel is voice.
    assert_eq!(listed.iter().filter(|c| c.kind.as_known() == Some(ChannelKind::CHANNEL_KIND_VOICE)).count(), 1);

    // The kind cannot change: an update keeps it.
    let updated = fixtures::guilds(&app, &alice)
        .update_channel(UpdateChannelRequest {
            guild_id: g.id,
            channel_id: voice_ch.id,
            name: Some("lounge2".into()),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_owned()
        .channel
        .unwrap();
    assert_eq!(updated.kind.as_known(), Some(ChannelKind::CHANNEL_KIND_VOICE));
    app.cleanup().await;
}

#[tokio::test]
async fn voice_channels_reject_messages_but_text_channels_do_not() {
    let app = start().await;
    let alice = user(&app, "alice").await;
    let g = guild(&app, &alice).await;
    let voice_ch = voice_channel(&app, &alice, g.id, "lounge").await;
    let text_ch = fixtures::channels(&app, &alice, g.id).await.unwrap().channels[0].clone();
    let messages = app.client(MessageServiceClient::new, Some(&alice.token));
    let err = messages
        .send_message(SendMessageRequest { channel_id: voice_ch.id, content: "hi".into(), ..Default::default() })
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::FailedPrecondition);
    messages
        .send_message(SendMessageRequest { channel_id: text_ch.id, content: "hi".into(), ..Default::default() })
        .await
        .expect("text channel still accepts messages");
    app.cleanup().await;
}

// -------------------------------------------------------------------- join

#[tokio::test]
async fn join_checks_membership_channel_kind_and_configuration() {
    let app = start().await;
    let (alice, bob, outsider) = (user(&app, "alice").await, user(&app, "bob").await, user(&app, "outsider").await);
    let g = guild(&app, &alice).await;
    join(&app, &bob, &g).await;
    let voice_ch = voice_channel(&app, &alice, g.id, "lounge").await;
    let text_ch = fixtures::channels(&app, &alice, g.id).await.unwrap().channels[0].clone();

    // Not a member: the channel does not exist for them. Unknown ids too.
    assert_eq!(join_voice(&app, &outsider, voice_ch.id).await.unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(join_voice(&app, &bob, 424242).await.unwrap_err().code, ErrorCode::NotFound);
    // A text channel.
    assert_eq!(join_voice(&app, &bob, text_ch.id).await.unwrap_err().code, ErrorCode::FailedPrecondition);
    // Unauthenticated.
    let anon = app.client(VoiceServiceClient::new, None);
    let err = anon.join_voice(JoinVoiceRequest { channel_id: voice_ch.id, ..Default::default() }).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Unauthenticated);
    // A plain member (no roles) has CONNECT by default.
    join_voice(&app, &bob, voice_ch.id).await.expect("members may connect by default");
    assert!(app.state.voice.get(outsider.id).is_none());

    // Without LiveKit settings the channel lists but joining is UNAVAILABLE.
    let bare = TestApp::start().await;
    let carol = user(&bare, "carol").await;
    let bg = guild(&bare, &carol).await;
    let vc = voice_channel(&bare, &carol, bg.id, "lounge").await;
    assert_eq!(fixtures::channels(&bare, &carol, bg.id).await.unwrap().channels.iter().filter(|c| c.id == vc.id).count(), 1);
    let err = join_voice(&bare, &carol, vc.id).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Unavailable);
    assert!(err.message.as_deref().unwrap_or_default().contains("not configured"));
    bare.cleanup().await;
    app.cleanup().await;
}

#[tokio::test]
async fn join_returns_a_livekit_token_for_the_room() {
    let app = start().await;
    let (alice, bob) = (user(&app, "alice").await, user(&app, "bob").await);
    let g = guild(&app, &alice).await;
    join(&app, &bob, &g).await;
    let ch = voice_channel(&app, &alice, g.id, "lounge").await;

    let joined = join_voice(&app, &bob, ch.id).await.unwrap();
    assert_eq!(joined.channel_id, ch.id);
    assert_eq!(joined.url, lk::URL);
    let c = claims(&joined.token);
    assert_eq!(c.iss, lk::API_KEY);
    assert_eq!(c.sub, bob.id.to_string());
    assert_eq!(c.name, app.name("bob"));
    assert!(c.video.room_join);
    assert_eq!(c.video.room, format!("voice-{}", ch.id));
    assert_eq!(c.video.can_publish, Some(true));
    assert_eq!(c.video.can_subscribe, Some(true));
    assert_eq!(c.video.can_publish_sources, vec!["microphone", "camera", "screen_share", "screen_share_audio"]);
    assert!(!c.video.room_admin && !c.video.room_create && !c.video.room_list);
    let ttl = c.exp - c.nbf;
    assert!((500..=700).contains(&ttl), "ttl {ttl}");

    // A token signed with another secret does not verify.
    let forged = TokenVerifier::with_api_key(lk::API_KEY, "another-secret-another-secret-123456").verify(&joined.token);
    assert!(forged.is_err());
    app.cleanup().await;
}

#[tokio::test]
async fn a_user_is_in_one_voice_channel_at_a_time() {
    let app = start().await;
    let (alice, bob) = (user(&app, "alice").await, user(&app, "bob").await);
    let g = guild(&app, &alice).await;
    join(&app, &bob, &g).await;
    let (a, b) = (voice_channel(&app, &alice, g.id, "a").await, voice_channel(&app, &alice, g.id, "b").await);
    let mut stream = subscribe!(app, alice);

    join_voice(&app, &bob, a.id).await.unwrap();
    let (user_id, guild_id, state) = next_voice_update!(stream);
    assert_eq!((user_id, guild_id), (bob.id, g.id));
    assert_eq!(state.unwrap().channel_id, a.id);

    join_voice(&app, &bob, b.id).await.unwrap();
    let (_, _, state) = next_voice_update!(stream);
    assert_eq!(state.unwrap().channel_id, b.id);
    assert_eq!(app.state.voice.len(), 1);
    assert!(app.state.voice.in_channel(a.id).is_empty());
    // The old room's connection was closed so its token is useless.
    let fake = app.livekit.as_ref().unwrap();
    assert!(fake.removed(&format!("voice-{}", a.id), bob.id));
    assert!(!fake.removed(&format!("voice-{}", b.id), bob.id));

    // Joining the channel they are in is a reconnect: new token, same state, no kick.
    fake.clear();
    let again = join_voice(&app, &bob, b.id).await.unwrap();
    assert_eq!(claims(&again.token).video.room, format!("voice-{}", b.id));
    assert!(fake.calls().is_empty());
    assert_eq!(app.state.voice.len(), 1);

    // Leaving.
    voice(&app, &bob).leave_voice(LeaveVoiceRequest::default()).await.unwrap();
    assert!(state_of(&app, bob.id).is_none());
    assert!(fake.removed(&format!("voice-{}", b.id), bob.id));
    // Leaving again is a no-op.
    voice(&app, &bob).leave_voice(LeaveVoiceRequest::default()).await.unwrap();
    app.cleanup().await;
}

// ----------------------------------------------------------------- gateway

#[tokio::test]
async fn gateway_delivers_voice_updates_to_guild_members_only() {
    let app = start().await;
    let (alice, bob, carol) = (user(&app, "alice").await, user(&app, "bob").await, user(&app, "carol").await);
    let g = guild(&app, &alice).await;
    join(&app, &bob, &g).await;
    guild(&app, &carol).await;
    let ch = voice_channel(&app, &alice, g.id, "lounge").await;
    let mut alice_stream = subscribe!(app, alice);
    let mut carol_stream = subscribe!(app, carol);

    join_voice(&app, &bob, ch.id).await.unwrap();
    let (user_id, guild_id, state) = next_voice_update!(alice_stream);
    let state = state.expect("joined");
    assert_eq!((user_id, guild_id), (bob.id, g.id));
    assert_eq!((state.guild_id, state.channel_id, state.user_id), (g.id, ch.id, bob.id));
    assert!(!state.self_mute && !state.self_deaf && !state.server_mute && !state.self_video && !state.self_stream);
    assert!(state.joined_at.is_set());

    voice(&app, &bob).leave_voice(LeaveVoiceRequest::default()).await.unwrap();
    let (user_id, guild_id, state) = next_voice_update!(alice_stream);
    assert_eq!((user_id, guild_id), (bob.id, g.id));
    assert!(state.is_none(), "unset state means the user left");

    // Carol shares no guild with them.
    assert!(no_event!(carol_stream, Duration::from_millis(500)));
    app.cleanup().await;
}

#[tokio::test]
async fn a_fresh_stream_starts_with_a_snapshot_of_who_is_in_voice() {
    let app = start().await;
    let (alice, bob, carol) = (user(&app, "alice").await, user(&app, "bob").await, user(&app, "carol").await);
    let g = guild(&app, &alice).await;
    join(&app, &bob, &g).await;
    let ch = voice_channel(&app, &alice, g.id, "lounge").await;
    join_voice(&app, &bob, ch.id).await.unwrap();
    voice(&app, &bob)
        .update_voice_state(UpdateVoiceStateRequest { self_mute: Some(true), ..Default::default() })
        .await
        .unwrap();

    let mut stream = subscribe!(app, alice);
    let first = next_event!(stream);
    assert!(matches!(first.payload, Some(Payload::Heartbeat(_))));
    let second = next_event!(stream);
    assert_eq!(second.sequence, 2);
    let Some(Payload::VoiceSnapshot(snapshot)) = second.payload else { panic!("expected a snapshot, got {second:?}") };
    assert_eq!(snapshot.guild_ids, vec![g.id]);
    assert_eq!(snapshot.states.len(), 1);
    let s = &snapshot.states[0];
    assert_eq!((s.user_id, s.channel_id, s.guild_id, s.self_mute), (bob.id, ch.id, g.id, true));

    // A user of another guild gets no snapshot (nothing but heartbeats).
    guild(&app, &carol).await;
    let mut carol_stream = subscribe!(app, carol);
    assert!(no_event!(carol_stream, Duration::from_millis(500)));

    // Joining a guild with people in voice sends a snapshot of that guild after guild_added.
    join(&app, &carol, &g).await;
    let event = next_real_event!(carol_stream);
    assert!(matches!(event.payload, Some(Payload::GuildAdded(_))), "got {event:?}");
    let event = next_real_event!(carol_stream);
    let Some(Payload::VoiceSnapshot(snapshot)) = event.payload else { panic!("expected a snapshot, got {event:?}") };
    assert_eq!(snapshot.guild_ids, vec![g.id]);
    assert_eq!(snapshot.states.len(), 1);
    app.cleanup().await;
}

#[tokio::test]
async fn self_flags_update_and_are_broadcast() {
    let app = start().await;
    let (alice, bob) = (user(&app, "alice").await, user(&app, "bob").await);
    let g = guild(&app, &alice).await;
    join(&app, &bob, &g).await;
    let ch = voice_channel(&app, &alice, g.id, "lounge").await;

    // Not in voice yet.
    let err = voice(&app, &bob).update_voice_state(UpdateVoiceStateRequest::default()).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::FailedPrecondition);

    join_voice(&app, &bob, ch.id).await.unwrap();
    let mut stream = subscribe!(app, alice);
    let _snapshot = next_real_event!(stream); // bob is in voice: snapshot first
    let set = |m: Option<bool>, d: Option<bool>, v: Option<bool>, s: Option<bool>| UpdateVoiceStateRequest {
        self_mute: m,
        self_deaf: d,
        self_video: v,
        self_stream: s,
        ..Default::default()
    };
    let state =
        voice(&app, &bob).update_voice_state(set(Some(true), Some(true), None, None)).await.unwrap().into_owned().state.unwrap();
    assert!(state.self_mute && state.self_deaf && !state.self_video);
    let (_, _, seen) = next_voice_update!(stream);
    let seen = seen.unwrap();
    assert!(seen.self_mute && seen.self_deaf);

    // Unset fields stay; set ones change.
    let state = voice(&app, &bob)
        .update_voice_state(set(Some(false), None, Some(true), Some(true)))
        .await
        .unwrap()
        .into_owned()
        .state
        .unwrap();
    assert!(!state.self_mute && state.self_deaf && state.self_video && state.self_stream);
    let (_, _, seen) = next_voice_update!(stream);
    let seen = seen.unwrap();
    assert!(!seen.self_mute && seen.self_deaf && seen.self_video && seen.self_stream);
    assert!(!seen.server_mute);
    app.cleanup().await;
}

// -------------------------------------------------------------- moderation

struct Crew {
    app: TestApp,
    guild: Guild,
    owner: Account,
    senior: Account,
    mod_: Account,
    mod_peer: Account,
    mute_only: Account,
    plain: Account,
    lounge: Channel,
    other: Channel,
}

/// Owner, plus members with roles (top to bottom): senior and mod (mute +
/// move), muter (mute only); `plain` has no roles.
async fn crew() -> Crew {
    let app = start().await;
    let owner = user(&app, "owner").await;
    let guild = guild(&app, &owner).await;
    let mut members = Vec::new();
    for name in ["senior", "mod", "peer", "muter", "plain"] {
        let who = user(&app, name).await;
        join(&app, &who, &guild).await;
        members.push(who);
    }
    // New roles land at the bottom, so create from the most senior down.
    let senior_role = fixtures::role(&app, &owner, guild.id, "senior", MUTE_MEMBERS | MOVE_MEMBERS).await.unwrap();
    let mod_role = fixtures::role(&app, &owner, guild.id, "mod", MUTE_MEMBERS | MOVE_MEMBERS).await.unwrap();
    let muter_role = fixtures::role(&app, &owner, guild.id, "muter", MUTE_MEMBERS).await.unwrap();
    let [senior, mod_, mod_peer, mute_only, plain]: [Account; 5] = members.try_into().ok().unwrap();
    fixtures::set_roles(&app, &owner, guild.id, senior.id, &[senior_role.id]).await.unwrap();
    fixtures::set_roles(&app, &owner, guild.id, mod_.id, &[mod_role.id]).await.unwrap();
    fixtures::set_roles(&app, &owner, guild.id, mod_peer.id, &[mod_role.id]).await.unwrap();
    fixtures::set_roles(&app, &owner, guild.id, mute_only.id, &[muter_role.id]).await.unwrap();
    let lounge = voice_channel(&app, &owner, guild.id, "lounge").await;
    let other = voice_channel(&app, &owner, guild.id, "other").await;
    Crew { app, guild, owner, senior, mod_, mod_peer, mute_only, plain, lounge, other }
}

async fn server_mute(c: &Crew, who: &Account, target: u64, muted: bool) -> Result<VoiceState, connectrpc::ConnectError> {
    Ok(voice(&c.app, who)
        .server_mute_member(ServerMuteMemberRequest { guild_id: c.guild.id, user_id: target, muted, ..Default::default() })
        .await?
        .into_owned()
        .state
        .unwrap())
}

async fn move_member(c: &Crew, who: &Account, target: u64, channel_id: u64) -> Result<VoiceState, connectrpc::ConnectError> {
    Ok(voice(&c.app, who)
        .move_member(MoveMemberRequest { guild_id: c.guild.id, user_id: target, channel_id, ..Default::default() })
        .await?
        .into_owned()
        .state
        .unwrap())
}

async fn disconnect(c: &Crew, who: &Account, target: u64) -> Result<(), connectrpc::ConnectError> {
    voice(&c.app, who)
        .disconnect_member(DisconnectMemberRequest { guild_id: c.guild.id, user_id: target, ..Default::default() })
        .await?;
    Ok(())
}

#[tokio::test]
async fn moderation_needs_the_permission_and_a_higher_role() {
    let c = crew().await;
    for who in [&c.owner, &c.senior, &c.mod_, &c.mod_peer, &c.plain] {
        join_voice(&c.app, who, c.lounge.id).await.unwrap();
    }
    let denied = ErrorCode::PermissionDenied;

    // No permission at all / only the other permission.
    assert_eq!(server_mute(&c, &c.plain, c.mod_.id, true).await.unwrap_err().code, denied);
    assert_eq!(move_member(&c, &c.plain, c.mod_.id, c.other.id).await.unwrap_err().code, denied);
    assert_eq!(disconnect(&c, &c.plain, c.mod_.id).await.unwrap_err().code, denied);
    assert_eq!(move_member(&c, &c.mute_only, c.plain.id, c.other.id).await.unwrap_err().code, denied);
    assert_eq!(disconnect(&c, &c.mute_only, c.plain.id).await.unwrap_err().code, denied);
    // An outsider cannot even see the guild.
    let outsider = user(&c.app, "outsider").await;
    assert_eq!(server_mute(&c, &outsider, c.plain.id, true).await.unwrap_err().code, ErrorCode::NotFound);

    // Hierarchy: not the owner, not someone above or level with you.
    assert_eq!(server_mute(&c, &c.mod_, c.owner.id, true).await.unwrap_err().code, denied);
    assert_eq!(server_mute(&c, &c.mod_, c.senior.id, true).await.unwrap_err().code, denied);
    assert_eq!(server_mute(&c, &c.mod_, c.mod_peer.id, true).await.unwrap_err().code, denied);
    assert_eq!(move_member(&c, &c.mod_, c.senior.id, c.other.id).await.unwrap_err().code, denied);
    assert_eq!(disconnect(&c, &c.mod_, c.mod_peer.id).await.unwrap_err().code, denied);
    assert!(state_of(&c.app, c.senior.id).is_some_and(|e| !e.server_mute && e.channel_id == c.lounge.id));

    // Below you, and the owner over anyone, works.
    server_mute(&c, &c.mod_, c.plain.id, true).await.expect("mod mutes a member without roles");
    server_mute(&c, &c.senior, c.mod_.id, true).await.expect("senior mutes mod");
    server_mute(&c, &c.owner, c.senior.id, true).await.expect("owner mutes anyone");
    // Not yourself.
    assert_eq!(server_mute(&c, &c.mod_, c.mod_.id, true).await.unwrap_err().code, ErrorCode::InvalidArgument);

    // Targets must be members in voice.
    assert_eq!(server_mute(&c, &c.owner, 999_999, true).await.unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(server_mute(&c, &c.owner, c.mute_only.id, true).await.unwrap_err().code, ErrorCode::FailedPrecondition);
    c.app.cleanup().await;
}

#[tokio::test]
async fn server_mute_is_enforced_in_livekit_and_survives_a_move() {
    let c = crew().await;
    let fake = c.app.livekit.clone().unwrap();
    join_voice(&c.app, &c.plain, c.lounge.id).await.unwrap();
    let mut owner_stream = subscribe!(c.app, c.owner);
    let _snapshot = next_real_event!(owner_stream);
    let room = format!("voice-{}", c.lounge.id);

    let state = server_mute(&c, &c.mod_, c.plain.id, true).await.unwrap();
    assert!(state.server_mute);
    let (_, _, seen) = next_voice_update!(owner_stream);
    assert!(seen.unwrap().server_mute);
    // No microphone any more; camera and screen share remain.
    assert_eq!(
        fake.calls().last().cloned(),
        Some(Call::Sources {
            room: room.clone(),
            identity: c.plain.id.to_string(),
            sources: vec![PublishSource::Camera, PublishSource::ScreenShare, PublishSource::ScreenShareAudio],
        })
    );

    // Rejoining a channel (here: after a move) keeps the mute, and the new token has no microphone.
    let moved = move_member(&c, &c.mod_, c.plain.id, c.other.id).await.unwrap();
    assert!(moved.server_mute && moved.channel_id == c.other.id);
    let token = join_voice(&c.app, &c.plain, c.other.id).await.unwrap().token;
    assert_eq!(claims(&token).video.can_publish_sources, vec!["camera", "screen_share", "screen_share_audio"]);

    // Unmute restores the microphone.
    let state = server_mute(&c, &c.mod_, c.plain.id, false).await.unwrap();
    assert!(!state.server_mute);
    assert_eq!(
        fake.calls().last().cloned(),
        Some(Call::Sources {
            room: format!("voice-{}", c.other.id),
            identity: c.plain.id.to_string(),
            sources: vec![
                PublishSource::Microphone,
                PublishSource::Camera,
                PublishSource::ScreenShare,
                PublishSource::ScreenShareAudio
            ],
        })
    );
    c.app.cleanup().await;
}

#[tokio::test]
async fn muting_before_the_member_connects_is_enforced_when_they_do() {
    let c = crew().await;
    let fake = c.app.livekit.clone().unwrap();
    join_voice(&c.app, &c.plain, c.lounge.id).await.unwrap();
    server_mute(&c, &c.mod_, c.plain.id, true).await.unwrap();
    fake.clear();
    let (body, auth) =
        lk::signed_webhook(lk::API_SECRET, "participant_joined", &format!("voice-{}", c.lounge.id), &c.plain.id.to_string());
    assert_eq!(post_webhook(&c.app, Some(&auth), &body).await, 200);
    assert!(matches!(fake.calls().as_slice(), [Call::Sources { sources, .. }] if !sources.contains(&PublishSource::Microphone)));
    c.app.cleanup().await;
}

#[tokio::test]
async fn move_and_disconnect_flows() {
    let c = crew().await;
    let fake = c.app.livekit.clone().unwrap();
    join_voice(&c.app, &c.plain, c.lounge.id).await.unwrap();
    let mut plain_stream = subscribe!(c.app, c.plain);
    let mut owner_stream = subscribe!(c.app, c.owner);
    let text_channel = fixtures::channels(&c.app, &c.owner, c.guild.id).await.unwrap().channels[0].clone();
    let (_snap_p, _snap_o) = (next_real_event!(plain_stream), next_real_event!(owner_stream));

    // Destination checks.
    assert_eq!(move_member(&c, &c.mod_, c.plain.id, text_channel.id).await.unwrap_err().code, ErrorCode::FailedPrecondition);
    assert_eq!(move_member(&c, &c.mod_, c.plain.id, 777_777).await.unwrap_err().code, ErrorCode::NotFound);
    let foreign = {
        let stranger = user(&c.app, "stranger").await;
        let g2 = guild(&c.app, &stranger).await;
        voice_channel(&c.app, &stranger, g2.id, "elsewhere").await
    };
    assert_eq!(move_member(&c, &c.mod_, c.plain.id, foreign.id).await.unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(move_member(&c, &c.mod_, c.mute_only.id, c.other.id).await.unwrap_err().code, ErrorCode::FailedPrecondition);

    // The move: state switches, the old participant is removed, the client is told where to go.
    let moved = move_member(&c, &c.mod_, c.plain.id, c.other.id).await.unwrap();
    assert_eq!(moved.channel_id, c.other.id);
    assert!(fake.removed(&format!("voice-{}", c.lounge.id), c.plain.id));
    let (_, _, seen) = next_voice_update!(owner_stream);
    assert_eq!(seen.unwrap().channel_id, c.other.id);
    let mut got_moved = None;
    for _ in 0..4 {
        let event = next_real_event!(plain_stream);
        if let Some(Payload::VoiceMoved(m)) = event.payload {
            got_moved = Some(m);
            break;
        }
    }
    let got_moved = got_moved.expect("the moved user gets voice_moved");
    assert_eq!((got_moved.guild_id, got_moved.channel_id), (c.guild.id, c.other.id));
    // The client follows up with JoinVoice for the new channel.
    let token = join_voice(&c.app, &c.plain, c.other.id).await.unwrap().token;
    assert_eq!(claims(&token).video.room, format!("voice-{}", c.other.id));
    let (_, _, rejoined) = next_voice_update!(owner_stream); // the reconnect republishes the state
    assert_eq!(rejoined.unwrap().channel_id, c.other.id);
    // Moving to where they already are changes nothing.
    fake.clear();
    move_member(&c, &c.mod_, c.plain.id, c.other.id).await.unwrap();
    assert!(fake.calls().is_empty());

    // Disconnect: gone from voice, removed from the room, guild told.
    disconnect(&c, &c.mod_, c.plain.id).await.unwrap();
    assert!(state_of(&c.app, c.plain.id).is_none());
    assert!(fake.removed(&format!("voice-{}", c.other.id), c.plain.id));
    let (user_id, _, seen) = next_voice_update!(owner_stream);
    assert_eq!(user_id, c.plain.id);
    assert!(seen.is_none());
    assert_eq!(disconnect(&c, &c.mod_, c.plain.id).await.unwrap_err().code, ErrorCode::FailedPrecondition);
    c.app.cleanup().await;
}

// ------------------------------------------------------------- stale state

#[tokio::test]
async fn deleting_a_channel_or_losing_membership_clears_voice() {
    let app = start().await;
    let (alice, bob, carol) = (user(&app, "alice").await, user(&app, "bob").await, user(&app, "carol").await);
    let g = guild(&app, &alice).await;
    join(&app, &bob, &g).await;
    join(&app, &carol, &g).await;
    let ch = voice_channel(&app, &alice, g.id, "lounge").await;
    let other = voice_channel(&app, &alice, g.id, "other").await;
    let fake = app.livekit.clone().unwrap();
    join_voice(&app, &bob, ch.id).await.unwrap();
    join_voice(&app, &carol, other.id).await.unwrap();
    let mut stream = subscribe!(app, alice);
    let _snapshot = next_real_event!(stream);

    thehideout_server::voice::on_channel_deleted(&app.state, ch.id).await;
    let (user_id, _, state) = next_voice_update!(stream);
    assert_eq!(user_id, bob.id);
    assert!(state.is_none());
    assert!(app.state.voice.get(bob.id).is_none() && app.state.voice.get(carol.id).is_some());
    assert!(fake.removed(&format!("voice-{}", ch.id), bob.id));

    // A member who leaves or is banned.
    thehideout_server::voice::on_member_removed(&app.state, g.id, carol.id).await;
    let (user_id, _, state) = next_voice_update!(stream);
    assert_eq!(user_id, carol.id);
    assert!(state.is_none());
    assert!(fake.removed(&format!("voice-{}", other.id), carol.id));
    // Another guild's removal does nothing.
    join_voice(&app, &carol, other.id).await.unwrap();
    thehideout_server::voice::on_member_removed(&app.state, g.id + 1, carol.id).await;
    assert!(app.state.voice.get(carol.id).is_some());
    app.cleanup().await;
}

async fn post_webhook(app: &TestApp, auth: Option<&str>, body: &str) -> u16 {
    let mut request = reqwest::Client::new()
        .post(format!("{}/livekit/webhook", app.base_url))
        .header("content-type", "application/webhook+json")
        .body(body.to_owned());
    if let Some(auth) = auth {
        request = request.header("authorization", auth);
    }
    request.send().await.unwrap().status().as_u16()
}

#[tokio::test]
async fn webhooks_with_a_valid_signature_clear_state_and_invalid_ones_are_rejected() {
    let app = start().await;
    let (alice, bob, carol) = (user(&app, "alice").await, user(&app, "bob").await, user(&app, "carol").await);
    let g = guild(&app, &alice).await;
    join(&app, &bob, &g).await;
    join(&app, &carol, &g).await;
    let ch = voice_channel(&app, &alice, g.id, "lounge").await;
    let other = voice_channel(&app, &alice, g.id, "other").await;
    let room = format!("voice-{}", ch.id);
    join_voice(&app, &bob, ch.id).await.unwrap();
    join_voice(&app, &carol, ch.id).await.unwrap();
    let mut stream = subscribe!(app, alice);
    let _snapshot = next_real_event!(stream);
    let fake = app.livekit.clone().unwrap();
    fake.clear();

    // Rejected: no signature, wrong secret, a body that does not match the signed hash, garbage.
    let (body, auth) = lk::signed_webhook(lk::API_SECRET, "participant_left", &room, &bob.id.to_string());
    assert_eq!(post_webhook(&app, None, &body).await, 401);
    let (forged_body, forged) =
        lk::signed_webhook("another-secret-another-secret-123456", "participant_left", &room, &bob.id.to_string());
    assert_eq!(post_webhook(&app, Some(&forged), &forged_body).await, 401);
    assert_eq!(post_webhook(&app, Some(&auth), &body.replace("participant_left", "room_finished")).await, 401);
    assert_eq!(post_webhook(&app, Some("garbage"), &body).await, 401);
    assert!(app.state.voice.get(bob.id).is_some(), "rejected webhooks change nothing");

    // Accepted: participant_left removes just that user and tells the guild.
    assert_eq!(post_webhook(&app, Some(&auth), &body).await, 200);
    let (user_id, _, state) = next_voice_update!(stream);
    assert_eq!(user_id, bob.id);
    assert!(state.is_none());
    assert!(app.state.voice.get(bob.id).is_none() && app.state.voice.get(carol.id).is_some());
    // The participant is already gone: no server-API call needed.
    assert!(fake.calls().is_empty());

    // A stale participant_left (the user moved on) does not drop their new state.
    join_voice(&app, &carol, other.id).await.unwrap();
    let (stale_body, stale_auth) = lk::signed_webhook(lk::API_SECRET, "participant_left", &room, &carol.id.to_string());
    assert_eq!(post_webhook(&app, Some(&stale_auth), &stale_body).await, 200);
    assert_eq!(app.state.voice.get(carol.id).map(|e| e.channel_id), Some(other.id));

    // room_finished clears the room.
    join_voice(&app, &bob, other.id).await.unwrap();
    let (body, auth) = lk::signed_webhook(lk::API_SECRET, "room_finished", &format!("voice-{}", other.id), "");
    assert_eq!(post_webhook(&app, Some(&auth), &body).await, 200);
    assert!(app.state.voice.is_empty());

    // A participant who joined without our say-so (a stale token) is disconnected.
    fake.clear();
    let (body, auth) = lk::signed_webhook(lk::API_SECRET, "participant_joined", &room, &bob.id.to_string());
    assert_eq!(post_webhook(&app, Some(&auth), &body).await, 200);
    assert!(fake.removed(&room, bob.id));
    assert!(app.state.voice.is_empty());

    // Unrelated rooms and events are ignored.
    let (other_body, other_auth) = lk::signed_webhook(lk::API_SECRET, "room_started", "lobby", "");
    assert_eq!(post_webhook(&app, Some(&other_auth), &other_body).await, 200);
    app.cleanup().await;

    // Without LiveKit settings the endpoint is unavailable.
    let bare = TestApp::start().await;
    assert_eq!(post_webhook(&bare, Some(&auth), &body).await, 503);
    bare.cleanup().await;
}

#[tokio::test]
async fn a_user_who_never_connects_is_dropped_and_a_connected_one_stays() {
    let app = TestApp::builder()
        .livekit()
        .heartbeat(Duration::from_millis(200))
        .connect_grace(Duration::from_millis(600))
        .start()
        .await;
    let (alice, bob, carol) = (user(&app, "alice").await, user(&app, "bob").await, user(&app, "carol").await);
    let g = guild(&app, &alice).await;
    join(&app, &bob, &g).await;
    join(&app, &carol, &g).await;
    let ch = voice_channel(&app, &alice, g.id, "lounge").await;
    let mut stream = subscribe!(app, alice);

    join_voice(&app, &bob, ch.id).await.unwrap();
    join_voice(&app, &carol, ch.id).await.unwrap();
    let (body, auth) =
        lk::signed_webhook(lk::API_SECRET, "participant_joined", &format!("voice-{}", ch.id), &carol.id.to_string());
    assert_eq!(post_webhook(&app, Some(&auth), &body).await, 200);

    // Two joins, then bob's drop.
    let mut dropped = None;
    for _ in 0..4 {
        let (user_id, _, state) = next_voice_update!(stream);
        if state.is_none() {
            dropped = Some(user_id);
            break;
        }
    }
    assert_eq!(dropped, Some(bob.id));
    assert!(app.state.voice.get(bob.id).is_none());
    assert!(app.state.voice.get(carol.id).is_some_and(|e| e.connected));
    // The sweeper also disconnects the dropped participant (it may be live after all,
    // with its participant_joined lost), and leaves the connected one alone.
    let fake = app.livekit.clone().unwrap();
    eventually("the sweeper kicks the dropped participant", || fake.removed(&format!("voice-{}", ch.id), bob.id)).await;
    assert!(!fake.removed(&format!("voice-{}", ch.id), carol.id));
    app.cleanup().await;
}

/// Polls `cond` for up to two seconds.
async fn eventually(what: &str, mut cond: impl FnMut() -> bool) {
    for _ in 0..40 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for: {what}");
}

/// Sends a signed webhook `event` for the participant `who` (connection `sid`) of a voice channel.
async fn participant_event(app: &TestApp, event: &str, channel_id: u64, who: u64, sid: &str) -> u16 {
    let (body, auth) =
        lk::signed_webhook_sid(lk::API_SECRET, event, &format!("voice-{channel_id}"), &who.to_string(), sid);
    post_webhook(app, Some(&auth), &body).await
}

#[tokio::test]
async fn a_same_channel_rejoin_keeps_the_connection_and_the_sweeper_leaves_it_alone() {
    let app = TestApp::builder()
        .livekit()
        .heartbeat(Duration::from_millis(200))
        .connect_grace(Duration::from_millis(600))
        .start()
        .await;
    let (alice, bob, carol) = (user(&app, "alice").await, user(&app, "bob").await, user(&app, "carol").await);
    let g = guild(&app, &alice).await;
    join(&app, &bob, &g).await;
    join(&app, &carol, &g).await;
    let ch = voice_channel(&app, &alice, g.id, "lounge").await;
    let fake = app.livekit.clone().unwrap();

    join_voice(&app, &bob, ch.id).await.unwrap();
    assert_eq!(participant_event(&app, "participant_joined", ch.id, bob.id, "PA_1").await, 200);
    let before = state_of(&app, bob.id).unwrap();
    assert!(before.connected);
    let again = join_voice(&app, &bob, ch.id).await.unwrap();
    assert_eq!(claims(&again.token).video.room, format!("voice-{}", ch.id));
    let after = state_of(&app, bob.id).unwrap();
    assert!(after.connected && after.sid.as_deref() == Some("PA_1"), "a rejoin keeps the confirmed connection");
    assert_eq!((after.since, after.joined_at), (before.since, before.joined_at));

    // A join that never connected keeps its original grace timer when it is repeated.
    join_voice(&app, &carol, ch.id).await.unwrap();
    let first = state_of(&app, carol.id).unwrap();
    join_voice(&app, &carol, ch.id).await.unwrap();
    let second = state_of(&app, carol.id).unwrap();
    assert!(!second.connected && second.since == first.since);

    // Well past the grace, bob (connected) is still there and was never kicked; carol is dropped.
    eventually("the unconnected join is dropped", || state_of(&app, carol.id).is_none()).await;
    tokio::time::sleep(Duration::from_millis(900)).await;
    assert!(state_of(&app, bob.id).is_some_and(|e| e.connected));
    assert!(!fake.removed(&format!("voice-{}", ch.id), bob.id));
    app.cleanup().await;
}

#[tokio::test]
async fn a_late_participant_left_of_a_replaced_connection_keeps_the_fresh_rejoin() {
    let app = start().await;
    let (alice, bob) = (user(&app, "alice").await, user(&app, "bob").await);
    let g = guild(&app, &alice).await;
    join(&app, &bob, &g).await;
    let ch = voice_channel(&app, &alice, g.id, "lounge").await;

    join_voice(&app, &bob, ch.id).await.unwrap();
    assert_eq!(participant_event(&app, "participant_joined", ch.id, bob.id, "PA_old").await, 200);
    // Rejoin: LiveKit replaces the old connection, confirms the new one, then reports the old one gone.
    join_voice(&app, &bob, ch.id).await.unwrap();
    assert_eq!(participant_event(&app, "participant_joined", ch.id, bob.id, "PA_new").await, 200);
    assert_eq!(state_of(&app, bob.id).unwrap().sid.as_deref(), Some("PA_new"));
    assert_eq!(participant_event(&app, "participant_left", ch.id, bob.id, "PA_old").await, 200);
    assert!(state_of(&app, bob.id).is_some_and(|e| e.connected), "the old connection's leave is ignored");
    // The current connection leaving does clear the state.
    assert_eq!(participant_event(&app, "participant_left", ch.id, bob.id, "PA_new").await, 200);
    assert!(state_of(&app, bob.id).is_none());
    app.cleanup().await;
}

#[tokio::test]
async fn a_server_mute_survives_leaving_rejoining_and_another_guild() {
    let c = crew().await;
    let fake = c.app.livekit.clone().unwrap();
    let no_mic = vec!["camera", "screen_share", "screen_share_audio"];
    let all = vec!["microphone", "camera", "screen_share", "screen_share_audio"];
    join_voice(&c.app, &c.plain, c.lounge.id).await.unwrap();
    assert!(server_mute(&c, &c.mod_, c.plain.id, true).await.unwrap().server_mute);

    // Leave, rejoin: still muted, in the state and in the token.
    voice(&c.app, &c.plain).leave_voice(LeaveVoiceRequest::default()).await.unwrap();
    assert!(state_of(&c.app, c.plain.id).is_none());
    // A mute needs the target in voice (and fails without changing anything when they are not).
    assert_eq!(server_mute(&c, &c.mod_, c.plain.id, false).await.unwrap_err().code, ErrorCode::FailedPrecondition);
    assert!(c.app.state.voice.server_muted(c.guild.id, c.plain.id));
    let token = join_voice(&c.app, &c.plain, c.lounge.id).await.unwrap().token;
    assert_eq!(claims(&token).video.can_publish_sources, no_mic);
    assert!(state_of(&c.app, c.plain.id).unwrap().server_mute);
    // ... also when LiveKit confirms the connection.
    fake.clear();
    assert_eq!(participant_event(&c.app, "participant_joined", c.lounge.id, c.plain.id, "PA_1").await, 200);
    assert!(matches!(fake.calls().as_slice(), [Call::Sources { sources, .. }] if !sources.contains(&PublishSource::Microphone)));

    // Another guild's voice has no mute for them; coming back to this guild brings it back.
    let (stranger, plain_elsewhere) = (user(&c.app, "stranger").await, c.plain.id);
    let g2 = guild(&c.app, &stranger).await;
    join(&c.app, &c.plain, &g2).await;
    let elsewhere = voice_channel(&c.app, &stranger, g2.id, "elsewhere").await;
    let token = join_voice(&c.app, &c.plain, elsewhere.id).await.unwrap().token;
    assert_eq!(claims(&token).video.can_publish_sources, all);
    assert!(state_of(&c.app, plain_elsewhere).is_some_and(|e| e.guild_id == g2.id && !e.server_mute));
    let token = join_voice(&c.app, &c.plain, c.lounge.id).await.unwrap().token;
    assert_eq!(claims(&token).video.can_publish_sources, no_mic);
    assert!(state_of(&c.app, c.plain.id).is_some_and(|e| e.guild_id == c.guild.id && e.server_mute));

    // Only an unmute lifts it.
    assert!(!server_mute(&c, &c.mod_, c.plain.id, false).await.unwrap().server_mute);
    voice(&c.app, &c.plain).leave_voice(LeaveVoiceRequest::default()).await.unwrap();
    let token = join_voice(&c.app, &c.plain, c.lounge.id).await.unwrap().token;
    assert_eq!(claims(&token).video.can_publish_sources, all);
    assert!(!state_of(&c.app, c.plain.id).unwrap().server_mute);
    c.app.cleanup().await;
}

#[tokio::test]
async fn a_server_mute_of_a_member_who_left_or_moved_applies_nothing() {
    let c = crew().await;
    join_voice(&c.app, &c.plain, c.lounge.id).await.unwrap();
    // The checks saw `plain` in the lounge; by the time the mute is applied they moved on.
    // `set_server_mute` is what ServerMuteMember applies, atomically, to the entry it checked.
    let states = &c.app.state.voice;
    states.join(c.plain.id, c.guild.id, c.other.id);
    assert!(states.set_server_mute(c.guild.id, c.plain.id, c.lounge.id, true).is_none());
    assert!(!states.server_muted(c.guild.id, c.plain.id));
    assert!(!state_of(&c.app, c.plain.id).unwrap().server_mute);
    // The same call for where they are now applies, and the RPC then publishes and enforces.
    let fake = c.app.livekit.clone().unwrap();
    fake.clear();
    assert!(server_mute(&c, &c.mod_, c.plain.id, true).await.unwrap().server_mute);
    assert_eq!(fake.calls().len(), 1);
    // A leave right before the mute: the RPC fails and records nothing.
    voice(&c.app, &c.plain).leave_voice(LeaveVoiceRequest::default()).await.unwrap();
    assert!(states.set_server_mute(c.guild.id, c.plain.id, c.other.id, false).is_none());
    assert!(states.server_muted(c.guild.id, c.plain.id), "an unmute that finds nobody changes nothing");
    c.app.cleanup().await;
}

#[tokio::test]
async fn role_changes_during_a_call_reapply_the_publish_permissions() {
    let c = crew().await;
    let fake = c.app.livekit.clone().unwrap();
    let everything = vec![
        PublishSource::Microphone,
        PublishSource::Camera,
        PublishSource::ScreenShare,
        PublishSource::ScreenShareAudio,
    ];
    let sources_of = |who: &Account| -> Vec<Call> {
        fake.calls()
            .into_iter()
            .filter(|call| matches!(call, Call::Sources { identity, .. } if *identity == who.id.to_string()))
            .collect()
    };
    let expected = |who: &Account| Call::Sources {
        room: format!("voice-{}", c.lounge.id),
        identity: who.id.to_string(),
        sources: everything.clone(),
    };
    // `mute_only` is connected; `plain` joined but LiveKit has not confirmed them.
    join_voice(&c.app, &c.mute_only, c.lounge.id).await.unwrap();
    assert_eq!(participant_event(&c.app, "participant_joined", c.lounge.id, c.mute_only.id, "PA_m").await, 200);
    join_voice(&c.app, &c.plain, c.lounge.id).await.unwrap();
    let extra = fixtures::role(&c.app, &c.owner, c.guild.id, "extra", 0).await.unwrap();
    fake.clear();

    // A member's roles change.
    fixtures::set_roles(&c.app, &c.owner, c.guild.id, c.mute_only.id, &[extra.id]).await.unwrap();
    assert_eq!(sources_of(&c.mute_only), vec![expected(&c.mute_only)]);
    // A member who is not in voice is not touched.
    fake.clear();
    fixtures::set_roles(&c.app, &c.owner, c.guild.id, c.senior.id, &[extra.id]).await.unwrap();
    assert!(fake.calls().is_empty());

    // A role's permissions change: everyone connected in the guild is re-enforced (not the unconfirmed one).
    roles_client(&c).update_role(UpdateRoleRequest {
        guild_id: c.guild.id,
        role_id: extra.id,
        permissions: Some(MUTE_MEMBERS),
        ..Default::default()
    })
    .await
    .unwrap();
    assert_eq!(sources_of(&c.mute_only), vec![expected(&c.mute_only)]);
    assert!(sources_of(&c.plain).is_empty());

    // Renaming alone changes no permissions.
    fake.clear();
    roles_client(&c).update_role(UpdateRoleRequest {
        guild_id: c.guild.id,
        role_id: extra.id,
        name: Some("renamed".into()),
        ..Default::default()
    })
    .await
    .unwrap();
    assert!(fake.calls().is_empty());

    // Deleting a role.
    roles_client(&c)
        .delete_role(DeleteRoleRequest { guild_id: c.guild.id, role_id: extra.id, ..Default::default() })
        .await
        .unwrap();
    assert_eq!(sources_of(&c.mute_only), vec![expected(&c.mute_only)]);
    // The unconfirmed member gets theirs when LiveKit confirms them.
    assert_eq!(participant_event(&c.app, "participant_joined", c.lounge.id, c.plain.id, "PA_p").await, 200);
    assert_eq!(sources_of(&c.plain), vec![expected(&c.plain)]);
    c.app.cleanup().await;
}

fn roles_client(c: &Crew) -> RoleServiceClient<connectrpc::client::HttpClient> {
    fixtures::roles(&c.app, &c.owner)
}

// ------------------------------------------------------------------ config

#[test]
fn livekit_api_url_defaults_to_the_http_form_of_the_client_url() {
    use thehideout_server::config::LiveKitConfig;
    let cfg = |url: &str, api: Option<&str>| LiveKitConfig::new(url, api, "k".into(), "s".into());
    assert_eq!(cfg("ws://127.0.0.1:7880", None).unwrap().api_url, "http://127.0.0.1:7880");
    assert_eq!(cfg("wss://lk.example.test/", None).unwrap().api_url, "https://lk.example.test");
    assert_eq!(cfg("wss://lk.example.test", Some("http://livekit:7880/")).unwrap().api_url, "http://livekit:7880");
    assert_eq!(cfg("wss://lk.example.test", None).unwrap().url, "wss://lk.example.test");
    assert!(cfg("ftp://x", None).is_err());
    assert!(cfg("nonsense", None).is_err());
    assert!(!format!("{:?}", cfg("ws://h:1", None).unwrap()).contains("\"s\""), "the secret is redacted in Debug");
}
