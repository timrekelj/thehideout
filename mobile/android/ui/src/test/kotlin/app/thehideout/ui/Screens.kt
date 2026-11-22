package app.thehideout.ui

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.AndroidComposeTestRule
import androidx.compose.ui.test.onRoot
import androidx.activity.ComponentActivity
import app.thehideout.design.*
import app.thehideout.ui.components.LocalTimeZone
import app.thehideout.ui.components.LocalToday
import app.thehideout.ui.theme.HideoutTheme
import app.thehideout.ui.theme.Look
import com.github.takahirom.roborazzi.RoborazziOptions
import com.github.takahirom.roborazzi.captureRoboImage
import org.junit.rules.TestRule
import java.time.ZoneOffset

/** The device classes the snapshot matrix runs on. `qualifiers` are Robolectric's. */
enum class Device(val tag: String, val qualifiers: String) {
    /** 411 x 891 dp at xxhdpi, the usual phone. */
    Phone("phone", "w411dp-h891dp-xxhdpi"),
    /** A tall phone, so a scrolling screen is captured whole. */
    PhoneTall("phone-tall", "w411dp-h4800dp-xxhdpi"),
    /** 1280 x 800 dp, a 10-inch tablet in landscape. */
    Tablet("tablet", "w1280dp-h800dp-xhdpi"),
}

/** The look and system appearance of a capture. */
data class Variant(val tag: String, val look: Look, val systemDark: Boolean) {
    companion object {
        val Dark = Variant("dark", Look(), true)
        val Light = Variant("light", Look(), false)
    }
}

fun look(f: ThemeSpec.() -> ThemeSpec) = Look(ThemeSpec().f())

val snapshotOptions = RoborazziOptions(compareOptions = RoborazziOptions.CompareOptions(changeThreshold = 0.005f))

/** Snapshots live next to the tests so they are committed with them. */
fun snapshotPath(screen: String, device: Device, variant: Variant) = "src/test/snapshots/${screen}_${device.tag}_${variant.tag}.png"

/** Composes [content] in the warm theme, pinned to UTC and the fake world's clock. */
fun <R : TestRule> AndroidComposeTestRule<R, ComponentActivity>.setThemed(variant: Variant, content: @Composable (Look) -> Unit) {
    setContent {
        CompositionLocalProvider(LocalTimeZone provides ZoneOffset.UTC, LocalToday provides java.time.LocalDate.of(2026, 11, 22)) {
            HideoutTheme(look = variant.look, systemDark = variant.systemDark) {
                content(variant.look)
            }
        }
    }
    waitForIdle()
}

/** Writes (or, under `verifyRoborazzi`, compares with) the committed reference. */
fun <R : TestRule> AndroidComposeTestRule<R, ComponentActivity>.capture(screen: String, device: Device, variant: Variant) {
    waitForIdle()
    onRoot().captureRoboImage(snapshotPath(screen, device, variant), snapshotOptions)
}

fun <R : TestRule> AndroidComposeTestRule<R, ComponentActivity>.snapshot(
    screen: String,
    device: Device,
    variant: Variant,
    content: @Composable (Look) -> Unit,
) {
    setThemed(variant, content)
    capture(screen, device, variant)
}
