package si.zabit.thehideout

import android.content.Context
import android.os.Build
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import app.thehideout.core.InMemoryTokenStore
import app.thehideout.core.TokenStore
import app.thehideout.core.api.ApiClient
import app.thehideout.core.api.ApiConfig
import app.thehideout.core.api.ConnectApiClient
import app.thehideout.core.cache.FileCache
import app.thehideout.core.cache.SessionCache
import app.thehideout.core.fake.FakeBackend
import app.thehideout.core.store.AppStore
import app.thehideout.ui.theme.Look
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.withContext
import java.io.File
import okhttp3.OkHttpClient
import okhttp3.Request

private val Context.prefs: DataStore<Preferences> by preferencesDataStore("preferences")

/** The look, persisted as the JSON desktop writes under `appearance`. */
class LookStore(private val store: DataStore<Preferences>) {
    val look: Flow<Look> = store.data.map { Look.fromJson(it[KEY]) }

    suspend fun save(look: Look) {
        store.edit { it[KEY] = look.toJson() }
    }

    private companion object {
        val KEY = stringPreferencesKey("appearance")
    }
}

/** The server the app talks to, persisted (debug default `http://10.0.2.2:8080`, release `https://thehideout.zabit.si`). */
class ServerStore(private val store: DataStore<Preferences>) {
    val url: Flow<String> = store.data.map { it[KEY] ?: "" }

    suspend fun save(url: String) {
        store.edit { it[KEY] = url }
    }

    private companion object {
        val KEY = stringPreferencesKey("server_url")
    }
}

/**
 * The manual dependency graph (no Hilt: the graph is small). Built once in [HideoutApp]; the
 * per-session [AppStore] is made by [newStore] and held by a ViewModel.
 */
class AppContainer(private val context: Context) {
    val looks = LookStore(context.prefs)
    val server = ServerStore(context.prefs)

    /** `x-thehideout-client: android/0.1.0 (Android 36)`: the server can refuse builds it no longer supports. */
    val clientHeader = "android/${BuildConfig.VERSION_NAME} (Android ${Build.VERSION.SDK_INT})"

    private val keystoreTokens: TokenStore by lazy { KeystoreTokenStore(context) }

    private val probe = OkHttpClient()

    /** Shared with Coil: media is public by its unguessable key, so no auth header is needed. */
    val mediaHttpClient: OkHttpClient = OkHttpClient()

    /** The default server of this build (debug `http://10.0.2.2:8080`, release the public one). */
    fun defaultServer(): String = BuildConfig.DEFAULT_SERVER

    /** One client per server URL (changing the server makes a new one). */
    private var cachedApi: Pair<String, ApiClient>? = null

    fun api(serverUrl: String, fake: Boolean): ApiClient {
        if (fake) return FakeBackend(unreadDemo = true)
        cachedApi?.let { if (it.first == serverUrl) return it.second }
        val api = ConnectApiClient(ApiConfig(serverUrl, keystoreTokens::load, clientHeader))
        cachedApi = serverUrl to api
        return api
    }

    /** A store over the given client (with the Keystore token and the file cache), or over the fake world. */
    fun newStore(scope: CoroutineScope, api: ApiClient, fake: Boolean): AppStore {
        if (fake) {
            // The fake world is "already signed in" and never touches disk or the network.
            return AppStore(api, scope, InMemoryTokenStore("fake-token"))
        }
        val cache = SessionCache(FileCache(File(context.cacheDir, "session")))
        return AppStore(api, scope, keystoreTokens, cache)
    }

    /** Whether the server answers at all (`GET /health`), so the server sheet can refuse a typo before saving it. */
    suspend fun probeServer(url: String): Boolean = withContext(Dispatchers.IO) {
        runCatching {
            probe.newCall(Request.Builder().url("$url/health").build()).execute().use { it.isSuccessful }
        }.getOrDefault(false)
    }
}
