package app.thehideout.ui

import androidx.activity.ComponentActivity
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.junit4.AndroidComposeTestRule
import androidx.compose.ui.test.junit4.StateRestorationTester
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

    @Test fun `deleting the account with a password confirms, then the store itself signs out`() {
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
        // The delayed sign-out is scheduled on the store's own scope (AppStore.deleteAccount), not
        // this screen's: it fires even though nothing here calls `signOut()` directly, which is the
        // whole point — leaving the screen (or it never existing) couldn't have skipped it either.
        compose.settle { store.state.value.phase is SessionPhase.SignedOut }
        assertTrue(store.state.value.phase is SessionPhase.SignedOut)
    }

    @Test fun `back is blocked while the delete-account request is pending`() {
        val fake = FakeBackend()
        runBlocking { fake.transferOwnership(FakeBackend.HIDEOUT, FakeBackend.ANA, "The Hideout") }
        harness(fake)
        compose.onNodeWithText("Security").performClick()
        compose.settle()
        compose.onNodeWithText("Delete account").performClick()
        compose.settle()
        compose.onNodeWithTag("delete-account-password").performTextInput("current")
        // Slow the fake down so the request is still in flight right after tapping submit.
        fake.latency = java.time.Duration.ofMillis(800)
        compose.onNodeWithTag("delete-account-submit").performClick()
        compose.waitForIdle()
        compose.onNodeWithTag("delete-account-back").assertIsNotEnabled()
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

    @Test fun `a per-channel notification override is saved and reflected on the row`() {
        val store = harness()
        compose.onNodeWithText("Notifications").performClick()
        compose.settle()
        compose.onNodeWithText("The Hideout").performClick()
        compose.settle()
        compose.onNodeWithText("general").performClick()
        compose.settle()
        compose.onNodeWithTag("channel-Mentions").performClick()
        compose.onNodeWithText("Save").performClick()
        compose.settle { store.state.value.notificationSettings?.channels?.any { it.id == FakeBackend.GENERAL && it.level == NotificationLevel.Mentions } == true }
        val override = store.state.value.notificationSettings?.channels?.first { it.id == FakeBackend.GENERAL }
        assertEquals(NotificationLevel.Mentions, override?.level)
    }

    @Test fun `renaming and deleting a passkey`() {
        val fake = FakeBackend()
        val store = harness(fake)
        // The rename dialog is driven at the store level, not by typing into it: see the comment
        // above `changing the email is reflected as verified` — the same `AlertDialog` +
        // `OutlinedTextField` flakiness applies here too. Deleting has no text field and is driven
        // through the real confirm dialog below.
        runBlocking { store.renamePasskey(9101L, "Work laptop").getOrThrow() }
        compose.onNodeWithText("Security").performClick()
        compose.settle()
        compose.onNodeWithText("Passkeys").performClick()
        compose.settle()
        compose.onNodeWithText("Work laptop").assertIsDisplayed()
        compose.onNodeWithContentDescription("Delete Work laptop").performClick()
        compose.settle()
        compose.onNodeWithText("Delete").performClick()
        compose.settle()
        compose.onNodeWithText("No passkeys yet.").assertIsDisplayed()
    }

    @Test fun `signing out every other device`() {
        harness()
        compose.onNodeWithText("Security").performClick()
        compose.settle()
        compose.onNodeWithText("Sessions").performClick()
        compose.settle()
        compose.onNodeWithText("Sign out others").performClick() // the top-bar action, opening the confirm dialog
        compose.settle()
        compose.onNodeWithText("Everywhere you're signed in except this device will be signed out.").assertIsDisplayed()
        compose.onNodeWithTag("revoke-others-submit").performClick()
        compose.settle()
        compose.onAllNodesWithText("Chrome on Mac").assertCountEquals(0)
        compose.onNodeWithText("This device").assertIsDisplayed()
    }

    // The "change email" and "disable 2FA" dialogs are driven at the store level rather than by
    // typing into their `AlertDialog`s: that combination (an `OutlinedTextField` inside an
    // `AlertDialog`, opened fresh) is flaky under this Robolectric configuration — it occasionally
    // never reports idle (`AppNotIdleException`) even before any text is typed, independent of
    // which dialog. `SecurityTest` (core) exercises the same `ChangeEmail`/`DisableTotp` calls
    // directly against the fake, and the live Maestro/adb check exercised password-change and TOTP
    // end to end against a real server; this only confirms the Security list reflects the result.
    @Test fun `changing the email is reflected as verified`() {
        val fake = FakeBackend()
        val store = harness(fake)
        runBlocking {
            store.changeEmail("new@example.com", "current").getOrThrow()
            store.confirmEmailChange(fake.emailCode).getOrThrow()
        }
        compose.onNodeWithText("Security").performClick()
        compose.settle()
        compose.onNodeWithText("Verified").assertIsDisplayed()
    }

    @Test fun `turning two-factor off is reflected in the summary`() {
        val fake = FakeBackend()
        val store = harness(fake)
        runBlocking {
            fake.beginTotpSetup()
            fake.confirmTotp(fake.emailCode)
            store.disableTotp(fake.emailCode).getOrThrow()
        }
        compose.onNodeWithText("Security").performClick()
        compose.settle()
        compose.onNodeWithText("Authenticator app").performClick()
        compose.settle()
        compose.onNodeWithText("Set up").assertIsDisplayed()
    }

    @Test fun `opening a licence page shows the real bundled text`() {
        harness() // already lands on the You tab
        compose.onNodeWithText("About").performClick()
        compose.settle()
        compose.onNodeWithText("Open-source licences").performClick()
        compose.settle()
        compose.onNodeWithText("Instrument Sans").performClick()
        compose.settle { compose.onAllNodesWithText("Loading…").fetchSemanticsNodes().isEmpty() }
        compose.onNodeWithText("This Font Software is licensed under the SIL Open Font License", substring = true).assertIsDisplayed()
    }

    /** Like [harness], but through a [StateRestorationTester] so `emulateSavedInstanceStateRestore`
     * can simulate a configuration change without disturbing the (already-constructed) store. */
    private fun restorationHarness(restorationTester: StateRestorationTester, fake: FakeBackend = FakeBackend()): AppStore {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val store = AppStore(fake, scope, InMemoryTokenStore("fake-token-${FakeBackend.TIM}"))
        store.bootstrap()
        restorationTester.setContent {
            HideoutTheme(look = Look(), systemDark = true) {
                val state by store.state.collectAsState()
                if (state.phase == SessionPhase.SignedIn) SignedInShell(state, Look(), {}, ShellActions.of(store), "0.1.0")
            }
        }
        compose.settle()
        compose.onNodeWithText("You").performClick()
        compose.settle()
        return store
    }

    @Test fun `the You tab's settings route survives a saved-state restore`() {
        val restorationTester = StateRestorationTester(compose)
        restorationHarness(restorationTester)
        compose.onNodeWithText("Appearance").performClick()
        compose.settle()
        restorationTester.emulateSavedInstanceStateRestore()
        compose.settle()
        // The You list stays mounted underneath the route overlay, so both its own "Appearance"
        // row and this page's header match a plain text query; "ACCENT" only exists on this page.
        compose.onNodeWithText("ACCENT").assertIsDisplayed() // still on the Appearance page, not back at the You list
    }

    @Test fun `the Security route survives a saved-state restore, but recovery codes are never persisted`() {
        val restorationTester = StateRestorationTester(compose)
        val fake = FakeBackend()
        restorationHarness(restorationTester, fake)
        compose.onNodeWithText("Security").performClick()
        compose.settle()
        compose.onNodeWithText("Sessions").performClick()
        compose.settle()
        restorationTester.emulateSavedInstanceStateRestore()
        compose.settle()
        compose.onNodeWithText("Devices", substring = true).assertIsDisplayed() // the Devices page survived the restore

        // Now check the data-bearing route specifically: it must restore to the Security list, not
        // crash trying to marshal recovery codes, and not show stale codes from before the restore.
        // The Security list stays mounted underneath the Devices overlay, so a plain "Back"
        // content-description query is ambiguous between the two; target the Devices one by tag.
        compose.onNodeWithTag("devices-back").performClick()
        compose.settle()
        compose.onNodeWithText("Authenticator app").performClick()
        compose.settle()
        compose.onNodeWithText("Set up").performClick()
        compose.settle()
        compose.onNodeWithText("Code from the app").performTextInput(fake.emailCode)
        compose.onNodeWithText("Turn on").performClick()
        compose.settle()
        compose.onNodeWithText("Save your recovery codes").assertIsDisplayed()
        restorationTester.emulateSavedInstanceStateRestore()
        compose.settle()
        compose.onAllNodesWithText("Save your recovery codes").assertCountEquals(0)
        // The You list's row title is merged as its own text node with the literal value
        // "Security" too, so even an exact match is ambiguous while it stays mounted underneath;
        // target the Security list's own back button by tag instead.
        compose.onNodeWithTag("security-back").assertIsDisplayed() // back on the Security list, not a sub-route
    }

    @Test fun `the About tab's licence route survives a saved-state restore`() {
        val restorationTester = StateRestorationTester(compose)
        restorationTester.setContent {
            HideoutTheme(look = Look(), systemDark = true) {
                app.thehideout.ui.shell.AboutScreen("0.1.0", "http://example.com", {}, onBack = {})
            }
        }
        compose.settle()
        compose.onNodeWithText("Open-source licences").performClick()
        compose.settle()
        restorationTester.emulateSavedInstanceStateRestore()
        compose.settle()
        compose.onNodeWithText("BUNDLED FONTS").assertIsDisplayed() // GroupLabel uppercases; only this page has this heading
    }
}
