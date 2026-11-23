mod common;

use common::TestApp;
use common::fixtures::*;
use connectrpc::{ConnectError, ErrorCode};
use thehideout_server::convert::id_to_db;
use thehideout_server::proto::thehideout::v1::*;

fn code<T>(r: Result<T, ConnectError>) -> ErrorCode {
    r.err().expect("expected an error").code
}

fn create_req(email: &str, guild_id: Option<u64>, guild_ids: &[u64], make_admin: bool) -> CreateInviteRequest {
    CreateInviteRequest { email: email.into(), guild_id, guild_ids: guild_ids.to_vec(), make_admin, ..Default::default() }
}

async fn create(app: &TestApp, who: &Account, req: CreateInviteRequest) -> Result<Invite, ConnectError> {
    Ok(invites(app, Some(&who.token)).create_invite(req).await?.into_owned().invite.unwrap())
}

fn accept_req(token_or_code: &str, username: &str, password: Option<&str>) -> AcceptInviteRequest {
    AcceptInviteRequest {
        token_or_code: token_or_code.into(),
        username: username.into(),
        password: password.map(str::to_owned),
        device_name: "new laptop".into(),
        accepted_terms_version: thehideout_server::policy::LEGAL_VERSION.into(),
        age_confirmed: true,
        ..Default::default()
    }
}

async fn accept(app: &TestApp, token_or_code: &str, username: &str) -> Result<AcceptInviteResponse, ConnectError> {
    Ok(invites(app, None).accept_invite(accept_req(token_or_code, username, Some(PASSWORD))).await?.into_owned())
}

async fn lookup(app: &TestApp, token_or_code: &str) -> Result<LookupInviteResponse, ConnectError> {
    Ok(invites(app, None)
        .lookup_invite(LookupInviteRequest { token_or_code: token_or_code.into(), ..Default::default() })
        .await?
        .into_owned())
}

/// The code in the link of an invite email (`XXXX-XXXX`).
fn link_code(app: &TestApp, text: &str) -> String {
    let prefix = format!("{}/invite/", app.state.public_base());
    let start = text.find(&prefix).expect("invite link") + prefix.len();
    text[start..].split_whitespace().next().unwrap().to_owned()
}

fn invitee(app: &TestApp, base: &str) -> String {
    format!("{}@invitee.test", app.name(base))
}

async fn my_guilds(app: &TestApp, token: &str) -> Vec<u64> {
    app.client(GuildServiceClient::new, Some(token))
        .list_my_guilds(ListMyGuildsRequest::default())
        .await
        .unwrap()
        .into_owned()
        .guilds
        .into_iter()
        .map(|g| g.id)
        .collect()
}

#[tokio::test]
async fn who_may_invite() {
    let app = TestApp::start().await;
    let admin = site_admin(&app, "admin").await;
    let owner = user(&app, "owner").await;
    let inviter = user(&app, "inviter").await;
    let member = user(&app, "member").await;
    let outsider = user(&app, "outsider").await;
    let g = guild(&app, &owner).await;
    for who in [&inviter, &member] {
        join(&app, who, &g).await;
    }
    let r = role(&app, &owner, g.id, "inviters", INVITE_PEOPLE).await.unwrap();
    set_roles(&app, &owner, g.id, inviter.id, &[r.id]).await.unwrap();

    // Plain members, outsiders and guild-less non-admins cannot invite.
    assert_eq!(code(create(&app, &member, create_req(&invitee(&app, "a"), Some(g.id), &[], false)).await), ErrorCode::PermissionDenied);
    assert_eq!(code(create(&app, &outsider, create_req(&invitee(&app, "a"), Some(g.id), &[], false)).await), ErrorCode::NotFound);
    assert_eq!(code(create(&app, &member, create_req(&invitee(&app, "a"), None, &[], false)).await), ErrorCode::PermissionDenied);
    // Only admins pre-join extra guilds or invite admins.
    assert_eq!(code(create(&app, &inviter, create_req(&invitee(&app, "a"), Some(g.id), &[g.id], false)).await), ErrorCode::PermissionDenied);
    assert_eq!(code(create(&app, &owner, create_req(&invitee(&app, "a"), Some(g.id), &[], true)).await), ErrorCode::PermissionDenied);

    // INVITE_PEOPLE holders and the owner invite into the guild; admins anywhere.
    let by_inviter = create(&app, &inviter, create_req(&invitee(&app, "b"), Some(g.id), &[], false)).await.unwrap();
    assert_eq!(by_inviter.guild_ids, vec![g.id]);
    assert!(by_inviter.code.as_deref().is_some_and(|c| c.len() == 9 && c.as_bytes()[4] == b'-'));
    // The creator also gets the link that was emailed, so it can be copied and shared.
    let link = by_inviter.link.clone().expect("link for the creator");
    assert!(link.starts_with(&format!("{}/invite/", app.state.public_base())), "{link}");
    let mail = wait_mail(&app, &invitee(&app, "b"), 0, "invited").await;
    assert!(mail.text.contains(&link), "the emailed link is the one returned");
    assert_eq!(lookup(&app, &link_code(&app, &link)).await.unwrap().status, InviteStatus::INVITE_STATUS_PENDING);
    assert_eq!(by_inviter.status, InviteStatus::INVITE_STATUS_PENDING);
    assert_eq!(by_inviter.inviter.as_option().unwrap().id, inviter.id);
    create(&app, &owner, create_req(&invitee(&app, "c"), Some(g.id), &[], false)).await.unwrap();
    let by_admin = create(&app, &admin, create_req(&invitee(&app, "d"), None, &[g.id], true)).await.unwrap();
    assert!(by_admin.make_admin);
    // Invalid or taken addresses.
    assert_eq!(code(create(&app, &admin, create_req("not an email", None, &[], false)).await), ErrorCode::InvalidArgument);
    assert_eq!(code(create(&app, &admin, create_req(&email_of(&app, "member"), None, &[], false)).await), ErrorCode::AlreadyExists);

    // Listing: the guild's invites for INVITE_PEOPLE holders, everything for admins.
    let list = |who: &Account, guild_id: Option<u64>| {
        let client = invites(&app, Some(&who.token));
        async move { client.list_invites(ListInvitesRequest { guild_id, ..Default::default() }).await.map(|r| r.into_owned().invites) }
    };
    let listed = list(&inviter, Some(g.id)).await.unwrap();
    assert_eq!(listed.len(), 3);
    assert!(listed.iter().all(|i| i.code.is_none() && i.link.is_none()));
    assert_eq!(code(list(&member, Some(g.id)).await), ErrorCode::PermissionDenied);
    assert_eq!(code(list(&inviter, None).await), ErrorCode::PermissionDenied);
    assert!(list(&admin, None).await.unwrap().iter().any(|i| i.id == by_admin.id));

    // Revoking: not for strangers (NOT_FOUND), yes for the guild's inviters.
    let revoke = |who: &Account, id: u64| {
        let client = invites(&app, Some(&who.token));
        async move { client.revoke_invite(RevokeInviteRequest { invite_id: id, ..Default::default() }).await.map(|r| r.into_owned()) }
    };
    assert_eq!(code(revoke(&member, by_inviter.id).await), ErrorCode::NotFound);
    let revoked = revoke(&owner, by_inviter.id).await.unwrap().invite.unwrap();
    assert_eq!(revoked.status, InviteStatus::INVITE_STATUS_REVOKED);
    assert_eq!(code(revoke(&admin, by_inviter.id).await), ErrorCode::FailedPrecondition);
    app.cleanup().await;
}

#[tokio::test]
async fn admin_invite_lookup_accept_and_reuse() {
    let app = TestApp::start().await;
    let admin = site_admin(&app, "admin").await;
    let g = guild(&app, &admin).await;
    let email = invitee(&app, "neo");
    let invite = create(&app, &admin, create_req(&email.to_uppercase(), None, &[g.id], true)).await.unwrap();
    assert_eq!(invite.email, email);

    let mail = wait_mail(&app, &email, 0, "invited").await;
    let token = link_code(&app, &mail.text);
    let code_shown = invite.code.clone().unwrap();
    assert_eq!(token, code_shown, "the link carries the code");
    let link = format!("{}/invite/{code_shown}", app.state.public_base());
    assert!(mail.text.contains(&format!("Your invite code: {code_shown}")), "{}", mail.text);
    assert!(mail.text.contains(&link) && mail.html.contains(&link) && mail.html.contains(&code_shown));
    assert_eq!(invite.link.as_deref(), Some(link.as_str()));
    // No long token anywhere in the email.
    for part in [&mail.text, &mail.html] {
        let long = part
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .find(|run| run.len() >= 32);
        assert!(long.is_none(), "unexpected long secret {long:?} in {part}");
    }

    // Lookup by the code in any case and without its dash; the dashed form and the link work too.
    let typed = code_shown.replace('-', "").to_lowercase();
    for key in [typed.as_str(), code_shown.as_str(), &code_shown.to_lowercase()] {
        let preview = lookup(&app, key).await.unwrap();
        assert_eq!(preview.masked_email, format!("{}***@invitee.test", &email[..1]));
        assert_eq!(preview.guild_names, vec![g.name.clone()]);
        assert_eq!(preview.inviter.as_option().unwrap().id, admin.id);
        assert_eq!(preview.inviter.as_option().unwrap().email, None);
        assert_eq!(preview.status, InviteStatus::INVITE_STATUS_PENDING);
    }
    assert_eq!(code(lookup(&app, "ZZZZ-ZZZZ").await), ErrorCode::NotFound);
    // Anything but exactly 8 Crockford characters is not found: long tokens, wrong lengths, bad letters.
    let long = "A".repeat(43);
    for bad in [long.as_str(), "ABCD-EFG", "ABCD-EFGHJ", "ABCD-EFGU", "", &format!("{typed}x"), &format!("https://x/invite/{code_shown}")] {
        assert_eq!(code(lookup(&app, bad).await), ErrorCode::NotFound, "{bad}");
        assert_eq!(code(accept(&app, bad, &app.name("neo9")).await), ErrorCode::NotFound, "{bad}");
    }

    // The landing page of the link answers (no web client here: a static page with the code).
    let page = reqwest::get(format!("{}/invite/{token}", app.base_url)).await.unwrap();
    assert_eq!(page.status(), 200);
    let page = page.text().await.unwrap();
    assert!(page.contains(&code_shown) && page.contains("Join with a code"), "{page}");

    let accepted = accept(&app, &typed, &app.name("neo")).await.unwrap();
    let user = accepted.user.unwrap();
    assert!(user.is_admin, "admin invite makes an admin");
    assert_eq!(user.email.as_deref(), Some(email.as_str()));
    assert_eq!(my_guilds(&app, &accepted.token).await, vec![g.id], "pre-joined");
    let login = auth(&app, None).login(login_req(&email, PASSWORD, "")).await.unwrap().into_owned();
    assert_eq!(login.user.unwrap().id, user.id);

    // Single use, and its status is visible.
    assert_eq!(code(accept(&app, &token, &app.name("neo2")).await), ErrorCode::FailedPrecondition);
    assert_eq!(lookup(&app, &token).await.unwrap().status, InviteStatus::INVITE_STATUS_ACCEPTED);
    app.cleanup().await;
}

#[tokio::test]
async fn accept_rejects_taken_names_expired_and_revoked_invites() {
    let app = TestApp::start().await;
    let admin = site_admin(&app, "admin").await;
    user(&app, "taken").await;

    let invite = create(&app, &admin, create_req(&invitee(&app, "x"), None, &[], false)).await.unwrap();
    let c = invite.code.unwrap();
    assert_eq!(code(accept(&app, &c, "Bad Name").await), ErrorCode::InvalidArgument);
    let short = invites(&app, None).accept_invite(accept_req(&c, &app.name("x"), Some("short"))).await;
    assert_eq!(code(short), ErrorCode::InvalidArgument);
    assert_eq!(code(accept(&app, &c, &app.name("TAKEN")).await), ErrorCode::AlreadyExists);
    // The failed attempt gave the invite back.
    let ok = accept(&app, &c, &app.name("x")).await.unwrap();
    assert!(!ok.user.unwrap().is_admin);
    // The email is now taken too.
    assert_eq!(code(create(&app, &admin, create_req(&invitee(&app, "x"), None, &[], false)).await), ErrorCode::AlreadyExists);

    let expired = create(&app, &admin, create_req(&invitee(&app, "y"), None, &[], false)).await.unwrap();
    app.state
        .session
        .execute_unpaged(
            "UPDATE invites SET expires_at = ? WHERE id = ?",
            (chrono::Utc::now() - chrono::Duration::minutes(1), id_to_db(expired.id)),
        )
        .await
        .unwrap();
    let ec = expired.code.unwrap();
    assert_eq!(lookup(&app, &ec).await.unwrap().status, InviteStatus::INVITE_STATUS_EXPIRED);
    assert_eq!(code(accept(&app, &ec, &app.name("y")).await), ErrorCode::FailedPrecondition);
    // Resending an expired invite revives it with a new code; the old one stops working.
    let resent = invites(&app, Some(&admin.token))
        .resend_invite(ResendInviteRequest { invite_id: expired.id, ..Default::default() })
        .await
        .unwrap()
        .into_owned()
        .invite
        .unwrap();
    assert_eq!(resent.status, InviteStatus::INVITE_STATUS_PENDING);
    let new_code = resent.code.unwrap();
    assert_ne!(new_code, ec);
    let new_link = resent.link.expect("a resend returns the new link");
    assert_ne!(Some(new_link.clone()), expired.link);
    assert_eq!(lookup(&app, &link_code(&app, &new_link)).await.unwrap().status, InviteStatus::INVITE_STATUS_PENDING);
    assert_eq!(code(lookup(&app, &ec).await), ErrorCode::NotFound);
    wait_mail(&app, &invitee(&app, "y"), 1, "invited").await;

    invites(&app, Some(&admin.token))
        .revoke_invite(RevokeInviteRequest { invite_id: expired.id, ..Default::default() })
        .await
        .unwrap();
    assert_eq!(code(accept(&app, &new_code, &app.name("y")).await), ErrorCode::FailedPrecondition);
    assert_eq!(code(accept(&app, "nonsense", &app.name("y")).await), ErrorCode::NotFound);
    app.cleanup().await;
}

#[tokio::test]
async fn member_invites_auto_join_their_guild() {
    let app = TestApp::start().await;
    let owner = user(&app, "owner").await;
    let g = guild(&app, &owner).await;
    let other = guild(&app, &owner).await;
    let invite = create(&app, &owner, create_req(&invitee(&app, "friend"), Some(g.id), &[], false)).await.unwrap();
    let mail = wait_mail(&app, &invitee(&app, "friend"), 0, "invited").await;
    assert!(mail.text.contains(&app.name("owner")) && mail.text.contains(&g.name));
    let token = link_code(&app, &mail.text);
    // Passwordless accounts are allowed.
    let accepted = invites(&app, None).accept_invite(accept_req(&token, &app.name("friend"), None)).await.unwrap().into_owned();
    let guilds = my_guilds(&app, &accepted.token).await;
    assert!(guilds.contains(&g.id) && !guilds.contains(&other.id));
    let members = app
        .client(GuildServiceClient::new, Some(&owner.token))
        .list_guild_members(ListGuildMembersRequest { guild_id: g.id, ..Default::default() })
        .await
        .unwrap()
        .into_owned();
    assert!(members.users.iter().any(|u| u.id == accepted.user.as_option().unwrap().id && u.email.is_none()));
    // No password: password login fails, the invite id shows the account.
    let err = auth(&app, None).login(login_req(&invitee(&app, "friend"), "", "")).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Unauthenticated);
    let listed = invites(&app, Some(&owner.token))
        .list_invites(ListInvitesRequest { guild_id: Some(g.id), ..Default::default() })
        .await
        .unwrap()
        .into_owned()
        .invites;
    let row = listed.iter().find(|i| i.id == invite.id).unwrap();
    assert_eq!(row.status, InviteStatus::INVITE_STATUS_ACCEPTED);
    assert_eq!(row.accepted_user_id, Some(accepted.user.as_option().unwrap().id));
    app.cleanup().await;
}

#[tokio::test]
async fn invite_people_is_a_valid_role_permission() {
    let app = TestApp::start().await;
    let owner = user(&app, "owner").await;
    let g = guild(&app, &owner).await;
    role(&app, &owner, g.id, "ok", INVITE_PEOPLE | MANAGE_EMOJIS).await.unwrap();
    assert_eq!(code(role(&app, &owner, g.id, "bad", 1 << 20).await), ErrorCode::InvalidArgument);
    app.cleanup().await;
}

#[tokio::test]
async fn lookup_and_accept_are_rate_limited() {
    let app = TestApp::start().await;
    for _ in 0..thehideout_server::services::invite::LOOKUP_IP_MAX {
        assert_eq!(code(lookup(&app, "AAAA-AAAA").await), ErrorCode::NotFound);
    }
    assert_eq!(code(lookup(&app, "AAAA-AAAA").await), ErrorCode::ResourceExhausted);
    app.cleanup().await;
}

#[tokio::test]
async fn resending_an_old_invite_drops_its_token_index_row() {
    let app = TestApp::start().await;
    let admin = site_admin(&app, "admin").await;
    let invite = create(&app, &admin, create_req(&invitee(&app, "old"), None, &[], false)).await.unwrap();
    // An invite from before the codes: its old token hash is on the row and in the index.
    let token = "A".repeat(43);
    let token_hash = thehideout_server::auth::hash_token(&token);
    app.state
        .session
        .execute_unpaged("INSERT INTO invites_by_hash (hash, invite_id) VALUES (?, ?)", (&token_hash, id_to_db(invite.id)))
        .await
        .unwrap();
    app.state
        .session
        .execute_unpaged("UPDATE invites SET token_hash = ? WHERE id = ?", (&token_hash, id_to_db(invite.id)))
        .await
        .unwrap();
    let indexed = |app: &TestApp| {
        let hash = token_hash.clone();
        let session = app.state.session.clone();
        async move {
            session
                .execute_unpaged("SELECT invite_id FROM invites_by_hash WHERE hash = ?", (&hash,))
                .await
                .unwrap()
                .into_rows_result()
                .unwrap()
                .rows_num()
        }
    };
    assert_eq!(code(lookup(&app, &token).await), ErrorCode::NotFound);
    assert_eq!(indexed(&app).await, 1);
    invites(&app, Some(&admin.token))
        .resend_invite(ResendInviteRequest { invite_id: invite.id, ..Default::default() })
        .await
        .unwrap();
    assert_eq!(indexed(&app).await, 0, "the old token's index row is gone");
    app.cleanup().await;
}
