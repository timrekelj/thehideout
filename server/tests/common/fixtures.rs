//! Helpers for the roles / layout / events tests.
#![allow(dead_code)]

use connectrpc::ConnectError;
use connectrpc::client::HttpClient;
use thehideout_server::proto::thehideout::v1::*;

use super::TestApp;

pub const PASSWORD: &str = "correct horse battery";

pub const ADMINISTRATOR: u64 = 1;
pub const MANAGE_SERVER: u64 = 2;
pub const MANAGE_CHANNELS: u64 = 4;
pub const MANAGE_ROLES: u64 = 8;
pub const MANAGE_EMOJIS: u64 = 16;
pub const INVITE_PEOPLE: u64 = 32;
pub const CONNECT: u64 = 64;
pub const SPEAK: u64 = 128;
pub const VIDEO: u64 = 256;
pub const MUTE_MEMBERS: u64 = 512;
pub const MOVE_MEMBERS: u64 = 1024;

pub struct Account {
    pub token: String,
    pub id: u64,
}

/// Creates the account `username` (unique per app, email
/// `<name>@example.test`) directly on the server (sign-up is invite-only) and
/// signs it in. A transient database error is retried until a generous deadline.
pub async fn user(app: &TestApp, username: &str) -> Account {
    let name = app.name(username);
    let email = format!("{name}@example.test");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let account = loop {
        let created = thehideout_server::accounts::create_user(
            &app.state,
            thehideout_server::accounts::NewUser { username: &name, password: Some(PASSWORD), email: Some(&email) },
        )
        .await;
        match created {
            Ok(account) => break account,
            Err(e) if std::time::Instant::now() < deadline && e.code != connectrpc::ErrorCode::AlreadyExists => {
                eprintln!("creating {name} failed ({e}); retrying");
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            Err(e) => panic!("create user {name}: {e}"),
        }
    };
    let token = thehideout_server::auth::create_session(&app.state, account.id, "test").await.expect("create session");
    Account { token, id: account.id }
}

pub fn guilds(app: &TestApp, who: &Account) -> GuildServiceClient<HttpClient> {
    app.client(GuildServiceClient::new, Some(&who.token))
}

pub fn roles(app: &TestApp, who: &Account) -> RoleServiceClient<HttpClient> {
    app.client(RoleServiceClient::new, Some(&who.token))
}

pub async fn guild(app: &TestApp, owner: &Account) -> Guild {
    guilds(app, owner)
        .create_guild(CreateGuildRequest {
            name: "guild".into(),
            ..Default::default()
        })
        .await
        .expect("create guild")
        .into_owned()
        .guild
        .unwrap()
}

pub async fn join(app: &TestApp, who: &Account, guild: &Guild) {
    guilds(app, who)
        .join_guild(JoinGuildRequest {
            invite_code: guild.invite_code.clone(),
            ..Default::default()
        })
        .await
        .expect("join guild");
}

pub async fn role(
    app: &TestApp,
    who: &Account,
    guild_id: u64,
    name: &str,
    permissions: u64,
) -> Result<Role, ConnectError> {
    Ok(roles(app, who)
        .create_role(CreateRoleRequest {
            guild_id,
            name: name.into(),
            permissions,
            ..Default::default()
        })
        .await?
        .into_owned()
        .role
        .unwrap())
}

pub async fn list_roles(app: &TestApp, who: &Account, guild_id: u64) -> Result<ListRolesResponse, ConnectError> {
    Ok(roles(app, who)
        .list_roles(ListRolesRequest {
            guild_id,
            ..Default::default()
        })
        .await?
        .into_owned())
}

pub async fn set_roles(
    app: &TestApp,
    who: &Account,
    guild_id: u64,
    user_id: u64,
    role_ids: &[u64],
) -> Result<Member, ConnectError> {
    Ok(roles(app, who)
        .set_member_roles(SetMemberRolesRequest {
            guild_id,
            user_id,
            role_ids: role_ids.to_vec(),
            ..Default::default()
        })
        .await?
        .into_owned()
        .member
        .unwrap())
}

pub async fn channels(app: &TestApp, who: &Account, guild_id: u64) -> Result<ListChannelsResponse, ConnectError> {
    Ok(guilds(app, who)
        .list_channels(ListChannelsRequest {
            guild_id,
            ..Default::default()
        })
        .await?
        .into_owned())
}

pub async fn channel(app: &TestApp, who: &Account, guild_id: u64, name: &str) -> Result<Channel, ConnectError> {
    Ok(guilds(app, who)
        .create_channel(CreateChannelRequest {
            guild_id,
            name: name.into(),
            ..Default::default()
        })
        .await?
        .into_owned()
        .channel
        .unwrap())
}

pub async fn section(app: &TestApp, who: &Account, guild_id: u64, name: &str) -> Result<Section, ConnectError> {
    Ok(guilds(app, who)
        .create_section(CreateSectionRequest {
            guild_id,
            name: name.into(),
            ..Default::default()
        })
        .await?
        .into_owned()
        .section
        .unwrap())
}

/// The channel ids of `section_id` (0 = top level) in `layout`.
pub fn entry(layout: &GuildLayout, section_id: u64) -> Vec<u64> {
    layout
        .entries
        .iter()
        .find(|e| e.section_id == section_id)
        .map(|e| e.channel_ids.clone())
        .unwrap_or_default()
}

pub fn layout_entry(section_id: u64, channel_ids: &[u64]) -> LayoutEntry {
    LayoutEntry {
        section_id,
        channel_ids: channel_ids.to_vec(),
        ..Default::default()
    }
}

pub async fn update_layout(
    app: &TestApp,
    who: &Account,
    guild_id: u64,
    expected_version: u64,
    section_ids: &[u64],
    entries: Vec<LayoutEntry>,
) -> Result<GuildLayout, ConnectError> {
    Ok(guilds(app, who)
        .update_layout(UpdateLayoutRequest {
            guild_id,
            expected_version,
            section_ids: section_ids.to_vec(),
            entries,
            ..Default::default()
        })
        .await?
        .into_owned()
        .layout
        .unwrap())
}

/// The email of a [`user`] created with `username`.
pub fn email_of(app: &TestApp, username: &str) -> String {
    format!("{}@example.test", app.name(username))
}

/// Creates a legacy-style account without an email.
pub async fn legacy_user(app: &TestApp, username: &str) -> Account {
    let name = app.name(username);
    let account = thehideout_server::accounts::create_user(
        &app.state,
        thehideout_server::accounts::NewUser { username: &name, password: Some(PASSWORD), email: None },
    )
    .await
    .expect("create legacy user");
    let token = thehideout_server::auth::create_session(&app.state, account.id, "test").await.expect("create session");
    Account { token, id: account.id }
}

/// A [`user`] made site admin.
pub async fn site_admin(app: &TestApp, username: &str) -> Account {
    let who = user(app, username).await;
    thehideout_server::admin::set_flag(&app.state, who.id, true).await.unwrap();
    who
}

pub fn auth(app: &TestApp, token: Option<&str>) -> AuthServiceClient<HttpClient> {
    app.client(AuthServiceClient::new, token)
}

pub fn invites(app: &TestApp, token: Option<&str>) -> InviteServiceClient<HttpClient> {
    app.client(InviteServiceClient::new, token)
}

/// Mail sent to `to` so far (the app's in-memory outbox).
pub fn mail(app: &TestApp, to: &str) -> Vec<thehideout_server::email::Email> {
    app.state.mailer.outbox.as_ref().expect("in-memory mailer").to(to)
}

/// Waits (up to 5 s) until `to` has received more than `seen` messages and
/// returns the newest one whose subject contains `subject`.
pub async fn wait_mail(app: &TestApp, to: &str, seen: usize, subject: &str) -> thehideout_server::email::Email {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let all = mail(app, to);
        if all.len() > seen
            && let Some(found) = all[seen..].iter().rev().find(|m| m.subject.contains(subject))
        {
            return found.clone();
        }
        assert!(std::time::Instant::now() < deadline, "no mail {subject:?} to {to}; got {:?}", all.iter().map(|m| &m.subject).collect::<Vec<_>>());
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

/// The first run of 6 digits in `text`.
pub fn six_digit_code(text: &str) -> String {
    let bytes = text.as_bytes();
    (0..bytes.len().saturating_sub(5))
        .find(|&i| {
            bytes[i..i + 6].iter().all(u8::is_ascii_digit)
                && (i == 0 || !bytes[i - 1].is_ascii_digit())
                && bytes.get(i + 6).is_none_or(|b| !b.is_ascii_digit())
        })
        .map(|i| text[i..i + 6].to_owned())
        .expect("a 6-digit code")
}

pub fn login_req(username: &str, password: &str, device: &str) -> LoginRequest {
    LoginRequest { username: username.into(), password: password.into(), device_name: device.into(), ..Default::default() }
}

/// The code of a failed call, and its `CodeError` detail (reason, attempts remaining) if any.
pub fn code_error<T>(r: Result<T, ConnectError>) -> (connectrpc::ErrorCode, Option<(CodeErrorReason, u32)>) {
    use base64::Engine as _;
    let err = r.err().expect("expected an error");
    let detail = err.details.iter().find(|d| d.type_url.ends_with("thehideout.v1.CodeError")).map(|d| {
        let raw = d.value.as_deref().unwrap_or_default();
        let bytes = base64::engine::general_purpose::STANDARD_NO_PAD
            .decode(raw.trim_end_matches('='))
            .expect("base64 detail");
        let e = <CodeError as buffa::Message>::decode_from_slice(&bytes).expect("CodeError");
        (e.reason.as_known().expect("known reason"), e.attempts_remaining)
    });
    (err.code, detail)
}
