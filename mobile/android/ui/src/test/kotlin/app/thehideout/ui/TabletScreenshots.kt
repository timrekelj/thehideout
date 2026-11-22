package app.thehideout.ui

import app.thehideout.ui.gallery.GalleryScreen
import app.thehideout.ui.shell.Destination
import app.thehideout.ui.shell.ShellActions
import app.thehideout.ui.shell.SignedInShell
import org.junit.Test
import org.robolectric.annotation.Config

/** The screens on a 1280 x 800 dp tablet in landscape: rail instead of bar, list beside detail. */
@Config(sdk = [35], qualifiers = "w1280dp-h800dp-xhdpi")
class TabletScreenshots : ScreenshotTest() {
    private fun shell(variant: Variant, tab: Destination = Destination.Servers) =
        compose.snapshot("shell-${tab.name.lowercase()}", Device.Tablet, variant) { look ->
            SignedInShell(world, look, {}, ShellActions(), "0.1.0", initial = tab)
        }

    @Test fun serversDark() = shell(Variant.Dark)
    @Test fun serversLight() = shell(Variant.Light)
    @Test fun messagesDark() = shell(Variant.Dark, Destination.Messages)
    @Test fun youDark() = shell(Variant.Dark, Destination.You)

    @Test fun galleryDark() = compose.snapshot("gallery-top", Device.Tablet, Variant.Dark) { look -> GalleryScreen(look, {}) }
    @Test fun galleryLight() = compose.snapshot("gallery-top", Device.Tablet, Variant.Light) { look -> GalleryScreen(look, {}) }
}
