package app.thehideout.ui

import app.thehideout.core.auth.AuthPresenter
import app.thehideout.core.auth.AuthUiState
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

/** An [AuthPresenter] that does nothing, for snapshots and tests that only need a state. */
class StubAuth(state: AuthUiState = AuthUiState()) : AuthPresenter {
    override val state: StateFlow<AuthUiState> = MutableStateFlow(state)
}
