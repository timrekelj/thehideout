package app.thehideout.core

import app.thehideout.core.api.AppError
import app.thehideout.core.fake.FakeBackend
import app.thehideout.core.fake.FakeBackend.Companion.ANA
import app.thehideout.core.fake.FakeBackend.Companion.BRUNO
import app.thehideout.core.fake.FakeBackend.Companion.CVETKA
import app.thehideout.core.fake.FakeBackend.Companion.GARAGE
import app.thehideout.core.fake.FakeBackend.Companion.HIDEOUT
import app.thehideout.core.fake.FakeBackend.Companion.TIM
import app.thehideout.core.model.Ban
import app.thehideout.core.model.Emoji
import app.thehideout.core.model.GatewayEvent
import app.thehideout.core.model.Guild
import app.thehideout.core.model.Member
import app.thehideout.core.model.Perm
import app.thehideout.core.model.RemovalReason
import app.thehideout.core.model.Role
import app.thehideout.core.model.User
import app.thehideout.core.store.Action
import app.thehideout.core.store.AppState
import app.thehideout.core.store.AppStore
import app.thehideout.core.store.Place
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

/** Server settings, roles, moderation and emojis (phase 4b-1). */
class ModerationTest {
    private val s0 = loadedState()

    /** A signed-in store with the fake world loaded, without going through the token/bootstrap dance. */
    private fun store(fake: FakeBackend = FakeBackend()): AppStore {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val s = AppStore(fake, scope, InMemoryTokenStore())
        val initial = kotlinx.coroutines.runBlocking { fake.loadInitial() }
        s.dispatch(Action.InitialLoaded(initial))
        return s
    }

    // ---- who may do what: hierarchy and permission gating (mirrors the server's rules) --------

    private fun hideout(owner: Long = TIM) = Guild(HIDEOUT, "The Hideout", owner, "HiDeout1", T0)

    private fun stateWithRoles(owner: Long, me: Long, roles: List<Role>, memberRoles: Map<Long, List<Long>>): AppState =
        AppState(
            me = User(me, "u$me", T0),
            guilds = listOf(hideout(owner)),
            roles = mapOf(HIDEOUT to roles),
            memberRoles = mapOf(HIDEOUT to memberRoles),
            users = memberRoles.keys.associateWith { User(it, "u$it", T0) } + (me to User(me, "u$me", T0)) + (owner to User(owner, "u$owner", T0)),
        )

    @Test fun `the owner outranks every role and member`() {
        val senior = Role(1, HIDEOUT, "senior", "", 9, 0, T0)
        val s = stateWithRoles(owner = TIM, me = TIM, roles = listOf(senior), memberRoles = mapOf(ANA to listOf(1L)))
        assertTrue(s.isOwner(HIDEOUT))
        assertTrue(s.canEditRole(HIDEOUT, senior))
        assertTrue(s.canModerate(HIDEOUT, ANA, Perm.KICK_MEMBERS))
    }

    @Test fun `a member without MANAGE_ROLES cannot edit even a junior role`() {
        val junior = Role(1, HIDEOUT, "junior", "", 0, 0, T0)
        val s = stateWithRoles(owner = TIM, me = ANA, roles = listOf(junior), memberRoles = emptyMap())
        assertFalse(s.canEditRole(HIDEOUT, junior))
    }

    @Test fun `a role can only be edited or handed out strictly below the caller's highest role, never their own`() {
        val mod = Role(1, HIDEOUT, "mod", "", 5, Perm.MANAGE_ROLES, T0)
        val junior = Role(2, HIDEOUT, "junior", "", 0, 0, T0)
        val senior = Role(3, HIDEOUT, "senior", "", 10, 0, T0)
        val s = stateWithRoles(owner = TIM, me = ANA, roles = listOf(mod, junior, senior), memberRoles = mapOf(ANA to listOf(1L)))
        assertTrue(s.canEditRole(HIDEOUT, junior), "below the caller's own highest role")
        assertFalse(s.canEditRole(HIDEOUT, senior), "above the caller's own highest role")
        assertFalse(s.canEditRole(HIDEOUT, mod), "not even their own highest role, to block self-escalation")
    }

    @Test fun `assignable roles are the ones below the caller's top that the target doesn't already hold`() {
        val mod = Role(1, HIDEOUT, "mod", "", 5, Perm.MANAGE_ROLES, T0)
        val held = Role(2, HIDEOUT, "held", "", 0, 0, T0)
        val s = stateWithRoles(owner = TIM, me = ANA, roles = listOf(mod, held), memberRoles = mapOf(ANA to listOf(1L), BRUNO to listOf(2L)))
        assertEquals(listOf(held.id), s.assignableRoles(HIDEOUT, TIM).map { it.id }, "held is assignable to someone who doesn't have it; mod is the caller's own, never assignable")
    }

    @Test fun `granting a permission requires holding it yourself, unless you're the owner`() {
        val s = stateWithRoles(owner = TIM, me = ANA, roles = emptyList(), memberRoles = emptyMap())
        assertFalse(s.canGrant(HIDEOUT, Perm.BAN_MEMBERS))
        val granter = s.copy(roles = mapOf(HIDEOUT to listOf(Role(1, HIDEOUT, "r", "", 0, Perm.BAN_MEMBERS, T0))), memberRoles = mapOf(HIDEOUT to mapOf(ANA to listOf(1L))))
        assertTrue(granter.canGrant(HIDEOUT, Perm.BAN_MEMBERS))
        assertTrue(s.copy(me = User(TIM, "tim", T0)).canGrant(HIDEOUT, Perm.BAN_MEMBERS), "the owner can grant anything")
    }

    @Test fun `kick and ban need the bit and never target yourself or someone outranking you`() {
        val mod = Role(1, HIDEOUT, "mod", "", 1, Perm.KICK_MEMBERS or Perm.BAN_MEMBERS, T0)
        val admin = Role(2, HIDEOUT, "admin", "", 2, Perm.ADMINISTRATOR, T0)
        val s = stateWithRoles(owner = TIM, me = ANA, roles = listOf(mod, admin), memberRoles = mapOf(ANA to listOf(1L), BRUNO to listOf(2L)))
        assertFalse(s.canKick(HIDEOUT, ANA), "never yourself")
        assertFalse(s.canKick(HIDEOUT, BRUNO), "their top role outranks the caller's")
        assertTrue(s.canKick(HIDEOUT, CVETKA), "a member with no roles is below any role")
        assertFalse(s.canBan(HIDEOUT, TIM), "nobody but the owner acts on the owner")
    }

    @Test fun `the owner can always be left alone but can never leave`() {
        val s = stateWithRoles(owner = TIM, me = TIM, roles = emptyList(), memberRoles = emptyMap())
        assertFalse(s.canLeaveGuild(HIDEOUT))
        assertTrue(s.canDeleteGuild(HIDEOUT))
        assertTrue(s.copy(me = User(ANA, "ana", T0)).canLeaveGuild(HIDEOUT))
        assertFalse(s.copy(me = User(ANA, "ana", T0)).canDeleteGuild(HIDEOUT))
    }

    @Test fun `the name colour is the highest-positioned role that has one`() {
        val none = Role(1, HIDEOUT, "none", "", 5, 0, T0)
        val teal = Role(2, HIDEOUT, "teal-role", "teal", 2, 0, T0)
        val s = stateWithRoles(owner = TIM, me = ANA, roles = listOf(none, teal), memberRoles = mapOf(ANA to listOf(1L, 2L)))
        assertEquals("teal", s.nameColor(HIDEOUT, ANA))
    }

    // ---- reducer: roles, members, bans, emojis, guild patches (the gateway-shaped actions) -----

    @Test fun `RolesReplaced sorts by seniority and keeps the version, like roles_updated`() {
        val roles = listOf(Role(1, HIDEOUT, "a", "", 0, 0, T0), Role(2, HIDEOUT, "b", "", 5, 0, T0))
        val s = s0.reduce(Action.RolesReplaced(HIDEOUT, roles, 9))
        assertEquals(listOf("b", "a"), s.roles[HIDEOUT]!!.map { it.name })
        assertEquals(9L, s.roleVersions[HIDEOUT])
    }

    @Test fun `MemberRolesPatched stores the member's new roles, like member_updated`() {
        val u = User(ANA, "ana", T0)
        val s = s0.reduce(Action.MembersLoaded(HIDEOUT, Result.success(app.thehideout.core.api.GuildMembers(emptyList(), listOf(Member(u, emptyList(), T0))))))
            .reduce(Action.MemberRolesPatched(HIDEOUT, Member(u, listOf(42L), T0)))
        assertEquals(listOf(42L), s.memberRoles[HIDEOUT]!![ANA])
    }

    @Test fun `MemberGone removes someone we kicked or banned, like guild_member_removed`() {
        val loaded = s0.reduce(Action.MembersLoaded(HIDEOUT, Result.success(app.thehideout.core.api.GuildMembers(emptyList(), listOf(TIM, ANA, BRUNO).map { Member(User(it, "u$it", T0), emptyList(), T0) }))))
        val s = loaded.reduce(Action.MemberGone(HIDEOUT, BRUNO, RemovalReason.Kicked))
        assertEquals(listOf(TIM, ANA), s.members[HIDEOUT])
    }

    @Test fun `GuildDropped forgets a guild we left or deleted ourselves at once, without a toast`() {
        val s = s0.reduce(Action.GuildDropped(GARAGE))
        assertNull(s.guild(GARAGE))
        assertTrue(s.notices.isEmpty())
    }

    @Test fun `ExitingFailed undoes the optimistic exiting mark`() {
        val s = s0.reduce(Action.Exiting(GARAGE)).reduce(Action.ExitingFailed(GARAGE))
        assertFalse(GARAGE in s.exiting)
    }

    @Test fun `GuildPatched applies a rename or icon change we made ourselves`() {
        val s = s0.reduce(Action.GuildPatched(s0.guild(HIDEOUT)!!.copy(name = "Den", icon = "key")))
        assertEquals("Den", s.guild(HIDEOUT)!!.name)
        assertEquals("key", s.guild(HIDEOUT)!!.icon)
    }

    @Test fun `BansLoaded and BanLifted keep the ban list current`() {
        val ban = Ban(User(99, "bad", T0), TIM, "spam", T0)
        val s = s0.reduce(Action.BansLoaded(HIDEOUT, Result.success(listOf(ban))))
        assertEquals(listOf(99L), s.bansOf(HIDEOUT).map { it.user.id })
        val lifted = s.reduce(Action.BanLifted(HIDEOUT, 99))
        assertTrue(lifted.bansOf(HIDEOUT).isEmpty())
    }

    @Test fun `EmojiUpserted inserts or replaces, EmojiRemoved deletes, sorted by name`() {
        val e = { id: Long, n: String -> Emoji(id, HIDEOUT, n, false, "k$id", TIM, T0) }
        val s = s0.reduce(Action.EmojiUpserted(HIDEOUT, e(1, "zed"))).reduce(Action.EmojiUpserted(HIDEOUT, e(2, "alpha")))
        assertEquals(listOf("alpha", "zed"), s.emojis[HIDEOUT]!!.map { it.name })
        val renamed = s.reduce(Action.EmojiUpserted(HIDEOUT, e(1, "aaa")))
        assertEquals(listOf("aaa", "alpha"), renamed.emojis[HIDEOUT]!!.map { it.name })
        val removed = renamed.reduce(Action.EmojiRemoved(HIDEOUT, 2))
        assertEquals(listOf("aaa"), removed.emojis[HIDEOUT]!!.map { it.name })
    }

    @Test fun `RoleCreated upserts by id, so a gateway echo landing before our own reply doesn't duplicate it`() {
        val created = Role(500, HIDEOUT, "Helper", "teal", 0, 0, T0)
        // The gateway's own echo of the create (everyone else shifted up, version bumped) lands first.
        val afterEcho = s0.event(GatewayEvent.RolesUpdated(HIDEOUT, listOf(created), 1))
        // Our own createRole RPC reply arrives after: RolesPage's items(key = id) must not see it twice.
        val afterReply = afterEcho.reduce(Action.RoleCreated(HIDEOUT, created, 1))
        assertEquals(1, afterReply.roles[HIDEOUT]!!.count { it.id == 500L })
        assertEquals(1L, afterReply.roleVersions[HIDEOUT])
    }

    @Test fun `RoleCreated shifts existing roles up one and puts the new role at the bottom, when no echo arrived yet`() {
        val existing = listOf(Role(1, HIDEOUT, "senior", "", 2, 0, T0), Role(2, HIDEOUT, "junior", "", 0, 0, T0))
        val withRoles = s0.copy(roles = mapOf(HIDEOUT to existing), roleVersions = mapOf(HIDEOUT to 4L))
        val created = Role(3, HIDEOUT, "Helper", "", 0, 0, T0)
        val s = withRoles.reduce(Action.RoleCreated(HIDEOUT, created, 5))
        val byId = s.roles[HIDEOUT]!!.associateBy { it.id }
        assertEquals(3, byId.getValue(1).position)
        assertEquals(1, byId.getValue(2).position)
        assertEquals(0, byId.getValue(3).position)
        assertEquals(5L, s.roleVersions[HIDEOUT])
    }

    @Test fun `a late updateRole or reorder reply doesn't clobber a newer gateway version`() {
        val stale = listOf(Role(1, HIDEOUT, "stale-name", "", 0, 0, T0))
        val fresh = listOf(Role(1, HIDEOUT, "fresh-name", "", 0, 0, T0))
        val s = s0.reduce(Action.RolesReplaced(HIDEOUT, fresh, 9)).reduce(Action.RolesReplaced(HIDEOUT, stale, 7))
        assertEquals("fresh-name", s.roles[HIDEOUT]!!.single().name, "the stale reply (lower version) was dropped")
        assertEquals(9L, s.roleVersions[HIDEOUT])
    }

    // ---- as a non-owner manager: still bound by the hierarchy and by their own permissions -----

    @Test fun `a manager lacking one of a role's bits can still rename it, sending only the changed field`() = runTest {
        val fake = FakeBackend()
        // As TIM (the owner): a "Manager" role senior to "Held", which carries MANAGE_EMOJIS - a bit
        // ANA (made a manager, but not an admin) doesn't hold herself.
        val manager = fake.createRole(HIDEOUT, "Manager", "", Perm.MANAGE_ROLES)
        val held = fake.createRole(HIDEOUT, "Held", "blue", Perm.MANAGE_EMOJIS)
        fake.setMemberRoles(HIDEOUT, ANA, listOf(manager.role.id))
        fake.actingAs(ANA)
        val renamed = fake.updateRole(HIDEOUT, held.role.id, name = "Renamed", color = null, permissions = null)
        assertEquals("Renamed", renamed.name)
        assertTrue(Perm.has(renamed.permissions, Perm.MANAGE_EMOJIS), "the bit ANA doesn't hold herself stayed untouched, since it wasn't sent")
        // Sending the unchanged permissions back explicitly (the old, buggy behaviour) is refused.
        val blocked = runCatching { fake.updateRole(HIDEOUT, held.role.id, name = null, color = null, permissions = renamed.permissions) }
        assertTrue(blocked.exceptionOrNull() is AppError.PermissionDenied)
    }

    @Test fun `a non-owner manager can't move a role into or above their own rank`() = runTest {
        val fake = FakeBackend()
        // Created in this order so "Mine" ends up senior to "Below" (each new role lands at the
        // bottom and pushes the rest up, like the server - see RoleCreated's test above). The seed
        // (Moderator/Admin/Designer) stays untouched around them.
        val mine = fake.createRole(HIDEOUT, "Mine", "", Perm.MANAGE_ROLES)
        val below = fake.createRole(HIDEOUT, "Below", "", 0)
        fake.setMemberRoles(HIDEOUT, ANA, listOf(mine.role.id))
        fake.actingAs(ANA)
        val before = fake.listRoles(HIDEOUT)
        val bySeniority = before.roles.sortedByDescending { it.position }.map { it.id }
        val mineAt = bySeniority.indexOf(mine.role.id)
        val belowAt = bySeniority.indexOf(below.role.id)
        assertEquals(mineAt + 1, belowAt, "Mine and Below are adjacent, Mine the more senior")
        // Swapping just these two would put "Below" at or above ANA's own rank.
        val swapped = bySeniority.toMutableList().also { it[mineAt] = below.role.id; it[belowAt] = mine.role.id }
        val ex = runCatching { fake.reorderRoles(HIDEOUT, before.version, swapped) }
        assertTrue(ex.exceptionOrNull() is AppError.PermissionDenied)
        assertEquals(bySeniority, fake.listRoles(HIDEOUT).roles.sortedByDescending { it.position }.map { it.id }, "refused: order unchanged")
    }

    @Test fun `setMemberRole on a role the actor doesn't outrank is blocked`() = runTest {
        val fake = FakeBackend()
        // Created in this order so "Above" ends up senior to "Mine" (and so, to ANA herself).
        val above = fake.createRole(HIDEOUT, "Above", "", 0)
        val mine = fake.createRole(HIDEOUT, "Mine", "", Perm.MANAGE_ROLES)
        fake.setMemberRoles(HIDEOUT, ANA, listOf(mine.role.id))
        fake.actingAs(ANA)
        val r = runCatching { fake.setMemberRoles(HIDEOUT, BRUNO, listOf(above.role.id)) }
        assertTrue(r.exceptionOrNull() is AppError.PermissionDenied)
    }

    // ---- end to end against FakeBackend, as the owner (TIM owns The Hideout) ------------------

    @Test fun `renaming the server and setting then removing its icon`() = runTest {
        val s = store()
        assertEquals("Den", s.renameGuild(HIDEOUT, "Den").getOrThrow().name)
        assertEquals("Den", s.state.value.guild(HIDEOUT)!!.name)
        val withIcon = s.uploadGuildIcon(HIDEOUT, byteArrayOf(1, 2, 3), "image/png").getOrThrow()
        assertTrue(withIcon.icon.isNotEmpty())
        assertTrue(s.state.value.guild(HIDEOUT)!!.icon.isNotEmpty())
        val cleared = s.removeGuildIcon(HIDEOUT).getOrThrow()
        assertEquals("", cleared.icon)
    }

    @Test fun `creating, editing, reordering and deleting a role`() = runTest {
        val s = store()
        val role = s.createRole(HIDEOUT, "Helper", "teal", Perm.MANAGE_MESSAGES).getOrThrow()
        assertTrue(s.state.value.rolesOf(HIDEOUT).any { it.id == role.id && it.name == "Helper" })
        val updated = s.updateRole(HIDEOUT, role.id, name = "Helpers", color = null, permissions = null).getOrThrow()
        assertEquals("Helpers", updated.name)
        val repermed = s.updateRole(HIDEOUT, role.id, name = null, color = null, permissions = Perm.MANAGE_EMOJIS).getOrThrow()
        assertTrue(Perm.has(repermed.permissions, Perm.MANAGE_EMOJIS))
        assertFalse(Perm.has(repermed.permissions, Perm.MANAGE_MESSAGES), "replaced, not OR'd in")
        val before = s.state.value.rolesOf(HIDEOUT).map { it.id }
        s.moveRole(HIDEOUT, role.id, -1).getOrThrow()
        assertTrue(s.state.value.rolesOf(HIDEOUT).indexOfFirst { it.id == role.id } < before.indexOf(role.id), "moved up (more senior)")
        s.deleteRole(HIDEOUT, role.id).getOrThrow()
        assertFalse(s.state.value.rolesOf(HIDEOUT).any { it.id == role.id })
    }

    @Test fun `moving a role rolls back when the server refuses the stale version`() = runTest {
        val fake = FakeBackend()
        val s = store(fake)
        val before = s.state.value.rolesOf(HIDEOUT)
        // Someone else's edit lands on the server first: the client's expected_version is now stale.
        fake.reorderRoles(HIDEOUT, fake.listRoles(HIDEOUT).version, before.reversed().map { it.id })
        val r = s.moveRole(HIDEOUT, before.first().id, 1)
        assertTrue(r.isFailure)
        assertTrue(r.exceptionOrNull() is AppError.Conflict)
        assertEquals(before.map { it.id }, s.state.value.rolesOf(HIDEOUT).map { it.id }, "rolled back to what was there before the optimistic move")
    }

    @Test fun `assigning and taking back a member's role`() = runTest {
        val s = store()
        val role = s.createRole(HIDEOUT, "Helper", "", 0).getOrThrow()
        s.setMemberRole(HIDEOUT, ANA, role.id, grant = true).getOrThrow()
        assertTrue(role.id in s.state.value.memberRoles[HIDEOUT]!![ANA].orEmpty())
        s.setMemberRole(HIDEOUT, ANA, role.id, grant = false).getOrThrow()
        assertFalse(role.id in s.state.value.memberRoles[HIDEOUT]!![ANA].orEmpty())
    }

    @Test fun `kicking, banning and unbanning a member`() = runTest {
        val s = store()
        s.kickMember(HIDEOUT, BRUNO).getOrThrow()
        assertFalse(BRUNO in s.state.value.members[HIDEOUT].orEmpty())
        s.banMember(HIDEOUT, CVETKA, "being rude", 3600).getOrThrow()
        assertFalse(CVETKA in s.state.value.members[HIDEOUT].orEmpty())
        val bans = s.loadBans(HIDEOUT).getOrThrow()
        assertTrue(bans.any { it.user.id == CVETKA })
        assertEquals(bans, s.state.value.bansOf(HIDEOUT))
        s.unbanMember(HIDEOUT, CVETKA).getOrThrow()
        assertTrue(s.state.value.bansOf(HIDEOUT).none { it.user.id == CVETKA })
    }

    @Test fun `banning with a cleanup window deletes the user's recent messages`() = runTest {
        val fake = FakeBackend()
        val s = store(fake)
        fake.post(FakeBackend.DEV, BRUNO, "right before the ban")
        s.banMember(HIDEOUT, BRUNO, "", 3600).getOrThrow()
        val page = fake.listMessages(FakeBackend.DEV, before = null, limit = 50)
        assertTrue(page.items.none { it.authorId == BRUNO && it.content == "right before the ban" })
    }

    @Test fun `creating, renaming and deleting a custom emoji`() = runTest {
        val s = store()
        val emoji = s.createEmoji(HIDEOUT, "new_face", byteArrayOf(1, 2), "image/png").getOrThrow()
        assertTrue(s.state.value.emojis[HIDEOUT]!!.any { it.id == emoji.id && it.name == "new_face" })
        val renamed = s.renameEmoji(HIDEOUT, emoji.id, "newer_face").getOrThrow()
        assertEquals("newer_face", renamed.name)
        s.deleteEmoji(HIDEOUT, emoji.id).getOrThrow()
        assertFalse(s.state.value.emojis[HIDEOUT]!!.any { it.id == emoji.id })
    }

    @Test fun `transferring ownership hands the server to another member`() = runTest {
        val s = store()
        val g = s.transferOwnership(HIDEOUT, BRUNO, "The Hideout").getOrThrow()
        assertEquals(BRUNO, g.ownerId)
        assertEquals(BRUNO, s.state.value.guild(HIDEOUT)!!.ownerId)
    }

    @Test fun `transferring with the wrong confirmation name is refused`() = runTest {
        val s = store()
        val r = s.transferOwnership(HIDEOUT, BRUNO, "wrong name")
        assertTrue(r.isFailure)
    }

    @Test fun `the owner cannot leave but can leave a server they don't own`() = runTest {
        val s = store()
        val r = s.leaveGuild(HIDEOUT)
        assertTrue(r.isFailure)
        assertTrue(s.state.value.guild(HIDEOUT) != null)
        s.openGuild(GARAGE)
        assertTrue(s.leaveGuild(GARAGE).isSuccess)
        assertNull(s.state.value.guild(GARAGE))
        assertEquals(Place.Home, s.state.value.place)
    }

    @Test fun `deleting a server with the wrong name is refused, with the right name it's gone`() = runTest {
        val s = store()
        assertTrue(s.deleteGuild(HIDEOUT, "nope").isFailure)
        assertTrue(s.state.value.guild(HIDEOUT) != null)
        assertTrue(s.deleteGuild(HIDEOUT, "The Hideout").isSuccess)
        assertNull(s.state.value.guild(HIDEOUT))
    }
}
