package app.thehideout.ui

import androidx.activity.ComponentActivity
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.AndroidComposeTestRule
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performClick
import app.thehideout.ui.components.AttachmentLightbox
import app.thehideout.ui.components.LightboxItem
import app.thehideout.ui.components.LightboxRequest
import app.thehideout.ui.components.LocalMediaBase
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

private fun AndroidComposeTestRule<*, ComponentActivity>.settle(until: () -> Boolean) {
    repeat(50) {
        waitForIdle()
        shadowOf(android.os.Looper.getMainLooper()).idle()
        if (until()) return
        Thread.sleep(20)
    }
}

private const val ASSET = "file:///android_asset/fake_media/"
private val missingItem = LightboxItem(ASSET + "does-not-exist.bin", "missing.bin", 0, 0, isVideo = false, contentType = "image/png")

/** The lightbox's Share and Save, on a fetch that fails (ATT-05): an error toast, busy cleared. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@LooperMode(LooperMode.Mode.PAUSED)
@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
class LightboxUiTest {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()

    private fun open() {
        compose.setContent {
            HideoutTheme(Look(), systemDark = true) {
                CompositionLocalProvider(LocalMediaBase provides "") {
                    AttachmentLightbox(LightboxRequest(listOf(missingItem), 0), onDismiss = {})
                }
            }
        }
        compose.waitForIdle()
    }

    @Test fun `Share shows a toast and clears busy on failure`() {
        open()
        compose.onNodeWithContentDescription("Share").performClick()
        compose.settle { ShadowToast.getLatestToast() != null }
        assert(ShadowToast.getLatestToast() != null) { "expected an error toast" }
        compose.onNodeWithContentDescription("Share").assertIsDisplayed()
    }

    @Test fun `Save shows a toast and clears busy on failure`() {
        open()
        compose.onNodeWithContentDescription("Save").performClick()
        compose.settle { ShadowToast.getLatestToast() != null }
        assert(ShadowToast.getLatestToast() != null) { "expected an error toast" }
        compose.onNodeWithContentDescription("Save").assertIsDisplayed()
    }
}
