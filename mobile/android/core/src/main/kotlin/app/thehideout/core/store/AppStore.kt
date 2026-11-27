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
import app.thehideout.core.model.Ban
import app.thehideout.core.model.Channel
import app.thehideout.core.model.ChannelKind
import app.thehideout.core.model.Emoji
import app.thehideout.core.model.Guild
import app.thehideout.core.model.Invite
import app.thehideout.core.model.Member
import app.thehideout.core.model.ReactionEmoji
import app.thehideout.core.model.RemovalReason
import app.thehideout.core.model.Role
import app.thehideout.core.model.Section
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

    // ---- management: create/join servers, channels, sections, invites (phase 4a) ---------------

    /** Creates a server (the server auto-creates its `general` channel) and opens it. */
    suspend fun createGuild(name: String): Result<Guild> =
        guard(runCatching { api.createGuild(name) }).also { r -> r.onSuccess { dispatch(Action.GuildEntered(it)) } }

    /** Joins the server its invite code points to and opens it (not an 8-character code; see [app.thehideout.core.api.ApiClient.joinGuild]). */
    suspend fun joinGuild(code: String): Result<Guild> =
        guard(runCatching { api.joinGuild(code) }).also { r -> r.onSuccess { dispatch(Action.GuildEntered(it)) } }

    suspend fun createChannel(guild: Long, name: String, kind: ChannelKind, sectionId: Long?): Result<Channel> =
        guard(runCatching { api.createChannel(guild, name, kind, sectionId) }).also { r -> r.onSuccess { dispatch(Action.ChannelCreated(it)) } }

    suspend fun createSection(guild: Long, name: String): Result<Section> =
        guard(runCatching { api.createSection(guild, name) }).also { r -> r.onSuccess { dispatch(Action.LayoutReplaced(guild, it.layout)) } }.map { it.section }

    suspend fun renameSection(guild: Long, section: Long, name: String): Result<Section> =
        guard(runCatching { api.renameSection(guild, section, name) }).also { r -> r.onSuccess { dispatch(Action.LayoutReplaced(guild, it.layout)) } }.map { it.section }

    suspend fun deleteSection(guild: Long, section: Long): Result<Unit> =
        guard(runCatching { api.deleteSection(guild, section) }).also { r -> r.onSuccess { dispatch(Action.LayoutReplaced(guild, it)) } }.map { }

    /** Replaces the server's invite code (MANAGE_SERVER); the old one stops working at once. */
    suspend fun rotateInviteCode(guild: Long): Result<String> =
        guard(runCatching { api.rotateInviteCode(guild) }).also { r -> r.onSuccess { dispatch(Action.GuildCodeRotated(guild, it)) } }

    suspend fun createInvite(email: String, guild: Long): Result<Invite> = guard(runCatching { api.createInvite(email, guild) })
    suspend fun listInvites(guild: Long): Result<List<Invite>> = guard(runCatching { api.listInvites(guild) })
    suspend fun revokeInvite(inviteId: Long): Result<Invite> = guard(runCatching { api.revokeInvite(inviteId) })
    suspend fun resendInvite(inviteId: Long): Result<Invite> = guard(runCatching { api.resendInvite(inviteId) })

    // ---- management: server settings, roles, moderation, emojis (phase 4b-1) -------------------

    suspend fun renameGuild(guild: Long, name: String): Result<Guild> =
        guard(runCatching { api.updateGuildName(guild, name) }).also { r -> r.onSuccess { dispatch(Action.GuildPatched(it)) } }

    suspend fun uploadGuildIcon(guild: Long, data: ByteArray, contentType: String): Result<Guild> =
        guard(runCatching { api.uploadGuildIcon(guild, data, contentType) }).also { r -> r.onSuccess { dispatch(Action.GuildPatched(it)) } }

    suspend fun removeGuildIcon(guild: Long): Result<Guild> =
        guard(runCatching { api.removeGuildIcon(guild) }).also { r -> r.onSuccess { dispatch(Action.GuildPatched(it)) } }

    /** Leaves [guild] at once (optimistic, like desktop's `exiting`): rolled back on refusal. The
     * caller's own dialog shows the error inline; this only undoes the optimistic mark. */
    suspend fun leaveGuild(guild: Long): Result<Unit> {
        dispatch(Action.Exiting(guild))
        return guard(runCatching { api.leaveGuild(guild) }).also { r ->
            r.onSuccess { dispatch(Action.GuildDropped(guild)) }
            r.onFailure { dispatch(Action.ExitingFailed(guild)) }
        }
    }

    /** Deletes [guild] for good; [confirmName] must be its name. The caller's own dialog shows the
     * error inline; this only undoes the optimistic mark. */
    suspend fun deleteGuild(guild: Long, confirmName: String): Result<Unit> {
        dispatch(Action.Exiting(guild))
        return guard(runCatching { api.deleteGuild(guild, confirmName) }).also { r ->
            r.onSuccess { dispatch(Action.GuildDropped(guild)) }
            r.onFailure { dispatch(Action.ExitingFailed(guild)) }
        }
    }

    /** Hands [guild] to [user] (a member other than the caller); [confirmName] must be its name. */
    suspend fun transferOwnership(guild: Long, user: Long, confirmName: String): Result<Guild> =
        guard(runCatching { api.transferOwnership(guild, user, confirmName) }).also { r -> r.onSuccess { dispatch(Action.GuildPatched(it)) } }

    /** Kicks [user]; the caller's own confirm dialog shows a failure inline. */
    suspend fun kickMember(guild: Long, user: Long): Result<Unit> =
        guard(runCatching { api.kickMember(guild, user) }).also { r -> r.onSuccess { dispatch(Action.MemberGone(guild, user, RemovalReason.Kicked)) } }

    /** Bans [user]; [deleteSeconds] also deletes their recent messages (at most 7 days). The
     * caller's own dialog shows a failure inline. */
    suspend fun banMember(guild: Long, user: Long, reason: String, deleteSeconds: Int?): Result<Unit> =
        guard(runCatching { api.banMember(guild, user, reason, deleteSeconds) }).also { r -> r.onSuccess { dispatch(Action.MemberGone(guild, user, RemovalReason.Banned)) } }

    suspend fun unbanMember(guild: Long, user: Long): Result<Unit> =
        guard(runCatching { api.unbanMember(guild, user) }).also { r ->
            r.onSuccess { dispatch(Action.BanLifted(guild, user)) }
            r.onFailure { dispatch(Action.Notice(NoticeKind.Danger, "Couldn't unban them. ${moderationErrorText(it)}")) }
        }

    suspend fun loadBans(guild: Long): Result<List<Ban>> =
        guard(runCatching { api.listBans(guild) }).also { r -> dispatch(Action.BansLoaded(guild, r)) }

    /** Creates a role at the bottom of the hierarchy; the caller's own editor sheet shows a failure inline. */
    suspend fun createRole(guild: Long, name: String, color: String, permissions: Long): Result<Role> =
        guard(runCatching { api.createRole(guild, name, color, permissions) }).also { r ->
            r.onSuccess { dispatch(Action.RoleCreated(guild, it.role, it.version)) }
        }.map { it.role }

    /** Updates a role; the caller's own editor sheet shows a failure inline. */
    suspend fun updateRole(guild: Long, roleId: Long, name: String?, color: String?, permissions: Long?): Result<Role> =
        guard(runCatching { api.updateRole(guild, roleId, name, color, permissions) }).also { r ->
            r.onSuccess {
                // Patches only the fields this call actually changed onto whatever's current, instead of
                // overwriting the whole row with this reply: a newer gateway update to the same role that
                // landed while this request was in flight (e.g. someone else edited it too) stays intact.
                dispatch(
                    Action.RolesReplaced(
                        guild,
                        state.value.rolesOf(guild).map { current ->
                            if (current.id != roleId) current
                            else current.copy(name = name ?: current.name, color = color ?: current.color, permissions = permissions ?: current.permissions)
                        },
                        state.value.roleVersion(guild),
                    ),
                )
            }
        }

    /** Deletes a role; the caller's own confirm dialog shows a failure inline. */
    suspend fun deleteRole(guild: Long, roleId: Long): Result<Unit> =
        guard(runCatching { api.deleteRole(guild, roleId) }).also { r ->
            r.onSuccess { version -> dispatch(Action.RoleDeleted(guild, roleId, version)) }
        }.map { }

    /** Moves a role one step up ([delta] -1) or down (+1) the hierarchy: applied at once, rolled back on refusal, with a notice (there is no dialog here to show it inline). */
    suspend fun moveRole(guild: Long, roleId: Long, delta: Int): Result<Unit> {
        val before = state.value.rolesOf(guild)
        val beforeVersion = state.value.roleVersion(guild)
        val from = before.indexOfFirst { it.id == roleId }
        val to = from + delta
        if (from < 0 || to < 0 || to >= before.size) return Result.success(Unit)
        val reordered = before.toMutableList().apply { add(to, removeAt(from)) }
        val n = before.size
        val renumbered = reordered.mapIndexed { i, r -> r.copy(position = n - 1 - i) }
        dispatch(Action.RolesReplaced(guild, renumbered, beforeVersion))
        return guard(runCatching { api.reorderRoles(guild, beforeVersion, renumbered.map { it.id }) }).also { r ->
            r.onSuccess { dispatch(Action.RolesReplaced(guild, it.roles, it.version)) }
            r.onFailure {
                dispatch(Action.RolesReplaced(guild, before, beforeVersion))
                dispatch(Action.Notice(NoticeKind.Danger, "Couldn't move the role. ${moderationErrorText(it)}"))
            }
        }.map { }
    }

    suspend fun setMemberRoles(guild: Long, userId: Long, roleIds: List<Long>): Result<Member> =
        guard(runCatching { api.setMemberRoles(guild, userId, roleIds) }).also { r ->
            r.onSuccess { dispatch(Action.MemberRolesPatched(guild, it)) }
            r.onFailure { dispatch(Action.Notice(NoticeKind.Danger, "Couldn't change their roles. ${moderationErrorText(it)}")) }
        }

    /** Gives or takes a single role from [userId] (the Members page's "+ role" / chip ✕). */
    suspend fun setMemberRole(guild: Long, userId: Long, roleId: Long, grant: Boolean): Result<Member> {
        val held = state.value.memberRoles[guild]?.get(userId).orEmpty()
        val next = if (grant) held + roleId else held - roleId
        return setMemberRoles(guild, userId, next)
    }

    /** Creates an emoji; the caller's own naming dialog shows a failure inline. */
    suspend fun createEmoji(guild: Long, name: String, data: ByteArray, contentType: String): Result<Emoji> =
        guard(runCatching { api.createEmoji(guild, name, data, contentType) }).also { r -> r.onSuccess { dispatch(Action.EmojiUpserted(guild, it)) } }

    /** Renames an emoji; the caller's own naming dialog shows a failure inline. */
    suspend fun renameEmoji(guild: Long, emojiId: Long, name: String): Result<Emoji> =
        guard(runCatching { api.renameEmoji(guild, emojiId, name) }).also { r -> r.onSuccess { dispatch(Action.EmojiUpserted(guild, it)) } }

    /** Deletes an emoji; the caller's own confirm dialog shows a failure inline. */
    suspend fun deleteEmoji(guild: Long, emojiId: Long): Result<Unit> =
        guard(runCatching { api.deleteEmoji(guild, emojiId) }).also { r -> r.onSuccess { dispatch(Action.EmojiRemoved(guild, emojiId)) } }.map { }

    /** What a refused moderation call says to the user (desktop's `moderation_error_text`). */
    private fun moderationErrorText(e: Throwable): String = when (e) {
        is AppError.RateLimited -> "You're doing that too fast. Wait a moment and try again."
        is AppError.NotFound -> "That no longer exists."
        is AppError.Precondition -> if (e.detail.contains("owner cannot leave")) "The owner can't leave. Transfer ownership or delete the server first." else Reducer.sentence(e.message.orEmpty())
        else -> Reducer.sentence(e.message.orEmpty())
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
    /** The embed header was tapped: its fold flips (ATT-07). */
    fun toggleEmbedCollapsed(message: Long, index: Int) = dispatch(Action.ToggleEmbedCollapsed(message, index))
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
                is Effect.LoadEmojis -> dispatch(Action.EmojisLoaded(effect.guild, guard(runCatching { api.listEmojis(effect.guild) })))
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
