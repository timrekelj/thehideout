package app.thehideout.core.store

import app.thehideout.core.TokenStore
import app.thehideout.core.api.ApiClient
import app.thehideout.core.api.AppError
import app.thehideout.core.api.LoginResult
import app.thehideout.core.cache.SessionCache
import app.thehideout.core.gateway.GatewayStream
import app.thehideout.core.model.ReactionEmoji
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import java.time.Instant

/**
 * One store per session: owns the whole mirror (guilds, channels, logs, outgoing messages, voice
 * states, unread, the open place) as an immutable [AppState] snapshot in a [StateFlow].
 * Unidirectional: UI event -> intent (a method here) -> [Reducer] -> new state -> UI; effects the
 * reducer asks for (fetches, sends) run on [scope] and come back as actions.
 */
class AppStore(
    private val api: ApiClient,
    private val scope: CoroutineScope,
    private val tokens: TokenStore,
    private val cache: SessionCache? = null,
    private val clock: () -> Instant = { Instant.now() },
    private val gateway: GatewayStream = GatewayStream(api::subscribe),
    initial: AppState = AppState(),
) {
    private val _state = MutableStateFlow(initial)
    val state: StateFlow<AppState> = _state.asStateFlow()

    private val lock = Any()
    private var gatewayJob: Job? = null

    /** Applies [action] and runs the effects it asks for. Safe to call from any thread. */
    fun dispatch(action: Action) {
        val effects = synchronized(lock) {
            val r = Reducer.reduce(_state.value, action)
            _state.value = r.state
            r.effects
        }
        effects.forEach(::run)
    }

    // ---- lifecycle ----------------------------------------------------------------------------

    /** Launch: restores from the cache and the saved token, then confirms with the server. */
    fun bootstrap(): Job = scope.launch {
        if (tokens.load() == null) {
            dispatch(Action.SignedOut())
            return@launch
        }
        val cached = cache?.loadInitial()
        if (cached != null) dispatch(Action.InitialLoaded(cached))
        guard(runCatching { api.loadInitial() }).onSuccess {
            dispatch(if (cached != null) Action.Refreshed(it) else Action.InitialLoaded(it))
            cache?.saveLists(state.value)
        }.onFailure {
            // Offline with a cache: stay on what we have; the gateway's reconnect refreshes later.
            if (cached == null && state.value.phase == SessionPhase.Loading) dispatch(Action.SignedOut(it as? AppError))
        }
        startGateway()
    }

    /** Signs in with a username and password; the token goes to the [TokenStore]. */
    suspend fun signIn(username: String, password: String, deviceName: String): Result<LoginResult> {
        val r = runCatching { api.login(username, password, deviceName) }
        r.onSuccess { result ->
            if (result is LoginResult.SignedIn) {
                tokens.save(result.session.token)
                dispatch(Action.SessionStarted(result.session.me))
                bootstrap()
            }
        }
        return r
    }

    fun signOut() {
        stopGateway()
        scope.launch { runCatching { api.logout() } }
        tokens.clear()
        cache?.clear()
        dispatch(Action.SignedOut())
    }

    /** The app is in the foreground: run the event stream. */
    fun startGateway() {
        if (state.value.phase != SessionPhase.SignedIn || gatewayJob?.isActive == true) return
        gatewayJob = scope.launch {
            gateway.updates().collect { update ->
                dispatch(Action.Gateway(update))
                if (state.value.phase is SessionPhase.SignedOut) tokens.clear()
            }
        }
    }

    /** The app left the foreground: no socket in the background (push wakes the user). */
    fun stopGateway() {
        gatewayJob?.cancel()
        gatewayJob = null
    }

    // ---- intents ------------------------------------------------------------------------------

    fun openHome() = dispatch(Action.OpenHome)
    fun openGuild(id: Long) = dispatch(Action.OpenGuild(id))
    fun openChannel(id: Long) = dispatch(Action.OpenChannel(id))
    fun loadOlder(channel: Long) = dispatch(Action.LoadOlder(channel))
    fun send(text: String) = dispatch(Action.Send(text, clock()))
    fun retry(nonce: String) = dispatch(Action.Retry(nonce))
    fun react(channel: Long, message: Long, emoji: ReactionEmoji) = dispatch(Action.React(channel, message, emoji))
    fun dismissNotice(id: Long) = dispatch(Action.DismissNotice(id))

    // ---- effects ------------------------------------------------------------------------------

    private fun run(effect: Effect) {
        scope.launch {
            when (effect) {
                is Effect.LoadMessages -> {
                    val r = guard(runCatching { api.listMessages(effect.channel, effect.before) })
                    dispatch(Action.MessagesLoaded(effect.channel, effect.before, r))
                    if (effect.before == null) cache?.saveChannel(effect.channel, state.value.logs[effect.channel]?.messages.orEmpty())
                }
                is Effect.LoadChannels -> dispatch(Action.ChannelsLoaded(effect.guild, guard(runCatching { api.listChannels(effect.guild) })))
                is Effect.LoadMembers -> dispatch(Action.MembersLoaded(effect.guild, guard(runCatching { api.listMembers(effect.guild) })))
                is Effect.LoadAccess -> dispatch(Action.AccessLoaded(effect.guild, guard(runCatching { api.listRoles(effect.guild) })))
                Effect.Refresh -> guard(runCatching { api.loadInitial() }).onSuccess {
                    dispatch(Action.Refreshed(it))
                    cache?.saveLists(state.value)
                }
                is Effect.SendMessage -> dispatch(
                    Action.MessageSent(effect.channel, effect.nonce, guard(runCatching { api.sendMessage(effect.channel, effect.content, effect.nonce, effect.attachmentIds) })),
                )
                is Effect.AddReaction -> guard(runCatching { api.addReaction(effect.channel, effect.message, effect.emoji) })
                is Effect.RemoveReaction -> guard(runCatching { api.removeReaction(effect.channel, effect.message, effect.emoji) })
            }
        }
    }

    /**
     * Failures that change what the app is: a refused token signs out (and forgets it), an old
     * client, a suspension or unaccepted terms put up their gate. Everything else is the caller's.
     */
    private fun <T> guard(r: Result<T>): Result<T> {
        val e = r.exceptionOrNull() ?: return r
        if (e is CancellationException) throw e
        when (e) {
            AppError.Unauthenticated -> {
                tokens.clear()
                dispatch(Action.SignedOut(AppError.Unauthenticated))
            }
            AppError.ClientTooOld -> dispatch(Action.SetPhase(SessionPhase.UpdateRequired))
            is AppError.Suspended -> dispatch(Action.SetPhase(SessionPhase.Suspended(e.reason, e.until)))
            AppError.TermsUpdateRequired -> dispatch(Action.SetPhase(SessionPhase.TermsRequired("")))
            else -> Unit
        }
        return r
    }
}
