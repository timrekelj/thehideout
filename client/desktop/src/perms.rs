//! What the signed-in user may do in a guild, computed the way the server does (`proto/README.md`):
//! the owner has every permission; anyone else has the OR of their roles' permissions, and
//! ADMINISTRATOR implies all. The client only uses this to decide which controls to show; the
//! server still checks every call.

use thehideout_api::{Guild, Permissions, Role};

/// Every bit set, which is what the owner holds.
pub const ALL: Permissions = Permissions(u64::MAX);

/// What every member has without any role (there is no @everyone role): the voice basics.
/// Mirrors the server's `permissions::BASELINE`.
pub const BASELINE: Permissions = Permissions::BASELINE;

/// The moderation bits, which no member has by default: delete others' messages, kick, ban.
#[cfg_attr(not(test), allow(dead_code))]
pub const MODERATION: Permissions = Permissions(
    Permissions::MANAGE_MESSAGES.0 | Permissions::KICK_MEMBERS.0 | Permissions::BAN_MEMBERS.0,
);

/// `me`'s effective permissions in `guild`, given the guild's roles and the role ids they hold.
/// Unknown role ids (a role deleted since) contribute nothing; the baseline always counts.
pub fn effective(guild: &Guild, me: u64, my_roles: &[u64], roles: &[Role]) -> Permissions {
    if guild.owner_id == me {
        return ALL;
    }
    roles
        .iter()
        .filter(|r| my_roles.contains(&r.id))
        .fold(BASELINE, |p, r| p | r.permissions)
}

/// The seniority of the highest of `held` among `roles`; `None` for a member without roles.
pub fn top_position(held: &[u64], roles: &[Role]) -> Option<i32> {
    roles
        .iter()
        .filter(|r| held.contains(&r.id))
        .map(|r| r.position)
        .max()
}

/// Whether `me` may moderate (server-mute, move, disconnect) `target`, by the server's rule: the
/// owner always (themself included); nobody else the owner; anyone else only members whose top
/// role is strictly below theirs (a member without roles is below any role).
pub fn outranks_member(
    guild: &Guild,
    me: u64,
    my_roles: &[u64],
    target: u64,
    target_roles: &[u64],
    roles: &[Role],
) -> bool {
    if guild.owner_id == me {
        return true;
    }
    if guild.owner_id == target {
        return false;
    }
    top_position(my_roles, roles)
        .is_some_and(|top| top_position(target_roles, roles).is_none_or(|p| p < top))
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use thehideout_api::PaletteColor;

    use super::*;

    fn guild(owner: u64) -> Guild {
        Guild {
            id: 1,
            name: "g".into(),
            owner_id: owner,
            invite_code: String::new(),
            created_at: UNIX_EPOCH,
            icon: None,
        }
    }

    fn role(id: u64, permissions: Permissions) -> Role {
        Role {
            id,
            guild_id: 1,
            name: format!("r{id}"),
            color: PaletteColor::None,
            position: 0,
            permissions,
            created_at: UNIX_EPOCH,
        }
    }

    #[test]
    fn the_owner_can_do_everything() {
        assert!(effective(&guild(7), 7, &[], &[]).contains(Permissions::MANAGE_CHANNELS));
    }

    #[test]
    fn roles_add_up_and_strangers_have_nothing() {
        let roles = [
            role(1, Permissions::MANAGE_ROLES),
            role(2, Permissions::MANAGE_CHANNELS),
            role(3, Permissions::MANAGE_SERVER),
        ];
        let p = effective(&guild(9), 7, &[1, 2, 99], &roles);
        assert!(p.contains(Permissions::MANAGE_CHANNELS | Permissions::MANAGE_ROLES));
        assert!(
            !p.contains(Permissions::MANAGE_SERVER),
            "role 3 is not theirs"
        );
        assert_eq!(effective(&guild(9), 7, &[], &roles), BASELINE);
    }

    #[test]
    fn every_member_has_the_voice_baseline_but_not_moderation() {
        let p = effective(&guild(9), 7, &[], &[]);
        assert!(p.contains(Permissions::CONNECT | Permissions::SPEAK | Permissions::VIDEO));
        assert!(!p.contains(Permissions::MUTE_MEMBERS) && !p.contains(Permissions::MOVE_MEMBERS));
    }

    #[test]
    fn the_moderation_bits_are_not_in_the_baseline_but_owners_and_admins_hold_them() {
        assert_eq!((Permissions::MANAGE_MESSAGES.bits(), Permissions::KICK_MEMBERS.bits(), Permissions::BAN_MEMBERS.bits()), (2048, 4096, 8192));
        let plain = effective(&guild(9), 7, &[], &[]);
        assert!(!plain.contains(Permissions::MANAGE_MESSAGES) && !plain.contains(Permissions::KICK_MEMBERS));
        assert_eq!(plain.bits() & MODERATION.bits(), 0);
        assert!(effective(&guild(7), 7, &[], &[]).contains(MODERATION));
        let roles = [role(1, Permissions::ADMINISTRATOR), role(2, Permissions::KICK_MEMBERS)];
        assert!(effective(&guild(9), 7, &[1], &roles).contains(MODERATION));
        let kicker = effective(&guild(9), 7, &[2], &roles);
        assert!(kicker.contains(Permissions::KICK_MEMBERS) && !kicker.contains(Permissions::BAN_MEMBERS));
    }

    #[test]
    fn moderation_follows_the_role_hierarchy() {
        let mut senior = role(1, Permissions::MUTE_MEMBERS);
        senior.position = 5;
        let mut junior = role(2, Permissions::NONE);
        junior.position = 2;
        let roles = [senior, junior];
        let g = guild(9);
        // Strictly below, and a member without roles is below any role.
        assert!(outranks_member(&g, 7, &[1], 8, &[2], &roles));
        assert!(outranks_member(&g, 7, &[1], 8, &[], &roles));
        // Not equal, not above, not without roles themselves, never the owner.
        assert!(!outranks_member(&g, 7, &[1], 8, &[1], &roles));
        assert!(!outranks_member(&g, 7, &[2], 8, &[1], &roles));
        assert!(!outranks_member(&g, 7, &[], 8, &[], &roles));
        assert!(!outranks_member(&g, 7, &[1], 9, &[], &roles));
        // The owner outranks everyone, themself included.
        assert!(outranks_member(&g, 9, &[], 7, &[1], &roles));
        assert!(outranks_member(&g, 9, &[], 9, &[], &roles));
    }

    #[test]
    fn administrator_implies_all() {
        let roles = [role(1, Permissions::ADMINISTRATOR)];
        assert!(effective(&guild(9), 7, &[1], &roles).contains(Permissions::MANAGE_CHANNELS));
    }
}
