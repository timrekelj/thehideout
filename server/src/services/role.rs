use std::collections::HashSet;
use std::time::Duration;

use chrono::{DateTime, Utc};
use connectrpc::{ConnectError, RequestContext, Response, ServiceRequest, ServiceResult};
use scylla::statement::batch::{Batch, BatchType};

use crate::AppState;
use crate::access::{find_user, require_guild_member};
use crate::auth::{authenticate, internal};
use crate::convert::{id_from_db, id_to_db, snowflake_datetime};
use crate::events::EventPayload;
use crate::permissions::{
    ALL_PERMISSIONS, MANAGE_ROLES, Standing, load_roles, member_role_ids, require_permission,
};
use crate::proto::thehideout::v1::*;
use crate::validation::{display_name, validate_color};
use crate::versions;

/// Roles one guild may have. Enforced by a count before insert, so racing
/// creators can overshoot by a few; the cap only bounds growth.
pub const MAX_ROLES_PER_GUILD: usize = 50;
/// Role mutations allowed per user within [`ROLE_WINDOW`].
pub const ROLE_MUTATIONS_MAX: usize = 60;
const ROLE_WINDOW: Duration = Duration::from_secs(60);

pub struct RoleSvc {
    state: AppState,
}

impl RoleSvc {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    fn throttle(&self, user_id: u64) -> Result<(), ConnectError> {
        self.state
            .limiter
            .check(&format!("roles:{user_id}"), ROLE_MUTATIONS_MAX, ROLE_WINDOW)
    }

    fn publish_roles(&self, guild_id: u64, roles: Vec<Role>, version: u64) {
        self.state.events.publish_to_guild(
            guild_id,
            EventPayload::RolesUpdated(RolesUpdated {
                guild_id,
                roles,
                version,
                ..Default::default()
            }),
        );
    }
}

fn check_permission_bits(standing: &Standing, bits: u64) -> Result<(), ConnectError> {
    if bits & !ALL_PERMISSIONS != 0 {
        return Err(ConnectError::invalid_argument("unknown permission bits"));
    }
    if !standing.holds_all(bits) {
        return Err(ConnectError::permission_denied("cannot grant permissions you do not hold"));
    }
    Ok(())
}

/// A member's state as sent to clients.
pub async fn load_member(state: &AppState, guild_id: u64, user_id: u64) -> Result<Member, ConnectError> {
    let not_found = || ConnectError::not_found("member not found");
    let (joined_at,) = state
        .session
        .execute_unpaged(
            "SELECT joined_at FROM guild_members WHERE guild_id = ? AND user_id = ?",
            (id_to_db(guild_id), id_to_db(user_id)),
        )
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .maybe_first_row::<(Option<DateTime<Utc>>,)>()
        .map_err(internal)?
        .ok_or_else(not_found)?;
    let user = find_user(state, id_to_db(user_id)).await?.ok_or_else(not_found)?;
    Ok(Member {
        user: user.into(),
        role_ids: member_role_ids(state, guild_id, user_id).await?,
        joined_at: crate::convert::datetime_to_proto(joined_at.unwrap_or_default()).into(),
        ..Default::default()
    })
}

impl RoleService for RoleSvc {
    async fn list_roles(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, ListRolesRequest>,
    ) -> ServiceResult<ListRolesResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        require_guild_member(&self.state, user_id, req.guild_id).await?;
        let version = versions::get(&self.state, req.guild_id, versions::ROLES).await?;
        let roles = load_roles(&self.state, req.guild_id).await?;
        Response::ok(ListRolesResponse {
            roles,
            version,
            ..Default::default()
        })
    }

    async fn create_role(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, CreateRoleRequest>,
    ) -> ServiceResult<CreateRoleResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let guild_id = req.guild_id;
        let standing = require_permission(&self.state, user_id, guild_id, MANAGE_ROLES).await?;
        self.throttle(user_id)?;
        let name = display_name(req.name, "role")?;
        validate_color(req.color)?;
        check_permission_bits(&standing, req.permissions)?;
        if load_roles(&self.state, guild_id).await?.len() >= MAX_ROLES_PER_GUILD {
            return Err(ConnectError::resource_exhausted("role limit reached"));
        }

        let version = versions::bump(&self.state, guild_id, versions::ROLES, None).await?;
        // The new role goes to the bottom (position 0); everyone else moves up one.
        let existing = load_roles(&self.state, guild_id).await?;
        let id = self.state.snowflake.next_id();
        let created_at = snowflake_datetime(id);
        let mut shifts = Batch::new(BatchType::Unlogged);
        let mut shift_values = Vec::new();
        for role in &existing {
            shifts.append_statement("UPDATE roles SET position = ? WHERE guild_id = ? AND id = ?");
            shift_values.push((role.position + 1, id_to_db(guild_id), id_to_db(role.id)));
        }
        if !existing.is_empty() {
            self.state.session.batch(&shifts, shift_values).await.map_err(internal)?;
        }
        self.state
            .session
            .execute_unpaged(
                "INSERT INTO roles (guild_id, id, name, color, position, permissions, created_at) \
                 VALUES (?, ?, ?, ?, 0, ?, ?)",
                (id_to_db(guild_id), id_to_db(id), &name, req.color, req.permissions as i64, created_at),
            )
            .await
            .map_err(internal)?;

        let roles = load_roles(&self.state, guild_id).await?;
        let role = roles.iter().find(|r| r.id == id).cloned().ok_or_else(|| internal("role vanished"))?;
        self.publish_roles(guild_id, roles, version);
        Response::ok(CreateRoleResponse {
            role: role.into(),
            version,
            ..Default::default()
        })
    }

    async fn update_role(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, UpdateRoleRequest>,
    ) -> ServiceResult<UpdateRoleResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let guild_id = req.guild_id;
        let standing = require_permission(&self.state, user_id, guild_id, MANAGE_ROLES).await?;
        self.throttle(user_id)?;
        let roles = load_roles(&self.state, guild_id).await?;
        let role = roles
            .iter()
            .find(|r| r.id == req.role_id)
            .ok_or_else(|| ConnectError::not_found("role not found"))?;
        if !standing.outranks(role.position) {
            return Err(ConnectError::permission_denied("role is at or above your highest role"));
        }
        let name = req.name.map(|n| display_name(n, "role")).transpose()?;
        if let Some(color) = req.color {
            validate_color(color)?;
        }
        if let Some(permissions) = req.permissions {
            // Every bit that changes (granted or revoked) must be one the caller holds.
            check_permission_bits(&standing, permissions)?;
            check_permission_bits(&standing, permissions ^ role.permissions)?;
        }

        let mut updated = role.clone();
        if let Some(name) = name {
            updated.name = name;
        }
        if let Some(color) = req.color {
            updated.color = color.to_owned();
        }
        if let Some(permissions) = req.permissions {
            updated.permissions = permissions;
        }
        self.state
            .session
            .execute_unpaged(
                "UPDATE roles SET name = ?, color = ?, permissions = ? WHERE guild_id = ? AND id = ?",
                (
                    &updated.name,
                    &updated.color,
                    updated.permissions as i64,
                    id_to_db(guild_id),
                    id_to_db(updated.id),
                ),
            )
            .await
            .map_err(internal)?;

        let version = versions::get(&self.state, guild_id, versions::ROLES).await?;
        let roles = roles.into_iter().map(|r| if r.id == updated.id { updated.clone() } else { r }).collect();
        self.publish_roles(guild_id, roles, version);
        if req.permissions.is_some() {
            crate::voice::reenforce_permissions(&self.state, guild_id, None).await;
        }
        Response::ok(UpdateRoleResponse {
            role: updated.into(),
            ..Default::default()
        })
    }

    async fn delete_role(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, DeleteRoleRequest>,
    ) -> ServiceResult<DeleteRoleResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let guild_id = req.guild_id;
        let standing = require_permission(&self.state, user_id, guild_id, MANAGE_ROLES).await?;
        self.throttle(user_id)?;
        let roles = load_roles(&self.state, guild_id).await?;
        let role = roles
            .iter()
            .find(|r| r.id == req.role_id)
            .ok_or_else(|| ConnectError::not_found("role not found"))?;
        if !standing.outranks(role.position) {
            return Err(ConnectError::permission_denied("role is at or above your highest role"));
        }
        let role_id = role.id;

        let version = versions::bump(&self.state, guild_id, versions::ROLES, None).await?;
        let db_guild = id_to_db(guild_id);
        self.state
            .session
            .execute_unpaged(
                "DELETE FROM roles WHERE guild_id = ? AND id = ?",
                (db_guild, id_to_db(role_id)),
            )
            .await
            .map_err(internal)?;
        // Compact positions so they stay 0..n-1 (relative order is unchanged).
        let remaining: Vec<&Role> = roles.iter().filter(|r| r.id != role_id).collect();
        let n = remaining.len() as i32;
        let mut remaining_roles = Vec::with_capacity(remaining.len());
        for (i, r) in remaining.iter().enumerate() {
            let position = n - 1 - i as i32;
            if position != r.position {
                self.state
                    .session
                    .execute_unpaged(
                        "UPDATE roles SET position = ? WHERE guild_id = ? AND id = ?",
                        (position, db_guild, id_to_db(r.id)),
                    )
                    .await
                    .map_err(internal)?;
            }
            let mut r = (*r).clone();
            r.position = position;
            remaining_roles.push(r);
        }

        // Strip the role from every member holding it.
        let holders = self
            .state
            .session
            .execute_unpaged("SELECT user_id, role_ids FROM member_roles WHERE guild_id = ?", (db_guild,))
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .rows::<(i64, Option<Vec<i64>>)>()
            .map_err(internal)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(internal)?;
        let mut affected = Vec::new();
        for (uid, ids) in holders {
            if ids.unwrap_or_default().contains(&id_to_db(role_id)) {
                self.state
                    .session
                    .execute_unpaged(
                        "UPDATE member_roles SET role_ids = role_ids - ? WHERE guild_id = ? AND user_id = ?",
                        (vec![id_to_db(role_id)], db_guild, uid),
                    )
                    .await
                    .map_err(internal)?;
                affected.push(id_from_db(uid));
            }
        }

        self.publish_roles(guild_id, remaining_roles, version);
        for uid in affected {
            if let Ok(member) = load_member(&self.state, guild_id, uid).await {
                self.state.events.publish_to_guild(
                    guild_id,
                    EventPayload::MemberUpdated(MemberUpdated {
                        guild_id,
                        member: member.into(),
                        ..Default::default()
                    }),
                );
            }
        }
        crate::voice::reenforce_permissions(&self.state, guild_id, None).await;
        Response::ok(DeleteRoleResponse {
            version,
            ..Default::default()
        })
    }

    async fn reorder_roles(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, ReorderRolesRequest>,
    ) -> ServiceResult<ReorderRolesResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let guild_id = req.guild_id;
        let standing = require_permission(&self.state, user_id, guild_id, MANAGE_ROLES).await?;
        self.throttle(user_id)?;
        let current_version = versions::get(&self.state, guild_id, versions::ROLES).await?;
        if current_version != req.expected_version {
            return Err(ConnectError::aborted("roles changed: refetch and retry"));
        }
        let roles = load_roles(&self.state, guild_id).await?;
        let desired: Vec<u64> = req.role_ids.iter().copied().collect();
        let known: HashSet<u64> = roles.iter().map(|r| r.id).collect();
        let distinct: HashSet<u64> = desired.iter().copied().collect();
        if desired.len() != roles.len() || distinct.len() != desired.len() || distinct != known {
            return Err(ConnectError::invalid_argument(
                "role_ids must contain every role of the guild exactly once",
            ));
        }

        let n = desired.len() as i32;
        let mut reordered = Vec::with_capacity(roles.len());
        for (i, id) in desired.iter().enumerate() {
            let new_position = n - 1 - i as i32;
            let mut role = roles.iter().find(|r| r.id == *id).expect("validated above").clone();
            if role.position != new_position && !(standing.outranks(role.position) && standing.outranks(new_position)) {
                return Err(ConnectError::permission_denied(
                    "cannot move roles at or above your highest role, or move a role to or above it",
                ));
            }
            role.position = new_position;
            reordered.push(role);
        }

        let version = versions::bump(&self.state, guild_id, versions::ROLES, Some(req.expected_version)).await?;
        let mut batch = Batch::new(BatchType::Unlogged);
        let mut values = Vec::new();
        for role in &reordered {
            batch.append_statement("UPDATE roles SET position = ? WHERE guild_id = ? AND id = ?");
            values.push((role.position, id_to_db(guild_id), id_to_db(role.id)));
        }
        if !values.is_empty() {
            self.state.session.batch(&batch, values).await.map_err(internal)?;
        }
        self.publish_roles(guild_id, reordered.clone(), version);
        Response::ok(ReorderRolesResponse {
            roles: reordered,
            version,
            ..Default::default()
        })
    }

    async fn set_member_roles(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, SetMemberRolesRequest>,
    ) -> ServiceResult<SetMemberRolesResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let guild_id = req.guild_id;
        let standing = require_permission(&self.state, user_id, guild_id, MANAGE_ROLES).await?;
        self.throttle(user_id)?;
        if !crate::access::is_member(&self.state, req.user_id, guild_id).await? {
            return Err(ConnectError::not_found("member not found"));
        }
        let roles = load_roles(&self.state, guild_id).await?;
        let desired: HashSet<u64> = req.role_ids.iter().copied().collect();
        if let Some(missing) = desired.iter().find(|id| !roles.iter().any(|r| r.id == **id)) {
            return Err(ConnectError::not_found(format!("role {missing} not found")));
        }
        let current: HashSet<u64> = member_role_ids(&self.state, guild_id, req.user_id).await?.into_iter().collect();
        for role in roles.iter().filter(|r| desired.contains(&r.id) != current.contains(&r.id)) {
            if !standing.outranks(role.position) {
                return Err(ConnectError::permission_denied(
                    "cannot add or remove a role at or above your highest role",
                ));
            }
        }

        let ids: Vec<i64> = desired.iter().map(|id| id_to_db(*id)).collect();
        self.state
            .session
            .execute_unpaged(
                "UPDATE member_roles SET role_ids = ? WHERE guild_id = ? AND user_id = ?",
                (ids, id_to_db(guild_id), id_to_db(req.user_id)),
            )
            .await
            .map_err(internal)?;

        let member = load_member(&self.state, guild_id, req.user_id).await?;
        self.state.events.publish_to_guild(
            guild_id,
            EventPayload::MemberUpdated(MemberUpdated {
                guild_id,
                member: member.clone().into(),
                ..Default::default()
            }),
        );
        crate::voice::reenforce_permissions(&self.state, guild_id, Some(req.user_id)).await;
        Response::ok(SetMemberRolesResponse {
            member: member.into(),
            ..Default::default()
        })
    }
}
