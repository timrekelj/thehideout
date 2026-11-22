package app.thehideout.core.store

import app.thehideout.core.api.AppError
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
    val noticeSeq: Long = 0,
    val nonceSeq: Long = 0,
    /** Guilds we are leaving or deleting ourselves, so their `guild_removed` is not announced as news. */
    val exiting: Set<Long> = emptySet(),
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

    /** Loaded messages that mention the signed-in user, newest first, across every channel whose history is in memory. */
    fun mentionFeed(): List<MentionEntry> {
        val name = me?.username ?: return emptyList()
        return logs.entries.flatMap { (channel, log) ->
            log.messages.filter { it.authorId != myId && mentions(it.content, name) }.map { MentionEntry(channel, it) }
        }.sortedByDescending { it.message.createdAt }
    }
}

/** One hit in [AppState.mentionFeed]. */
data class MentionEntry(val channelId: Long, val message: Message)

/** Whether [content] has an `@username` mention (case-insensitive, whole word). */
fun mentions(content: String, username: String): Boolean {
    if (username.isEmpty()) return false
    val re = Regex("(?<![\\p{L}\\p{N}_])@" + Regex.escape(username) + "(?![\\p{L}\\p{N}_])", RegexOption.IGNORE_CASE)
    return re.containsMatchIn(content)
}
