package app.thehideout.ui

import androidx.activity.ComponentActivity
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.junit4.AndroidComposeTestRule
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextReplacement
import app.thehideout.core.InMemoryTokenStore
import app.thehideout.core.fake.FakeBackend
import app.thehideout.core.store.Action
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

    @Test fun `joining by an invite code opens that server`() {
        val store = harness()
        compose.onNodeWithTag("add-server").performClick()
        compose.onNodeWithText("Join server").performClick()
        compose.settle()
        // Exact case: a guild's invite_code is server-generated and case-sensitive, unlike an
        // email invite's Crockford code (confirmed against a real server, see the fix in this PR).
        compose.onNodeWithTag("join-server-code").performTextInput("GaraGe22")
        compose.onNodeWithTag("join-server-submit").performClick()
        compose.settle { store.state.value.currentGuild?.id == FakeBackend.GARAGE }
        assertEquals(FakeBackend.GARAGE, store.state.value.currentGuild?.id)
    }

    @Test fun `joining with the wrong case is refused`() {
        val store = harness()
        compose.onNodeWithTag("add-server").performClick()
        compose.onNodeWithText("Join server").performClick()
        compose.settle()
        compose.onNodeWithTag("join-server-code").performTextInput("garage22")
        compose.onNodeWithTag("join-server-submit").performClick()
        // The sheet stays open with an error instead of closing into the server, and the button stays enabled to try again.
        compose.settle { compose.onAllNodesWithTag("join-server-submit").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithTag("join-server-submit").assertIsDisplayed()
        assertTrue(store.state.value.currentGuild?.id != FakeBackend.GARAGE)
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

    @Test fun `the invite sheet shows the server code and shares it (not a link)`() {
        harness()
        compose.onNodeWithTag("server-menu").performClick()
        compose.onNodeWithText("Invite people").performClick()
        compose.settle()
        compose.onNodeWithTag("invite-code-value").assertIsDisplayed().assertTextEquals("HiDeout1")
        compose.onNodeWithContentDescription("Share the code").performClick()
        compose.settle { shadowOf(compose.activity).peekNextStartedActivity() != null }
        val started = shadowOf(compose.activity).peekNextStartedActivity()
        assertTrue("expected a chooser for an ACTION_SEND intent, got $started", started != null)
        // `Intent.createChooser` wraps the real ACTION_SEND intent in `EXTRA_INTENT`.
        val sent = started!!.getParcelableExtra(android.content.Intent.EXTRA_INTENT, android.content.Intent::class.java) ?: started
        val text = sent.getStringExtra(android.content.Intent.EXTRA_TEXT).orEmpty()
        assertTrue("the shared text should carry the code, not a link: $text", text.contains("HiDeout1") && !text.contains("http"))
    }

    @Test fun `creating a channel inside a section places it there`() {
        val store = harness()
        compose.onNodeWithTag("section-menu-${FakeBackend.BUILDING}").performClick()
        compose.onNodeWithText("Create channel here").performClick()
        compose.settle()
        compose.onNodeWithTag("create-channel-name").performTextInput("roadmap")
        compose.onNodeWithTag("create-channel-submit").performClick()
        compose.settle { store.state.value.channelsOf(FakeBackend.HIDEOUT).any { it.name == "roadmap" } }
        val channel = store.state.value.channelsOf(FakeBackend.HIDEOUT).first { it.name == "roadmap" }
        assertEquals(FakeBackend.BUILDING, channel.sectionId)
        compose.onNodeWithText("roadmap").assertIsDisplayed()
    }

    @Test fun `renaming and deleting a section through its menu updates the layout`() {
        val store = harness()
        compose.onNodeWithTag("section-menu-${FakeBackend.BUILDING}").performClick()
        compose.onNodeWithText("Rename").performClick()
        compose.settle()
        compose.onNodeWithTag("section-name-input").performTextReplacement("Workshop")
        compose.onNodeWithText("Save").performClick()
        compose.settle { store.state.value.layouts[FakeBackend.HIDEOUT]?.sections?.any { it.id == FakeBackend.BUILDING && it.name == "Workshop" } == true }

        compose.onNodeWithTag("section-menu-${FakeBackend.BUILDING}").performClick()
        compose.onNodeWithText("Delete").performClick()
        compose.settle()
        compose.onNodeWithTag("delete-section-confirm").performClick()
        compose.settle { store.state.value.layouts[FakeBackend.HIDEOUT]?.sections?.none { it.id == FakeBackend.BUILDING } == true }
        assertTrue(store.state.value.layouts[FakeBackend.HIDEOUT]!!.sections.none { it.id == FakeBackend.BUILDING })
    }

    @Test fun `the section menu is hidden and the server menu has only Server settings without MANAGE_CHANNELS or INVITE_PEOPLE`() {
        val store = harness()
        // ana (role 1003 on The Hideout: MANAGE_EMOJIS only) has neither permission, but the server
        // menu still offers Server settings (she can open its Emojis page).
        store.dispatch(Action.SessionStarted(store.state.value.users.getValue(FakeBackend.ANA)))
        compose.settle()
        compose.onAllNodesWithTag("section-menu-${FakeBackend.BUILDING}").assertCountEquals(0)
        compose.onNodeWithTag("server-menu").performClick()
        compose.settle()
        compose.onAllNodesWithText("Invite people").assertCountEquals(0)
        compose.onAllNodesWithText("Create channel").assertCountEquals(0)
        compose.onAllNodesWithText("Create section").assertCountEquals(0)
        compose.onNodeWithText("Server settings").assertIsDisplayed()
        // The "+" to create/join a server has nothing to do with this guild's permissions.
        compose.onNodeWithTag("add-server").assertIsDisplayed()
    }
}
