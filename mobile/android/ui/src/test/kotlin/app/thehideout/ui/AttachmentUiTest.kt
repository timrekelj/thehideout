package app.thehideout.ui

import androidx.activity.ComponentActivity
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeLeft
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
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.robolectric.annotation.LooperMode
import org.robolectric.shadows.ShadowDialog

/** Tap an image attachment -> the lightbox opens; swipe -> the next picture; back -> it closes (ATT-05). */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@LooperMode(LooperMode.Mode.PAUSED)
@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
class AttachmentUiTest {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()

    private fun harness(): AppStore {
        val fake = FakeBackend()
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
        compose.waitForIdle()
        store.openChannel(FakeBackend.DESIGN)
        compose.waitForIdle()
        return store
    }

    @Test fun `tapping an image opens the lightbox, swiping moves to the next, back closes it`() {
        harness()
        compose.onAllNodesWithContentDescription("View mood-1.jpg")[0].performScrollTo().performClick()
        compose.waitForIdle()
        compose.onNodeWithText("mood-1.jpg · 1 of 4").assertIsDisplayed()

        compose.onNodeWithContentDescription("mood-1.jpg").performTouchInput { swipeLeft() }
        compose.waitForIdle()
        compose.onNodeWithText("mood-2.jpg · 2 of 4").assertIsDisplayed()

        // The lightbox is a Dialog, a separate window: exercise its own back handling
        // (DialogProperties.dismissOnBackPress -> onDismissRequest), not the host Activity's.
        @Suppress("DEPRECATION")
        ShadowDialog.getLatestDialog().onBackPressed()
        compose.waitForIdle()
        compose.onAllNodesWithText("mood-2.jpg · 2 of 4").assertCountEquals(0)
    }
}
