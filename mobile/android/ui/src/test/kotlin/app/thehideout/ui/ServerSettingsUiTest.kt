package app.thehideout.ui

import androidx.activity.ComponentActivity
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertIsOff
import androidx.compose.ui.test.assertIsOn
import androidx.compose.ui.test.junit4.AndroidComposeTestRule
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
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
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
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
 * Server settings: Overview, Roles, Members, Bans, Emojis and leave server (phase 4b-1), through
 * the real Compose screens against [FakeBackend]. A tablet width, like [ManageUiTest]: the server
 * strip is on screen without a back navigation first.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@LooperMode(LooperMode.Mode.PAUSED)
@Config(sdk = [35], qualifiers = "w1280dp-h800dp-xhdpi")
class ServerSettingsUiTest {
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

    private fun openServerSettings() {
        compose.onNodeWithTag("server-menu").performClick()
        compose.onNodeWithText("Server settings").performClick()
        compose.settle()
    }

    @Test fun `server settings opens on overview for the owner, every tab visible`() {
        harness()
        openServerSettings()
        compose.onNodeWithTag("server-name-input").assertIsDisplayed()
        listOf("Overview", "Roles", "Members", "Bans", "Emojis").forEach {
            compose.onNodeWithTag("server-settings-tab-$it").performScrollTo().assertIsDisplayed()
        }
    }

    @Test fun `renaming the server`() {
        val store = harness()
        openServerSettings()
        compose.onNodeWithTag("server-name-input").performTextReplacement("Den")
        compose.onNodeWithTag("server-name-save").performClick()
        compose.settle { store.state.value.guild(FakeBackend.HIDEOUT)?.name == "Den" }
        assertEquals("Den", store.state.value.guild(FakeBackend.HIDEOUT)?.name)
    }

    @Test fun `creating a role through the editor lists it under Roles`() {
        val store = harness()
        openServerSettings()
        compose.onNodeWithTag("server-settings-tab-Roles").performClick()
        compose.settle()
        compose.onNodeWithTag("role-new").performClick()
        compose.settle()
        compose.onNodeWithTag("role-save").performScrollTo().assertIsDisplayed()
        kotlinx.coroutines.runBlocking { store.createRole(FakeBackend.HIDEOUT, "Helper", "", 0) }.getOrThrow()
        assertTrue(store.state.value.rolesOf(FakeBackend.HIDEOUT).any { it.name == "Helper" })
    }

    @Test fun `a permission switch in the role editor toggles on and off`() {
        harness()
        openServerSettings()
        compose.onNodeWithTag("server-settings-tab-Roles").performClick()
        compose.settle()
        compose.onNodeWithTag("role-new").performClick()
        compose.settle()
        compose.onNodeWithTag("perm-manage-emojis").assertIsOff()
        compose.onNodeWithTag("perm-manage-emojis").performClick()
        compose.settle()
        compose.onNodeWithTag("perm-manage-emojis").assertIsOn()
        compose.onNodeWithTag("perm-manage-emojis").performClick()
        compose.settle()
        compose.onNodeWithTag("perm-manage-emojis").assertIsOff()
    }

    @Test fun `opening an existing role shows its current name and colour in the editor`() {
        harness()
        openServerSettings()
        compose.onNodeWithTag("server-settings-tab-Roles").performClick()
        compose.settle()
        compose.onNodeWithTag("role-row-1001").performClick()
        compose.settle()
        compose.onNodeWithTag("role-name-input").assert(androidx.compose.ui.test.hasText("Moderator"))
        compose.onNodeWithTag("role-colour-teal").assertIsDisplayed()
    }

    @Test fun `assigning a role to a member shows its chip, removing it takes it back`() {
        val store = harness()
        openServerSettings()
        compose.onNodeWithTag("server-settings-tab-Members").performClick()
        compose.settle()
        // Designer (1003) is below the owner's infinite rank, so it's assignable to bruno;
        // ana already holds it, so the dropdown's own item (the last one composed) is matched.
        compose.onNodeWithTag("member-add-${FakeBackend.BRUNO}").performClick()
        compose.onAllNodesWithText("Designer").onLast().performClick()
        compose.settle { 1003L in store.state.value.memberRoles[FakeBackend.HIDEOUT]!!.getOrDefault(FakeBackend.BRUNO, emptyList()) }
        compose.onNodeWithTag("chip-x-${FakeBackend.BRUNO}-1003").assertIsDisplayed()
        compose.onNodeWithTag("chip-x-${FakeBackend.BRUNO}-1003").performClick()
        compose.settle { 1003L !in store.state.value.memberRoles[FakeBackend.HIDEOUT]!!.getOrDefault(FakeBackend.BRUNO, emptyList()) }
    }

    @Test fun `kicking a member asks first, then removes them`() {
        val store = harness()
        openServerSettings()
        compose.onNodeWithTag("server-settings-tab-Members").performClick()
        compose.settle()
        compose.onNodeWithTag("member-kick-${FakeBackend.BRUNO}").performClick()
        compose.settle()
        compose.onNodeWithTag("kick-confirm").assertIsDisplayed()
        compose.onNodeWithTag("kick-confirm").performClick()
        compose.settle { FakeBackend.BRUNO !in store.state.value.members[FakeBackend.HIDEOUT].orEmpty() }
        assertFalse(FakeBackend.BRUNO in store.state.value.members[FakeBackend.HIDEOUT].orEmpty())
    }

    @Test fun `banning with a cleanup window, then unbanning from the Bans tab`() {
        val store = harness()
        openServerSettings()
        compose.onNodeWithTag("server-settings-tab-Members").performClick()
        compose.settle()
        compose.onNodeWithTag("member-ban-${FakeBackend.CVETKA}").performClick()
        compose.settle()
        compose.onNodeWithTag("ban-reason").performTextInput("being rude")
        compose.onNodeWithTag("ban-cleanup").performClick()
        compose.onNodeWithText("Last hour").performClick()
        compose.onNodeWithTag("ban-confirm").performClick()
        compose.settle { FakeBackend.CVETKA !in store.state.value.members[FakeBackend.HIDEOUT].orEmpty() }

        compose.onNodeWithTag("server-settings-tab-Bans").performClick()
        compose.settle { store.state.value.bansOf(FakeBackend.HIDEOUT).any { it.user.id == FakeBackend.CVETKA } }
        compose.onNodeWithTag("ban-${FakeBackend.CVETKA}").assertIsDisplayed()
        compose.onNodeWithTag("unban-${FakeBackend.CVETKA}").performClick()
        compose.settle { store.state.value.bansOf(FakeBackend.HIDEOUT).none { it.user.id == FakeBackend.CVETKA } }
    }

    @Test fun `creating and deleting a custom emoji`() {
        val store = harness()
        // The real photo picker needs an Activity result contract; exercised instead through
        // AppStore directly (ApiClient/FakeBackend coverage is in ModerationTest), while this
        // test covers the rename/delete rows that don't need a picker.
        val emoji = kotlinx.coroutines.runBlocking { store.createEmoji(FakeBackend.HIDEOUT, "new_face", byteArrayOf(1, 2), "image/png") }.getOrThrow()
        openServerSettings()
        compose.onNodeWithTag("server-settings-tab-Emojis").performScrollTo().performClick()
        compose.settle { compose.onAllNodesWithTag("emoji-row-${emoji.id}").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithTag("emoji-row-${emoji.id}").performScrollTo().assertIsDisplayed()
        compose.onNodeWithTag("emoji-delete-${emoji.id}").performScrollTo().performClick()
        compose.onNodeWithTag("emoji-delete-confirm").performClick()
        compose.settle { store.state.value.emojis[FakeBackend.HIDEOUT]!!.none { it.id == emoji.id } }
    }

    @Test fun `deleting the server requires the exact name`() {
        val store = harness()
        openServerSettings()
        compose.onNodeWithTag("server-delete").performClick()
        compose.settle()
        compose.onNodeWithTag("delete-guild-confirm-name").performTextInput("not the name")
        compose.onNodeWithTag("delete-guild-submit").assertIsNotEnabled()
        compose.onNodeWithTag("delete-guild-confirm-name").performTextReplacement("The Hideout")
        compose.onNodeWithTag("delete-guild-submit").performClick()
        compose.settle { store.state.value.guild(FakeBackend.HIDEOUT) == null }
        assertNull(store.state.value.guild(FakeBackend.HIDEOUT))
    }

    @Test fun `a member without MANAGE_SERVER, MANAGE_ROLES or KICK_BAN_MEMBERS sees only the tabs their one permission opens`() {
        val store = harness()
        store.dispatch(Action.SessionStarted(store.state.value.users.getValue(FakeBackend.ANA)))
        compose.settle()
        // ana (role 1003: MANAGE_EMOJIS only) opens settings straight to Emojis, its only open
        // page: a single page shows no tab row at all (nothing else to switch to).
        compose.onNodeWithTag("server-menu").performClick()
        compose.onNodeWithText("Server settings").performClick()
        compose.settle()
        compose.onNodeWithTag("emoji-add").assertIsDisplayed()
        compose.onAllNodesWithTag("server-settings-tab-Overview").assertCountEquals(0)
        compose.onAllNodesWithTag("server-settings-tab-Roles").assertCountEquals(0)
        compose.onAllNodesWithTag("server-settings-tab-Members").assertCountEquals(0)
        compose.onAllNodesWithTag("server-settings-tab-Bans").assertCountEquals(0)
        compose.onAllNodesWithTag("server-settings-tab-Emojis").assertCountEquals(0)
    }
}
