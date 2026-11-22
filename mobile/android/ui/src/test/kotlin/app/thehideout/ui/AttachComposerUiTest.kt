package app.thehideout.ui

import androidx.activity.ComponentActivity
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import app.thehideout.core.InMemoryTokenStore
import app.thehideout.core.api.ApiClient
import app.thehideout.core.api.PendingAttachment
import app.thehideout.core.api.UploadSource
import app.thehideout.core.fake.FakeBackend
import app.thehideout.core.store.AppStore
import app.thehideout.core.store.AttachUploadPhase
import app.thehideout.core.store.PickedFile
import app.thehideout.core.store.SessionPhase
import app.thehideout.ui.shell.ShellActions
import app.thehideout.ui.shell.SignedInShell
import app.thehideout.ui.theme.HideoutTheme
import app.thehideout.ui.theme.Look
import kotlinx.coroutines.CompletableDeferred
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
import java.io.ByteArrayInputStream
import java.time.Duration

private fun source(size: Int = 10): UploadSource = UploadSource { ByteArrayInputStream(ByteArray(size)) }

private fun picked(name: String, contentType: String = "text/plain", preview: String? = null) =
    PickedFile(name, contentType, 10, preview, source())



/**
 * The composer's attach path end to end against the fake backend, through the real Compose screens:
 * the "+" menu, the upload tray (progress → ready → remove), Send carrying the uploaded files, and
 * the pending row's local previews (ATT-09).
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@LooperMode(LooperMode.Mode.PAUSED)
@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
class AttachComposerUiTest {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()

    private fun harness(api: ApiClient): AppStore {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val store = AppStore(api, scope, InMemoryTokenStore("fake-token-${FakeBackend.TIM}"))
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

    @Test fun `the composer's + opens the attach menu`() {
        harness(FakeBackend())
        compose.onNodeWithContentDescription("Attach").assertIsDisplayed().performClick()
        compose.onNodeWithText("Photos and videos").assertIsDisplayed()
        compose.onNodeWithText("Files").assertIsDisplayed()
        compose.onNodeWithText("Take a photo").assertIsDisplayed()
        compose.onNodeWithText("Record a video").assertIsDisplayed()
    }

    @Test fun `a picked file shows in the tray, uploads to ready, and sends with the message`() {
        val fake = FakeBackend()
        val store = harness(fake)
        store.attachFiles(FakeBackend.GENERAL, listOf(picked("notes.txt")))
        compose.waitForIdle()

        compose.onNodeWithTag("attach-tray").assertIsDisplayed()
        compose.onNodeWithTag("attach-chip-1").assertIsDisplayed()
        compose.onNodeWithText("notes.txt").assertIsDisplayed()

        // The file made it to Ready, so Send is enabled even with an empty field.
        compose.waitUntil(5000) { store.state.value.attachDraft(FakeBackend.GENERAL).files.single().state.phase == AttachUploadPhase.Ready }
        compose.onNodeWithContentDescription("Send").assertIsEnabled().performClick()
        compose.waitUntil(5000) { fake.sent.isNotEmpty() }

        assertEquals(listOf("notes.txt"), fake.sent.single().attachments.map { it.filename })
        assertTrue("the tray clears once the files are sent", store.state.value.attachDraft(FakeBackend.GENERAL).files.isEmpty())
        assertTrue("the tray is gone once its files are sent", compose.onAllNodesWithTag("attach-tray").fetchSemanticsNodes().isEmpty())
    }

    @Test fun `send is disabled while a file is still uploading and enabled once it is ready`() {
        val inner = FakeBackend()
        val gate = CompletableDeferred<Unit>()
        val fake = object : ApiClient by inner {
            override suspend fun uploadAttachment(channel: Long, filename: String, contentType: String, size: Long, source: UploadSource, progress: (Long) -> Unit): PendingAttachment {
                gate.await()
                return inner.uploadAttachment(channel, filename, contentType, size, source, progress)
            }
        }
        val store = harness(fake)
        store.attachFiles(FakeBackend.GENERAL, listOf(picked("notes.txt")))
        compose.waitForIdle()

        compose.onNodeWithTag("attach-chip-1").assertIsDisplayed()
        compose.onNodeWithContentDescription("Send").assertIsNotEnabled()

        gate.complete(Unit)
        compose.waitUntil(5000) { store.state.value.attachDraft(FakeBackend.GENERAL).files.single().state.phase == AttachUploadPhase.Ready }
        compose.onNodeWithContentDescription("Send").assertIsEnabled()
    }

    @Test fun `the tray's remove drops the file's chip`() {
        val fake = FakeBackend()
        val store = harness(fake)
        store.attachFiles(FakeBackend.GENERAL, listOf(picked("a.txt"), picked("b.txt")))
        compose.waitUntil(5000) { store.state.value.attachDraft(FakeBackend.GENERAL).files.all { it.state.phase == AttachUploadPhase.Ready } }
        compose.waitForIdle()

        compose.onNodeWithContentDescription("Remove a.txt").performClick()
        compose.waitForIdle()

        val left = store.state.value.attachDraft(FakeBackend.GENERAL).files
        assertEquals(listOf("b.txt"), left.map { it.name })
        assertTrue("a.txt's chip is gone", compose.onAllNodesWithText("a.txt").fetchSemanticsNodes().isEmpty())
        compose.onNodeWithText("b.txt").assertIsDisplayed()
    }

    @Test fun `a failed send keeps the pending row with its local file preview`() {
        val fake = FakeBackend().apply { failSends = 1 }
        val store = harness(fake)
        store.attachFiles(FakeBackend.GENERAL, listOf(picked("notes.txt", preview = "content://picked/notes.txt")))
        compose.waitUntil(5000) { store.state.value.attachDraft(FakeBackend.GENERAL).files.single().state.phase == AttachUploadPhase.Ready }
        compose.waitForIdle()

        compose.onNodeWithContentDescription("Send").performClick()
        compose.waitUntil(5000) { store.state.value.outgoing.any { it.attachmentIds.isNotEmpty() && it.state == app.thehideout.core.store.Delivery.Failed } }

        // The tray has cleared, so the only "notes.txt" on screen is the pending row's local preview.
        compose.onNodeWithText("notes.txt").assertIsDisplayed()
        compose.onNodeWithText("Not sent.").assertIsDisplayed()
    }
}
