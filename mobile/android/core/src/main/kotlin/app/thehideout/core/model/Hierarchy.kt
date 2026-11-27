package app.thehideout.core.model

/** The seniority of the highest of [held] among [roles]; `null` for a member without roles. */
fun topPosition(held: List<Long>, roles: List<Role>): Int? = roles.filter { it.id in held }.maxOfOrNull { it.position }

/**
 * Whether [me] may moderate (kick, ban, edit or hand out a role at [target]'s position) [target]
 * in [guild]: the owner always (themself included); nobody else the owner; anyone else only a
 * target whose top role is strictly below their own (a member without roles is below any role).
 * Mirrors the server's `permissions::outranks_member` (`proto/README.md`).
 */
fun outranksMember(guild: Guild, me: Long, myRoles: List<Long>, target: Long, targetRoles: List<Long>, roles: List<Role>): Boolean {
    if (guild.ownerId == me) return true
    if (guild.ownerId == target) return false
    val top = topPosition(myRoles, roles) ?: return false
    val theirTop = topPosition(targetRoles, roles)
    return theirTop == null || theirTop < top
}
