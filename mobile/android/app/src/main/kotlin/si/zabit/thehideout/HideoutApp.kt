package si.zabit.thehideout

import android.app.Application
import coil3.ImageLoader
import coil3.PlatformContext
import coil3.SingletonImageLoader
import coil3.disk.DiskCache
import coil3.disk.directory
import coil3.gif.AnimatedImageDecoder
import coil3.memory.MemoryCache
import coil3.network.okhttp.OkHttpNetworkFetcherFactory
import coil3.request.crossfade
import okio.Path.Companion.toOkioPath

class HideoutApp : Application(), SingletonImageLoader.Factory {
    lateinit var container: AppContainer
        private set

    override fun onCreate() {
        super.onCreate()
        container = AppContainer(this)
    }

    /** One `ImageLoader` for the whole app, sharing [AppContainer]'s OkHttp client with the API calls. */
    override fun newImageLoader(context: PlatformContext): ImageLoader = ImageLoader.Builder(context)
        .components {
            add(OkHttpNetworkFetcherFactory(callFactory = { container.mediaHttpClient }))
            add(AnimatedImageDecoder.Factory())
        }
        .memoryCache { MemoryCache.Builder().maxSizePercent(context, 0.25).build() }
        .diskCache {
            DiskCache.Builder()
                .directory(cacheDir.resolve("image_cache").toOkioPath())
                .maxSizeBytes(128L * 1024 * 1024)
                .build()
        }
        .crossfade(true)
        .build()
}
