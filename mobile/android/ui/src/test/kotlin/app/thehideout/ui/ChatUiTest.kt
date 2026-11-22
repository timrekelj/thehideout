package app.thehideout.ui

import androidx.activity.ComponentActivity
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
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
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.robolectric.annotation.LooperMode

/** Chat against the fake backend, through the real Compose screens (phase 3a). */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@LooperMode(LooperMode.Mode.PAUSED)
@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
class ChatUiTest {
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
        compose.waitForIdle()
        return store
    }

    @Test fun `opening a channel and sending a message shows it pending then sent`() {
        val fake = FakeBackend(latency = java.time.Duration.ZERO)
        val store = harness(fake)
        compose.onNodeWithText("general").performClick()
        compose.onNodeWithTag("composer-field").performTextInput("hello from the test")
        compose.onNodeWithContentDescription("Send").performClick()
        compose.waitForIdle()
        assertTrue(fake.sent.any { it.content == "hello from the test" })
        assertTrue("resolved once the fake echoes it", store.state.value.outgoing.none { it.content == "hello from the test" })
        compose.onNodeWithText("hello from the test").assertIsDisplayed()
    }

    @Test fun `reacting toggles the chip and asks the fake`() {
        val store = harness()
        compose.onNodeWithText("general").performClick()
        compose.waitForIdle()
        val channel = FakeBackend.GENERAL
        val message = store.state.value.log(channel)!!.messages.last { it.reactions.isNotEmpty() }
        val before = message.reactions.first().me
        store.react(channel, message.id, message.reactions.first().emoji)
        compose.waitForIdle()
        val after = store.state.value.log(channel)!!.messages.first { it.id == message.id }.reactions.first()
        assertEquals(!before, after.me)
    }

    @Test fun `editing own message changes its content`() {
        val fake = FakeBackend()
        val store = harness(fake)
        compose.onNodeWithText("general").performClick()
        compose.onNodeWithTag("composer-field").performTextInput("to be edited")
        compose.onNodeWithContentDescription("Send").performClick()
        compose.waitForIdle()
        val mine = store.state.value.log(FakeBackend.GENERAL)!!.messages.last { it.content == "to be edited" }
        store.editMessage(FakeBackend.GENERAL, mine.id, "edited content")
        compose.waitForIdle()
        assertEquals("edited content", store.state.value.log(FakeBackend.GENERAL)!!.messages.first { it.id == mine.id }.content)
    }

    @Test fun `deleting own message removes it`() {
        val store = harness()
        compose.onNodeWithText("general").performClick()
        compose.onNodeWithTag("composer-field").performTextInput("to be deleted")
        compose.onNodeWithContentDescription("Send").performClick()
        compose.waitForIdle()
        val mine = store.state.value.log(FakeBackend.GENERAL)!!.messages.last { it.content == "to be deleted" }
        store.deleteMessage(FakeBackend.GENERAL, mine.id)
        compose.waitForIdle()
        assertTrue(store.state.value.log(FakeBackend.GENERAL)!!.messages.none { it.id == mine.id })
    }

    @Test fun `opening a dm from the dms tab shows its transcript`() {
        val store = harness()
        compose.onNodeWithText("DMs").performClick()
        compose.onNodeWithText("ana").performClick()
        compose.waitForIdle()
        val s = store.state.value
        assertEquals(FakeBackend.DM_ANA, s.homeDm)
        assertTrue(s.log(FakeBackend.DM_ANA)?.messages?.isNotEmpty() == true)
    }
}
