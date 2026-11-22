mod common;

use common::TestApp;
use common::fixtures::*;
use connectrpc::ErrorCode;
use thehideout_server::proto::thehideout::v1::*;
use thehideout_server::services::role::MAX_ROLES_PER_GUILD;

#[tokio::test]
async fn permission_matrix_for_owner_member_and_outsider() {
    let app = TestApp::start().await;
    let (alice, bob, carol, dave) = (
        user(&app, "alice").await,
        user(&app, "bob").await,
        user(&app, "carol").await,
        user(&app, "dave").await,
    );
    let guild = guild(&app, &alice).await;
    join(&app, &bob, &guild).await;
    join(&app, &carol, &guild).await;

    let chan_role = role(&app, &alice, guild.id, "chan", MANAGE_CHANNELS).await.unwrap();
    set_roles(&app, &alice, guild.id, bob.id, &[chan_role.id]).await.unwrap();

    // Carol has no role: reads work, every mutation is PERMISSION_DENIED.
    assert!(list_roles(&app, &carol, guild.id).await.is_ok());
    assert_eq!(channel(&app, &carol, guild.id, "x").await.unwrap_err().code, ErrorCode::PermissionDenied);
    assert_eq!(section(&app, &carol, guild.id, "s").await.unwrap_err().code, ErrorCode::PermissionDenied);
    assert_eq!(role(&app, &carol, guild.id, "r", 0).await.unwrap_err().code, ErrorCode::PermissionDenied);
    let err = guilds(&app, &carol)
        .update_guild(UpdateGuildRequest {
            guild_id: guild.id,
            name: Some("new".into()),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::PermissionDenied);

    // Bob (MANAGE_CHANNELS) manages channels, but not the server or roles.
    channel(&app, &bob, guild.id, "bobs").await.expect("bob creates a channel");
    section(&app, &bob, guild.id, "bobs-section").await.expect("bob creates a section");
    let err = guilds(&app, &bob)
        .update_guild(UpdateGuildRequest {
            guild_id: guild.id,
            name: Some("new".into()),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::PermissionDenied);
    assert_eq!(role(&app, &bob, guild.id, "r", 0).await.unwrap_err().code, ErrorCode::PermissionDenied);

    // Outsiders get NOT_FOUND everywhere, never PERMISSION_DENIED.
    assert_eq!(list_roles(&app, &dave, guild.id).await.unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(role(&app, &dave, guild.id, "r", 0).await.unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(channel(&app, &dave, guild.id, "x").await.unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(
        set_roles(&app, &dave, guild.id, bob.id, &[]).await.unwrap_err().code,
        ErrorCode::NotFound
    );

    // The owner passes everything.
    channel(&app, &alice, guild.id, "owners").await.expect("owner");
    app.cleanup().await;
}

#[tokio::test]
async fn administrator_implies_all_but_stays_in_the_hierarchy() {
    let app = TestApp::start().await;
    let (alice, bob) = (user(&app, "alice").await, user(&app, "bob").await);
    let guild = guild(&app, &alice).await;
    join(&app, &bob, &guild).await;
    let admin = role(&app, &alice, guild.id, "admin", ADMINISTRATOR).await.unwrap();
    let below = role(&app, &alice, guild.id, "below", 0).await.unwrap();
    set_roles(&app, &alice, guild.id, bob.id, &[admin.id]).await.unwrap();

    channel(&app, &bob, guild.id, "x").await.expect("admin manages channels");
    guilds(&app, &bob)
        .update_guild(UpdateGuildRequest {
            guild_id: guild.id,
            name: Some("renamed".into()),
            ..Default::default()
        })
        .await
        .expect("admin manages the server");
    // Holding everything, an administrator may grant everything to a lower role...
    roles(&app, &bob)
        .update_role(UpdateRoleRequest {
            guild_id: guild.id,
            role_id: below.id,
            permissions: Some(MANAGE_ROLES | MANAGE_SERVER),
            ..Default::default()
        })
        .await
        .expect("grant");
    // ...but not touch their own (equal) role; only the owner is exempt.
    let err = roles(&app, &bob)
        .update_role(UpdateRoleRequest {
            guild_id: guild.id,
            role_id: admin.id,
            name: Some("boss".into()),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::PermissionDenied);
    roles(&app, &alice)
        .update_role(UpdateRoleRequest {
            guild_id: guild.id,
            role_id: admin.id,
            name: Some("boss".into()),
            ..Default::default()
        })
        .await
        .expect("owner bypasses the hierarchy");
    app.cleanup().await;
}

#[tokio::test]
async fn manage_roles_hierarchy_is_enforced() {
    let app = TestApp::start().await;
    let (alice, bob, carol) = (
        user(&app, "alice").await,
        user(&app, "bob").await,
        user(&app, "carol").await,
    );
    let guild = guild(&app, &alice).await;
    join(&app, &bob, &guild).await;
    join(&app, &carol, &guild).await;
    // Created first = most senior: senior > mgr > junior.
    let senior = role(&app, &alice, guild.id, "senior", 0).await.unwrap();
    let mgr = role(&app, &alice, guild.id, "mgr", MANAGE_ROLES | MANAGE_CHANNELS).await.unwrap();
    let junior = role(&app, &alice, guild.id, "junior", 0).await.unwrap();
    set_roles(&app, &alice, guild.id, bob.id, &[mgr.id]).await.unwrap();
    let denied = ErrorCode::PermissionDenied;

    let update = |role_id: u64, name: Option<&str>, permissions: Option<u64>| {
        let client = roles(&app, &bob);
        let name = name.map(str::to_owned);
        let guild_id = guild.id;
        async move {
            client
                .update_role(UpdateRoleRequest {
                    guild_id,
                    role_id,
                    name,
                    permissions,
                    ..Default::default()
                })
                .await
        }
    };

    // Strictly below: allowed. At or above: denied.
    update(junior.id, Some("junior2"), None).await.expect("below");
    assert_eq!(update(senior.id, Some("x"), None).await.unwrap_err().code, denied);
    assert_eq!(update(mgr.id, Some("x"), None).await.unwrap_err().code, denied);
    let err = roles(&app, &bob)
        .delete_role(DeleteRoleRequest {
            guild_id: guild.id,
            role_id: mgr.id,
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(err.code, denied);

    // Cannot grant what is not held, whether by create or update.
    assert_eq!(role(&app, &bob, guild.id, "r", MANAGE_SERVER).await.unwrap_err().code, denied);
    assert_eq!(update(junior.id, None, Some(MANAGE_SERVER)).await.unwrap_err().code, denied);
    update(junior.id, None, Some(MANAGE_CHANNELS)).await.expect("held permission");
    // Nor revoke a permission it does not hold from a lower role.
    roles(&app, &alice)
        .update_role(UpdateRoleRequest {
            guild_id: guild.id,
            role_id: junior.id,
            permissions: Some(MANAGE_CHANNELS | MANAGE_SERVER),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(update(junior.id, None, Some(MANAGE_CHANNELS)).await.unwrap_err().code, denied);

    // Bob's new role lands at the bottom, below junior.
    let fresh = role(&app, &bob, guild.id, "fresh", MANAGE_CHANNELS).await.expect("created at the bottom");
    assert_eq!(fresh.position, 0);

    // Assignment: only roles strictly below bob's highest.
    set_roles(&app, &bob, guild.id, carol.id, &[junior.id]).await.expect("assign below");
    assert_eq!(set_roles(&app, &bob, guild.id, carol.id, &[senior.id]).await.unwrap_err().code, denied);
    assert_eq!(set_roles(&app, &bob, guild.id, carol.id, &[mgr.id]).await.unwrap_err().code, denied);
    // Removing a role at or above bob's is denied too.
    set_roles(&app, &alice, guild.id, carol.id, &[junior.id, senior.id]).await.unwrap();
    assert_eq!(set_roles(&app, &bob, guild.id, carol.id, &[]).await.unwrap_err().code, denied);
    set_roles(&app, &bob, guild.id, carol.id, &[senior.id]).await.expect("keeps senior, drops junior");

    // Reorder: cannot move roles at/above bob's, nor move one to/above it.
    let listing = list_roles(&app, &bob, guild.id).await.unwrap();
    let order = |ids: &[u64]| ids.to_vec();
    let reorder = |version: u64, ids: Vec<u64>| {
        let client = roles(&app, &bob);
        let guild_id = guild.id;
        async move {
            client
                .reorder_roles(ReorderRolesRequest {
                    guild_id,
                    expected_version: version,
                    role_ids: ids,
                    ..Default::default()
                })
                .await
        }
    };
    let ids: Vec<u64> = listing.roles.iter().map(|r| r.id).collect();
    assert_eq!(ids, vec![senior.id, mgr.id, junior.id, fresh.id]);
    let err = reorder(listing.version, order(&[mgr.id, senior.id, junior.id, fresh.id])).await.unwrap_err();
    assert_eq!(err.code, denied);
    let err = reorder(listing.version, order(&[senior.id, junior.id, mgr.id, fresh.id])).await.unwrap_err();
    assert_eq!(err.code, denied);
    let ok = reorder(listing.version, order(&[senior.id, mgr.id, fresh.id, junior.id]))
        .await
        .expect("swap below")
        .into_owned();
    assert_eq!(ok.roles.iter().map(|r| r.id).collect::<Vec<_>>(), vec![senior.id, mgr.id, fresh.id, junior.id]);
    app.cleanup().await;
}

#[tokio::test]
async fn role_validation() {
    let app = TestApp::start().await;
    let alice = user(&app, "alice").await;
    let guild = guild(&app, &alice).await;
    let invalid = ErrorCode::InvalidArgument;
    assert_eq!(role(&app, &alice, guild.id, "  ", 0).await.unwrap_err().code, invalid);
    assert_eq!(role(&app, &alice, guild.id, &"x".repeat(101), 0).await.unwrap_err().code, invalid);
    assert_eq!(role(&app, &alice, guild.id, "r", 1 << 20).await.unwrap_err().code, invalid);
    let err = roles(&app, &alice)
        .create_role(CreateRoleRequest {
            guild_id: guild.id,
            name: "r".into(),
            color: "mauve".into(),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(err.code, invalid);
    let ok = roles(&app, &alice)
        .create_role(CreateRoleRequest {
            guild_id: guild.id,
            name: " Mods ".into(),
            color: "teal".into(),
            permissions: MANAGE_CHANNELS,
            ..Default::default()
        })
        .await
        .unwrap()
        .into_owned();
    let created = ok.role.unwrap();
    assert_eq!((created.name.as_str(), created.color.as_str(), created.permissions), ("Mods", "teal", MANAGE_CHANNELS));
    assert!(created.created_at.is_set());
    // Unknown role and bad update values.
    let err = roles(&app, &alice)
        .update_role(UpdateRoleRequest {
            guild_id: guild.id,
            role_id: 12345,
            name: Some("x".into()),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    let err = roles(&app, &alice)
        .update_role(UpdateRoleRequest {
            guild_id: guild.id,
            role_id: created.id,
            color: Some("nope".into()),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(err.code, invalid);
    // Clearing the colour with "" works.
    let cleared = roles(&app, &alice)
        .update_role(UpdateRoleRequest {
            guild_id: guild.id,
            role_id: created.id,
            color: Some(String::new()),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_owned()
        .role
        .unwrap();
    assert_eq!(cleared.color, "");
    app.cleanup().await;
}

#[tokio::test]
async fn crud_reorder_and_version_conflicts() {
    let app = TestApp::start().await;
    let alice = user(&app, "alice").await;
    let guild = guild(&app, &alice).await;
    assert_eq!(list_roles(&app, &alice, guild.id).await.unwrap().version, 0);

    let a = role(&app, &alice, guild.id, "a", 0).await.unwrap();
    let b = role(&app, &alice, guild.id, "b", 0).await.unwrap();
    let c = role(&app, &alice, guild.id, "c", 0).await.unwrap();
    let listing = list_roles(&app, &alice, guild.id).await.unwrap();
    assert_eq!(listing.version, 3);
    // Most senior first; positions unique and contiguous.
    assert_eq!(listing.roles.iter().map(|r| r.id).collect::<Vec<_>>(), vec![a.id, b.id, c.id]);
    assert_eq!(listing.roles.iter().map(|r| r.position).collect::<Vec<_>>(), vec![2, 1, 0]);

    let reorder = |version: u64, ids: &[u64]| {
        let client = roles(&app, &alice);
        let (guild_id, ids) = (guild.id, ids.to_vec());
        async move {
            client
                .reorder_roles(ReorderRolesRequest {
                    guild_id,
                    expected_version: version,
                    role_ids: ids,
                    ..Default::default()
                })
                .await
        }
    };
    // Stale version: ABORTED and nothing applied.
    assert_eq!(reorder(2, &[c.id, b.id, a.id]).await.unwrap_err().code, ErrorCode::Aborted);
    assert_eq!(list_roles(&app, &alice, guild.id).await.unwrap().roles[0].id, a.id);
    // Not a permutation.
    for bad in [vec![a.id, b.id], vec![a.id, a.id, b.id], vec![a.id, b.id, 999], vec![a.id, b.id, c.id, 999]] {
        assert_eq!(reorder(3, &bad).await.unwrap_err().code, ErrorCode::InvalidArgument, "{bad:?}");
    }
    let done = reorder(3, &[c.id, a.id, b.id]).await.unwrap().into_owned();
    assert_eq!(done.version, 4);
    assert_eq!(done.roles.iter().map(|r| (r.id, r.position)).collect::<Vec<_>>(), vec![(c.id, 2), (a.id, 1), (b.id, 0)]);
    assert_eq!(list_roles(&app, &alice, guild.id).await.unwrap().roles.iter().map(|r| r.id).collect::<Vec<_>>(), vec![c.id, a.id, b.id]);
    // The same stale version cannot be reused.
    assert_eq!(reorder(3, &[a.id, b.id, c.id]).await.unwrap_err().code, ErrorCode::Aborted);

    // UpdateRole does not bump the version; DeleteRole does and keeps positions contiguous.
    roles(&app, &alice)
        .update_role(UpdateRoleRequest {
            guild_id: guild.id,
            role_id: a.id,
            name: Some("a2".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(list_roles(&app, &alice, guild.id).await.unwrap().version, 4);
    let del = roles(&app, &alice)
        .delete_role(DeleteRoleRequest {
            guild_id: guild.id,
            role_id: c.id,
            ..Default::default()
        })
        .await
        .unwrap()
        .into_owned();
    assert_eq!(del.version, 5);
    let listing = list_roles(&app, &alice, guild.id).await.unwrap();
    assert_eq!(listing.roles.iter().map(|r| (r.id, r.position)).collect::<Vec<_>>(), vec![(a.id, 1), (b.id, 0)]);
    assert_eq!(listing.roles[0].name, "a2");
    app.cleanup().await;
}

#[tokio::test]
async fn member_roles_show_up_in_member_listing_and_deletion_strips_them() {
    let app = TestApp::start().await;
    let (alice, bob) = (user(&app, "alice").await, user(&app, "bob").await);
    let guild = guild(&app, &alice).await;
    join(&app, &bob, &guild).await;
    let a = role(&app, &alice, guild.id, "a", 0).await.unwrap();
    let b = role(&app, &alice, guild.id, "b", 0).await.unwrap();

    let members = |who: &Account| {
        let client = guilds(&app, who);
        let guild_id = guild.id;
        async move {
            client
                .list_guild_members(ListGuildMembersRequest {
                    guild_id,
                    ..Default::default()
                })
                .await
                .unwrap()
                .into_owned()
        }
    };
    let listing = members(&bob).await;
    assert_eq!(listing.members.len(), 2);
    assert!(listing.members.iter().all(|m| m.role_ids.is_empty() && m.joined_at.is_set()));
    assert_eq!(
        listing.users.iter().map(|u| u.id).collect::<Vec<_>>(),
        listing.members.iter().map(|m| m.user.as_option().unwrap().id).collect::<Vec<_>>()
    );

    let member = set_roles(&app, &alice, guild.id, bob.id, &[b.id, a.id, a.id]).await.unwrap();
    assert_eq!(member.user.as_option().unwrap().id, bob.id);
    let mut expected = vec![a.id, b.id];
    expected.sort_unstable();
    assert_eq!(member.role_ids, expected);
    let listing = members(&alice).await;
    let bob_entry = listing.members.iter().find(|m| m.user.as_option().unwrap().id == bob.id).unwrap();
    assert_eq!(bob_entry.role_ids, expected);
    assert!(listing.members.iter().find(|m| m.user.as_option().unwrap().id == alice.id).unwrap().role_ids.is_empty());

    // Errors: unknown role, non-member target.
    assert_eq!(set_roles(&app, &alice, guild.id, bob.id, &[999]).await.unwrap_err().code, ErrorCode::NotFound);
    let outsider = user(&app, "dave").await;
    assert_eq!(set_roles(&app, &alice, guild.id, outsider.id, &[]).await.unwrap_err().code, ErrorCode::NotFound);

    // Deleting a role removes it from members.
    roles(&app, &alice)
        .delete_role(DeleteRoleRequest {
            guild_id: guild.id,
            role_id: a.id,
            ..Default::default()
        })
        .await
        .unwrap();
    let listing = members(&alice).await;
    let bob_entry = listing.members.iter().find(|m| m.user.as_option().unwrap().id == bob.id).unwrap();
    assert_eq!(bob_entry.role_ids, vec![b.id]);
    // And an empty set clears everything.
    assert!(set_roles(&app, &alice, guild.id, bob.id, &[]).await.unwrap().role_ids.is_empty());
    app.cleanup().await;
}

#[tokio::test]
async fn permissions_follow_role_changes() {
    let app = TestApp::start().await;
    let (alice, bob) = (user(&app, "alice").await, user(&app, "bob").await);
    let guild = guild(&app, &alice).await;
    join(&app, &bob, &guild).await;
    let r = role(&app, &alice, guild.id, "r", MANAGE_CHANNELS).await.unwrap();
    set_roles(&app, &alice, guild.id, bob.id, &[r.id]).await.unwrap();
    channel(&app, &bob, guild.id, "one").await.expect("has permission");
    roles(&app, &alice)
        .update_role(UpdateRoleRequest {
            guild_id: guild.id,
            role_id: r.id,
            permissions: Some(0),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(channel(&app, &bob, guild.id, "two").await.unwrap_err().code, ErrorCode::PermissionDenied);
    roles(&app, &alice)
        .update_role(UpdateRoleRequest {
            guild_id: guild.id,
            role_id: r.id,
            permissions: Some(MANAGE_CHANNELS),
            ..Default::default()
        })
        .await
        .unwrap();
    channel(&app, &bob, guild.id, "three").await.expect("restored");
    roles(&app, &alice)
        .delete_role(DeleteRoleRequest {
            guild_id: guild.id,
            role_id: r.id,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(channel(&app, &bob, guild.id, "four").await.unwrap_err().code, ErrorCode::PermissionDenied);
    app.cleanup().await;
}

#[tokio::test]
async fn role_count_is_capped() {
    let app = TestApp::start().await;
    let alice = user(&app, "alice").await;
    let guild = guild(&app, &alice).await;
    for i in 0..MAX_ROLES_PER_GUILD {
        role(&app, &alice, guild.id, &format!("r{i}"), 0).await.expect("under the cap");
    }
    assert_eq!(
        role(&app, &alice, guild.id, "one-too-many", 0).await.unwrap_err().code,
        ErrorCode::ResourceExhausted
    );
    let listing = list_roles(&app, &alice, guild.id).await.unwrap();
    assert_eq!(listing.roles.len(), MAX_ROLES_PER_GUILD);
    let mut positions: Vec<i32> = listing.roles.iter().map(|r| r.position).collect();
    positions.sort_unstable();
    assert_eq!(positions, (0..MAX_ROLES_PER_GUILD as i32).collect::<Vec<_>>());
    app.cleanup().await;
}
