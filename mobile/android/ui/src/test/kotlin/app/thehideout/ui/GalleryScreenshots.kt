package app.thehideout.ui

import app.thehideout.design.*
import app.thehideout.ui.gallery.GalleryScreen
import org.junit.Test
import org.robolectric.annotation.Config

/** The whole gallery on a phone, captured tall so one image shows every section. */
@Config(sdk = [35], qualifiers = "w411dp-h4800dp-xxhdpi")
class GalleryScreenshots : ScreenshotTest() {
    @Test fun phoneDark() = compose.snapshot("gallery", Device.PhoneTall, Variant.Dark) { look -> GalleryScreen(look, {}) }
    @Test fun phoneLight() = compose.snapshot("gallery", Device.PhoneTall, Variant.Light) { look -> GalleryScreen(look, {}) }

    // The same page in other looks: the accent, the background and the contrast all have to hold.
    @Test fun phoneOceanCoolDark() = compose.snapshot("gallery-ocean-cool", Device.PhoneTall, Variant("dark", look { copy(accent = AccentPreset.Ocean, background = BackgroundPreset.Cool) }, true)) { look -> GalleryScreen(look, {}) }
    @Test fun phoneEmberPaperLight() = compose.snapshot("gallery-ember-paper", Device.PhoneTall, Variant("light", look { copy(accent = AccentPreset.Ember, background = BackgroundPreset.Paper) }, false)) { look -> GalleryScreen(look, {}) }
    @Test fun phoneHighContrastCompactLarge() = compose.snapshot(
        "gallery-high-compact-125", Device.PhoneTall,
        Variant("dark", look { copy(contrast = ContrastLevel.High, density = Density.Compact, textSize = TextSize.P125) }, true),
    ) { look -> GalleryScreen(look, {}) }
}
