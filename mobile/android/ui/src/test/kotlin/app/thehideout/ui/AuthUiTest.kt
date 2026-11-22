package app.thehideout.ui

import androidx.activity.ComponentActivity
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import app.thehideout.core.InMemoryTokenStore
import app.thehideout.core.auth.AuthFlow
import app.thehideout.core.fake.FakeBackend
import app.thehideout.core.store.AppStore
import app.thehideout.core.store.SessionPhase
import app.thehideout.ui.auth.AuthScreen
import app.thehideout.ui.theme.HideoutTheme
import app.thehideout.ui.theme.Look
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.robolectric.annotation.LooperMode

/**
 * The auth flows against the fake backend, through the real Compose screens: sign in lands in the
 * shell, an invite creates the account and lands in the shell, a wrong code shows its inline error.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@LooperMode(LooperMode.Mode.PAUSED)
@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
class AuthUiTest {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()

    private fun harness(fake: FakeBackend = FakeBackend()) {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val store = AppStore(fake, scope, InMemoryTokenStore())
        val auth = AuthFlow({ fake }, scope, { "test device" }, onSession = store::onSignedIn)
        compose.setContent {
            HideoutTheme(look = Look(), systemDark = true) {
                val state by store.state.collectAsState()
                val authState by auth.state.collectAsState()
                if (state.phase == SessionPhase.SignedIn) {
                    androidx.compose.material3.Text("SHELL ${state.me?.username}")
                } else {
                    AuthScreen(authState, auth, "http://10.0.2.2:8080", {}, { true }, {})
                }
            }
        }
    }

    private fun field(index: Int, text: String) {
        compose.onAllNodes(hasSetTextAction()).let { it[index].performClick() }
        compose.onAllNodes(hasSetTextAction()).let { it[index].performTextInput(text) }
        compose.waitForIdle()
    }

    @Test fun `sign in with a password lands in the shell`() {
        harness()
        field(0, "tim")
        field(1, "hunter2")
        compose.onNodeWithText("Sign in").performClick()
        compose.waitForIdle()
        compose.onNodeWithText("SHELL tim").assertIsDisplayed()
    }

    @Test fun `a wrong password shows the form error and stays`() {
        harness()
        field(0, "tim")
        field(1, "wrong")
        compose.onNodeWithText("Sign in").performClick()
        compose.waitForIdle()
        compose.onNodeWithText("Wrong username, email or password.").assertIsDisplayed()
        org.junit.Assert.assertTrue(compose.onAllNodes(hasText("SHELL tim")).fetchSemanticsNodes().isEmpty())
    }

    @Test fun `an invite code creates the account and lands in the shell`() {
        harness()
        compose.onNodeWithText("Join with a code").performClick()
        compose.waitForIdle()
        field(0, "wxyz2345")
        compose.onNodeWithText("Continue").performClick()
        compose.waitForIdle()
        compose.onNodeWithText("tim invited you to join The Hideout").assertIsDisplayed()
        field(0, "erin")
        field(1, "password123")
        compose.onNodeWithContentDescription("I agree to the Terms and Privacy Policy").performClick()
        compose.onNodeWithContentDescription("I'm 16 or older").performClick()
        compose.waitForIdle()
        compose.onNodeWithText("Create account").performClick()
        compose.waitForIdle()
        compose.onNodeWithText("SHELL erin").assertIsDisplayed()
    }

    @Test fun `a wrong email code shows the inline error under the code field`() {
        harness()
        compose.onNodeWithText("Email me a code").performClick()
        compose.waitForIdle()
        field(0, "tim@example.com")
        compose.onNodeWithText("Send code").performClick()
        compose.waitForIdle()
        compose.onNodeWithText("Check your email").assertIsDisplayed()
        field(0, "000000") // a full code goes at once
        compose.waitForIdle()
        compose.onNodeWithText("That code didn't work. Check it and try again.").assertIsDisplayed()
    }

    @Test fun `the server sheet opens prefilled with the current server url`() {
        harness()
        compose.onNodeWithText("Change").performClick()
        compose.waitForIdle()
        compose.onNodeWithText("http://10.0.2.2:8080").assertIsDisplayed()
    }

    @Test fun `a pasted invite link is rejected with the code-only hint`() {
        harness()
        compose.onNodeWithText("Join with a code").performClick()
        compose.waitForIdle()
        field(0, "https://thehideout.zabit.si/invite/tok-wxyz")
        compose.onNodeWithText("Continue").performClick()
        compose.waitForIdle()
        compose.onNodeWithText("Paste just the code from your invite email, not the link.").assertIsDisplayed()
    }
}
