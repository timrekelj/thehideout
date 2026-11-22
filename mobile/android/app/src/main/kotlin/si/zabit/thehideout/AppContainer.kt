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
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map
import java.io.File

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

/**
 * The manual dependency graph (no Hilt: the graph is small). Built once in [HideoutApp]; the
 * per-session [AppStore] is made by [newStore] and held by a ViewModel.
 */
class AppContainer(private val context: Context) {
    val looks = LookStore(context.prefs)

    /** `x-thehideout-client: android/0.1.0 (Android 36)`: the server can refuse builds it no longer supports. */
    val clientHeader = "android/${BuildConfig.VERSION_NAME} (Android ${Build.VERSION.SDK_INT})"

    private val keystoreTokens: TokenStore by lazy { KeystoreTokenStore(context) }

    /** A store over the real server (with the Keystore token and the file cache), or over the fake world. */
    fun newStore(scope: CoroutineScope, fake: Boolean): AppStore {
        if (fake) {
            // The fake world is "already signed in" and never touches disk or the network.
            val api: ApiClient = FakeBackend(unreadDemo = true)
            return AppStore(api, scope, InMemoryTokenStore("fake-token"))
        }
        val api = ConnectApiClient(ApiConfig(BuildConfig.DEFAULT_SERVER, keystoreTokens::load, clientHeader))
        val cache = SessionCache(FileCache(File(context.cacheDir, "session")))
        return AppStore(api, scope, keystoreTokens, cache)
    }
}
