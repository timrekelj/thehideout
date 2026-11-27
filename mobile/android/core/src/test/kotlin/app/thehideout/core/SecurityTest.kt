package app.thehideout.core

import app.thehideout.core.api.AppError
import app.thehideout.core.fake.FakeBackend
import app.thehideout.core.store.Action
import app.thehideout.core.store.Reducer
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertThrows
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

/** Account security (SEC-01..08): the fake's answers to the phase-5 `AuthService` RPCs. */
class SecurityTest {
    @Test
    fun `security summary starts with no two-factor and no passkeys`() = runTest {
        val fake = FakeBackend()
        val s = fake.getSecurity()
        assertFalse(s.totpEnabled)
        assertTrue(s.hasPassword)
        assertEquals(0, s.recoveryCodesRemaining)
        assertEquals(1, s.passkeyCount) // tim is seeded with one passkey
    }

    @Test
    fun `change password refuses the wrong current password`() = runTest {
        val fake = FakeBackend()
        assertThrows(AppError.PermissionDenied::class.java) {
            kotlinx.coroutines.runBlocking { fake.changePassword(fake.wrongPassword, "a-new-password") }
        }
    }

    @Test
    fun `change password enforces the minimum length`() = runTest {
        val fake = FakeBackend()
        assertThrows(AppError.InvalidArgument::class.java) {
            kotlinx.coroutines.runBlocking { fake.changePassword("current", "short") }
        }
    }

    @Test
    fun `change password succeeds and a passwordless account skips the current password`() = runTest {
        val fake = FakeBackend()
        fake.changePassword("current", "a-new-password-1")
        fake.actingAs(FakeBackend.BRUNO) // seeded passwordless
        assertFalse(fake.getSecurity().hasPassword)
        fake.changePassword("", "a-new-password-2")
        assertTrue(fake.getSecurity().hasPassword)
    }

    @Test
    fun `change email then confirm with the wrong code fails and the right one lands`() = runTest {
        val fake = FakeBackend()
        fake.changeEmail("new@example.com", "current")
        assertThrows(AppError.WrongCode::class.java) {
            kotlinx.coroutines.runBlocking { fake.confirmEmailChange("000000") }
        }
        val user = fake.confirmEmailChange(fake.emailCode)
        assertEquals("new@example.com", user.email)
        assertTrue(fake.getSecurity().emailVerified)
    }

    @Test
    fun `totp setup, confirm, disable and regenerate`() = runTest {
        val fake = FakeBackend()
        val setup = fake.beginTotpSetup()
        assertTrue(setup.otpauthUri.startsWith("otpauth://totp/"))
        assertThrows(AppError.WrongCode::class.java) {
            kotlinx.coroutines.runBlocking { fake.confirmTotp("000000") }
        }
        val codes = fake.confirmTotp(fake.emailCode)
        assertEquals(10, codes.size)
        assertTrue(fake.getSecurity().totpEnabled)
        assertEquals(10, fake.getSecurity().recoveryCodesRemaining)

        val fresh = fake.regenerateRecoveryCodes(fake.emailCode)
        assertEquals(10, fresh.size)

        fake.disableTotp(fake.emailCode)
        assertFalse(fake.getSecurity().totpEnabled)
        assertEquals(0, fake.getSecurity().recoveryCodesRemaining)
    }

    @Test
    fun `passkeys list, rename and delete`() = runTest {
        val fake = FakeBackend()
        val before = fake.listPasskeys()
        assertEquals(1, before.size)
        val renamed = fake.renamePasskey(before[0].id, "Work laptop")
        assertEquals("Work laptop", renamed.name)
        fake.deletePasskey(before[0].id)
        assertTrue(fake.listPasskeys().isEmpty())
        assertThrows(AppError.NotFound::class.java) {
            kotlinx.coroutines.runBlocking { fake.deletePasskey(before[0].id) }
        }
    }

    @Test
    fun `sessions list, revoke one, revoke the others`() = runTest {
        val fake = FakeBackend()
        val sessions = fake.listSessions()
        assertEquals(2, sessions.size)
        assertTrue(sessions.any { it.current })

        val other = sessions.first { !it.current }
        fake.revokeSession(other.id)
        assertEquals(1, fake.listSessions().size)

        fake.beginTotpSetup() // unrelated, just exercising the fake stays independent per call
        val revoked = fake.revokeOtherSessions()
        assertEquals(0, revoked) // the only session left is the current one
        assertEquals(1, fake.listSessions().size)
        assertTrue(fake.listSessions().single().current)
    }

    @Test
    fun `delete account is blocked while owning a guild, then succeeds after leaving it`() = runTest {
        val fake = FakeBackend()
        val e = assertThrows(AppError.OwnsGuilds::class.java) {
            kotlinx.coroutines.runBlocking { fake.deleteAccount("current", null, null) }
        }
        assertTrue(e.guilds.any { it.name == "The Hideout" })

        fake.transferOwnership(FakeBackend.HIDEOUT, FakeBackend.ANA, "The Hideout")
        fake.deleteAccount("current", null, null)
    }

    @Test
    fun `delete account for a passwordless account needs a requested code`() = runTest {
        val fake = FakeBackend()
        fake.actingAs(FakeBackend.BRUNO)
        fake.transferOwnership(FakeBackend.COOKING, FakeBackend.TIM, "Cooking Club")
        assertThrows(AppError.Precondition::class.java) {
            kotlinx.coroutines.runBlocking { fake.deleteAccount(null, fake.emailCode, null) }
        }
        fake.requestAccountDeletionCode()
        fake.deleteAccount(null, fake.emailCode, null)
    }

    @Test
    fun `delete account with two-factor on requires a totp or recovery code`() = runTest {
        val fake = FakeBackend()
        fake.beginTotpSetup()
        fake.confirmTotp(fake.emailCode)
        fake.transferOwnership(FakeBackend.HIDEOUT, FakeBackend.ANA, "The Hideout")
        assertThrows(AppError.WrongCode::class.java) {
            kotlinx.coroutines.runBlocking { fake.deleteAccount("current", null, "000000") }
        }
        fake.deleteAccount("current", null, fake.emailCode)
    }

    @Test
    fun `export data is rate limited to once`() = runTest {
        val fake = FakeBackend()
        val masked = fake.exportData()
        assertTrue(masked.contains("@"))
        assertThrows(AppError.RateLimited::class.java) {
            kotlinx.coroutines.runBlocking { fake.exportData() }
        }
    }

    // ---- notification settings: merge and override precedence ---------------------------------

    @Test
    fun `notification settings default to mentions-only guilds and dms on`() = runTest {
        val fake = FakeBackend()
        val s = fake.getNotificationSettings()
        assertTrue(s.dmsEnabled)
        assertEquals(app.thehideout.core.model.NotificationLevel.Mentions, s.guildDefault)
    }

    @Test
    fun `an inherit override with no muted_until removes the entry instead of keeping a no-op row`() = runTest {
        val fake = FakeBackend()
        fake.updateNotificationSettings(
            guilds = listOf(app.thehideout.core.model.NotificationOverride(FakeBackend.HIDEOUT, app.thehideout.core.model.NotificationLevel.All)),
        )
        assertEquals(1, fake.getNotificationSettings().guilds.size)
        fake.updateNotificationSettings(
            guilds = listOf(app.thehideout.core.model.NotificationOverride(FakeBackend.HIDEOUT, app.thehideout.core.model.NotificationLevel.Inherit)),
        )
        assertTrue(fake.getNotificationSettings().guilds.isEmpty())
    }

    @Test
    fun `reducer applies a replaced notification settings reply like the gateway event`() {
        val s0 = loadedState()
        val replaced = app.thehideout.core.model.NotificationSettings(dmsEnabled = false, guildDefault = app.thehideout.core.model.NotificationLevel.All, showPreviews = false)
        val s1 = Reducer.reduce(s0, Action.NotificationSettingsReplaced(replaced)).state
        assertEquals(replaced, s1.notificationSettings)
    }
}
