package app.thehideout.ui

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import app.thehideout.ui.gallery.LookControls
import app.thehideout.ui.theme.Hideout
import org.junit.Test
import org.robolectric.annotation.Config

/** The look sheet's controls, drawn on the sheet's surface (the sheet itself is a dialog window, out of reach of a root capture). */
@Config(sdk = [35], qualifiers = "w411dp-h820dp-xxhdpi")
class ControlsScreenshots : ScreenshotTest() {
    private fun controls(variant: Variant) = compose.snapshot("look-controls", Device.PhoneTall, variant) { look ->
        Surface(Modifier.fillMaxSize(), color = Hideout.colors.surfaceOverlay) { LookControls(look, {}) }
    }

    @Test fun dark() = controls(Variant.Dark)
    @Test fun light() = controls(Variant.Light)
}
