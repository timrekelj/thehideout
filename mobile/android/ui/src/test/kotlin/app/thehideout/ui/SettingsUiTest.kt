package app.thehideout.ui

import androidx.activity.ComponentActivity
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.AndroidComposeTestRule
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import app.thehideout.core.InMemoryTokenStore
import app.thehideout.core.fake.FakeBackend
import app.thehideout.core.model.NotificationLevel
import app.thehideout.core.store.AppStore
import app.thehideout.core.store.SessionPhase
import app.thehideout.design.AccentPreset
import app.thehideout.ui.shell.AppearanceScreen
import app.thehideout.ui.shell.ShellActions
import app.thehideout.ui.shell.SignedInShell
import app.thehideout.ui.theme.HideoutTheme
import app.thehideout.ui.theme.Look
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.robolectric.annotation.LooperMode

private fun AndroidComposeTestRule<*, ComponentActivity>.settle(until: () -> Boolean = { true }) {
    repeat(150) {
        waitForIdle()
        shadowOf(android.os.Looper.getMainLooper()).idleFor(java.time.Duration.ofMillis(50))
        if (until()) return
        Thread.sleep(20)
    }
}

/**
 * Phase 5 flows through the real Compose screens against [FakeBackend]: changing the look,
 * two-factor setup and recovery codes, revoking a device, deleting the account (both the password
 * path and the owned-servers branch) and a notification override.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@LooperMode(LooperMode.Mode.PAUSED)
@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
class SettingsUiTest {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()

    private fun harness(fake: FakeBackend = FakeBackend()): AppStore {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val store = AppStore(fake, scope, InMemoryTokenStore("fake-token-${FakeBackend.TIM}"))
        store.bootstrap()
        compose.setContent {
            HideoutTheme(look = Look(), systemDark = true) {
                val state by store.state.collectAsState()
                if (state.phase == SessionPhase.SignedIn) {
                    SignedInShell(state, Look(), {}, ShellActions.of(store), "0.1.0")
                }
            }
        }
        compose.settle()
        compose.onNodeWithText("You").performClick()
        compose.settle()
        return store
    }

    @Test fun `changing the accent calls back with the new look`() {
        var current = Look()
        compose.setContent {
            HideoutTheme(look = current, systemDark = true) {
                AppearanceScreen(current, { current = it }, onBack = {})
            }
        }
        compose.settle()
        assertEquals(AccentPreset.SageTeal, current.spec.accent)
        compose.onNodeWithContentDescription("Ocean").performClick()
        compose.settle()
        assertEquals(AccentPreset.Ocean, current.spec.accent)
    }

    @Test fun `totp setup confirms, shows recovery codes once, and the summary reflects it`() {
        val store = harness()
        compose.onNodeWithText("Security").performClick()
        compose.settle()
        compose.onNodeWithText("Authenticator app").performClick()
        compose.settle()
        compose.onNodeWithText("Set up").performClick()
        compose.settle()
        compose.onNodeWithText("Code from the app").performTextInput("123456")
        compose.onNodeWithText("Turn on").performClick()
        compose.settle()
        compose.onNodeWithText("Save your recovery codes").assertIsDisplayed()
        compose.onNodeWithText("I saved these codes somewhere safe").performClick()
        compose.onNodeWithText("Done").performClick()
        compose.settle()
        compose.onNodeWithText("On · 10 recovery codes left").assertIsDisplayed()
    }

    @Test fun `regenerating recovery codes replaces them and the security summary reflects it`() {
        val fake = FakeBackend()
        val store = harness(fake)
        runBlocking {
            fake.beginTotpSetup()
            fake.confirmTotp("123456")
        }
        val after = runBlocking { store.regenerateRecoveryCodes("123456") }.getOrThrow()
        assertEquals(10, after.size)
        compose.onNodeWithText("Security").performClick()
        compose.settle()
        compose.onNodeWithText("On · 10 recovery codes left").assertIsDisplayed()
    }

    @Test fun `revoking a device removes it from the list`() {
        harness()
        compose.onNodeWithText("Security").performClick()
        compose.settle()
        compose.onNodeWithText("Sessions").performClick()
        compose.settle()
        compose.onNodeWithText("Chrome on Mac").assertIsDisplayed()
        compose.onNodeWithContentDescription("Sign out").performClick()
        compose.settle()
        compose.onNodeWithTag("revoke-session-submit").performClick()
        compose.settle()
        compose.onAllNodesWithText("Chrome on Mac").assertCountEquals(0)
    }

    @Test fun `deleting the account with a password accepts it and confirms before signing out`() {
        val fake = FakeBackend()
        runBlocking { fake.transferOwnership(FakeBackend.HIDEOUT, FakeBackend.ANA, "The Hideout") }
        val store = harness(fake)
        compose.onNodeWithText("Security").performClick()
        compose.settle()
        compose.onNodeWithText("Delete account").performClick()
        compose.settle()
        compose.onNodeWithTag("delete-account-password").performTextInput("current")
        compose.onNodeWithTag("delete-account-submit").performClick()
        compose.settle()
        // The server already accepted the deletion (sessions revoked); the screen shows a
        // confirmation before the app itself signs out, so the message is never silently skipped.
        compose.onNodeWithText("Your account has been deleted. Signing you out…").assertIsDisplayed()
        assertTrue(store.state.value.phase == SessionPhase.SignedIn) // still mounted; the UI's own delayed signOut hasn't fired yet
    }

    @Test fun `deleting the account while owning a server shows the owned-servers branch`() {
        harness() // tim owns The Hideout by default
        compose.onNodeWithText("Security").performClick()
        compose.settle()
        compose.onNodeWithText("Delete account").performClick()
        compose.settle()
        compose.onNodeWithTag("delete-account-password").performTextInput("current")
        compose.onNodeWithTag("delete-account-submit").performClick()
        compose.settle()
        compose.onNodeWithText("You still own these servers. Transfer or delete them from each server's settings first:").assertIsDisplayed()
        compose.onNodeWithText("• The Hideout (4 members)").assertIsDisplayed()
    }

    @Test fun `a per-server notification override is saved and reflected back`() {
        val store = harness()
        compose.onNodeWithText("Notifications").performClick()
        compose.settle()
        compose.onNodeWithText("The Hideout").performClick()
        compose.settle()
        compose.onNodeWithTag("guild-All").performClick()
        compose.settle { store.state.value.notificationSettings?.guilds?.any { it.id == FakeBackend.HIDEOUT && it.level == NotificationLevel.All } == true }
        val override = store.state.value.notificationSettings?.guilds?.first { it.id == FakeBackend.HIDEOUT }
        assertEquals(NotificationLevel.All, override?.level)
    }
}
