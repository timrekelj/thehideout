package app.thehideout.ui

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.AndroidComposeTestRule
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performClick
import app.thehideout.core.attachment.FileKind
import app.thehideout.core.model.Attachment
import app.thehideout.ui.components.AttachmentAudioRow
import app.thehideout.ui.components.AttachmentFileCard
import app.thehideout.ui.components.AttachmentVideoCard
import app.thehideout.ui.theme.HideoutTheme
import app.thehideout.ui.theme.Look
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.robolectric.annotation.LooperMode
import org.robolectric.shadows.ShadowToast

/**
 * [AndroidComposeTestRule.waitForIdle] drives the compose clock, but the suspend fetch hops onto
 * a real background thread (`Dispatchers.IO`) and posts its result back to the paused main looper:
 * poll both until `until` is true, or give up (the last state is left for the assertion to report).
 */
private fun AndroidComposeTestRule<*, ComponentActivity>.settle(until: () -> Boolean) {
    repeat(50) {
        waitForIdle()
        shadowOf(android.os.Looper.getMainLooper()).idle()
        if (until()) return
        Thread.sleep(20)
    }
}

private const val ASSET = "file:///android_asset/fake_media/"
private val video = Attachment(1, "demo.mp4", "video/mp4", 590_746, ASSET + "clip-1.mp4", 960, 540, thumbnail = ASSET + "clip-1-poster.jpg")
private val audio = Attachment(2, "voice-note.m4a", "audio/mp4", 98_808, ASSET + "tone-1.m4a")
private val openableFile = Attachment(3, "report.pdf", "application/pdf", 218, ASSET + "notes-1.txt")
private val missingFile = Attachment(4, "missing.bin", "application/octet-stream", 0, ASSET + "does-not-exist.bin")

/** The file card's Open and Download, the audio row's play, and the video card's play -> expand (ATT-02..04). */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@LooperMode(LooperMode.Mode.PAUSED)
@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
class AttachmentComponentUiTest {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()

    @Test fun `file card Open opens the system viewer`() {
        compose.setContent { HideoutTheme(Look(), systemDark = true) { AttachmentFileCard(openableFile, "", FileKind.Pdf) } }
        compose.onNodeWithContentDescription("Open").performClick()
        compose.settle { shadowOf(compose.activity).peekNextStartedActivity() != null }
        val started = shadowOf(compose.activity).peekNextStartedActivity()
        assert(started != null && started.action == android.content.Intent.ACTION_VIEW) { "expected an ACTION_VIEW intent, got $started" }
    }

    @Test fun `file card Download succeeds without an error toast`() {
        compose.setContent { HideoutTheme(Look(), systemDark = true) { AttachmentFileCard(openableFile, "", FileKind.Pdf) } }
        compose.onNodeWithContentDescription("Download report.pdf").performClick()
        compose.settle { compose.onAllNodesWithContentDescription("Download report.pdf").fetchSemanticsNodes().isNotEmpty() }
        assert(ShadowToast.getLatestToast() == null) { "unexpected toast: ${ShadowToast.getTextOfLatestToast()}" }
    }

    @Test fun `file card Open shows a toast and clears busy on failure`() {
        compose.setContent { HideoutTheme(Look(), systemDark = true) { AttachmentFileCard(missingFile, "", FileKind.Pdf) } }
        compose.onNodeWithContentDescription("Open").performClick()
        compose.settle { ShadowToast.getLatestToast() != null }
        assert(ShadowToast.getLatestToast() != null) { "expected an error toast" }
        compose.onNodeWithContentDescription("Open").assertIsDisplayed()
    }

    @Test fun `file card Download shows a toast and clears busy on failure`() {
        compose.setContent { HideoutTheme(Look(), systemDark = true) { AttachmentFileCard(missingFile, "", FileKind.Other) } }
        compose.onNodeWithContentDescription("Download missing.bin").performClick()
        compose.settle { ShadowToast.getLatestToast() != null }
        assert(ShadowToast.getLatestToast() != null) { "expected an error toast" }
        compose.onNodeWithContentDescription("Download missing.bin").assertIsDisplayed()
    }

    @Test fun `audio row play then pause toggles the icon`() {
        compose.setContent { HideoutTheme(Look(), systemDark = true) { AttachmentAudioRow(audio, "") } }
        compose.onNodeWithContentDescription("Play").performClick()
        compose.settle { compose.onAllNodesWithContentDescription("Pause").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithContentDescription("Pause").assertIsDisplayed()
    }

    @Test fun `audio row download failure shows a toast and clears busy`() {
        compose.setContent { HideoutTheme(Look(), systemDark = true) { AttachmentAudioRow(missingFile.copy(filename = "voice-note.m4a"), "") } }
        compose.onNodeWithContentDescription("Download").performClick()
        compose.settle { ShadowToast.getLatestToast() != null }
        assert(ShadowToast.getLatestToast() != null) { "expected an error toast" }
        compose.onNodeWithContentDescription("Download").assertIsDisplayed()
    }

    @Test fun `video card play then expand hands off to the lightbox`() {
        var expanded: Long? = null
        compose.setContent {
            HideoutTheme(Look(), systemDark = true) {
                AttachmentVideoCard(video, "", onExpand = { pos -> expanded = pos })
            }
        }
        compose.onNodeWithContentDescription("Play video").performClick()
        // Starting playback swaps the poster for the controls bar, with its own play/pause button.
        compose.settle { compose.onAllNodesWithContentDescription("Expand").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithContentDescription("Expand").assertIsDisplayed()
        compose.onNodeWithContentDescription("Expand").performClick()
        compose.waitForIdle()
        assert(expanded != null) { "expected onExpand to fire with the inline player's position" }
    }
}
