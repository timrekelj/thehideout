package app.thehideout.ui

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.test.core.app.ApplicationProvider
import coil3.ImageLoader
import coil3.SingletonImageLoader
import coil3.annotation.DelicateCoilApi
import coil3.gif.AnimatedImageDecoder
import coil3.network.okhttp.OkHttpNetworkFetcherFactory
import kotlinx.coroutines.Dispatchers
import org.junit.After
import org.junit.Before
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

    // Every image a snapshot loads is a bundled asset (file:///android_asset/...), never a real
    // network call. The production ImageLoader still fetches and decodes off the main thread
    // (Dispatchers.IO), which races `capture()` under Robolectric's paused looper: sometimes the
    // picture is in, sometimes it's still the placeholder. Running the same pipeline unconfined
    // keeps it on the test thread, so decoding finishes before the coroutine that started it ever
    // suspends, and `waitForIdle()` alone is enough.
    @OptIn(DelicateCoilApi::class)
    @Before
    fun installDeterministicImageLoader() {
        val loader = ImageLoader.Builder(ApplicationProvider.getApplicationContext())
            .components {
                add(OkHttpNetworkFetcherFactory())
                add(AnimatedImageDecoder.Factory())
            }
            .interceptorCoroutineContext(Dispatchers.Unconfined)
            .fetcherCoroutineContext(Dispatchers.Unconfined)
            .decoderCoroutineContext(Dispatchers.Unconfined)
            .build()
        SingletonImageLoader.setUnsafe(loader)
    }

    @OptIn(DelicateCoilApi::class)
    @After
    fun resetImageLoader() {
        SingletonImageLoader.reset()
    }
}
