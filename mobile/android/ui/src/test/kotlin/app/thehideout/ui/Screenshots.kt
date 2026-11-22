package app.thehideout.ui

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import org.junit.Rule
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.GraphicsMode
import org.robolectric.annotation.LooperMode

/** Base of the snapshot classes: Robolectric on the JVM, native graphics, one device class per subclass. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@LooperMode(LooperMode.Mode.PAUSED)
abstract class ScreenshotTest {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()
    protected val world by lazy { fakeWorldState() }
}
