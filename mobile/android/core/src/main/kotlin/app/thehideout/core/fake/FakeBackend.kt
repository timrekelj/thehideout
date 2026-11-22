package app.thehideout.core.fake

import app.thehideout.core.api.*
import app.thehideout.core.auth.InvitePreview
import app.thehideout.core.auth.PasskeyCeremony
import app.thehideout.core.auth.PasskeyFinish
import app.thehideout.core.auth.PasskeyRegistered
import app.thehideout.core.auth.SecondFactorMethod
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
    private val users = mutableMapOf(
        TIM to user(TIM, "tim", "tim@example.com"),
        ANA to user(ANA, "ana", "ana@example.com"),
        BRUNO to user(BRUNO, "bruno", null),
        CVETKA to user(CVETKA, "cvetka", "cvetka@example.com", admin = true),
        DORA to user(DORA, "dora", "dora@example.com"),
    )
    private var currentUserId = TIM
    private val me: User get() = users.getValue(currentUserId)

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
    /** The next [n] edits fail with a network error, independent of [failSends]. */
    var failEdits: Int = 0

    /** Every message ever sent through [sendMessage], oldest first. */
    val sent: MutableList<Message> = mutableListOf()

    // ---- the fake server's accounts (desktop's `fake_account.rs`, with the same fixed secrets) ----

    /** The one valid emailed code and authenticator code. */
    var emailCode: String = "123456"
    /** The terms version the fake server requires; [termsOutdated] makes `GetMe` ask for a re-accept. */
    var termsVersion: String = "2026-11-22"
    var termsOutdated: Boolean = false
    /** Passwords that are refused ("wrong" and the empty one), like the desktop fake. */
    var wrongPassword: String = "wrong"
    /** Users with an authenticator app (their sign-in asks for the second factor) and their recovery codes. */
    val totpUsers = mutableMapOf(DORA to RECOVERY_CODES.toMutableList())
    /** Users whose passkey answers a second-factor challenge. */
    val secondFactorPasskeyUsers = setOf(DORA)
    /** Users who have a passkey for sign-in. */
    val passkeyUsers = mutableSetOf(TIM)
    /** Second-factor challenges: id, username, wrong answers so far (five kills the challenge). */
    private val challenges = mutableListOf<Triple<String, String, Int>>()
    private var nextChallenge = 0L
    /** Passkey ceremonies: id -> how many empty-credential polls still answer `pending`. */
    private val ceremonies = mutableMapOf<String, Int>()
    private var nextCeremony = 0L
    private data class FakeInvite(val email: String, val code: String, val token: String, val status: InviteStatus, val inviter: User?, val guildNames: List<String>, val expiresAt: Instant)
    private val invites = listOf(
        FakeInvite("erin@example.com", "WXYZ-2345", "tok-wxyz", InviteStatus.Pending, users.getValue(TIM), listOf("The Hideout"), now + Duration.ofDays(6)),
        FakeInvite("finn@example.com", "EXPD-0001", "tok-expd", InviteStatus.Expired, users.getValue(TIM), listOf("The Hideout"), now - Duration.ofDays(2)),
        FakeInvite("gus@example.com", "RVKD-0001", "tok-rvkd", InviteStatus.Revoked, users.getValue(TIM), listOf("The Hideout"), now + Duration.ofDays(4)),
        FakeInvite("ana@example.com", "USED-0001", "tok-used", InviteStatus.Accepted, null, listOf("The Hideout"), now - Duration.ofDays(53)),
    )
    private var nextUserId = 100L

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
        val u = byLogin(username) ?: throw AppError.Unauthenticated
        if (password.isEmpty() || password == wrongPassword) throw AppError.Unauthenticated
        return firstFactor(u)
    }

    override suspend fun me(): Me {
        pause()
        return Me(me, if (termsOutdated) termsVersion else null, termsVersion)
    }

    override suspend fun loadInitial(): Initial {
        pause()
        return Initial(me, guilds, channels.toList(), layouts.toMap(), openedDms.toList())
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

    private val openedDms = mutableListOf<DmChannel>().apply { addAll(dms) }
    private var nextDmId = 60L

    override suspend fun openDm(userId: Long): DmChannel {
        pause()
        val peer = users[userId] ?: throw AppError.NotFound
        if (userId == currentUserId) throw AppError.InvalidArgument("cannot DM yourself")
        openedDms.firstOrNull { dm -> dm.participants.any { it.id == userId } }?.let { return it }
        val dm = DmChannel(nextDmId++, listOf(me, peer), now)
        openedDms += dm
        emit(GatewayEvent.DmOpened(dm))
        return dm
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

    override suspend fun editMessage(channel: Long, message: Long, content: String): Message {
        pause()
        if (failEdits > 0) {
            failEdits--
            throw AppError.Unavailable("connection reset")
        }
        val list = messages[channel] ?: throw AppError.NotFound
        val i = list.indexOfFirst { it.id == message }
        if (i < 0) throw AppError.NotFound
        if (list[i].authorId != currentUserId) throw AppError.PermissionDenied
        val edited = list[i].copy(content = content, editedAt = now)
        list[i] = edited
        emit(GatewayEvent.MessageUpdated(edited))
        return edited
    }

    override suspend fun deleteMessage(channel: Long, message: Long) {
        pause()
        val list = messages[channel] ?: throw AppError.NotFound
        val i = list.indexOfFirst { it.id == message }
        if (i < 0) throw AppError.NotFound
        val m = list[i]
        val guildId = channels.firstOrNull { it.id == channel }?.guildId
        val mine = m.authorId == currentUserId
        val moderator = guildId != null && Perm.has(effectivePerms(guildId), Perm.MANAGE_MESSAGES)
        if (!mine && !moderator) throw AppError.PermissionDenied
        list.removeAt(i)
        emit(GatewayEvent.MessageDeleted(channel, message))
    }

    /** What [currentUserId] may do in [guildId], same rule the reducer uses. */
    private fun effectivePerms(guildId: Long): Long {
        val guild = guilds.firstOrNull { it.id == guildId } ?: return 0L
        val mine = memberRoles[guildId]?.get(currentUserId).orEmpty()
        return Perm.effective(guild, currentUserId, mine, roles[guildId].orEmpty())
    }

    override suspend fun logout() = Unit

    // ---- authentication and onboarding (the rules of desktop's `fake_account.rs`) -----------------

    /** A username or an email, as `Login` takes it. */
    private fun byLogin(login: String): User? {
        val l = login.trim()
        return if (l.contains('@')) users.values.firstOrNull { it.email.equals(l, ignoreCase = true) }
        else users.values.firstOrNull { it.username.equals(l, ignoreCase = true) }
    }

    /** A first factor passed: a session, or the second factor when the account has an app. */
    private fun firstFactor(u: User): LoginResult {
        if (totpUsers.containsKey(u.id)) {
            nextChallenge++
            val id = "fake-challenge-$nextChallenge"
            challenges += Triple(id, u.username.lowercase(), 0)
            val methods = buildSet {
                add(SecondFactorMethod.Totp)
                add(SecondFactorMethod.RecoveryCode)
                if (secondFactorPasskeyUsers.contains(u.id)) add(SecondFactorMethod.Passkey)
            }
            return LoginResult.SecondFactor(id, methods, now.plus(Duration.ofMinutes(5)))
        }
        return LoginResult.SignedIn(sessionFor(u))
    }

    private fun sessionFor(u: User) = Session("fake-token-${u.id}", u)

    /** `XXXX-XXXX` without case, dashes or spaces, as the server forgives them. */
    private fun plain(code: String) = code.filter { it.isLetterOrDigit() }.uppercase()

    private fun findInvite(codeOrLink: String): FakeInvite? {
        val raw = codeOrLink.trim()
        val token = raw.substringAfterLast('/')
        return invites.firstOrNull { it.token == token || plain(it.code) == plain(raw) }
    }

    /** The masked form of an email: `e***@example.com`. */
    private fun mask(email: String): String =
        email.split('@').let { if (it.size == 2) "${it[0].firstOrNull() ?: '*'}***@${it[1]}" else "***" }

    private fun newCeremony(pollsBeforeDone: Int = 1): PasskeyCeremony {
        nextCeremony++
        val id = "fake-ceremony-$nextCeremony"
        ceremonies[id] = pollsBeforeDone
        return PasskeyCeremony(id, "http://127.0.0.1:8080/auth/passkey?ceremony=$id", """{"publicKey":{"challenge":"ZmFrZQ"}}""")
    }

    /** Finishes a ceremony: a non-empty credential answers at once, an empty one polls. */
    private fun finishCeremony(ceremonyId: String, credentialJson: String, session: () -> Session): PasskeyFinish {
        if (credentialJson.isNotEmpty()) return PasskeyFinish(session(), false)
        val left = (ceremonies[ceremonyId] ?: 0) - 1
        ceremonies[ceremonyId] = left
        return if (left < 0) PasskeyFinish(session(), false) else PasskeyFinish(null, true)
    }

    override suspend fun requestEmailCode(email: String) {
        pause()
    }

    override suspend fun verifyEmailCode(email: String, code: String, deviceName: String): LoginResult {
        pause()
        val u = users.values.firstOrNull { it.email.equals(email.trim(), ignoreCase = true) } ?: throw AppError.Unauthenticated
        if (code.trim() != emailCode) throw AppError.Unauthenticated
        return firstFactor(u)
    }

    override suspend fun completeSecondFactor(challengeId: String, totpCode: String, recoveryCode: String): Session {
        pause()
        val i = challenges.indexOfFirst { it.first == challengeId }
        if (i < 0) throw AppError.CodeExpired
        val (id, name, tries) = challenges[i]
        val u = users.values.first { it.username.lowercase() == name }
        val code = if (recoveryCode.isNotEmpty()) recoveryCode else totpCode
        val ok = if (recoveryCode.isNotEmpty()) {
            val left = totpUsers[u.id]
            val at = left?.indexOfFirst { plain(it) == plain(code) } ?: -1
            if (at >= 0) {
                left!!.removeAt(at)
                true
            } else false
        } else code.trim() == emailCode
        if (!ok) {
            val attempts = tries + 1
            challenges[i] = Triple(id, name, attempts)
            if (attempts >= SECOND_FACTOR_TRIES) {
                challenges.removeAt(i)
                throw AppError.TooManyAttempts
            }
            throw AppError.WrongCode(SECOND_FACTOR_TRIES - attempts)
        }
        challenges.removeAt(i)
        return sessionFor(u)
    }

    override suspend fun beginSecondFactorPasskey(challengeId: String): PasskeyCeremony {
        pause()
        val i = challenges.indexOfFirst { it.first == challengeId }
        if (i < 0) throw AppError.CodeExpired
        val u = users.values.first { it.username.lowercase() == challenges[i].second }
        if (!secondFactorPasskeyUsers.contains(u.id)) throw AppError.Precondition("no passkey for this account")
        challenges.removeAt(i)
        return newCeremony()
    }

    override suspend fun finishSecondFactorPasskey(ceremonyId: String, credentialJson: String): PasskeyFinish {
        pause()
        val u = users.getValue(currentUserId)
        return finishCeremony(ceremonyId, credentialJson) { sessionFor(users.values.first { passkeyUsers.contains(it.id) || secondFactorPasskeyUsers.contains(it.id) }) }
    }

    override suspend fun requestPasswordReset(email: String) {
        pause()
    }

    override suspend fun resetPassword(email: String, code: String, newPassword: String) {
        pause()
        val known = users.values.any { it.email.equals(email.trim(), ignoreCase = true) }
        if (!known || code.trim() != emailCode) throw AppError.Unauthenticated
        if (newPassword.length < MIN_PASSWORD) throw AppError.InvalidArgument("password must be at least 8 characters")
    }

    override suspend fun beginPasskeyLogin(username: String, deviceName: String): PasskeyCeremony {
        pause()
        val u = byLogin(username) ?: throw AppError.NotFound
        if (!passkeyUsers.contains(u.id)) throw AppError.Precondition("this account has no passkey")
        return newCeremony()
    }

    override suspend fun finishPasskeyLogin(ceremonyId: String, credentialJson: String): PasskeyFinish {
        pause()
        val u = users.values.first { passkeyUsers.contains(it.id) }
        return finishCeremony(ceremonyId, credentialJson) { sessionFor(u) }
    }

    override suspend fun beginPasskeyRegistration(name: String): PasskeyCeremony {
        pause()
        return newCeremony()
    }

    override suspend fun finishPasskeyRegistration(ceremonyId: String, credentialJson: String): PasskeyRegistered {
        pause()
        if (credentialJson.isEmpty()) {
            val left = (ceremonies[ceremonyId] ?: 0) - 1
            ceremonies[ceremonyId] = left
            if (left >= 0) return PasskeyRegistered("", true)
        }
        passkeyUsers += currentUserId
        return PasskeyRegistered("Passkey", false)
    }

    override suspend fun lookupInvite(tokenOrCode: String): InvitePreview {
        pause()
        val i = findInvite(tokenOrCode) ?: throw AppError.NotFound
        return InvitePreview(
            maskedEmail = mask(i.email),
            inviter = i.inviter,
            guildNames = i.guildNames,
            expiresAt = i.expiresAt,
            status = i.status,
            termsVersion = termsVersion,
            minimumAge = MINIMUM_AGE,
        )
    }

    override suspend fun acceptInvite(
        tokenOrCode: String,
        username: String,
        password: String?,
        deviceName: String,
        acceptedTermsVersion: String,
        ageConfirmed: Boolean,
    ): Session {
        pause()
        val i = findInvite(tokenOrCode) ?: throw AppError.NotFound
        when (i.status) {
            InviteStatus.Pending -> {}
            InviteStatus.Accepted -> throw AppError.Precondition("this invite was already used")
            InviteStatus.Expired -> throw AppError.Precondition("this invite has expired")
            else -> throw AppError.Precondition("this invite was revoked")
        }
        if (acceptedTermsVersion != termsVersion) throw AppError.TermsUpdateRequired
        if (!ageConfirmed) throw AppError.InvalidArgument("you must confirm that you are at least $MINIMUM_AGE years old")
        val name = username.trim()
        if (name.length !in 3..32 || !name.all { it.isLetterOrDigit() || it == '_' || it == '.' }) {
            throw AppError.InvalidArgument("usernames are 3 to 32 letters, digits, dots or underscores")
        }
        if (users.values.any { it.username.equals(name, ignoreCase = true) }) throw AppError.AlreadyExists
        if (password != null && password.length < MIN_PASSWORD) throw AppError.InvalidArgument("password must be at least 8 characters")
        val id = nextUserId++
        val u = user(id, name.lowercase(), i.email)
        users[id] = u
        currentUserId = id
        return sessionFor(u)
    }

    override suspend fun acceptTerms(version: String) {
        pause()
        if (version != termsVersion) throw AppError.Precondition("the current terms version is $termsVersion")
        termsOutdated = false
    }

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

        /** The account with an authenticator app (signing in as her asks for the second factor). */
        const val DORA = 5L

        /** Wrong answers a second-factor challenge takes before it dies. */
        const val SECOND_FACTOR_TRIES = 5
        const val MIN_PASSWORD = 8
        const val MINIMUM_AGE = 16

        /** Dora's recovery codes (each works once). */
        val RECOVERY_CODES = listOf(
            "7KQ2-M9XD", "4HTW-PZ3N", "C8RB-V6YE", "J2LF-Q7KA", "N5XG-D4MS",
            "R9PC-W2HT", "T3YE-K8VB", "V6MA-F5QR", "X2ND-H9LC", "Z7GK-B3WP",
        )

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
