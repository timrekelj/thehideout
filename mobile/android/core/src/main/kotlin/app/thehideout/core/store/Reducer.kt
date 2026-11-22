package app.thehideout.core.store

import app.thehideout.core.api.AppError
import app.thehideout.core.api.GuildChannels
import app.thehideout.core.api.GuildMembers
import app.thehideout.core.api.GuildRoles
import app.thehideout.core.api.Initial
import app.thehideout.core.api.MessagePage
import app.thehideout.core.api.PendingAttachment
import app.thehideout.core.attachment.MAX_ATTACHMENTS
import app.thehideout.core.attachment.humanSize
import app.thehideout.core.model.*
import java.time.Instant

/** Work the store does after a transition (desktop's `Command`s on the outbox). */
sealed interface Effect {
    data class LoadMessages(val channel: Long, val before: Long?) : Effect
    data class LoadChannels(val guild: Long) : Effect
    data class LoadMembers(val guild: Long) : Effect
    data class LoadAccess(val guild: Long) : Effect
    /** The lists again after a gap in the stream. */
    data object Refresh : Effect
    data class SendMessage(val channel: Long, val content: String, val nonce: String, val attachmentIds: List<Long>) : Effect
    data class AddReaction(val channel: Long, val message: Long, val emoji: ReactionEmoji) : Effect
    data class RemoveReaction(val channel: Long, val message: Long, val emoji: ReactionEmoji) : Effect
    data class EditMessage(val channel: Long, val message: Long, val content: String, val previous: String) : Effect
    data class DeleteMessage(val channel: Long, val message: Long) : Effect
    data class OpenDm(val userId: Long) : Effect
    /** Streams [source]'s bytes as a pending attachment; the runner gates concurrency and retries transient failures. */
    data class UploadAttachment(val id: Long, val channel: Long, val filename: String, val contentType: String, val size: Long, val source: app.thehideout.core.api.UploadSource) : Effect
    /** The tray's ✕ on a file that is queued or uploading: stop it. */
    data class CancelUpload(val id: Long) : Effect
}

/** Where editing a message landed: the new content, or the server's refusal and what to roll back to. */
sealed interface EditOutcome {
    data class Edited(val message: Message) : EditOutcome
    data class Failed(val channel: Long, val message: Long, val previous: String, val error: AppError) : EditOutcome
}

/** Replies and user intents the reducer understands. */
sealed interface Action {
    // ---- replies ----
    data class InitialLoaded(val initial: Initial) : Action
    data class Refreshed(val initial: Initial) : Action
    data class ChannelsLoaded(val guild: Long, val result: Result<GuildChannels>) : Action
    data class MessagesLoaded(val channel: Long, val before: Long?, val result: Result<MessagePage>) : Action
    data class MessageSent(val channel: Long, val nonce: String, val result: Result<Message>) : Action
    data class MessageEdited(val outcome: EditOutcome) : Action
    data class MessageDeleteResult(val channel: Long, val message: Long, val result: Result<Unit>) : Action
    data class DmOpened(val result: Result<DmChannel>) : Action
    data class MembersLoaded(val guild: Long, val result: Result<GuildMembers>) : Action
    data class AccessLoaded(val guild: Long, val result: Result<GuildRoles>) : Action
    data class Gateway(val update: GatewayUpdate) : Action

    // ---- session ----
    /** Signed out (explicitly, or the token was refused). */
    data class SignedOut(val error: AppError? = null) : Action
    data class SetPhase(val phase: SessionPhase) : Action
    data class SessionStarted(val me: User) : Action

    // ---- intents ----
    data object OpenHome : Action
    data class OpenGuild(val id: Long) : Action
    /** Opens (or asks the server to open) a DM with a user and shows it. */
    data class StartDm(val userId: Long) : Action
    data class OpenChannel(val channel: Long) : Action
    data class LoadOlder(val channel: Long) : Action
    data class Send(val content: String, val now: Instant) : Action
    data class Retry(val nonce: String) : Action
    data class React(val channel: Long, val message: Long, val emoji: ReactionEmoji) : Action
    data class EditMessage(val channel: Long, val message: Long, val content: String) : Action
    data class DeleteMessage(val channel: Long, val message: Long) : Action
    data class SetDraft(val channel: Long, val text: String) : Action
    /** Files the picker, camera or a share/paste handed the composer; each starts uploading (concurrency gated by the runner). */
    data class AttachFiles(val channel: Long, val files: List<PickedFile>) : Action
    /** A queued upload got its turn (the runner acquired a concurrency slot). */
    data class AttachStarted(val id: Long) : Action
    data class AttachProgress(val id: Long, val sent: Long, val total: Long) : Action
    data class AttachUploaded(val id: Long, val result: Result<PendingAttachment>) : Action
    /** The tray's ✕: forgets the file (and cancels it if it was still going up). */
    data class RemoveAttachment(val id: Long) : Action
    /** The failed chip's "Try again". */
    data class RetryAttachment(val id: Long) : Action
    data class DismissNotice(val id: Long) : Action
    /** Raises a snackbar notice. */
    data class Notice(val kind: NoticeKind, val text: String) : Action
    /** Marks a guild as being left or deleted by us, so its `guild_removed` is not announced. */
    data class Exiting(val guild: Long) : Action
    /** The shell's tab that can show [AppState.currentChannel]'s transcript (Servers for a guild,
     * DMs for home) became, or stopped being, the foreground destination. */
    data class SetChannelForeground(val visible: Boolean) : Action
}

data class Reduction(val state: AppState, val effects: List<Effect> = emptyList())

const val MAX_MESSAGE_CHARS = 4000
private const val MAX_NOTICES = 3

/**
 * The Kotlin port of desktop's `apply.rs` plus the state transitions in `state.rs`, `moderation.rs`
 * and `voice.rs`: a pure function from state and action to the next state and the effects to run.
 * Everything the UI shows is derived from [AppState]; nothing here touches Android or the network.
 */
object Reducer {
    fun reduce(s: AppState, a: Action): Reduction = when (a) {
        is Action.InitialLoaded -> initial(s, a.initial)
        is Action.Refreshed -> refreshed(s, a.initial)
        is Action.ChannelsLoaded -> a.result.fold({ setChannels(s, a.guild, it) }, { Reduction(s) })
        is Action.MessagesLoaded -> messages(s, a.channel, a.before, a.result)
        is Action.MessageSent -> sent(s, a.channel, a.nonce, a.result)
        is Action.MessageEdited -> when (val o = a.outcome) {
            is EditOutcome.Edited -> Reduction(messageUpdated(s, o.message))
            is EditOutcome.Failed -> {
                val rolledBack = mapMessage(s, o.channel, o.message) { it.copy(content = o.previous) }
                Reduction(notice(rolledBack, NoticeKind.Danger, "Couldn't save the edit. ${sentence(o.error.message.orEmpty())}"))
            }
        }
        is Action.MessageDeleteResult -> a.result.fold(
            { Reduction(s) },
            { e -> Reduction(notice(s, NoticeKind.Danger, "Couldn't delete the message. ${sentence(e.message.orEmpty())}")) },
        )
        is Action.DmOpened -> a.result.fold(
            { dm -> openedDm(s, dm) },
            { e -> Reduction(notice(s, NoticeKind.Danger, "Couldn't open the conversation. ${sentence(e.message.orEmpty())}")) },
        )
        is Action.MembersLoaded -> membersLoaded(s, a.guild, a.result)
        is Action.AccessLoaded -> a.result.fold(
            { Reduction(s.copy(roles = s.roles + (a.guild to sortRoles(it.roles)), roleVersions = s.roleVersions + (a.guild to it.version))) },
            { Reduction(s.copy(accessAsked = s.accessAsked - a.guild)) },
        )
        is Action.Gateway -> gateway(s, a.update)
        is Action.SignedOut -> Reduction(AppState(phase = SessionPhase.SignedOut(a.error)))
        is Action.SetPhase -> Reduction(s.copy(phase = a.phase))
        is Action.SessionStarted -> Reduction(s.copy(me = a.me, users = s.users + (a.me.id to a.me)))
        Action.OpenHome -> goHome(s)
        is Action.OpenGuild -> goGuild(s, a.id)
        is Action.StartDm -> {
            val existing = oneToOneDm(s, a.userId)
            if (existing != null) goHomeAndSelect(s, existing.id) else Reduction(s, listOf(Effect.OpenDm(a.userId)))
        }
        is Action.OpenChannel -> selectChannel(s, a.channel)
        is Action.LoadOlder -> loadOlder(s, a.channel)
        is Action.Send -> send(s, a.content, a.now)
        is Action.Retry -> retry(s, a.nonce)
        is Action.React -> react(s, a.channel, a.message, a.emoji)
        is Action.EditMessage -> editMessage(s, a.channel, a.message, a.content)
        is Action.DeleteMessage -> Reduction(s, listOf(Effect.DeleteMessage(a.channel, a.message)))
        is Action.SetDraft -> Reduction(s.copy(drafts = if (a.text.isEmpty()) s.drafts - a.channel else s.drafts + (a.channel to a.text)))
        is Action.AttachFiles -> attachFiles(s, a.channel, a.files)
        is Action.AttachStarted -> Reduction(updateDraftFile(s, a.id) { it.copy(state = AttachUploadState(AttachUploadPhase.Uploading, total = it.size)) })
        is Action.AttachProgress -> Reduction(
            updateDraftFile(s, a.id) { f ->
                if (f.state.phase != AttachUploadPhase.Uploading) f
                else f.copy(size = maxOf(f.size, a.total), state = f.state.copy(sent = a.sent, total = a.total))
            },
        )
        is Action.AttachUploaded -> attachUploaded(s, a.id, a.result)
        is Action.RemoveAttachment -> removeAttachment(s, a.id)
        is Action.RetryAttachment -> retryAttachment(s, a.id)
        is Action.DismissNotice -> Reduction(s.copy(notices = s.notices.filter { it.id != a.id }))
        is Action.Notice -> Reduction(notice(s, a.kind, a.text))
        is Action.Exiting -> Reduction(s.copy(exiting = s.exiting + a.guild))
        // Coming back into foreground is an arrival in its own right: whatever piled up in
        // `currentChannel` while away (its `currentChannel` never changed, so `OpenChannel` would
        // have no-opped) needs the same unread-clearing, marker-fixing treatment.
        is Action.SetChannelForeground -> {
            if (a.visible && !s.channelForeground) arrive(s.copy(channelForeground = true))
            else Reduction(s.copy(channelForeground = a.visible))
        }
    }

    // ---- data in ------------------------------------------------------------------------------

    private fun initial(s: AppState, init: Initial): Reduction {
        val users = buildMap {
            putAll(s.users)
            put(init.me.id, init.me)
            init.dms.forEach { dm -> dm.participants.forEach { put(it.id, it) } }
        }
        var next = s.copy(
            phase = SessionPhase.SignedIn,
            me = init.me,
            users = users,
            guilds = init.guilds,
            channels = groupChannels(init.guilds, init.channels),
            layouts = init.layouts.mapValues { it.value.normalized() },
            dms = init.dms,
            place = Place.Home,
        )
        val first = init.guilds.firstOrNull()?.id
        return if (first != null) goGuild(next, first) else goHome(next.copy(place = Place.InGuild(-1))) // forces `arrive`
    }

    private fun groupChannels(guilds: List<Guild>, channels: List<Channel>): Map<Long, List<Channel>> {
        val out = LinkedHashMap<Long, List<Channel>>()
        guilds.forEach { out[it.id] = emptyList() }
        channels.forEach { c -> out[c.guildId] = out[c.guildId].orEmpty() + c }
        return out
    }

    /** The lists again after a gap in the stream: place, selection and loaded history stay as they are. */
    private fun refreshed(s: AppState, init: Initial): Reduction {
        if (s.phase != SessionPhase.SignedIn) return Reduction(s)
        val open = (s.place as? Place.InGuild)?.id
        val users = buildMap {
            putAll(s.users)
            put(init.me.id, init.me)
            init.dms.forEach { dm -> dm.participants.forEach { put(it.id, it) } }
        }
        var next = s.copy(
            me = init.me,
            users = users,
            layouts = s.layouts + init.layouts.mapValues { it.value.normalized() },
            // Roles and assignments may have changed while away: the open server asks again below, the others when visited.
            roles = emptyMap(),
            roleVersions = emptyMap(),
            memberRoles = emptyMap(),
            accessAsked = emptySet(),
            channels = groupChannels(init.guilds, init.channels),
            guilds = init.guilds,
            dms = init.dms,
            // Members who joined and emojis that changed while away: the open server asks now, the others when visited.
            members = s.members.filterKeys { it == open },
            membersAsked = s.membersAsked.filter { it == open }.toSet(),
            emojis = s.emojis.filterKeys { it == open },
        )
        val effects = mutableListOf<Effect>()
        if (open != null) {
            if (next.guild(open) == null) return goHome(next)
            if (next.currentChannel == null) next.firstTextChannel(open)?.let { first ->
                val r = selectChannel(next, first)
                next = r.state
                effects += r.effects
            }
            effects += Effect.LoadMembers(open)
            next = next.copy(membersAsked = next.membersAsked + open)
            val r = wantAccess(next, open)
            next = r.state
            effects += r.effects
        }
        return Reduction(next, effects)
    }

    private fun setChannels(s: AppState, guild: Long, r: GuildChannels): Reduction {
        var next = s.copy(
            channels = s.channels + (guild to r.channels),
            layouts = s.layouts + (guild to r.layout.normalized()),
        )
        if (next.place == Place.InGuild(guild) && next.currentChannel == null) {
            next.firstTextChannel(guild)?.let { return selectChannel(next, it) }
        }
        return Reduction(next)
    }

    private fun membersLoaded(s: AppState, guild: Long, result: Result<GuildMembers>): Reduction = result.fold(
        { m ->
            val users = s.users + m.users.associateBy { it.id } + m.members.associate { it.user.id to it.user }
            Reduction(
                s.copy(
                    users = users,
                    members = s.members + (guild to m.members.map { it.user.id }),
                    memberRoles = s.memberRoles + (guild to m.members.associate { it.user.id to it.roleIds }),
                ),
            )
        },
        // Refused: leave the guild asked-for-and-empty cleared so a later visit asks again.
        { Reduction(s.copy(membersAsked = s.membersAsked - guild)) },
    )

    private fun messages(s: AppState, channel: Long, before: Long?, result: Result<MessagePage>): Reduction {
        val log0 = s.logs[channel] ?: return Reduction(s)
        val page = result.getOrElse { e ->
            // A failed refresh of what is shown is tried again on the next visit.
            val stale = before == null && log0.loaded
            val fetch = if (before != null || log0.fetch != Fetch.Older) Fetch.Idle else log0.fetch
            val failed = log0.copy(stale = stale, fetch = fetch)
            return Reduction(notice(s.copy(logs = s.logs + (channel to failed)), NoticeKind.Danger, "Couldn't load messages. ${sentence(e.message.orEmpty())}"))
        }
        // A refresh of the newest page does not end an older page's fetch that is still out.
        var log = if (before != null || log0.fetch != Fetch.Older) log0.copy(fetch = Fetch.Idle) else log0
        if (before != null) {
            log = log.insertPage(page.items).copy(hasMore = page.hasMore)
        } else {
            val newestPageOldest = page.items.minOfOrNull { it.id }
            val gap = log.loaded && newestPageOldest != null && log.newestId != null && newestPageOldest > log.newestId!!
            if (!log.loaded || gap) log = log.copy(messages = emptyList(), hasMore = page.hasMore)
            log = log.insertPage(page.items).copy(loaded = true, stale = false)
        }
        log = log.copy(oldestId = log.messages.firstOrNull()?.id)
        var next = s.copy(logs = s.logs + (channel to log))
        val effects = mutableListOf<Effect>()
        for (m in log.messages) {
            val r = wantAuthor(next, m)
            next = r.state
            effects += r.effects
        }
        return Reduction(resolveMarker(next, channel), effects)
    }

    /** A "New" divider that was waiting for its history gets its place. */
    private fun resolveMarker(s: AppState, channel: Long): AppState {
        val m = s.marker as? Marker.Pending ?: return s
        if (m.channel != channel) return s
        val log = s.logs[channel] ?: return s
        val at = log.messages.size - (m.unread + 1)
        return s.copy(marker = Marker.After(channel, if (at >= 0) log.messages[at].id else null))
    }

    private fun sent(s: AppState, channel: Long, nonce: String, result: Result<Message>): Reduction = result.fold(
        { m ->
            val log = s.logs[channel]?.takeIf { it.loaded }?.insert(m)
            Reduction(
                s.copy(
                    outgoing = s.outgoing.filter { it.nonce != nonce },
                    logs = if (log != null) s.logs + (channel to log) else s.logs,
                ),
            )
        },
        { e ->
            val outgoing = s.outgoing.map { if (it.nonce == nonce) it.copy(state = Delivery.Failed) else it }
            val next = s.copy(outgoing = outgoing)
            val transient = (e as? AppError)?.isTransient == true
            Reduction(if (transient) next else notice(next, NoticeKind.Danger, "Message not sent. ${sentence(e.message.orEmpty())}"))
        },
    )

    // ---- gateway ------------------------------------------------------------------------------

    private fun gateway(s: AppState, u: GatewayUpdate): Reduction = when (u) {
        GatewayUpdate.Connecting ->
            Reduction(if (s.connection != ConnectionStatus.Connected) s.copy(connection = ConnectionStatus.Connecting) else s)
        GatewayUpdate.Connected -> {
            // A new stream starts with no voice states; a snapshot follows when anyone is in voice.
            val base = s.copy(connection = ConnectionStatus.Connected, voice = emptyMap(), stream = StreamState.Live)
            // Events missed while disconnected are not replayed: fetch them again.
            if (s.stream == StreamState.Interrupted) resync(base) else Reduction(base)
        }
        is GatewayUpdate.Disconnected -> Reduction(s.copy(stream = StreamState.Interrupted, connection = ConnectionStatus.Reconnecting))
        GatewayUpdate.AuthExpired -> Reduction(AppState(phase = SessionPhase.SignedOut(AppError.Unauthenticated)))
        is GatewayUpdate.Event -> event(s, u.event)
    }

    /** The gateway is back after a gap: lists again, the open channel's newest page now, every other loaded channel's on its next visit. */
    private fun resync(s: AppState): Reduction {
        val current = s.currentChannel
        val logs = s.logs.mapValues { (id, log) -> log.copy(stale = log.loaded && id != current) }
        val effects = buildList {
            add(Effect.Refresh)
            if (current != null) add(Effect.LoadMessages(current, null))
        }
        return Reduction(s.copy(logs = logs), effects)
    }

    private fun event(s: AppState, e: GatewayEvent): Reduction = when (e) {
        GatewayEvent.Heartbeat -> Reduction(s)
        is GatewayEvent.EmojisUpdated -> Reduction(s.copy(emojis = s.emojis + (e.guildId to e.emojis.sortedWith(compareBy({ it.name }, { it.id })))))
        is GatewayEvent.ReactionAdded -> Reduction(reactionEvent(s, e.channelId, e.messageId, e.emoji, e.userId, add = true))
        is GatewayEvent.ReactionRemoved -> Reduction(reactionEvent(s, e.channelId, e.messageId, e.emoji, e.userId, add = false))
        is GatewayEvent.MessageUpdated -> Reduction(messageUpdated(s, e.message))
        is GatewayEvent.GuildUpdated -> Reduction(s.copy(guilds = s.guilds.map { if (it.id == e.guild.id) e.guild else it }))
        // Joined or created elsewhere (another device): list it and load its channels and layout, leaving the current place alone.
        is GatewayEvent.GuildAdded -> {
            val next = if (s.guilds.any { it.id == e.guild.id }) s else s.copy(guilds = s.guilds + e.guild)
            Reduction(next, if (e.guild.id !in s.channels) listOf(Effect.LoadChannels(e.guild.id)) else emptyList())
        }
        is GatewayEvent.ChannelUpdated -> Reduction(
            s.copy(channels = s.channels + (e.channel.guildId to s.channelsOf(e.channel.guildId).map { if (it.id == e.channel.id) e.channel else it })),
        )
        is GatewayEvent.ChannelCreated -> {
            val list = s.channelsOf(e.channel.guildId)
            Reduction(if (list.any { it.id == e.channel.id }) s else s.copy(channels = s.channels + (e.channel.guildId to list + e.channel)))
        }
        is GatewayEvent.UserUpdated -> Reduction(userUpdated(s, e.user))
        is GatewayEvent.LayoutUpdated -> Reduction(setLayout(s, e.guildId, e.layout))
        is GatewayEvent.RolesUpdated ->
            Reduction(s.copy(roles = s.roles + (e.guildId to sortRoles(e.roles)), roleVersions = s.roleVersions + (e.guildId to e.version)))
        is GatewayEvent.MemberUpdated -> {
            val mr = s.memberRoles[e.guildId]
            Reduction(
                s.copy(
                    memberRoles = if (mr != null) s.memberRoles + (e.guildId to (mr + (e.member.user.id to e.member.roleIds))) else s.memberRoles,
                    users = s.users + (e.member.user.id to e.member.user),
                ),
            )
        }
        is GatewayEvent.VoiceStateUpdate -> Reduction(voiceUpdate(s, e.guildId, e.userId, e.state))
        is GatewayEvent.VoiceSnapshot -> Reduction(voiceSnapshot(s, e.guildIds, e.states))
        is GatewayEvent.NotificationSettingsUpdated -> Reduction(s.copy(notificationSettings = e.settings))
        // Rejoining needs a live voice session (phase 6); this device is never in voice yet.
        is GatewayEvent.VoiceMoved -> Reduction(s)
        is GatewayEvent.MessageDeleted -> Reduction(messageRemoved(s, e.channelId, e.messageId))
        is GatewayEvent.MemberRemoved -> memberRemoved(s, e.guildId, e.userId, e.reason)
        is GatewayEvent.GuildRemoved -> guildRemovedEvent(s, e.guildId, null)
        is GatewayEvent.ChannelDeleted -> channelRemoved(s, e.guildId, e.channelId)
        is GatewayEvent.DmOpened -> Reduction(addDm(s, e.dm))
        is GatewayEvent.MemberJoined -> {
            val list = s.members[e.guildId]
            // Before the list is loaded the join is part of it; a list of one would pass for it.
            val members = if (list != null && e.user.id !in list) s.members + (e.guildId to list + e.user.id) else s.members
            Reduction(s.copy(members = members, users = s.users + (e.user.id to e.user)))
        }
        is GatewayEvent.MessageCreated -> messageCreated(s, e.message)
    }

    private fun addDm(s: AppState, dm: DmChannel): AppState {
        val users = s.users + dm.participants.associateBy { it.id }
        return s.copy(users = users, dms = if (s.dms.any { it.id == dm.id }) s.dms else s.dms + dm)
    }

    /** The 1:1 DM with [userId] (never myself, never a group), if one is already known. */
    private fun oneToOneDm(s: AppState, userId: Long): DmChannel? {
        if (userId == s.myId) return null
        return s.dms.firstOrNull { dm ->
            dm.participants.size == 2 && dm.participants.any { it.id == userId } && dm.participants.any { it.id == s.myId }
        }
    }

    /** Goes home and opens [channel] there, keeping both steps' effects (an unloaded DM must still fetch its history). */
    private fun goHomeAndSelect(s: AppState, channel: Long): Reduction {
        val home = goHome(s)
        val selected = selectChannel(home.state, channel)
        return Reduction(selected.state, home.effects + selected.effects)
    }

    /** A DM just opened (ours already, or the server's answer to [Effect.OpenDm]): list it and show it. */
    private fun openedDm(s: AppState, dm: DmChannel): Reduction {
        val next = addDm(s, dm)
        return goHomeAndSelect(next, dm.id)
    }

    private fun userUpdated(s: AppState, u: User): AppState = s.copy(
        me = if (s.me?.id == u.id) u else s.me,
        dms = s.dms.map { dm -> dm.copy(participants = dm.participants.map { if (it.id == u.id) u else it }) },
        users = s.users + (u.id to u),
    )

    /** Takes the server's layout unless it is older than what is held. */
    private fun setLayout(s: AppState, guild: Long, l: GuildLayout): AppState {
        val cur = s.layouts[guild]
        return if (cur == null || l.version >= cur.version) s.copy(layouts = s.layouts + (guild to l.normalized())) else s
    }

    /** Roles by seniority, most senior first. */
    private fun sortRoles(roles: List<Role>): List<Role> = roles.sortedByDescending { it.position }

    /** A message changed on the server: new content, attachments and embeds; your reactions stay (events keep those current). */
    private fun messageUpdated(s: AppState, m: Message): AppState {
        val log = s.logs[m.channelId] ?: return s
        val at = log.messages.indexOfFirst { it.id == m.id }
        if (at < 0) return s
        val old = log.messages[at]
        val list = log.messages.toMutableList()
        list[at] = old.copy(content = m.content, editedAt = m.editedAt, attachments = m.attachments, embeds = m.embeds)
        return s.copy(logs = s.logs + (m.channelId to log.copy(messages = list)))
    }

    /** Files an incoming message: reconciles a pending send by nonce, keeps unread counts for channels that are not open. */
    private fun messageCreated(s: AppState, m: Message): Reduction {
        val channel = m.channelId
        var next = s
        if (m.nonce.isNotEmpty()) next = next.copy(outgoing = next.outgoing.filter { it.nonce != m.nonce })
        val effects = mutableListOf<Effect>()
        wantAuthor(next, m).let { next = it.state; effects += it.effects }
        val visible = next.phase == SessionPhase.SignedIn && next.currentChannel == channel && next.channelForeground
        val mine = m.authorId == next.myId
        val mention = !mine && (next.dm(channel) != null || next.mentionsMe(m.content))
        val log = next.logs[channel]?.takeIf { it.loaded }
        if (log != null) next = next.copy(logs = next.logs + (channel to log.insert(m)))
        if (!visible && !mine) {
            val u = next.unreadOf(channel)
            next = next.copy(unread = next.unread + (channel to Unread(u.count + 1, u.mentions + if (mention) 1 else 0)))
        }
        return Reduction(next, effects)
    }

    private fun reactionEvent(s: AppState, channel: Long, message: Long, emoji: ReactionEmoji, user: Long, add: Boolean): AppState {
        val mine = user == s.myId
        return mapMessage(s, channel, message) { m -> if (mine) setMine(m, emoji, add) else setOther(m, emoji, add) }
    }

    private fun mapMessage(s: AppState, channel: Long, message: Long, f: (Message) -> Message): AppState {
        val log = s.logs[channel] ?: return s
        val at = log.messages.indexOfFirst { it.id == message }
        if (at < 0) return s
        val list = log.messages.toMutableList()
        list[at] = f(list[at])
        return s.copy(logs = s.logs + (channel to log.copy(messages = list)))
    }

    /** Your part of the reaction on [emoji] set or taken back; unchanged when it already was so. */
    private fun setMine(m: Message, emoji: ReactionEmoji, mine: Boolean): Message {
        val at = m.reactions.indexOfFirst { it.emoji == emoji }
        val rs = m.reactions.toMutableList()
        when {
            at >= 0 && mine -> { if (rs[at].me) return m; rs[at] = rs[at].copy(me = true, count = rs[at].count + 1) }
            at < 0 && mine -> rs += Reaction(emoji, 1, true)
            at >= 0 -> {
                if (!rs[at].me) return m
                val count = maxOf(0, rs[at].count - 1)
                if (count == 0) rs.removeAt(at) else rs[at] = rs[at].copy(me = false, count = count)
            }
            else -> return m
        }
        return m.copy(reactions = rs)
    }

    /** Someone else reacted or took theirs back. */
    private fun setOther(m: Message, emoji: ReactionEmoji, add: Boolean): Message {
        val at = m.reactions.indexOfFirst { it.emoji == emoji }
        val rs = m.reactions.toMutableList()
        when {
            at >= 0 && add -> rs[at] = rs[at].copy(count = rs[at].count + 1)
            at < 0 && add -> rs += Reaction(emoji, 1, false)
            at >= 0 -> {
                val count = maxOf(0, rs[at].count - 1)
                if (count == 0) rs.removeAt(at) else rs[at] = rs[at].copy(count = count)
            }
            else -> return m
        }
        return m.copy(reactions = rs)
    }

    // ---- voice (the states only; the connection arrives with phase 6) --------------------------

    private fun voiceUpdate(s: AppState, guild: Long, user: Long, state: VoiceState?): AppState {
        var voice = s.voice
        if (state != null) {
            // A user is in one voice channel at a time, so a state in another guild is stale.
            voice = voice.mapValues { (g, users) -> if (g != guild) users - user else users }
            voice = voice + (guild to (voice[guild].orEmpty() + (user to state)))
        } else {
            voice[guild]?.let { voice = voice + (guild to (it - user)) }
        }
        return s.copy(voice = voice)
    }

    /** Replaces the voice states of the listed guilds. */
    private fun voiceSnapshot(s: AppState, guilds: List<Long>, states: List<VoiceState>): AppState {
        val byGuild = guilds.associateWith { emptyMap<Long, VoiceState>() }.toMutableMap()
        for (st in states) byGuild[st.guildId] = byGuild[st.guildId].orEmpty() + (st.userId to st)
        return s.copy(voice = s.voice + byGuild)
    }

    // ---- moderation ---------------------------------------------------------------------------

    /** `message_deleted`: the message leaves its transcript (and a send of it still pending is untouched: it has a nonce, not an id). */
    private fun messageRemoved(s: AppState, channel: Long, message: Long): AppState {
        val log = s.logs[channel] ?: return s
        val msgs = log.messages.filter { it.id != message }
        return s.copy(logs = s.logs + (channel to log.copy(messages = msgs, oldestId = msgs.firstOrNull()?.id)))
    }

    /** `guild_member_removed`: the member leaves the lists; when it is us, the guild goes too. */
    private fun memberRemoved(s: AppState, guild: Long, user: Long, reason: RemovalReason): Reduction {
        if (user == s.myId) return guildRemovedEvent(s, guild, reason)
        return Reduction(
            s.copy(
                members = s.members[guild]?.let { s.members + (guild to it.filter { id -> id != user }) } ?: s.members,
                memberRoles = s.memberRoles[guild]?.let { s.memberRoles + (guild to (it - user)) } ?: s.memberRoles,
                voice = s.voice[guild]?.let { s.voice + (guild to (it - user)) } ?: s.voice,
            ),
        )
    }

    /** `guild_removed`: we are no longer in the guild. Said to the user unless they did it themselves (leaving or deleting). */
    private fun guildRemovedEvent(s: AppState, guild: Long, reason: RemovalReason?): Reduction {
        val name = s.guild(guild)?.name ?: return Reduction(s)
        val own = guild in s.exiting
        val r = guildDropped(s, guild)
        if (own) return r
        val text = when (reason) {
            RemovalReason.Kicked -> "You were removed from $name"
            RemovalReason.Banned -> "You were banned from $name"
            RemovalReason.Left -> "You left $name"
            // The server says nothing more when a guild is deleted (left, kicked and banned come with a member removal first).
            else -> "$name was deleted"
        }
        return Reduction(notice(r.state, NoticeKind.Info, text), r.effects)
    }

    /** Forgets everything about [guild]; if it was open the app goes home. */
    private fun guildDropped(s: AppState, guild: Long): Reduction {
        val channelIds = s.channelsOf(guild).map { it.id }.toSet()
        val wasOpen = s.place == Place.InGuild(guild)
        val next = s.copy(
            exiting = s.exiting - guild,
            voice = s.voice - guild,
            logs = s.logs - channelIds,
            unread = s.unread - channelIds,
            outgoing = s.outgoing.filter { it.channelId !in channelIds },
            guilds = s.guilds.filter { it.id != guild },
            channels = s.channels - guild,
            layouts = s.layouts - guild,
            roles = s.roles - guild,
            roleVersions = s.roleVersions - guild,
            memberRoles = s.memberRoles - guild,
            accessAsked = s.accessAsked - guild,
            members = s.members - guild,
            membersAsked = s.membersAsked - guild,
            emojis = s.emojis - guild,
            guildChannel = s.guildChannel - guild,
        )
        return if (wasOpen) goHome(next.copy(place = Place.InGuild(guild))) else Reduction(next)
    }

    /** `channel_deleted`: the channel leaves the lists and, if it was open, the guild opens on its first text channel. */
    private fun channelRemoved(s: AppState, guild: Long, channel: Long): Reduction {
        val layout = s.layouts[guild]?.let { l -> l.copy(entries = l.entries.map { e -> e.copy(channelIds = e.channelIds.filter { it != channel }) }) }
        var next = s.copy(
            channels = s.channels[guild]?.let { s.channels + (guild to it.filter { c -> c.id != channel }) } ?: s.channels,
            layouts = if (layout != null) s.layouts + (guild to layout) else s.layouts,
            logs = s.logs - channel,
            unread = s.unread - channel,
            outgoing = s.outgoing.filter { it.channelId != channel },
            voice = s.voice[guild]?.let { s.voice + (guild to it.filterValues { st -> st.channelId != channel }) } ?: s.voice,
        )
        if (next.guildChannel[guild] == channel) {
            next = next.copy(guildChannel = next.guildChannel - guild)
            if (next.place == Place.InGuild(guild)) next.firstTextChannel(guild)?.let { return selectChannel(next, it) }
        }
        return Reduction(next)
    }

    // ---- navigation ---------------------------------------------------------------------------

    private fun goHome(s: AppState): Reduction {
        if (s.place == Place.Home) return Reduction(s)
        return arrive(s.copy(place = Place.Home))
    }

    private fun goGuild(s: AppState, id: Long): Reduction {
        if (s.place == Place.InGuild(id) || s.guild(id) == null) return Reduction(s)
        var next = s.copy(place = Place.InGuild(id))
        if (id !in next.guildChannel) next.firstTextChannel(id)?.let { next = next.copy(guildChannel = next.guildChannel + (id to it)) }
        val effects = mutableListOf<Effect>()
        wantMembers(next, id).let { next = it.state; effects += it.effects }
        wantAccess(next, id).let { next = it.state; effects += it.effects }
        if (id !in next.channels) effects += Effect.LoadChannels(id)
        arrive(next).let { next = it.state; effects += it.effects }
        return Reduction(next, effects)
    }

    private fun wantMembers(s: AppState, guild: Long): Reduction =
        if (guild !in s.members && guild !in s.membersAsked) Reduction(s.copy(membersAsked = s.membersAsked + guild), listOf(Effect.LoadMembers(guild))) else Reduction(s)

    private fun wantAccess(s: AppState, guild: Long): Reduction =
        if (guild !in s.roles && guild !in s.accessAsked) Reduction(s.copy(accessAsked = s.accessAsked + guild), listOf(Effect.LoadAccess(guild))) else Reduction(s)

    /** Asks for the members of the guild [m] was posted in when its author is a stranger. */
    private fun wantAuthor(s: AppState, m: Message): Reduction {
        val g = m.guildId
        return if (g != null && m.authorId !in s.users) wantMembers(s, g) else Reduction(s)
    }

    /** Opens [channel] in the current place. A voice channel is joined, not opened: the open text channel stays. */
    private fun selectChannel(s: AppState, channel: Long): Reduction {
        val place = s.place
        if (place is Place.InGuild && s.channel(place.id, channel)?.kind == ChannelKind.Voice) return Reduction(s)
        // Still arrives (clears unread, fixes the marker) even when this channel was already
        // selected: a tab switch away and a tap back on the same channel does not change
        // `currentChannel`, but the reader is genuinely arriving at it again.
        if (s.currentChannel == channel) return arrive(s)
        val next = when (place) {
            Place.Home -> s.copy(homeDm = channel)
            is Place.InGuild -> s.copy(guildChannel = s.guildChannel + (place.id to channel))
        }
        return arrive(next)
    }

    /** The reader just arrived at the current channel: fix the "New" divider, clear its unread count, fetch its history if needed. */
    private fun arrive(s: AppState): Reduction {
        val channel = s.currentChannel ?: return Reduction(s.copy(marker = Marker.None))
        val unread = s.unread[channel]?.takeIf { it.count > 0 }
        var log = s.logs[channel] ?: ChannelLog()
        val marker: Marker = when {
            unread == null -> Marker.None
            log.loaded -> Marker.After(channel, log.messages.getOrNull(log.messages.size - (unread.count + 1))?.id)
            else -> Marker.Pending(channel, unread.count)
        }
        val effects = mutableListOf<Effect>()
        if (!log.loaded && log.fetch == Fetch.Idle) {
            log = log.copy(fetch = Fetch.Initial)
            effects += Effect.LoadMessages(channel, null)
        } else if (log.loaded && log.stale) {
            // Keeps what is shown; the newest page merges in when it lands.
            log = log.copy(stale = false)
            effects += Effect.LoadMessages(channel, null)
        }
        return Reduction(s.copy(unread = s.unread - channel, logs = s.logs + (channel to log), marker = marker), effects)
    }

    /** Asks for the page older than what is loaded, once. */
    private fun loadOlder(s: AppState, channel: Long): Reduction {
        val log = s.logs[channel] ?: return Reduction(s)
        if (!log.loaded || !log.hasMore || log.fetch != Fetch.Idle) return Reduction(s)
        return Reduction(s.copy(logs = s.logs + (channel to log.copy(fetch = Fetch.Older))), listOf(Effect.LoadMessages(channel, log.oldestId)))
    }

    // ---- sending ------------------------------------------------------------------------------

    /**
     * Queues [text] (with the channel's uploaded files) as a pending message. Nothing queued when
     * there is no channel, the text is over the limit, a file is still going up, or there is
     * neither text nor a finished file (desktop's `AppState::send`).
     */
    private fun send(s: AppState, text: String, now: Instant): Reduction {
        val content = text.trim()
        val channel = s.currentChannel ?: return Reduction(s)
        if (content.codePointCount(0, content.length) > MAX_MESSAGE_CHARS) return Reduction(s)
        val draft = s.attachDrafts[channel]
        if (draft != null && draft.uploading) return Reduction(s)
        if (content.isEmpty() && (draft == null || draft.readyCount == 0)) return Reduction(s)
        val ready = draft?.files.orEmpty().filter { it.state.phase == AttachUploadPhase.Ready }
        val kept = draft?.files.orEmpty().filterNot { it.state.phase == AttachUploadPhase.Ready }
        val attachmentIds = ready.mapNotNull { it.state.pendingId }
        val previews = ready.map { PendingPreview(it.name, it.contentType, it.size, it.preview) }
        val seq = s.nonceSeq + 1
        val nonce = "${s.myId}-$seq"
        val out = Outgoing(nonce, channel, content, now, attachmentIds = attachmentIds, attachments = previews)
        val drafts = if (draft == null) s.attachDrafts else s.attachDrafts + (channel to draft.copy(files = kept, note = null))
        return Reduction(
            s.copy(nonceSeq = seq, outgoing = s.outgoing + out, attachDrafts = drafts),
            listOf(Effect.SendMessage(channel, content, nonce, attachmentIds)),
        )
    }

    // ---- attachments (ATT-09) ------------------------------------------------------------------

    /** Admits each file against [MAX_ATTACHMENTS] and the channel's upload cap, then starts it uploading; the note says what the last refused file broke. */
    private fun attachFiles(s: AppState, channel: Long, files: List<PickedFile>): Reduction {
        var draft = s.attachDraft(channel)
        var seq = s.uploadSeq
        val effects = mutableListOf<Effect>()
        for (f in files) {
            when {
                draft.files.size >= MAX_ATTACHMENTS -> draft = draft.copy(note = "You can attach up to $MAX_ATTACHMENTS files to a message")
                f.size > s.maxUploadBytes -> draft = draft.copy(note = "${f.name} is ${humanSize(f.size)}, over the ${humanSize(s.maxUploadBytes)} limit")
                else -> {
                    seq++
                    draft = draft.copy(files = draft.files + DraftFile(seq, f.name, f.contentType, f.size, f.preview, f.source, AttachUploadState(AttachUploadPhase.Queued)), note = null)
                    effects += Effect.UploadAttachment(seq, channel, f.name, f.contentType, f.size, f.source)
                }
            }
        }
        return Reduction(s.copy(uploadSeq = seq, attachDrafts = s.attachDrafts + (channel to draft)), effects)
    }

    /** Applies [f] to the draft file [id], wherever its channel is. */
    private fun updateDraftFile(s: AppState, id: Long, f: (DraftFile) -> DraftFile): AppState {
        var changed = false
        val drafts = s.attachDrafts.mapValues { (_, d) ->
            val at = d.files.indexOfFirst { it.id == id }
            if (at < 0) d else {
                changed = true
                d.copy(files = d.files.toMutableList().also { it[at] = f(it[at]) })
            }
        }
        return if (changed) s.copy(attachDrafts = drafts) else s
    }

    private fun attachUploaded(s: AppState, id: Long, result: Result<PendingAttachment>): Reduction = result.fold(
        { p ->
            Reduction(updateDraftFile(s, id) { f -> f.copy(size = p.size, state = AttachUploadState(AttachUploadPhase.Ready, sent = p.size, total = p.size, pendingId = p.id)) })
        },
        { e ->
            val reason = uploadErrorMessage(e as? AppError ?: AppError.Other(e.message.orEmpty()))
            Reduction(updateDraftFile(s, id) { f -> f.copy(state = AttachUploadState(AttachUploadPhase.Failed, reason = reason)) })
        },
    )

    /** What a refused upload's chip says (desktop's `attach::upload_error`). */
    private fun uploadErrorMessage(e: AppError): String = when (e) {
        AppError.TooLarge -> "Too large"
        is AppError.Unavailable -> "Upload failed"
        AppError.PermissionDenied -> "You can't attach files here"
        AppError.RateLimited -> "Slow down, try again"
        else -> sentence(e.message.orEmpty()).trimEnd('.')
    }

    /** The tray's ✕: forgets the file in whichever channel holds it, cancelling it if it was still going up. */
    private fun removeAttachment(s: AppState, id: Long): Reduction {
        var wasRunning = false
        val drafts = s.attachDrafts.mapValues { (_, d) ->
            val f = d.files.firstOrNull { it.id == id } ?: return@mapValues d
            if (f.state.phase == AttachUploadPhase.Queued || f.state.phase == AttachUploadPhase.Uploading) wasRunning = true
            d.copy(files = d.files.filter { it.id != id }, note = null)
        }
        val effects = if (wasRunning) listOf(Effect.CancelUpload(id)) else emptyList()
        return Reduction(s.copy(attachDrafts = drafts), effects)
    }

    /** "Try again" on a file that failed: back to queued, with the same source so it streams again. */
    private fun retryAttachment(s: AppState, id: Long): Reduction {
        for ((channel, d) in s.attachDrafts) {
            val f = d.files.firstOrNull { it.id == id && it.state.phase == AttachUploadPhase.Failed } ?: continue
            val next = updateDraftFile(s, id) { it.copy(state = AttachUploadState(AttachUploadPhase.Queued)) }
            return Reduction(next, listOf(Effect.UploadAttachment(id, channel, f.name, f.contentType, f.size, f.source)))
        }
        return Reduction(s)
    }

    private fun retry(s: AppState, nonce: String): Reduction {
        val o = s.outgoing.firstOrNull { it.nonce == nonce && it.state == Delivery.Failed } ?: return Reduction(s)
        return Reduction(
            s.copy(outgoing = s.outgoing.map { if (it.nonce == nonce) it.copy(state = Delivery.Pending) else it }),
            listOf(Effect.SendMessage(o.channelId, o.content, o.nonce, o.attachmentIds)),
        )
    }

    /** Edits an own message in place (optimistic; a refusal rolls back to what was there, see [EditOutcome.Failed]). */
    private fun editMessage(s: AppState, channel: Long, message: Long, content: String): Reduction {
        val trimmed = content.trim()
        if (trimmed.isEmpty() || trimmed.codePointCount(0, trimmed.length) > MAX_MESSAGE_CHARS) return Reduction(s)
        val previous = s.logs[channel]?.messages?.firstOrNull { it.id == message }?.content ?: return Reduction(s)
        val next = mapMessage(s, channel, message) { it.copy(content = trimmed) }
        return Reduction(next, listOf(Effect.EditMessage(channel, message, trimmed, previous)))
    }

    /** Toggles our reaction optimistically and asks the server; the echo from the gateway changes nothing. */
    private fun react(s: AppState, channel: Long, message: Long, emoji: ReactionEmoji): Reduction {
        val m = s.logs[channel]?.messages?.firstOrNull { it.id == message } ?: return Reduction(s)
        val mine = m.reactions.firstOrNull { it.emoji == emoji }?.me == true
        val next = mapMessage(s, channel, message) { setMine(it, emoji, !mine) }
        return Reduction(next, listOf(if (mine) Effect.RemoveReaction(channel, message, emoji) else Effect.AddReaction(channel, message, emoji)))
    }

    // ---- notices ------------------------------------------------------------------------------

    private fun notice(s: AppState, kind: NoticeKind, text: String): AppState {
        val id = s.noticeSeq + 1
        return s.copy(noticeSeq = id, notices = (s.notices + Notice(id, kind, text)).takeLast(MAX_NOTICES))
    }

    /** "wrong code" -> "Wrong code." (desktop's `sentence`). */
    fun sentence(m: String): String {
        val t = m.trim()
        if (t.isEmpty()) return t
        val s = t.replaceFirstChar { it.uppercase() }
        return if (s.last() in ".!?") s else "$s."
    }
}

private fun sentence(m: String): String = Reducer.sentence(m)
