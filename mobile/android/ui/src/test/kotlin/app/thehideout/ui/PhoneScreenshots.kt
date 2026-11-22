package app.thehideout.ui

import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import app.thehideout.ui.gallery.GalleryScreen
import app.thehideout.ui.root.HideoutRoot
import app.thehideout.ui.shell.Destination
import app.thehideout.ui.shell.ShellActions
import app.thehideout.ui.shell.SignedInShell
import app.thehideout.ui.theme.Look
import org.junit.Test
import org.robolectric.annotation.Config

/** The screens on a 411 x 891 dp phone. */
@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
class PhoneScreenshots : ScreenshotTest() {
    private fun shell(variant: Variant, tab: Destination = Destination.Servers) =
        compose.snapshot("shell-${tab.name.lowercase()}", Device.Phone, variant) { look ->
            SignedInShell(world, look, {}, ShellActions(), "0.1.0", initial = tab)
        }

    @Test fun serversDark() = shell(Variant.Dark)
    @Test fun serversLight() = shell(Variant.Light)
    @Test fun messagesDark() = shell(Variant.Dark, Destination.Messages)
    @Test fun youDark() = shell(Variant.Dark, Destination.You)
    @Test fun youLight() = shell(Variant.Light, Destination.You)

    private fun transcript(variant: Variant) {
        compose.setThemed(variant) { look -> SignedInShell(world, look, {}, ShellActions(), "0.1.0") }
        compose.onNodeWithText("general").performClick()
        compose.capture("transcript", Device.Phone, variant)
    }

    @Test fun transcriptDark() = transcript(Variant.Dark)
    @Test fun transcriptLight() = transcript(Variant.Light)

    @Test fun signedOutDark() = compose.snapshot("signed-out", Device.Phone, Variant.Dark) { _ ->
        HideoutRoot(app.thehideout.core.store.AppState(phase = app.thehideout.core.store.SessionPhase.SignedOut()), Look(), {}, ShellActions(useDemo = {}), "0.1.0")
    }

    @Test fun galleryTopDark() = compose.snapshot("gallery-top", Device.Phone, Variant.Dark) { look -> GalleryScreen(look, {}) }
}
