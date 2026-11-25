package app.thehideout.ui

import app.thehideout.design.*
import app.thehideout.ui.gallery.GallerySectionsPage
import app.thehideout.ui.gallery.gallerySections
import org.junit.Test
import org.robolectric.annotation.Config

// The gallery's 15 sections, captured in three bounded groups instead of one ~14'400px-tall
// bitmap (that blew the test worker's heap, see PR #23). Each group's qualifier height is sized
// to its content with some headroom; a group running short just leaves background below it.
private val colorsAndType = gallerySections.subList(0, 5)
private val controls = gallerySections.subList(5, 11)
private val appComponents = gallerySections.subList(11, 15)

private const val GROUP_A_Q = "w411dp-h1900dp-xxhdpi"
private const val GROUP_B_Q = "w411dp-h2300dp-xxhdpi"
private const val GROUP_C_Q = "w411dp-h2200dp-xxhdpi"

/** The gallery, section by section (ATT/MD parity gallery), on a phone: three groups so the
 *  capture buffer stays small, instead of one huge bitmap for the whole scroll. */
class GalleryScreenshots : ScreenshotTest() {
    @Config(sdk = [35], qualifiers = GROUP_A_Q)
    @Test fun colorsAndTypeDark() = compose.snapshot("gallery-colors-type", Device.PhoneTall, Variant.Dark) { GallerySectionsPage(colorsAndType) }
    @Config(sdk = [35], qualifiers = GROUP_A_Q)
    @Test fun colorsAndTypeLight() = compose.snapshot("gallery-colors-type", Device.PhoneTall, Variant.Light) { GallerySectionsPage(colorsAndType) }

    @Config(sdk = [35], qualifiers = GROUP_B_Q)
    @Test fun controlsDark() = compose.snapshot("gallery-controls", Device.PhoneTall, Variant.Dark) { GallerySectionsPage(controls) }
    @Config(sdk = [35], qualifiers = GROUP_B_Q)
    @Test fun controlsLight() = compose.snapshot("gallery-controls", Device.PhoneTall, Variant.Light) { GallerySectionsPage(controls) }

    @Config(sdk = [35], qualifiers = GROUP_C_Q)
    @Test fun appComponentsDark() = compose.snapshot("gallery-app-components", Device.PhoneTall, Variant.Dark) { GallerySectionsPage(appComponents) }
    @Config(sdk = [35], qualifiers = GROUP_C_Q)
    @Test fun appComponentsLight() = compose.snapshot("gallery-app-components", Device.PhoneTall, Variant.Light) { GallerySectionsPage(appComponents) }

    // The same page in other looks: the accent, the background and the contrast all have to hold.
    // Only the color/type group needs re-checking per look; the controls and app-components
    // groups don't vary with accent/background/contrast in a way those sections wouldn't also show.
    @Config(sdk = [35], qualifiers = GROUP_A_Q)
    @Test fun oceanCoolDark() = compose.snapshot(
        "gallery-ocean-cool", Device.PhoneTall,
        Variant("dark", look { copy(accent = AccentPreset.Ocean, background = BackgroundPreset.Cool) }, true),
    ) { GallerySectionsPage(colorsAndType) }

    @Config(sdk = [35], qualifiers = GROUP_A_Q)
    @Test fun emberPaperLight() = compose.snapshot(
        "gallery-ember-paper", Device.PhoneTall,
        Variant("light", look { copy(accent = AccentPreset.Ember, background = BackgroundPreset.Paper) }, false),
    ) { GallerySectionsPage(colorsAndType) }

    @Config(sdk = [35], qualifiers = GROUP_A_Q)
    @Test fun highContrastCompactLarge() = compose.snapshot(
        "gallery-high-compact-125", Device.PhoneTall,
        Variant("dark", look { copy(contrast = ContrastLevel.High, density = Density.Compact, textSize = TextSize.P125) }, true),
    ) { GallerySectionsPage(colorsAndType) }
}
