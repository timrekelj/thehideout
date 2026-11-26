package app.thehideout.ui

import androidx.activity.ComponentActivity
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.junit4.AndroidComposeTestRule
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithContentDescription
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
    repeat(50) {
        waitForIdle()
        shadowOf(android.os.Looper.getMainLooper()).idle()
        if (until()) return
        Thread.sleep(20)
    }
}

/**
 * Create/join servers, create channels and sections, and invites (phase 4a), through the real
 * Compose screens against [FakeBackend]. A tablet width, like [TabletScreenshots]: the list and
 * detail panes of [app.thehideout.ui.shell.ServersScreen]'s adaptive scaffold show at once, so the
 * server strip and its "+"/"..." menus are on screen without a back navigation first (phone width
 * auto-navigates to the open channel's detail pane on launch, as [ChatUiTest] notes).
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@LooperMode(LooperMode.Mode.PAUSED)
@Config(sdk = [35], qualifiers = "w1280dp-h800dp-xhdpi")
class ManageUiTest {
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
        return store
    }

    @Test fun `create server appears in the strip and opens on its general channel`() {
        val store = harness()
        compose.onNodeWithTag("add-server").performClick()
        compose.onNodeWithText("Create server").performClick()
        compose.settle()
        compose.onNodeWithTag("create-server-name").performTextInput("Pizza Club")
        compose.onNodeWithTag("create-server-submit").performClick()
        compose.settle { store.state.value.guilds.any { it.name == "Pizza Club" } }
        assertTrue(store.state.value.guilds.any { it.name == "Pizza Club" })
        // The sheet's own closing animation can still hold its text field's "Pizza Club" an instant after dismissal.
        compose.settle { compose.onAllNodesWithTag("create-server-name").fetchSemanticsNodes().isEmpty() }
        compose.onNodeWithText("general").assertIsDisplayed()
    }

    @Test fun `joining by an 8-box code opens that server`() {
        val store = harness()
        compose.onNodeWithTag("add-server").performClick()
        compose.onNodeWithText("Join server").performClick()
        compose.settle()
        compose.onNodeWithTag("invite-code-input").performTextInput("garage22")
        compose.onNodeWithTag("join-server-submit").performClick()
        compose.settle { store.state.value.currentGuild?.id == FakeBackend.GARAGE }
        assertEquals(FakeBackend.GARAGE, store.state.value.currentGuild?.id)
    }

    @Test fun `creating a channel lists it under the server`() {
        val store = harness()
        compose.onNodeWithTag("server-menu").performClick()
        compose.onNodeWithText("Create channel").performClick()
        compose.settle()
        compose.onNodeWithTag("create-channel-name").performTextInput("ops")
        compose.onNodeWithTag("create-channel-submit").performClick()
        compose.settle { store.state.value.channelsOf(FakeBackend.HIDEOUT).any { it.name == "ops" } }
        compose.onNodeWithText("ops").assertIsDisplayed()
    }

    @Test fun `the invite sheet shows the server code and shares the link`() {
        harness()
        compose.onNodeWithTag("server-menu").performClick()
        compose.onNodeWithText("Invite people").performClick()
        compose.settle()
        compose.onNodeWithTag("invite-code-value").assertIsDisplayed().assertTextEquals("HIDEOUT1")
        compose.onNodeWithContentDescription("Share the link").performClick()
        compose.settle { shadowOf(compose.activity).peekNextStartedActivity() != null }
        val started = shadowOf(compose.activity).peekNextStartedActivity()
        assertTrue("expected a chooser for an ACTION_SEND intent, got $started", started != null)
    }
}
