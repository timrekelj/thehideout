//! Roles and permission checks. See `Permission` in `types.proto` for the model.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use connectrpc::ConnectError;

use crate::AppState;
use crate::access::require_guild_member;
use crate::auth::internal;
use crate::convert::{datetime_to_proto, id_from_db, id_to_db};
use crate::proto::thehideout::v1::Role;

pub const ADMINISTRATOR: u64 = 1;
pub const MANAGE_SERVER: u64 = 2;
pub const MANAGE_CHANNELS: u64 = 4;
pub const MANAGE_ROLES: u64 = 8;
pub const MANAGE_EMOJIS: u64 = 16;
/// Invite people by email (the invite auto-joins the guild).
pub const INVITE_PEOPLE: u64 = 32;
/// Join voice channels.
pub const CONNECT: u64 = 64;
/// Publish a microphone track in voice channels.
pub const SPEAK: u64 = 128;
/// Publish a camera and share the screen in voice channels.
pub const VIDEO: u64 = 256;
/// Server-mute other members in voice channels.
pub const MUTE_MEMBERS: u64 = 512;
/// Move other members between voice channels and disconnect them.
pub const MOVE_MEMBERS: u64 = 1024;
/// Delete other members' messages.
pub const MANAGE_MESSAGES: u64 = 2048;
/// Kick members (subject to the hierarchy).
pub const KICK_MEMBERS: u64 = 4096;
/// Ban and unban members, list bans (subject to the hierarchy).
pub const BAN_MEMBERS: u64 = 8192;
/// Every defined permission bit.
pub const ALL_PERMISSIONS: u64 = ADMINISTRATOR
    | MANAGE_SERVER
    | MANAGE_CHANNELS
    | MANAGE_ROLES
    | MANAGE_EMOJIS
    | INVITE_PEOPLE
    | CONNECT
    | SPEAK
    | VIDEO
    | MUTE_MEMBERS
    | MOVE_MEMBERS
    | MANAGE_MESSAGES
    | KICK_MEMBERS
    | BAN_MEMBERS;
/// What every guild member has without any role (there is no @everyone role):
/// the voice basics. MUTE_MEMBERS / MOVE_MEMBERS are not in it.
pub const BASELINE: u64 = CONNECT | SPEAK | VIDEO;

/// A caller's standing within a guild.
pub struct Standing {
    pub is_owner: bool,
    /// Effective permissions: everything for the owner and for ADMINISTRATOR holders.
    pub permissions: u64,
    /// Position of the caller's highest role; `None` without roles.
    pub top_position: Option<i32>,
}

impl Standing {
    pub fn has(&self, permission: u64) -> bool {
        self.permissions & permission == permission
    }

    /// Whether the caller may manage a role at `position`: the owner always,
    /// everyone else only strictly below their highest role.
    pub fn outranks(&self, position: i32) -> bool {
        self.is_owner || self.top_position.is_some_and(|top| position < top)
    }

    /// Whether the caller may moderate (mute, move, disconnect) `target`: the
    /// owner always, nobody but the owner the owner, everyone else only members
    /// whose highest role is strictly below their own (members without roles
    /// are below any role).
    pub fn outranks_member(&self, target: &Standing) -> bool {
        if self.is_owner {
            return true;
        }
        if target.is_owner {
            return false;
        }
        self.top_position.is_some_and(|top| target.top_position.is_none_or(|p| p < top))
    }

    /// Whether the caller may grant or revoke every bit of `bits`.
    pub fn holds_all(&self, bits: u64) -> bool {
        self.is_owner || self.permissions & bits == bits
    }
}

pub fn role_to_proto(
    guild_id: u64,
    (id, name, color, position, permissions, created_at): RoleRow,
) -> Role {
    Role {
        id: id_from_db(id),
        guild_id,
        name: name.unwrap_or_default(),
        color: color.unwrap_or_default(),
        position: position.unwrap_or(0),
        permissions: permissions.map_or(0, |p| p as u64),
        created_at: datetime_to_proto(created_at.unwrap_or_default()).into(),
        ..Default::default()
    }
}

pub type RoleRow = (i64, Option<String>, Option<String>, Option<i32>, Option<i64>, Option<DateTime<Utc>>);

/// All roles of the guild, most senior first.
pub async fn load_roles(state: &AppState, guild_id: u64) -> Result<Vec<Role>, ConnectError> {
    let mut roles = state
        .session
        .execute_unpaged(
            "SELECT id, name, color, position, permissions, created_at FROM roles WHERE guild_id = ?",
            (id_to_db(guild_id),),
        )
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .rows::<RoleRow>()
        .map_err(internal)?
        .map(|r| r.map(|row| role_to_proto(guild_id, row)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?;
    roles.sort_by(|a, b| b.position.cmp(&a.position).then(a.id.cmp(&b.id)));
    Ok(roles)
}

/// The role IDs assigned to a member (possibly none).
pub async fn member_role_ids(state: &AppState, guild_id: u64, user_id: u64) -> Result<Vec<u64>, ConnectError> {
    let ids = state
        .session
        .execute_unpaged(
            "SELECT role_ids FROM member_roles WHERE guild_id = ? AND user_id = ?",
            (id_to_db(guild_id), id_to_db(user_id)),
        )
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .maybe_first_row::<(Option<Vec<i64>>,)>()
        .map_err(internal)?
        .and_then(|(ids,)| ids)
        .unwrap_or_default();
    let mut ids: Vec<u64> = ids.into_iter().map(id_from_db).collect();
    ids.sort_unstable();
    Ok(ids)
}

/// The caller's standing in the guild; `NotFound` for non-members.
pub async fn standing(state: &AppState, user_id: u64, guild_id: u64) -> Result<Standing, ConnectError> {
    require_guild_member(state, user_id, guild_id).await?;
    let (owner_id,) = state
        .session
        .execute_unpaged("SELECT owner_id FROM guilds WHERE id = ?", (id_to_db(guild_id),))
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .maybe_first_row::<(i64,)>()
        .map_err(internal)?
        .ok_or_else(|| ConnectError::not_found("guild not found"))?;
    if id_from_db(owner_id) == user_id {
        return Ok(Standing {
            is_owner: true,
            permissions: ALL_PERMISSIONS,
            top_position: None,
        });
    }
    let mine: HashSet<u64> = member_role_ids(state, guild_id, user_id).await?.into_iter().collect();
    let mut permissions = BASELINE;
    let mut top_position = None;
    for role in load_roles(state, guild_id).await?.into_iter().filter(|r| mine.contains(&r.id)) {
        permissions |= role.permissions;
        top_position = top_position.max(Some(role.position));
    }
    if permissions & ADMINISTRATOR != 0 {
        permissions = ALL_PERMISSIONS;
    }
    Ok(Standing {
        is_owner: false,
        permissions,
        top_position,
    })
}

/// Like [`standing`], and additionally requires `permission`: `NotFound` for
/// non-members, `PermissionDenied` for members without it.
pub async fn require_permission(
    state: &AppState,
    user_id: u64,
    guild_id: u64,
    permission: u64,
) -> Result<Standing, ConnectError> {
    let standing = standing(state, user_id, guild_id).await?;
    if standing.has(permission) {
        Ok(standing)
    } else {
        Err(ConnectError::permission_denied("missing permission"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(is_owner: bool, permissions: u64, top: Option<i32>) -> Standing {
        Standing { is_owner, permissions, top_position: top }
    }

    #[test]
    fn hierarchy_rules() {
        let member = st(false, MANAGE_ROLES, Some(3));
        assert!(member.outranks(2) && !member.outranks(3) && !member.outranks(4));
        assert!(member.holds_all(MANAGE_ROLES) && !member.holds_all(MANAGE_ROLES | MANAGE_SERVER));
        assert!(!st(false, ALL_PERMISSIONS, None).outranks(0));
        let owner = st(true, ALL_PERMISSIONS, None);
        assert!(owner.outranks(i32::MAX) && owner.holds_all(ALL_PERMISSIONS));
    }

    #[test]
    fn moderation_bits_are_not_in_the_baseline() {
        assert_eq!((MANAGE_MESSAGES, KICK_MEMBERS, BAN_MEMBERS), (2048, 4096, 8192));
        for bit in [MANAGE_MESSAGES, KICK_MEMBERS, BAN_MEMBERS] {
            assert_eq!(BASELINE & bit, 0);
            assert_eq!(ALL_PERMISSIONS & bit, bit);
        }
    }

    #[test]
    fn moderation_hierarchy() {
        let owner = st(true, ALL_PERMISSIONS, None);
        let senior = st(false, MUTE_MEMBERS, Some(5));
        let junior = st(false, BASELINE, Some(2));
        let rolesless = st(false, BASELINE, None);
        assert!(owner.outranks_member(&senior) && owner.outranks_member(&owner));
        assert!(senior.outranks_member(&junior) && senior.outranks_member(&rolesless));
        assert!(!junior.outranks_member(&senior) && !senior.outranks_member(&senior));
        assert!(!senior.outranks_member(&owner));
        assert!(!rolesless.outranks_member(&rolesless));
    }
}
