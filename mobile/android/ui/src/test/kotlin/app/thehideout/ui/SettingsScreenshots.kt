package app.thehideout.ui

import androidx.activity.ComponentActivity
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.junit4.AndroidComposeTestRule
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import app.thehideout.core.InMemoryTokenStore
import app.thehideout.core.fake.FakeBackend
import app.thehideout.core.store.AppStore
import app.thehideout.core.store.SessionPhase
import app.thehideout.ui.shell.ShellActions
import app.thehideout.ui.shell.SignedInShell
import app.thehideout.ui.theme.HideoutTheme
import app.thehideout.ui.theme.Look
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.robolectric.annotation.LooperMode

private fun AndroidComposeTestRule<*, ComponentActivity>.settle() {
    repeat(20) {
        waitForIdle()
        shadowOf(android.os.Looper.getMainLooper()).idle()
    }
}

/**
 * Phase 5's Appearance, Security, Notifications and About pages, through the real Compose screens
 * against [FakeBackend] (so the Security summary, passkey, device and notification rows show real
 * data, not an "unavailable" fallback). Dark and light, on a phone.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@LooperMode(LooperMode.Mode.PAUSED)
@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
class SettingsScreenshots {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()

    private fun harness(dark: Boolean): AppStore {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val store = AppStore(FakeBackend(), scope, InMemoryTokenStore("fake-token-${FakeBackend.TIM}"))
        store.bootstrap()
        compose.setContent {
            HideoutTheme(look = Look(), systemDark = dark) {
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

    private fun page(dark: Boolean, open: String, screen: String) {
        harness(dark)
        compose.onNodeWithText(open).performClick()
        compose.settle()
        compose.capture(screen, Device.Phone, if (dark) Variant.Dark else Variant.Light)
    }

    @Test fun appearanceDark() = page(true, "Appearance", "settings-appearance")
    @Test fun appearanceLight() = page(false, "Appearance", "settings-appearance")
    @Test fun securityDark() = page(true, "Security", "settings-security")
    @Test fun securityLight() = page(false, "Security", "settings-security")
    @Test fun notificationsDark() = page(true, "Notifications", "settings-notifications")
    @Test fun notificationsLight() = page(false, "Notifications", "settings-notifications")
    @Test fun aboutDark() = page(true, "About", "settings-about")
    @Test fun aboutLight() = page(false, "About", "settings-about")

    @Config(sdk = [35], qualifiers = "w1280dp-h800dp-xhdpi")
    @Test fun securityTabletDark() {
        harness(true)
        compose.onNodeWithText("Security").performClick()
        compose.settle()
        compose.capture("settings-security", Device.Tablet, Variant.Dark)
    }

    @Test fun totpSetupDialogDark() {
        harness(true)
        compose.onNodeWithText("Security").performClick()
        compose.settle()
        compose.onNodeWithText("Authenticator app").performClick()
        compose.settle()
        compose.onNodeWithText("Set up").performClick()
        compose.settle()
        // The QR bitmap is encoded off the UI thread (Dispatchers.Default): wait for it to land.
        var qrReady = false
        repeat(50) {
            if (!qrReady) {
                qrReady = compose.onAllNodesWithContentDescription("QR code").fetchSemanticsNodes().isNotEmpty()
                if (!qrReady) { Thread.sleep(20); compose.settle() }
            }
        }
        compose.capture("settings-security-totp-setup", Device.Phone, Variant.Dark)
    }

    @Test fun deleteAccountOwnsServersDark() {
        harness(true)
        compose.onNodeWithText("Security").performClick()
        compose.settle()
        compose.onNodeWithText("Delete account").performClick()
        compose.settle()
        compose.onNodeWithTag("delete-account-password").performTextInput("current")
        compose.onNodeWithTag("delete-account-submit").performClick()
        compose.settle()
        compose.capture("settings-delete-account-owns-servers", Device.Phone, Variant.Dark)
    }
}
