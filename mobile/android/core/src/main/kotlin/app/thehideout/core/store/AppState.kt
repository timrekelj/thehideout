package app.thehideout.core.store

import app.thehideout.core.api.AppError
import app.thehideout.core.attachment.DEFAULT_MAX_UPLOAD_BYTES
import app.thehideout.core.model.*
import java.time.Instant

/** What the root view switches on. */
sealed interface SessionPhase {
    /** Launch with a saved session to check. */
    data object Loading : SessionPhase
    data class SignedOut(val error: AppError? = null) : SessionPhase
    data object SignedIn : SessionPhase
    data class TermsRequired(val version: String) : SessionPhase
    data object UpdateRequired : SessionPhase
    data class Suspended(val reason: String, val until: Instant?) : SessionPhase
}

enum class ConnectionStatus { Connecting, Connected, Reconnecting }

/**
 * Whether the event stream has been whole since the last `Connected`. The gateway does not replay
 * what it missed, so an [Interrupted] stream means the lists must be fetched again.
 */
enum class StreamState { Live, Interrupted }

sealed interface Place {
    /** Home: the direct-message list. */
    data object Home : Place
    data class InGuild(val id: Long) : Place
}

data class Unread(val count: Int = 0, val mentions: Int = 0)

/** Where the "New" divider sits in the channel being read. */
sealed interface Marker {
    data object None : Marker
    /** Entered with [unread] new messages before the history arrived. */
    data class Pending(val channel: Long, val unread: Int) : Marker
    /** The divider goes after message [id] (`null`: before the first). */
    data class After(val channel: Long, val id: Long?) : Marker
}

enum class Fetch { Idle, Initial, Older }

/** One channel's loaded history, oldest first. */
data class ChannelLog(
    val messages: List<Message> = emptyList(),
    val oldestId: Long? = null,
    val hasMore: Boolean = false,
    val loaded: Boolean = false,
    val fetch: Fetch = Fetch.Idle,
    /** Loaded before the gateway dropped and not refetched since: the next visit fetches the newest page again and merges it. */
    val stale: Boolean = false,
) {
    val newestId: Long? get() = messages.lastOrNull()?.id

    /** Adds [m] in id order unless it is already here (the RPC reply and the gateway echo both bring a sent message). */
    fun insert(m: Message): ChannelLog {
        val at = messages.binarySearchBy(m.id) { it.id }
        if (at >= 0) return this
        val next = messages.toMutableList().also { it.add(-(at + 1), m) }
        return copy(messages = next, oldestId = next.first().id)
    }

    /** Merges a page (newest first). A message already here takes the page's reactions: the page is what the server says now. */
    fun insertPage(items: List<Message>): ChannelLog {
        var log = this
        for (m in items.sortedBy { it.id }) {
            val at = log.messages.binarySearchBy(m.id) { it.id }
            log = if (at >= 0) {
                val list = log.messages.toMutableList()
                list[at] = list[at].copy(reactions = m.reactions)
                log.copy(messages = list)
            } else {
                log.insert(m)
            }
        }
        return log
    }
}

enum class Delivery { Pending, Failed }

/** A message this client sent that the server has not confirmed yet (or refused). */
data class Outgoing(
    val nonce: String,
    val channelId: Long,
    val content: String,
    val at: Instant,
    val state: Delivery = Delivery.Pending,
    val attachmentIds: List<Long> = emptyList(),
    /** Local previews of [attachmentIds], shown until the server's message replaces this pending one. */
    val attachments: List<PendingPreview> = emptyList(),
)

enum class NoticeKind { Info, Success, Danger }

/** A transient message for a snackbar. */
data class Notice(val id: Long, val kind: NoticeKind, val text: String)

/** The immutable snapshot the UI reads (the Kotlin twin of desktop's `AppState`). */
data class AppState(
    val phase: SessionPhase = SessionPhase.Loading,
    val me: User? = null,
    val guilds: List<Guild> = emptyList(),
    val channels: Map<Long, List<Channel>> = emptyMap(),
    val layouts: Map<Long, GuildLayout> = emptyMap(),
    val dms: List<DmChannel> = emptyList(),
    val users: Map<Long, User> = emptyMap(),
    /** Member ids per guild, once loaded. */
    val members: Map<Long, List<Long>> = emptyMap(),
    /** Guilds whose members were asked for, so one question is ever in flight. */
    val membersAsked: Set<Long> = emptySet(),
    val roles: Map<Long, List<Role>> = emptyMap(),
    val roleVersions: Map<Long, Long> = emptyMap(),
    /** Role ids per member per guild. */
    val memberRoles: Map<Long, Map<Long, List<Long>>> = emptyMap(),
    val accessAsked: Set<Long> = emptySet(),
    val emojis: Map<Long, List<Emoji>> = emptyMap(),
    /** Guilds whose emojis were asked for, so one question is ever in flight. */
    val emojisAsked: Set<Long> = emptySet(),
    /** Bans per guild, once the Bans page has asked for them (BAN_MEMBERS). */
    val bans: Map<Long, List<Ban>> = emptyMap(),
    /** Folded embeds (ATT-07), oldest first, capped like desktop's `collapsed_embeds`. */
    val collapsedEmbeds: List<app.thehideout.core.embed.EmbedRef> = emptyList(),
    /** The account's notification settings, once known (a change on another session arrives as an event). */
    val notificationSettings: NotificationSettings? = null,
    val logs: Map<Long, ChannelLog> = emptyMap(),
    val outgoing: List<Outgoing> = emptyList(),
    val unread: Map<Long, Unread> = emptyMap(),
    /** Who is in voice, per guild per user (the server keeps one state per user). */
    val voice: Map<Long, Map<Long, VoiceState>> = emptyMap(),
    val place: Place = Place.Home,
    /** The channel open in each guild. */
    val guildChannel: Map<Long, Long> = emptyMap(),
    val homeDm: Long? = null,
    val marker: Marker = Marker.None,
    val connection: ConnectionStatus = ConnectionStatus.Connecting,
    val stream: StreamState = StreamState.Live,
    val notices: List<Notice> = emptyList(),
    /** The composer's unsent text per channel, kept while switching channels. */
    val drafts: Map<Long, String> = emptyMap(),
    /** The composer's attached-but-unsent files per channel (ATT-09). */
    val attachDrafts: Map<Long, AttachDraft> = emptyMap(),
    /** The server's per-upload byte cap, assumed until the server says otherwise (none of our RPCs report it yet). */
    val maxUploadBytes: Long = DEFAULT_MAX_UPLOAD_BYTES,
    val noticeSeq: Long = 0,
    val nonceSeq: Long = 0,
    val uploadSeq: Long = 0,
    /** Guilds we are leaving or deleting ourselves, so their `guild_removed` is not announced as news. */
    val exiting: Set<Long> = emptySet(),
    /** Whether the shell's tab that can show [currentChannel]'s transcript (Servers for a guild,
     * DMs for home) is the foreground destination. */
    val channelForeground: Boolean = true,
) {
    val myId: Long get() = me?.id ?: 0L

    fun userName(id: Long): String = users[id]?.displayName ?: "user $id"

    fun guild(id: Long): Guild? = guilds.firstOrNull { it.id == id }

    fun channelsOf(guild: Long): List<Channel> = channels[guild].orEmpty()

    fun channel(guild: Long, id: Long): Channel? = channelsOf(guild).firstOrNull { it.id == id }

    /** A guild's channels in sidebar order: the layout's, then any the layout does not list yet. */
    fun orderedChannels(guild: Long): List<Channel> {
        val all = channelsOf(guild)
        val layout = layouts[guild] ?: return all
        val listed = layout.channelOrder().mapNotNull { id -> all.firstOrNull { it.id == id } }
        return listed + all.filter { c -> listed.none { it.id == c.id } }
    }

    /** What a server opens on: voice channels are joined, not opened. */
    fun firstTextChannel(guild: Long): Long? = orderedChannels(guild).firstOrNull { it.kind == ChannelKind.Text }?.id

    fun dm(id: Long): DmChannel? = dms.firstOrNull { it.id == id }

    fun dmPeer(dm: DmChannel): User? = dm.participants.firstOrNull { it.id != myId } ?: dm.participants.firstOrNull()

    val currentChannel: Long?
        get() = when (val p = place) {
            Place.Home -> homeDm
            is Place.InGuild -> guildChannel[p.id]
        }

    val currentGuild: Guild? get() = (place as? Place.InGuild)?.let { guild(it.id) }

    fun log(channel: Long): ChannelLog? = logs[channel]

    fun unreadOf(channel: Long): Unread = unread[channel] ?: Unread()

    fun guildUnread(guild: Long): Unread = channelsOf(guild).fold(Unread()) { a, c ->
        val u = unreadOf(c.id)
        Unread(a.count + u.count, a.mentions + u.mentions)
    }

    val homeUnread: Unread
        get() = dms.fold(Unread()) { a, d ->
            val u = unreadOf(d.id)
            Unread(a.count + u.count, a.mentions + u.mentions)
        }

    fun voiceChannelStates(guild: Long, channel: Long): List<VoiceState> =
        voice[guild].orEmpty().values.filter { it.channelId == channel }.sortedBy { it.joinedAt }

    /** What the signed-in user may do in [guild] (nothing until its roles arrived, unless they own it). */
    fun perms(guild: Long): Long {
        val g = guild(guild) ?: return 0L
        val mine = memberRoles[guild]?.get(myId).orEmpty()
        return Perm.effective(g, myId, mine, roles[guild].orEmpty())
    }

    fun mentionsMe(content: String): Boolean = me?.let { mentions(content, it.username) } ?: false

    /** Everyone the user could start a DM with: known users but themselves, by name. */
    fun knownUsers(): List<User> = users.values.filter { it.id != myId }.sortedBy { it.username.lowercase() }

    fun draft(channel: Long): String = drafts[channel].orEmpty()

    fun attachDraft(channel: Long): AttachDraft = attachDrafts[channel] ?: AttachDraft()

    /** Loaded messages that mention the signed-in user, newest first, across every channel whose history is in memory. */
    fun mentionFeed(): List<MentionEntry> {
        val name = me?.username ?: return emptyList()
        return logs.entries.flatMap { (channel, log) ->
            log.messages.filter { it.authorId != myId && mentions(it.content, name) }.map { MentionEntry(channel, it) }
        }.sortedByDescending { it.message.createdAt }
    }

    // ---- server settings and moderation: who may do what (mirrors the server's rules so the
    // controls can be greyed out; the server still checks every call) --------------------------

    fun isOwner(guild: Long): Boolean = guild(guild)?.ownerId == myId

    fun rolesOf(guild: Long): List<Role> = roles[guild].orEmpty()

    fun role(guild: Long, id: Long): Role? = rolesOf(guild).firstOrNull { it.id == id }

    fun roleVersion(guild: Long): Long = roleVersions[guild] ?: 0L

    /** The position of the signed-in user's highest role in [guild] (`null` without roles). */
    fun topPosition(guild: Long): Int? = topPosition(memberRoles[guild]?.get(myId).orEmpty(), rolesOf(guild))

    /** Whether the user may manage a role at [position]: the owner always, anyone else only strictly below their highest role. */
    fun outranks(guild: Long, position: Int): Boolean = isOwner(guild) || topPosition(guild)?.let { position < it } == true

    fun canManageRoles(guild: Long): Boolean = Perm.has(perms(guild), Perm.MANAGE_ROLES)
    fun canManageServer(guild: Long): Boolean = Perm.has(perms(guild), Perm.MANAGE_SERVER)
    fun canManageEmojis(guild: Long): Boolean = Perm.has(perms(guild), Perm.MANAGE_EMOJIS)

    /** Whether the user may kick or ban at all (so the Members page's moderation actions are theirs to see). */
    fun canRemovePeople(guild: Long): Boolean = Perm.has(perms(guild), Perm.KICK_MEMBERS) || Perm.has(perms(guild), Perm.BAN_MEMBERS)

    /** "Server settings" is offered to anyone who may manage the server, its roles or emojis, or remove people. */
    fun canOpenServerSettings(guild: Long): Boolean = canManageServer(guild) || canManageRoles(guild) || canManageEmojis(guild) || canRemovePeople(guild)

    /** Whether the user may edit, delete or hand out [role]. */
    fun canEditRole(guild: Long, role: Role): Boolean = canManageRoles(guild) && outranks(guild, role.position)

    /** Whether the user may grant or revoke every bit of [bits] (must hold them; the owner holds all). */
    fun canGrant(guild: Long, bits: Long): Boolean = isOwner(guild) || Perm.has(perms(guild), bits)

    /** The colour of a member's name: that of their highest-positioned role that has one, "" for none. */
    fun nameColor(guild: Long, user: Long): String {
        if (users[user]?.deleted == true) return ""
        val mine = memberRoles[guild]?.get(user).orEmpty()
        return rolesOf(guild).filter { it.id in mine && it.color.isNotEmpty() }.maxByOrNull { it.position }?.color ?: ""
    }

    /** How many of the guild's loaded members hold [role]. */
    fun roleMembers(guild: Long, role: Long): Int = memberRoles[guild]?.values?.count { role in it } ?: 0

    /** The roles [user] doesn't hold that the signed-in user may hand out. */
    fun assignableRoles(guild: Long, user: Long): List<Role> {
        val held = memberRoles[guild]?.get(user).orEmpty()
        return rolesOf(guild).filter { it.id !in held && canEditRole(guild, it) }
    }

    /** Whether the signed-in user holds [bit] in [guild] and outranks [user] there (or is [user]): the server's moderation rule. */
    fun canModerate(guild: Long, user: Long, bit: Long): Boolean {
        val g = guild(guild) ?: return false
        if (!Perm.has(perms(guild), bit)) return false
        if (user == myId) return true
        return outranksMember(g, myId, memberRoles[guild]?.get(myId).orEmpty(), user, memberRoles[guild]?.get(user).orEmpty(), rolesOf(guild))
    }

    /** Whether [user] may be removed by the signed-in user: KICK_MEMBERS and a higher top role; never yourself. */
    fun canKick(guild: Long, user: Long): Boolean = user != myId && canModerate(guild, user, Perm.KICK_MEMBERS)

    /** Whether [user] may be banned by the signed-in user: BAN_MEMBERS and the same hierarchy rule as [canKick]. */
    fun canBan(guild: Long, user: Long): Boolean = user != myId && canModerate(guild, user, Perm.BAN_MEMBERS)

    /** Whether the ban list is available to the signed-in user. */
    fun canViewBans(guild: Long): Boolean = Perm.has(perms(guild), Perm.BAN_MEMBERS)

    /** Whether leaving is offered: members may, the owner may not. */
    fun canLeaveGuild(guild: Long): Boolean = guild(guild)?.let { it.ownerId != myId } == true

    /** Whether deleting the guild or handing it over is offered: only the owner. */
    fun canDeleteGuild(guild: Long): Boolean = isOwner(guild)

    /** The bans of [guild], newest first, once loaded. */
    fun bansOf(guild: Long): List<Ban> = bans[guild].orEmpty()

    /** The members a server can be handed to (everyone but the signed-in user), by name. */
    fun transferCandidates(guild: Long): List<User> =
        members[guild].orEmpty().filter { it != myId }.mapNotNull { users[it] }.sortedBy { it.username.lowercase() }
}

/** One hit in [AppState.mentionFeed]. */
data class MentionEntry(val channelId: Long, val message: Message)

/** Whether [content] has an `@username` mention (case-insensitive, whole word). */
fun mentions(content: String, username: String): Boolean {
    if (username.isEmpty()) return false
    val re = Regex("(?<![\\p{L}\\p{N}_])@" + Regex.escape(username) + "(?![\\p{L}\\p{N}_])", RegexOption.IGNORE_CASE)
    return re.containsMatchIn(content)
}
