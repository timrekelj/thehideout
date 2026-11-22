package app.thehideout.core.store

import app.thehideout.core.TokenStore
import app.thehideout.core.api.ApiClient
import app.thehideout.core.api.AppError
import app.thehideout.core.api.LoginResult
import app.thehideout.core.api.PendingAttachment
import app.thehideout.core.attachment.MAX_CONCURRENT_UPLOADS
import app.thehideout.core.cache.SessionCache
import app.thehideout.core.gateway.Backoff
import app.thehideout.core.gateway.GatewayStream
import app.thehideout.core.model.ReactionEmoji
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit
import java.time.Duration
import java.time.Instant

/** Extra attempts an upload gets after a transient failure, with backoff, before it shows as failed. */
private const val UPLOAD_RETRIES = 3

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

    // ---- uploads: a concurrency gate and the running job per file, so the tray's ✕ can cancel one -----

    private val uploadSlots = Semaphore(MAX_CONCURRENT_UPLOADS)
    private val uploadJobs = mutableMapOf<Long, Job>()

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
        // Terms first: an unaccepted terms version puts up the gate before anything else loads.
        val me = guard(runCatching { api.me() })
        val terms = me.getOrNull()?.termsUpdateRequired
        if (terms != null) {
            dispatch(Action.SetPhase(SessionPhase.TermsRequired(terms)))
            return@launch
        }
        if (me.isSuccess) {
            guard(runCatching { api.loadInitial() }).onSuccess {
                dispatch(if (cached != null) Action.Refreshed(it) else Action.InitialLoaded(it))
                cache?.saveLists(state.value)
            }.onFailure {
                // Offline without a cache: the sign-in screen, but the token stays (only a refused token is cleared).
                if (cached == null && state.value.phase == SessionPhase.Loading) dispatch(Action.SignedOut(it as? AppError))
            }
        } else if (cached == null && state.value.phase == SessionPhase.Loading) {
            // Offline (or refused) without a cache: the sign-in screen. `guard` already cleared a refused token.
            dispatch(Action.SignedOut(me.exceptionOrNull() as? AppError))
        }
        // Offline with a cache: stay on what we have; the gateway's reconnect refreshes later.
        startGateway()
    }

    /** Accepts the current terms version on the gate; back into the app when it lands. */
    fun acceptTerms(version: String): Job = scope.launch {
        runCatching { api.acceptTerms(version) }.onSuccess {
            dispatch(Action.SetPhase(SessionPhase.SignedIn))
            bootstrap()
        }.onFailure { e ->
            if (e is AppError) dispatch(Action.Notice(NoticeKind.Danger, e.message ?: "Couldn't accept the terms."))
        }
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

    /** A sign-in landed (from the auth flow): the token goes to the [TokenStore] and the world loads. */
    fun onSignedIn(session: app.thehideout.core.api.Session) {
        tokens.save(session.token)
        dispatch(Action.SessionStarted(session.me))
        bootstrap()
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
    fun startDm(userId: Long) = dispatch(Action.StartDm(userId))
    fun openChannel(id: Long) = dispatch(Action.OpenChannel(id))
    fun loadOlder(channel: Long) = dispatch(Action.LoadOlder(channel))
    fun send(text: String) = dispatch(Action.Send(text, clock()))
    fun retry(nonce: String) = dispatch(Action.Retry(nonce))
    fun react(channel: Long, message: Long, emoji: ReactionEmoji) = dispatch(Action.React(channel, message, emoji))
    fun editMessage(channel: Long, message: Long, content: String) = dispatch(Action.EditMessage(channel, message, content))
    fun deleteMessage(channel: Long, message: Long) = dispatch(Action.DeleteMessage(channel, message))
    fun setDraft(channel: Long, text: String) = dispatch(Action.SetDraft(channel, text))
    fun attachFiles(channel: Long, files: List<PickedFile>) = dispatch(Action.AttachFiles(channel, files))
    fun removeAttachment(id: Long) = dispatch(Action.RemoveAttachment(id))
    fun retryAttachment(id: Long) = dispatch(Action.RetryAttachment(id))
    fun dismissNotice(id: Long) = dispatch(Action.DismissNotice(id))
    fun setChannelForeground(visible: Boolean) = dispatch(Action.SetChannelForeground(visible))

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
                is Effect.EditMessage -> {
                    val r = guard(runCatching { api.editMessage(effect.channel, effect.message, effect.content) })
                    val outcome = r.fold(
                        { EditOutcome.Edited(it) },
                        { e -> EditOutcome.Failed(effect.channel, effect.message, effect.previous, e as? AppError ?: AppError.Other(e.message.orEmpty())) },
                    )
                    dispatch(Action.MessageEdited(outcome))
                }
                is Effect.DeleteMessage -> dispatch(
                    Action.MessageDeleteResult(effect.channel, effect.message, guard(runCatching { api.deleteMessage(effect.channel, effect.message) })),
                )
                is Effect.OpenDm -> dispatch(Action.DmOpened(guard(runCatching { api.openDm(effect.userId) })))
                is Effect.UploadAttachment -> {
                    val job = scope.launch {
                        uploadSlots.withPermit {
                            dispatch(Action.AttachStarted(effect.id))
                            dispatch(Action.AttachUploaded(effect.id, guard(uploadWithRetry(effect))))
                        }
                    }
                    job.invokeOnCompletion { uploadJobs.remove(effect.id, job) }
                    uploadJobs[effect.id] = job
                }
                is Effect.CancelUpload -> uploadJobs.remove(effect.id)?.cancel()
            }
        }
    }

    /** Retries a transient failure ([AppError.isTransient]) with backoff, up to [UPLOAD_RETRIES] extra attempts. */
    private suspend fun uploadWithRetry(effect: Effect.UploadAttachment): Result<PendingAttachment> {
        val backoff = Backoff(initial = Duration.ofSeconds(1), max = Duration.ofSeconds(8))
        var attempt = 0
        while (true) {
            val r = runCatching {
                api.uploadAttachment(effect.channel, effect.filename, effect.contentType, effect.size, effect.source) { sent ->
                    dispatch(Action.AttachProgress(effect.id, sent, effect.size))
                }
            }
            val error = r.exceptionOrNull() ?: return r
            if (error is CancellationException) throw error
            if ((error as? AppError)?.isTransient != true || attempt >= UPLOAD_RETRIES) return r
            attempt++
            delay(backoff.delayFor(attempt).toMillis())
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
