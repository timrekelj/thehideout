package si.zabit.thehideout

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import app.thehideout.core.store.AppStore
import app.thehideout.ui.theme.Look
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch

/** Holds the session's [AppStore] across rotation: UI event -> store intent -> reducer -> state -> UI. */
class AppViewModel(private val container: AppContainer, fake: Boolean) : ViewModel() {
    val store: AppStore = container.newStore(viewModelScope, fake)

    val look: StateFlow<Look> = container.looks.look.stateIn(viewModelScope, SharingStarted.Eagerly, Look())

    /** The token of a `thehideout://invite/<token>` link that opened the app; the sign-in phase consumes it. */
    var pendingInvite: String? = null

    init {
        store.bootstrap()
    }

    fun setLook(look: Look) {
        viewModelScope.launch { container.looks.save(look) }
    }

    class Factory(private val container: AppContainer, private val fake: Boolean) : ViewModelProvider.Factory {
        @Suppress("UNCHECKED_CAST")
        override fun <T : ViewModel> create(modelClass: Class<T>): T = AppViewModel(container, fake) as T
    }
}
