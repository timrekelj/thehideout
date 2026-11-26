package app.thehideout.core

import app.thehideout.core.api.AppError
import app.thehideout.core.fake.FakeBackend
import app.thehideout.core.fake.FakeBackend.Companion.ANA
import app.thehideout.core.fake.FakeBackend.Companion.GARAGE
import app.thehideout.core.fake.FakeBackend.Companion.HIDEOUT
import app.thehideout.core.fake.FakeBackend.Companion.TIM
import app.thehideout.core.model.ChannelKind
import app.thehideout.core.model.Perm
import app.thehideout.core.store.AppStore
import app.thehideout.core.store.Place
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNotEquals
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

/** Create/join servers, create channels and sections, and invites (phase 4a), end to end against [FakeBackend]. */
class ManagementTest {
    /** A signed-in store with the fake world loaded, without going through the token/bootstrap dance. */
    private fun store(fake: FakeBackend = FakeBackend()): AppStore {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val s = AppStore(fake, scope, InMemoryTokenStore())
        val initial = kotlinx.coroutines.runBlocking { fake.loadInitial() }
        s.dispatch(app.thehideout.core.store.Action.InitialLoaded(initial))
        return s
    }

    @Test fun `creating a server lists it and opens it on its auto-created general channel`() = runTest {
        val s = store()
        val r = s.createGuild("New Server")
        assertTrue(r.isSuccess)
        val guild = r.getOrThrow()
        assertEquals(Place.InGuild(guild.id), s.state.value.place)
        assertEquals(TIM, guild.ownerId)
        assertTrue(s.state.value.guilds.any { it.id == guild.id })
    }

    @Test fun `joining by code opens the server and lists you as a member`() = runTest {
        val s = store()
        val r = s.joinGuild("garage22")
        assertTrue(r.isSuccess)
        assertEquals(Place.InGuild(GARAGE), s.state.value.place)
    }

    @Test fun `an unknown code is refused`() = runTest {
        val s = store()
        val r = s.joinGuild("ZZZZ-0000")
        assertTrue(r.isFailure)
        assertTrue(r.exceptionOrNull() is AppError.NotFound)
    }

    @Test fun `creating a channel lists it at once, like a channel_created echo`() = runTest {
        val s = store()
        val r = s.createChannel(HIDEOUT, "ops", ChannelKind.Text, null)
        assertTrue(r.isSuccess)
        val c = r.getOrThrow()
        assertEquals(HIDEOUT, c.guildId)
        assertTrue(s.state.value.channelsOf(HIDEOUT).any { it.id == c.id })
    }

    @Test fun `creating, renaming and deleting a section updates the layout each time`() = runTest {
        val s = store()
        val created = s.createSection(HIDEOUT, "Projects").getOrThrow()
        assertTrue(s.state.value.layouts[HIDEOUT]!!.sections.any { it.id == created.id && it.name == "Projects" })
        s.renameSection(HIDEOUT, created.id, "Side projects")
        assertEquals("Side projects", s.state.value.layouts[HIDEOUT]!!.sections.first { it.id == created.id }.name)
        s.deleteSection(HIDEOUT, created.id)
        assertFalse(s.state.value.layouts[HIDEOUT]!!.sections.any { it.id == created.id })
    }

    @Test fun `resetting the invite code replaces it and the old one stops matching a join`() = runTest {
        val s = store()
        val old = s.state.value.guild(HIDEOUT)!!.inviteCode
        val fresh = s.rotateInviteCode(HIDEOUT).getOrThrow()
        assertNotEquals(old, fresh)
        assertEquals(fresh, s.state.value.guild(HIDEOUT)?.inviteCode)
    }

    @Test fun `inviting by email creates a pending invite with a code and a link`() = runTest {
        val s = store()
        val invite = s.createInvite("new.person@example.com", HIDEOUT).getOrThrow()
        assertEquals(8, invite.code?.replace("-", "")?.length)
        assertTrue(invite.link.orEmpty().endsWith(invite.code.orEmpty()))
        val pending = s.listInvites(HIDEOUT).getOrThrow()
        assertTrue(pending.any { it.id == invite.id })
        assertEquals(null, pending.first { it.id == invite.id }.code, "the list hides the code, like CreateInvite/ListInvites")
    }

    @Test fun `withdrawing and resending an invite change its status and code`() = runTest {
        val s = store()
        val invite = s.createInvite("another@example.com", HIDEOUT).getOrThrow()
        val resent = s.resendInvite(invite.id).getOrThrow()
        assertNotEquals(invite.code, resent.code)
        val revoked = s.revokeInvite(invite.id).getOrThrow()
        assertEquals(app.thehideout.core.model.InviteStatus.Revoked, revoked.status)
    }

    @Test fun `only MANAGE_CHANNELS, MANAGE_SERVER and INVITE_PEOPLE gate their rows, like desktop's perms`() {
        val base = 0L
        assertFalse(Perm.has(base, Perm.MANAGE_CHANNELS))
        assertTrue(Perm.has(base or Perm.MANAGE_CHANNELS, Perm.MANAGE_CHANNELS))
        assertFalse(Perm.has(base or Perm.MANAGE_CHANNELS, Perm.MANAGE_SERVER))
        assertTrue(Perm.has(base or Perm.ADMINISTRATOR or Perm.MANAGE_SERVER, Perm.MANAGE_SERVER))
        assertTrue(Perm.has(base or Perm.INVITE_PEOPLE, Perm.INVITE_PEOPLE))
    }
}
