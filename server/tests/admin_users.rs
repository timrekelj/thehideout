//! Admin suspension and deletion of users.
mod common;

use std::time::Duration;

use common::TestApp;
use common::fixtures::*;
use common::http::detail;
use connectrpc::client::HttpClient;
use connectrpc::{ConnectError, ErrorCode};
use thehideout_server::convert::id_to_db;
use thehideout_server::proto::thehideout::v1::*;

fn admin_client(app: &TestApp, who: &Account) -> AdminServiceClient<HttpClient> {
    app.client(AdminServiceClient::new, Some(&who.token))
}

fn code<T>(r: Result<T, ConnectError>) -> ErrorCode {
    r.err().unwrap_or_else(|| panic!("expected an error")).code
}

/// The admin row of `id`, walking the pages (the shared keyspace holds many users).
async fn row(app: &TestApp, admin: &Account, id: u64) -> AdminUserRow {
    let mut token = String::new();
    loop {
        let page = admin_client(app, admin)
            .list_users(ListUsersRequest { limit: 200, page_token: token.clone(), ..Default::default() })
            .await
            .unwrap()
            .into_owned();
        if let Some(found) = page.users.into_iter().find(|u| u.user.as_option().unwrap().id == id) {
            return found;
        }
        assert!(!page.next_page_token.is_empty(), "user row {id} not listed");
        token = page.next_page_token;
    }
}

/// Audit entries (action, target, reason), newest first, across pages.
async fn audit_actions(app: &TestApp, admin: &Account) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut token = String::new();
    for _ in 0..10 {
        let page = admin_client(app, admin)
            .list_audit_log(ListAuditLogRequest { limit: 200, page_token: token.clone(), ..Default::default() })
            .await
            .unwrap()
            .into_owned();
        out.extend(page.entries.into_iter().map(|e| (e.action, e.target, e.reason)));
        if page.next_page_token.is_empty() {
            break;
        }
        token = page.next_page_token;
    }
    out
}

async fn wait_deleted(app: &TestApp, id: u64) {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let account = thehideout_server::accounts::load_any(&app.state, id).await.unwrap().unwrap();
        if account.deletion_state.as_deref() == Some("deleted") {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "deletion did not finish");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn suspension_blocks_sign_in_and_sessions_until_lifted() {
    let app = TestApp::start().await;
    let admin = site_admin(&app, "admin").await;
    let victim = user(&app, "victim").await;
    let name = app.name("victim");
    let until = std::time::SystemTime::now() + Duration::from_secs(3600);

    // Only admins; never admins themselves; `until` must be in the future.
    let plain = user(&app, "plain").await;
    let suspend = |who: &Account, id: u64, reason: &str, until: Option<std::time::SystemTime>| {
        let client = admin_client(&app, who);
        let req = SuspendUserRequest {
            user_id: id,
            reason: reason.into(),
            until: until.map(buffa_types::google::protobuf::Timestamp::from).into(),
            ..Default::default()
        };
        async move { client.suspend_user(req).await.map(|r| r.into_owned()) }
    };
    assert_eq!(code(suspend(&plain, victim.id, "x", None).await), ErrorCode::PermissionDenied);
    assert_eq!(code(suspend(&admin, admin.id, "x", None).await), ErrorCode::FailedPrecondition);
    assert_eq!(code(suspend(&admin, victim.id, "x", Some(std::time::SystemTime::now() - Duration::from_secs(5))).await), ErrorCode::InvalidArgument);
    assert_eq!(code(suspend(&admin, 1, "x", None).await), ErrorCode::NotFound);

    let suspended = suspend(&admin, victim.id, "spamming", Some(until)).await.unwrap();
    let r = suspended.user.as_option().unwrap();
    assert!(r.suspended && r.suspended_reason == "spamming" && r.suspended_until.is_set());

    // Existing sessions are dead, sign-in says why.
    let err = auth(&app, Some(&victim.token)).get_me(GetMeRequest::default()).await.err().unwrap();
    assert_eq!(err.code, ErrorCode::Unauthenticated);
    let err = auth(&app, None).login(login_req(&name, PASSWORD, "new")).await.err().unwrap_or_else(|| panic!("refused"));
    assert_eq!(err.code, ErrorCode::PermissionDenied);
    assert_eq!(err.message.as_deref(), Some("Your account is suspended"));
    let d: AccountSuspended = detail(&err, "AccountSuspended");
    assert_eq!(d.reason, "spamming");
    assert!(d.until.is_set());

    // The email-code path refuses too.
    let email = email_of(&app, "victim");
    auth(&app, None).request_email_code(RequestEmailCodeRequest { email: email.clone(), ..Default::default() }).await.unwrap();
    let code_mail = wait_mail(&app, &email, 0, "sign-in code").await;
    let err = auth(&app, None)
        .verify_email_code(VerifyEmailCodeRequest { email, code: six_digit_code(&code_mail.text), device_name: "x".into(), ..Default::default() })
        .await
        .err()
        .unwrap_or_else(|| panic!("refused"));
    assert_eq!(err.code, ErrorCode::PermissionDenied);
    assert!(thehideout_server::auth::create_session(&app.state, victim.id, "x").await.is_err());

    let listed = row(&app, &admin, victim.id).await;
    assert!(listed.suspended && listed.suspended_reason == "spamming");

    // Unsuspend: sign-in works again.
    let back = admin_client(&app, &admin)
        .unsuspend_user(UnsuspendUserRequest { user_id: victim.id, ..Default::default() })
        .await
        .unwrap()
        .into_owned();
    assert!(!back.user.as_option().unwrap().suspended);
    auth(&app, None).login(login_req(&name, PASSWORD, "new")).await.unwrap();

    // An indefinite suspension has no `until`; a lapsed `until` is over.
    let s = suspend(&admin, victim.id, "again", None).await.unwrap();
    assert!(s.user.as_option().unwrap().suspended && !s.user.as_option().unwrap().suspended_until.is_set());
    assert_eq!(code(auth(&app, None).login(login_req(&name, PASSWORD, "x")).await), ErrorCode::PermissionDenied);
    app.state
        .session
        .execute_unpaged("UPDATE users SET suspended_until = ? WHERE id = ?", (chrono::Utc::now() - chrono::Duration::seconds(5), id_to_db(victim.id)))
        .await
        .unwrap();
    auth(&app, None).login(login_req(&name, PASSWORD, "x")).await.unwrap();
    assert!(!row(&app, &admin, victim.id).await.suspended);

    let actions = audit_actions(&app, &admin).await;
    let target = format!("user:{}", victim.id);
    for action in ["user.suspend", "user.unsuspend"] {
        assert!(actions.iter().any(|(a, t, _)| a == action && *t == target), "{action} audited");
    }
    assert!(actions.iter().any(|(a, t, r)| a == "user.suspend" && *t == target && r.contains("spamming")));
}

#[tokio::test]
async fn admin_delete_refuses_admins_and_owners_without_force() {
    let app = TestApp::builder().storage().start().await;
    let admin = site_admin(&app, "admin").await;
    let other_admin = site_admin(&app, "other_admin").await;
    let plain = user(&app, "plain").await;
    let owner = user(&app, "owner").await;
    let g = guild(&app, &owner).await;
    let delete = |who: &Account, id: u64, force: bool| {
        let client = admin_client(&app, who);
        async move {
            client
                .admin_delete_user(AdminDeleteUserRequest { user_id: id, force_transfer: force, reason: "abuse".into(), ..Default::default() })
                .await
                .map(|_| ())
        }
    };
    assert_eq!(code(delete(&plain, owner.id, false).await), ErrorCode::PermissionDenied);
    assert_eq!(code(delete(&admin, other_admin.id, true).await), ErrorCode::FailedPrecondition);
    assert_eq!(code(delete(&admin, 1, false).await), ErrorCode::NotFound);

    let err = delete(&admin, owner.id, false).await.err().unwrap();
    assert_eq!(err.code, ErrorCode::FailedPrecondition);
    let owned: OwnedGuilds = detail(&err, "OwnedGuilds");
    assert_eq!(owned.guilds.len(), 1);
    assert_eq!((owned.guilds[0].id, owned.guilds[0].member_count), (g.id, 1));
    // Still intact.
    assert!(thehideout_server::accounts::load(&app.state, owner.id).await.unwrap().is_some());
    app.cleanup().await;
}

#[tokio::test]
async fn forced_admin_delete_hands_guilds_over_or_deletes_them() {
    let app = TestApp::builder().storage().start().await;
    let admin = site_admin(&app, "admin").await;
    let owner = user(&app, "owner").await;
    let veteran = user(&app, "veteran").await;
    let ranked = user(&app, "ranked").await;
    let shared = guild(&app, &owner).await;
    // `veteran` joined first, `ranked` has a role: the role wins over tenure.
    join(&app, &veteran, &shared).await;
    join(&app, &ranked, &shared).await;
    let mods = role(&app, &owner, shared.id, "mods", 0).await.unwrap();
    set_roles(&app, &owner, shared.id, ranked.id, &[mods.id]).await.unwrap();
    // A second guild without role holders: the longest member gets it.
    let tenure = guild(&app, &owner).await;
    join(&app, &veteran, &tenure).await;
    join(&app, &ranked, &tenure).await;
    // A third where the owner is alone.
    let alone = guild(&app, &owner).await;
    let alone_channel = channel(&app, &owner, alone.id, "talk").await.unwrap();

    admin_client(&app, &admin)
        .admin_delete_user(AdminDeleteUserRequest { user_id: owner.id, force_transfer: true, reason: "abuse".into(), ..Default::default() })
        .await
        .unwrap();
    wait_deleted(&app, owner.id).await;

    let owner_of = |id: u64| {
        let app = &app;
        async move {
            thehideout_server::services::guild::load_guild(&app.state, id_to_db(id)).await.unwrap().map(|g| g.owner_id)
        }
    };
    assert_eq!(owner_of(shared.id).await, Some(ranked.id), "highest role");
    assert_eq!(owner_of(tenure.id).await, Some(veteran.id), "longest member");
    assert_eq!(owner_of(alone.id).await, None, "alone: deleted");
    // The solo guild is unreachable.
    assert!(channels(&app, &owner, alone.id).await.is_err());
    let _ = alone_channel;
    assert!(!guilds(&app, &ranked).list_my_guilds(ListMyGuildsRequest::default()).await.unwrap().into_owned().guilds.iter().any(|x| x.id == alone.id));

    let row = row(&app, &admin, owner.id).await;
    assert!(row.deleted && row.user.as_option().unwrap().deleted);
    assert_eq!(row.user.as_option().unwrap().username, "Deleted user");
    let actions = audit_actions(&app, &admin).await;
    let target = format!("user:{}", owner.id);
    assert!(actions.iter().any(|(a, t, r)| a == "account.delete" && *t == target && r == "abuse"), "{actions:?}");
    assert!(actions.iter().any(|(a, t, _)| a == "guild.transfer" && t.ends_with(&format!("user:{}", ranked.id))));
    assert!(actions.iter().any(|(a, ..)| a == "guild.delete"));
    app.cleanup().await;
}
