package app.thehideout.ui

import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import app.thehideout.ui.shell.ShellActions
import app.thehideout.ui.shell.SignedInShell
import org.junit.Test
import org.robolectric.annotation.Config

/** Every attachment grid layout (1, 2, 3, 4), a video, an audio row, a file card, the removed
 * placeholder, and the lightbox, in #design (ATT-01..05). */
@Config(sdk = [35], qualifiers = "w411dp-h2400dp-xxhdpi")
class AttachmentScreenshots : ScreenshotTest() {
    private val attachmentWorld by lazy { attachmentWorldState() }

    private fun grids(variant: Variant) {
        compose.setThemed(variant) { look -> SignedInShell(attachmentWorld, look, {}, ShellActions(), "0.1.0") }
        compose.capture("attachments-grids", Device.PhoneTall, variant)
    }

    @Test fun gridsDark() = grids(Variant.Dark)
    @Test fun gridsLight() = grids(Variant.Light)

    private fun lightbox(variant: Variant) {
        compose.setThemed(variant) { look -> SignedInShell(attachmentWorld, look, {}, ShellActions(), "0.1.0") }
        compose.onAllNodesWithContentDescription("View mood-1.jpg")[0].performScrollTo().performClick()
        compose.capture("attachments-lightbox", Device.Phone, variant)
    }

    @Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
    @Test
    fun lightboxDark() = lightbox(Variant.Dark)
}
