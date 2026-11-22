package si.zabit.thehideout

import android.os.Build
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import app.thehideout.core.api.ApiClient
import app.thehideout.core.auth.AuthFlow
import app.thehideout.core.firstOrDefault
import app.thehideout.core.store.AppStore
import app.thehideout.ui.theme.Look
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch

/** One signed-in session's store plus the auth flow that signs into it. */
class AppSession(val store: AppStore, val auth: AuthFlow, val api: ApiClient)

/**
 * Holds the session's [AppStore] across rotation: UI event -> store intent -> reducer -> state -> UI.
 * The auth form ([AuthFlow]) signs into the store; changing the server rebuilds both.
 */
class AppViewModel(private val container: AppContainer, fake: Boolean) : ViewModel() {
    private val isFake = fake

    val look: StateFlow<Look> = container.looks.look.stateIn(viewModelScope, SharingStarted.Eagerly, Look())

    /** The server in use; empty means the build default. */
    val serverUrl: StateFlow<String> = container.server.url
        .map { it.ifEmpty { container.defaultServer() } }
        .stateIn(viewModelScope, SharingStarted.Eagerly, container.defaultServer())

    private val _session = MutableStateFlow<AppSession?>(null)
    val session: StateFlow<AppSession?> = _session.asStateFlow()

    /** The token of a `thehideout://invite/<token>` (or App Link) that opened the app; the sign-in phase consumes it. */
    var pendingInvite: String? = null

    init {
        // DataStore's flow needs a dispatch to emit the persisted URL; `serverUrl.value` reads its
        // eagerly-seeded default (BuildConfig.DEFAULT_SERVER) until then. Fake mode ignores the URL
        // entirely, so only it may start the session synchronously; the real client must await the
        // persisted value, or a cold start silently reopens against the build default.
        if (isFake) {
            openSession(container.defaultServer())
        } else {
            viewModelScope.launch { openSession(container.server.url.firstOrDefault(container.defaultServer())) }
        }
    }

    private fun openSession(url: String) {
        val api = container.api(url, isFake)
        val store = container.newStore(viewModelScope, api, isFake)
        val auth = AuthFlow({ api }, viewModelScope, { Build.MODEL }, onSession = store::onSignedIn)
        _session.value = AppSession(store, auth, api)
        store.bootstrap()
    }

    /** Saves a new server URL and starts over against it (the old session is signed out and cleared). */
    fun changeServer(url: String) {
        viewModelScope.launch {
            val trimmed = url.trim().trimEnd('/')
            container.server.save(trimmed)
            _session.value?.store?.signOut()
            openSession(trimmed)
        }
    }

    fun setLook(look: Look) {
        viewModelScope.launch { container.looks.save(look) }
    }

    class Factory(private val container: AppContainer, private val fake: Boolean) : ViewModelProvider.Factory {
        @Suppress("UNCHECKED_CAST")
        override fun <T : ViewModel> create(modelClass: Class<T>): T = AppViewModel(container, fake) as T
    }
}
