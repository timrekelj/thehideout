package app.thehideout.ui

import androidx.activity.ComponentActivity
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextReplacement
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.SemanticsNodeInteraction
import app.thehideout.core.InMemoryTokenStore
import app.thehideout.core.fake.FakeBackend
import app.thehideout.core.model.ReactionEmoji
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

/**
 * Chat against the fake backend, through the real Compose screens (phase 3a). Bootstrapping
 * already opens The Hideout's #general (the reducer's first-guild/first-channel rule), so on this
 * phone width the detail pane - the transcript - is what these tests land on directly.
 */
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

    /** The markdown showcase line (CVETKA's): the stored content, and how it renders with its markup stripped. */
    private val MARKDOWN_RAW = "Release notes are up: **v0.0.3** adds a live gateway, ~~polling~~ and *much* better reconnects."
    private val MARKDOWN_DISPLAY = "Release notes are up: v0.0.3 adds a live gateway, polling and much better reconnects."

    private fun SemanticsNodeInteraction.editableText(): String =
        fetchSemanticsNode().config.getOrNull(SemanticsProperties.EditableText)?.text.orEmpty()

    /**
     * Only the transcript's top bar has a "Back" action (it appears once the list-detail
     * navigator can go back): existence, not screen bounds, is what proves the detail pane - not
     * just the list - is what is showing on this phone width.
     */
    private fun assertOnDetailPane() {
        assertTrue("the detail pane (with its Back action) should be showing", compose.onAllNodesWithContentDescription("Back").fetchSemanticsNodes().isNotEmpty())
    }

    @Test fun `opening a channel and sending a message shows it pending then sent`() {
        val fake = FakeBackend(latency = java.time.Duration.ZERO)
        val store = harness(fake)
        compose.onNodeWithTag("composer-field").performTextInput("hello from the test")
        compose.onNodeWithContentDescription("Send").performClick()
        compose.waitForIdle()
        assertTrue(fake.sent.any { it.content == "hello from the test" })
        assertTrue("resolved once the fake echoes it", store.state.value.outgoing.none { it.content == "hello from the test" })
        compose.onNodeWithText("hello from the test").assertIsDisplayed()
    }

    @Test fun `long-pressing a message and tapping a quick reaction toggles it`() {
        val store = harness()
        compose.onAllNodesWithText(MARKDOWN_DISPLAY)[0].performTouchInput { longClick() }
        compose.waitForIdle()
        compose.onNodeWithText("😂").performClick()
        compose.waitForIdle()
        val message = store.state.value.log(FakeBackend.GENERAL)!!.messages.first { it.content == MARKDOWN_RAW }
        assertTrue(message.reactions.any { it.emoji == ReactionEmoji.Unicode("😂") && it.me })
    }

    @Test fun `editing own message through the action sheet's Save changes its content`() {
        val fake = FakeBackend(latency = java.time.Duration.ZERO)
        val store = harness(fake)
        compose.onNodeWithTag("composer-field").performTextInput("to be edited")
        compose.onNodeWithContentDescription("Send").performClick()
        compose.waitForIdle()
        compose.onAllNodesWithText("to be edited")[0].performTouchInput { longClick() }
        compose.waitForIdle()
        compose.onNodeWithText("Edit message").performClick()
        compose.waitForIdle()
        compose.onNodeWithTag("composer-field").performTextReplacement("edited content")
        compose.onNodeWithContentDescription("Save").performClick()
        compose.waitForIdle()
        val mine = store.state.value.log(FakeBackend.GENERAL)!!.messages.last { it.id != 0L && it.authorId == FakeBackend.TIM }
        assertEquals("edited content", mine.content)
        compose.onNodeWithText("edited content").assertIsDisplayed()
    }

    @Test fun `cancelling an edit restores the draft that was there before`() {
        val fake = FakeBackend(latency = java.time.Duration.ZERO)
        val store = harness(fake)
        compose.onNodeWithTag("composer-field").performTextInput("own message to edit")
        compose.onNodeWithContentDescription("Send").performClick()
        compose.waitForIdle()
        compose.onNodeWithTag("composer-field").performTextInput("my unsent draft")
        compose.waitForIdle()
        compose.onAllNodesWithText("own message to edit")[0].performTouchInput { longClick() }
        compose.waitForIdle()
        compose.onNodeWithText("Edit message").performClick()
        compose.waitForIdle()
        assertEquals("own message to edit", compose.onNodeWithTag("composer-field").editableText())
        compose.onNodeWithText("Cancel").performClick()
        compose.waitForIdle()
        assertEquals("my unsent draft", compose.onNodeWithTag("composer-field").editableText())
        assertEquals("my unsent draft", store.state.value.draft(FakeBackend.GENERAL))
    }

    @Test fun `deleting own message through the confirm dialog removes it`() {
        val fake = FakeBackend(latency = java.time.Duration.ZERO)
        val store = harness(fake)
        compose.onNodeWithTag("composer-field").performTextInput("to be deleted")
        compose.onNodeWithContentDescription("Send").performClick()
        compose.waitForIdle()
        compose.onAllNodesWithText("to be deleted")[0].performTouchInput { longClick() }
        compose.waitForIdle()
        compose.onNodeWithText("Delete message").performClick()
        compose.waitForIdle()
        compose.onNodeWithText("Delete").performClick()
        compose.waitForIdle()
        assertTrue(store.state.value.log(FakeBackend.GENERAL)!!.messages.none { it.content == "to be deleted" })
    }

    @Test fun `tapping a mention autocomplete suggestion inserts the full mention`() {
        val fake = FakeBackend(latency = java.time.Duration.ZERO)
        harness(fake)
        compose.onNodeWithTag("composer-field").performTextInput("hey @a")
        compose.waitForIdle()
        compose.onNodeWithTag("autocomplete-ana").performClick()
        compose.waitForIdle()
        compose.onNodeWithContentDescription("Send").performClick()
        compose.waitForIdle()
        assertEquals("hey @ana", fake.sent.last().content.trim())
    }

    @Test fun `switching channels keeps each one's draft and restores it on return`() {
        val store = harness()
        compose.onNodeWithTag("composer-field").performTextInput("general draft")
        compose.waitForIdle()
        store.openChannel(FakeBackend.DEV)
        compose.waitForIdle()
        assertEquals("", compose.onNodeWithTag("composer-field").editableText())
        compose.onNodeWithTag("composer-field").performTextInput("dev draft")
        compose.waitForIdle()
        store.openChannel(FakeBackend.GENERAL)
        compose.waitForIdle()
        assertEquals("general draft", compose.onNodeWithTag("composer-field").editableText())
        store.openChannel(FakeBackend.DEV)
        compose.waitForIdle()
        assertEquals("dev draft", compose.onNodeWithTag("composer-field").editableText())
    }

    @Test fun `opening a dm from the dms tab shows its transcript`() {
        val store = harness()
        compose.onNodeWithText("DMs").performClick()
        compose.onNodeWithText("ana").performClick()
        compose.waitForIdle()
        val s = store.state.value
        assertEquals(FakeBackend.DM_ANA, s.homeDm)
        assertTrue(s.log(FakeBackend.DM_ANA)?.messages?.isNotEmpty() == true)
        assertOnDetailPane()
    }

    @Test fun `tapping Message on a member's profile opens the dm, even on a phone`() {
        harness()
        compose.onNodeWithContentDescription("Members").performClick()
        compose.waitForIdle()
        compose.onNodeWithTag("member-row-${FakeBackend.ANA}").performClick()
        compose.waitForIdle()
        compose.onNodeWithText("Message").performClick()
        compose.waitForIdle()
        // The detail pane, not just the DMs list, must be showing (a "Back" action only the transcript's top bar has).
        assertOnDetailPane()
    }

    @Test fun `tapping a mention opens its channel and shows the transcript, even on a phone`() {
        val fake = FakeBackend()
        val store = harness(fake)
        store.openChannel(FakeBackend.DEV)
        compose.waitForIdle()
        fake.post(FakeBackend.DEV, FakeBackend.BRUNO, "hi @tim, can you look?")
        compose.waitForIdle()
        store.openChannel(FakeBackend.GENERAL)
        compose.waitForIdle()
        compose.onNodeWithText("Mentions").performClick()
        compose.waitForIdle()
        compose.onNodeWithText("hi @tim, can you look?").performClick()
        compose.waitForIdle()
        // The detail pane must be showing #dev's transcript, not just the channel list.
        compose.onNodeWithText("# dev").assertIsDisplayed()
    }
}
