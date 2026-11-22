package app.thehideout.core.fake

import app.thehideout.core.api.*
import app.thehideout.core.model.*
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.flow
import java.time.Duration
import java.time.Instant

/**
 * An in-memory [ApiClient] seeded like desktop's `fake.rs`: the users ana, bruno, cvetka and tim,
 * The Hideout with sections, text and voice channels, two more servers and two DMs. It is what
 * previews, Roborazzi, the debug gallery and the `fake=true` launch flag run on, and what tests use
 * to play a server: [emit] pushes a gateway event to every open subscription.
 *
 * Deterministic: the seed is laid out backwards from [now], so screenshots do not drift.
 */
class FakeBackend(
    private val now: Instant = DEFAULT_NOW,
    /** Seconds every call waits before it answers (virtual time in tests). */
    var latency: Duration = Duration.ZERO,
    /** After the first heartbeat, deliver a few messages in channels nobody has open, so unread badges show. */
    private val unreadDemo: Boolean = false,
) : ApiClient {
    private val users = listOf(
        user(TIM, "tim", "tim@example.com"),
        user(ANA, "ana", "ana@example.com"),
        user(BRUNO, "bruno", null),
        user(CVETKA, "cvetka", "cvetka@example.com", admin = true),
    ).associateBy { it.id }
    private val me: User get() = users.getValue(TIM)

    private val guilds = listOf(
        guild(HIDEOUT, "The Hideout", TIM, "HIDEOUT1"),
        guild(GARAGE, "Garage Band", ANA, "GARAGE22"),
        guild(COOKING, "Cooking Club", BRUNO, "COOK333"),
    )

    private val channels = mutableListOf<Channel>().apply {
        add(text(GENERAL, HIDEOUT, "general", ChannelIconRef.Phosphor("chat-circle-dots"), "teal", "Say hi, share links, plan the weekend"))
        add(text(DEV, HIDEOUT, "dev", ChannelIconRef.Phosphor("code"), "blue", "Builds, reviews and the odd rant about lifetimes"))
        add(text(RANDOM, HIDEOUT, "random", ChannelIconRef.Phosphor("dice-five"), "amber"))
        add(text(PLANS, HIDEOUT, "plans", ChannelIconRef.Phosphor("calendar-check"), "green"))
        add(text(ANNOUNCE, HIDEOUT, "announcements", ChannelIconRef.Phosphor("megaphone"), "orange", "Read-only news from the mods"))
        add(text(DESIGN, HIDEOUT, "design", ChannelIconRef.Phosphor("paint-brush"), "pink"))
        add(text(MUSIC, HIDEOUT, "music", ChannelIconRef.Phosphor("music-notes"), "indigo"))
        add(text(GAMING, HIDEOUT, "gaming", ChannelIconRef.Phosphor("game-controller"), "red"))
        add(text(MEMES, HIDEOUT, "memes", ChannelIconRef.Phosphor("smiley"), "yellow"))
        add(text(BOOKS, HIDEOUT, "books", ChannelIconRef.Phosphor("books"), "slate"))
        add(voice(LOUNGE, HIDEOUT, "lounge", ChannelIconRef.Phosphor("speaker-high"), VOICE))
        add(voice(GAME_NIGHT, HIDEOUT, "game-night", ChannelIconRef.Phosphor("game-controller"), VOICE))
        add(text(20, GARAGE, "general"))
        add(text(21, GARAGE, "gigs"))
        add(text(30, COOKING, "recipes"))
        add(text(31, COOKING, "shopping"))
    }

    private val layouts = mutableMapOf(
        HIDEOUT to GuildLayout(
            version = 3,
            sections = listOf(
                Section(BUILDING, HIDEOUT, "Building", 0),
                Section(HANGOUT, HIDEOUT, "Hangout", 1),
                Section(ARCHIVE, HIDEOUT, "Archive", 2),
                Section(VOICE, HIDEOUT, "Voice", 3),
            ),
            entries = listOf(
                LayoutEntry(0, listOf(GENERAL, DEV, ANNOUNCE)),
                LayoutEntry(BUILDING, listOf(DESIGN, PLANS)),
                LayoutEntry(HANGOUT, listOf(RANDOM, MUSIC, GAMING, MEMES)),
                LayoutEntry(ARCHIVE, listOf(BOOKS)),
                LayoutEntry(VOICE, listOf(LOUNGE, GAME_NIGHT)),
            ),
        ),
        GARAGE to GuildLayout(1, entries = listOf(LayoutEntry(0, listOf(20, 21)))),
        COOKING to GuildLayout(1, entries = listOf(LayoutEntry(0, listOf(30, 31)))),
    )

    private val dms = listOf(
        DmChannel(DM_ANA, listOf(users.getValue(TIM), users.getValue(ANA)), now - Duration.ofDays(80)),
        DmChannel(DM_BRUNO, listOf(users.getValue(TIM), users.getValue(BRUNO)), now - Duration.ofDays(80)),
    )

    private val members = mapOf(
        HIDEOUT to listOf(TIM, ANA, BRUNO, CVETKA),
        GARAGE to listOf(TIM, ANA, CVETKA),
        COOKING to listOf(TIM, BRUNO),
    )

    private val roles = mapOf(
        HIDEOUT to listOf(
            role(1001, HIDEOUT, "Moderator", "teal", 3, Perm.MANAGE_CHANNELS or Perm.MANAGE_MESSAGES or Perm.KICK_MEMBERS),
            role(1002, HIDEOUT, "Admin", "red", 4, Perm.ADMINISTRATOR),
            role(1003, HIDEOUT, "Designer", "pink", 2, Perm.MANAGE_EMOJIS),
        ),
        GARAGE to emptyList(),
        COOKING to emptyList(),
    )

    private val memberRoles = mapOf(
        HIDEOUT to mapOf(BRUNO to listOf(1001L), CVETKA to listOf(1002L), ANA to listOf(1003L)),
    )

    private val voiceStates = listOf(
        VoiceState(HIDEOUT, LOUNGE, ANA, joinedAt = now - Duration.ofMinutes(25)),
        VoiceState(HIDEOUT, LOUNGE, BRUNO, selfMute = true, joinedAt = now - Duration.ofMinutes(12)),
    )

    private val messages = mutableMapOf<Long, MutableList<Message>>()
    private var nextId = 10_000L
    private var unreadDemoSent = false
    private val events = MutableSharedFlow<GatewayEvent>(extraBufferCapacity = 256)

    /** The next [n] sends fail with a network error (for tests of the failed-send state). */
    var failSends: Int = 0

    /** Every message ever sent through [sendMessage], oldest first. */
    val sent: MutableList<Message> = mutableListOf()

    init {
        seed()
    }

    /** The voice states the stream's snapshot carries (ana and bruno in #lounge). */
    fun voiceSnapshotStates(): List<VoiceState> = voiceStates

    // ---- the server's side: push events ----------------------------------------------------------

    /** Delivers [event] to every open subscription. */
    fun emit(event: GatewayEvent) {
        events.tryEmit(event)
    }

    /** A message from [author] arrives in [channel] as if sent from another device; returns it. */
    fun post(channel: Long, author: Long, content: String, at: Instant = now): Message {
        val m = push(channel, author, content, at)
        emit(GatewayEvent.MessageCreated(m))
        return m
    }

    // ---- ApiClient ---------------------------------------------------------------------------------

    override suspend fun login(username: String, password: String, deviceName: String): LoginResult {
        pause()
        val u = users.values.firstOrNull { it.username == username.trim().lowercase() } ?: throw AppError.Unauthenticated
        if (password.isEmpty()) throw AppError.Unauthenticated
        return LoginResult.SignedIn(Session("fake-token-${u.id}", u))
    }

    override suspend fun me(): Me {
        pause()
        return Me(me, null, "2026-01-01")
    }

    override suspend fun loadInitial(): Initial {
        pause()
        return Initial(me, guilds, channels.toList(), layouts.toMap(), dms)
    }

    override suspend fun listChannels(guild: Long): GuildChannels {
        pause()
        val layout = layouts[guild] ?: throw AppError.NotFound
        return GuildChannels(channels.filter { it.guildId == guild }, layout)
    }

    override suspend fun listMessages(channel: Long, before: Long?, limit: Int): MessagePage {
        pause()
        val all = messages[channel].orEmpty().filter { before == null || it.id < before }
        val page = all.takeLast(limit).asReversed()
        return MessagePage(page, hasMore = all.size > limit)
    }

    override suspend fun sendMessage(channel: Long, content: String, nonce: String, attachmentIds: List<Long>): Message {
        pause()
        if (failSends > 0) {
            failSends--
            throw AppError.Unavailable("connection reset")
        }
        val m = push(channel, TIM, content, now + Duration.ofMinutes(5), nonce)
        sent += m
        emit(GatewayEvent.MessageCreated(m))
        return m
    }

    override suspend fun listMembers(guild: Long): GuildMembers {
        pause()
        val ids = members[guild] ?: throw AppError.NotFound
        val list = ids.map { id ->
            Member(users.getValue(id), memberRoles[guild]?.get(id).orEmpty(), now - Duration.ofDays(70))
        }
        return GuildMembers(list.map { it.user }, list)
    }

    override suspend fun listRoles(guild: Long): GuildRoles {
        pause()
        return GuildRoles(roles[guild].orEmpty(), version = 4)
    }

    override suspend fun addReaction(channel: Long, message: Long, emoji: ReactionEmoji) {
        pause()
        edit(channel, message) { m ->
            val at = m.reactions.indexOfFirst { it.emoji == emoji }
            if (at >= 0) m.copy(reactions = m.reactions.toMutableList().also { it[at] = it[at].copy(count = it[at].count + 1) })
            else m.copy(reactions = m.reactions + Reaction(emoji, 1, false))
        }
        emit(GatewayEvent.ReactionAdded(channel, message, emoji, TIM))
    }

    override suspend fun removeReaction(channel: Long, message: Long, emoji: ReactionEmoji) {
        pause()
        emit(GatewayEvent.ReactionRemoved(channel, message, emoji, TIM))
    }

    override suspend fun logout() = Unit

    override fun subscribe(): Flow<GatewayEvent> = flow {
        emit(GatewayEvent.Heartbeat)
        // Someone is in voice: the snapshot follows the first heartbeat, as on the real stream.
        emit(GatewayEvent.VoiceSnapshot(guilds.map { it.id }, voiceStates))
        // Once per backend: a resubscribe (rotation, foreground) must not deliver them again.
        if (unreadDemo && !unreadDemoSent) {
            unreadDemoSent = true
            for ((channel, author, text) in UNREAD_DEMO) emit(GatewayEvent.MessageCreated(push(channel, author, text, now + Duration.ofMinutes(1))))
        }
        events.collect { emit(it) }
    }

    // ---- internals ---------------------------------------------------------------------------------

    private suspend fun pause() {
        if (!latency.isZero) delay(latency.toMillis())
    }

    private fun edit(channel: Long, id: Long, f: (Message) -> Message) {
        val list = messages[channel] ?: return
        val i = list.indexOfFirst { it.id == id }
        if (i >= 0) list[i] = f(list[i])
    }

    private fun push(channel: Long, author: Long, content: String, at: Instant, nonce: String = ""): Message {
        val guild = channels.firstOrNull { it.id == channel }?.guildId
        val m = Message(nextId++, channel, guild, author, content, at, nonce = nonce)
        messages.getOrPut(channel) { mutableListOf() }.add(m)
        return m
    }

    private fun seed() {
        // #general: forty messages over three days, authors in runs so groups form, ending 18 minutes before `now`.
        val who = listOf(ANA, ANA, BRUNO, TIM, TIM, TIM, CVETKA, ANA, BRUNO, BRUNO, TIM, CVETKA)
        val total = 120
        val lines = List(total) { i ->
            val k = i - (total - MARKDOWN_LINES.size)
            if (k >= 0) MARKDOWN_LINES[k] else GENERAL_LINES[(i * 7) % GENERAL_LINES.size]
        }
        // The history spans a few days and ends 18 minutes before `now`, so a "Yesterday" divider falls inside it.
        var t = now - Duration.ofMinutes(18) - Duration.ofHours(50)
        lines.forEachIndexed { i, line ->
            val author = who[i % who.size]
            t = t.plusSeconds(if (i % 9 == 0) 5400L else if (i > 0 && who[i % who.size] == who[(i - 1) % who.size]) 25L else 140L + (i * 37L) % 400)
            push(GENERAL, author, line, t)
        }
        // A few reactions and one edit, so rows show them.
        messages.getValue(GENERAL).let { list ->
            val k = list.size - MARKDOWN_LINES.size
            list[k] = list[k].copy(
                reactions = listOf(
                    Reaction(ReactionEmoji.Unicode("🎉"), 3, true),
                    Reaction(ReactionEmoji.Unicode("🚀"), 1, false),
                ),
            )
            list[k + 1] = list[k + 1].copy(editedAt = list[k + 1].createdAt.plusSeconds(90))
        }
        DEV_LINES.forEachIndexed { i, line ->
            push(DEV, listOf(BRUNO, TIM, ANA)[i % 3], line, now - Duration.ofDays(4) + Duration.ofSeconds(i * 400L))
        }
        push(RANDOM, CVETKA, "found this at the flea market, 5 euros", now - Duration.ofDays(5))
        DM_ANA_LINES.forEachIndexed { i, (who, line) -> push(DM_ANA, who, line, now - Duration.ofHours(14) + Duration.ofSeconds(i * 90L)) }
        DM_BRUNO_LINES.forEachIndexed { i, (who, line) -> push(DM_BRUNO, who, line, now - Duration.ofDays(5) + Duration.ofSeconds(i * 120L)) }
        push(20, ANA, "rehearsal moved to Thursday", now - Duration.ofDays(6))
        push(30, BRUNO, "sourdough starter is alive", now - Duration.ofDays(7))
    }

    companion object {
        val DEFAULT_NOW: Instant = Instant.parse("2026-11-22T10:00:00Z")

        const val TIM = 1L
        const val ANA = 2L
        const val BRUNO = 3L
        const val CVETKA = 4L

        const val HIDEOUT = 1L
        const val GARAGE = 2L
        const val COOKING = 3L

        const val GENERAL = 10L
        const val DEV = 11L
        const val RANDOM = 12L
        const val PLANS = 13L
        const val ANNOUNCE = 14L
        const val DESIGN = 15L
        const val MUSIC = 16L
        const val GAMING = 17L
        const val MEMES = 18L
        const val BOOKS = 19L
        const val LOUNGE = 22L
        const val GAME_NIGHT = 23L

        const val BUILDING = 101L
        const val HANGOUT = 102L
        const val ARCHIVE = 103L
        const val VOICE = 104L

        const val DM_ANA = 50L
        const val DM_BRUNO = 51L

        private val UNREAD_DEMO = listOf(
            Triple(DEV, BRUNO, "Gateway backoff now resets on a clean connect, @tim can you review?"),
            Triple(DEV, ANA, "Added a test for the silence timeout too."),
            Triple(DESIGN, CVETKA, "New icon set is in the wiki."),
            Triple(DM_ANA, ANA, "got a minute later tonight?"),
        )

        private fun user(id: Long, name: String, email: String?, admin: Boolean = false) =
            User(id, name, Instant.parse("2026-01-05T12:00:00Z"), isAdmin = admin, email = email)

        private fun guild(id: Long, name: String, owner: Long, code: String) =
            Guild(id, name, owner, code, Instant.parse("2026-01-06T09:00:00Z"))

        private fun text(id: Long, guild: Long, name: String, icon: ChannelIconRef = ChannelIconRef.Default, color: String = "", topic: String = "") =
            Channel(id, guild, name, Instant.parse("2026-01-06T09:00:00Z"), icon = icon, color = color, topic = topic)

        private fun voice(id: Long, guild: Long, name: String, icon: ChannelIconRef, section: Long) =
            Channel(id, guild, name, Instant.parse("2026-01-06T09:00:00Z"), sectionId = section, icon = icon, kind = ChannelKind.Voice)

        private fun role(id: Long, guild: Long, name: String, color: String, position: Int, perms: Long) =
            Role(id, guild, name, color, position, perms, Instant.parse("2026-01-06T09:00:00Z"))
    }
}

/** The newest #general messages, one of each kind of markup. */
val MARKDOWN_LINES = listOf(
    "Release notes are up: **v0.0.3** adds a live gateway, ~~polling~~ and *much* better reconnects.",
    "Draft is in [the wiki](https://example.com/wiki/release), the changelog is at https://example.com/changelog.",
    "> staging disk is at 80%\nThat is the first thing to fix. ||it was the old snapshots||",
    "Plan for Friday:\n- cut the release\n- update the `README`\n- tell @cvetka",
    "```sh\ncargo build --release\n```",
    "__underlined__, `inline code` and _italic_ words, all in one line.",
)

val GENERAL_LINES = listOf(
    "Morning all.",
    "Staging is back up after the migration.",
    "Pushed the fix for the reconnect loop, review welcome.",
    "On it. Is `backoff_ms` capped?",
    "Yep, capped at 30s. @tim can you check the gateway side?",
    "Looks right to me.",
    "Lunch? The new ramen place on Elm opens at noon.",
    "@ana you coming on Friday?",
    "Count me in, I'll bring the speaker.",
    "Did anyone see the build fail on main?",
    "That was me, sorry. Fixed in `a41c9e2`.",
    "No worries.",
    "Reminder: standup moved to 10:15 tomorrow.",
    "Thanks @bruno, noted.",
    "Anyone tried the new release of the router crate?",
    "Not yet. Changelog says it drops `tokio` 0.2 support, which is fine by us.",
    "All green here.",
    "Nice. Merging.",
    "I'm out for an hour, dentist.",
    "Good luck!",
    "The staging database is at 80% disk, I'll clean it up tonight.",
    "Please do. The old snapshots are the culprit.",
    "Will do @cvetka.",
    "Movie night Saturday? I can host.",
    "Yes please.",
    "Which one though? Last time we argued for an hour.",
    "Vote in #plans when it exists.",
    "Ha. Fair.",
    "Quick one: does the invite code expire?",
    "No, but the owner can rotate it.",
    "Makes sense.",
    "The deploy script now prints the commit it ships. Small thing but it saved me twice.",
    "Love that.",
    "Coffee machine is broken again, don't trust it.",
)

val DEV_LINES = listOf(
    "Gateway backoff now resets on a clean connect.",
    "Can you add a test for the silence timeout?",
    "Added, plus one for jitter bounds.",
    "Heartbeat interval is 30s, so the dead-line is 75s.",
    "@tim the `Page<T>` cursor is exclusive, right?",
    "Yes: `before` returns strictly older ids.",
    "Cool, then the client can keep `oldest_id` as is.",
    "Unread counts are client-side for now.",
    "Server-side read state can come later.",
)

val DM_ANA_LINES = listOf(
    FakeBackend.ANA to "hey, got a minute?",
    FakeBackend.TIM to "sure, what's up",
    FakeBackend.ANA to "I'm stuck on the invite flow, can you look at it tonight?",
    FakeBackend.TIM to "yep. I'll ping you when it's pushed",
    FakeBackend.ANA to "perfect, thank you!",
)

val DM_BRUNO_LINES = listOf(
    FakeBackend.BRUNO to "are we still on for Saturday?",
    FakeBackend.TIM to "yes, my place at 7",
    FakeBackend.BRUNO to "I'll bring the good bread",
)
