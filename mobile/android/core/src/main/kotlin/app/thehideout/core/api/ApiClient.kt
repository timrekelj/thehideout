package app.thehideout.core.api

import app.thehideout.core.auth.InvitePreview
import app.thehideout.core.auth.PasskeyCeremony
import app.thehideout.core.auth.PasskeyFinish
import app.thehideout.core.auth.PasskeyRegistered
import app.thehideout.core.model.*
import kotlinx.coroutines.flow.Flow

data class Session(val token: String, val me: User)

sealed interface LoginResult {
    data class SignedIn(val session: Session) : LoginResult
    /** The first factor passed; answer the challenge (valid for 5 minutes). */
    data class SecondFactor(val challengeId: String, val methods: Set<app.thehideout.core.auth.SecondFactorMethod>, val expiresAt: java.time.Instant) : LoginResult
}

data class Me(val user: User, val termsUpdateRequired: String?, val termsVersion: String)

/** Everything the app needs on launch (and again after a gap in the event stream). */
data class Initial(
    val me: User,
    val guilds: List<Guild>,
    val channels: List<Channel>,
    val layouts: Map<Long, GuildLayout>,
    val dms: List<DmChannel>,
)

/** A page of messages, newest first as the API returns it. */
data class MessagePage(val items: List<Message>, val hasMore: Boolean)

data class GuildMembers(val users: List<User>, val members: List<Member>)
data class GuildRoles(val roles: List<Role>, val version: Long)
data class GuildChannels(val channels: List<Channel>, val layout: GuildLayout)
data class SectionResult(val section: Section, val layout: GuildLayout)

/**
 * The server, as the store sees it. [ConnectApiClient] is the real thing; `FakeBackend` is the
 * in-memory twin used by previews, screenshots, the gallery and the `fake=true` launch flag.
 * Every method throws [AppError].
 */
interface ApiClient {
    suspend fun login(username: String, password: String, deviceName: String): LoginResult
    suspend fun me(): Me
    suspend fun loadInitial(): Initial
    suspend fun listChannels(guild: Long): GuildChannels
    suspend fun listMessages(channel: Long, before: Long?, limit: Int = PAGE): MessagePage
    suspend fun sendMessage(channel: Long, content: String, nonce: String, attachmentIds: List<Long> = emptyList()): Message
    /**
     * Streams [source] as a new pending attachment of [channel] over plain HTTP (not Connect), so
     * the body is never buffered whole; [progress] reports bytes sent as chunks are written. Link
     * the result with [sendMessage] within an hour (`attachment.proto`). Throws [AppError];
     * [AppError.TooLarge] when the server refuses the size (413).
     */
    suspend fun uploadAttachment(channel: Long, filename: String, contentType: String, size: Long, source: UploadSource, progress: (Long) -> Unit): PendingAttachment
    /** Opens a DM with [userId], or returns the existing one (idempotent). */
    suspend fun openDm(userId: Long): DmChannel
    suspend fun listMembers(guild: Long): GuildMembers
    suspend fun listRoles(guild: Long): GuildRoles
    /** The guild's custom emojis, ordered by name (MSG-09); mutations arrive as `emojis_updated`. */
    suspend fun listEmojis(guild: Long): List<Emoji>
    suspend fun addReaction(channel: Long, message: Long, emoji: ReactionEmoji)
    suspend fun removeReaction(channel: Long, message: Long, emoji: ReactionEmoji)
    /** Replaces the content of the caller's own message. */
    suspend fun editMessage(channel: Long, message: Long, content: String): Message
    /** The author, or MANAGE_MESSAGES in a guild channel. */
    suspend fun deleteMessage(channel: Long, message: Long)
    suspend fun logout()

    // ---- management: create/join servers, channels, sections, invites (phase 4a) ---------------

    /** Creates a guild owned by the caller; the server auto-creates its `general` channel. */
    suspend fun createGuild(name: String): Guild
    /** Joins the guild an 8-character invite code (`XXXX-XXXX`) points to. */
    suspend fun joinGuild(code: String): Guild
    /** Creates a text or voice channel; MANAGE_CHANNELS required. */
    suspend fun createChannel(guild: Long, name: String, kind: ChannelKind, sectionId: Long?): Channel
    /** Creates a section, appended last; MANAGE_CHANNELS required. */
    suspend fun createSection(guild: Long, name: String): SectionResult
    /** Renames a section; MANAGE_CHANNELS required. */
    suspend fun renameSection(guild: Long, section: Long, name: String): SectionResult
    /** Deletes a section; its channels move to the top level. MANAGE_CHANNELS required. */
    suspend fun deleteSection(guild: Long, section: Long): GuildLayout
    /** Replaces the guild's invite code; the old one stops working. MANAGE_SERVER required. */
    suspend fun rotateInviteCode(guild: Long): String
    /** Invites someone by email into a guild; requires INVITE_PEOPLE there. */
    suspend fun createInvite(email: String, guildId: Long): Invite
    /** This guild's invites (code/link unset, see `Invite`); requires INVITE_PEOPLE. */
    suspend fun listInvites(guildId: Long): List<Invite>
    /** Withdraws a pending invite. */
    suspend fun revokeInvite(inviteId: Long): Invite
    /** Emails a pending (or expired) invite again with a fresh code, link and expiry. */
    suspend fun resendInvite(inviteId: Long): Invite

    // ---- authentication and onboarding (the unauthenticated RPCs; the rest need the token) ----

    /** Emails a one-time sign-in code; always succeeds unless rate limited. */
    suspend fun requestEmailCode(email: String)
    /** Signs in with an emailed code; may answer with a second-factor challenge. */
    suspend fun verifyEmailCode(email: String, code: String, deviceName: String): LoginResult
    /** Answers a second-factor challenge with a TOTP or a recovery code. */
    suspend fun completeSecondFactor(challengeId: String, totpCode: String, recoveryCode: String): Session
    /** Starts answering a second-factor challenge with a passkey. */
    suspend fun beginSecondFactorPasskey(challengeId: String): PasskeyCeremony
    /** Finishes it: a session, or `pending` while the ceremony page runs (poll with ""). */
    suspend fun finishSecondFactorPasskey(ceremonyId: String, credentialJson: String): PasskeyFinish
    /** Emails a password-reset code; always succeeds unless rate limited. */
    suspend fun requestPasswordReset(email: String)
    /** Sets a new password with the emailed code; signs out every session. */
    suspend fun resetPassword(email: String, code: String, newPassword: String)
    /** Starts signing in with a passkey. */
    suspend fun beginPasskeyLogin(username: String, deviceName: String): PasskeyCeremony
    /** Finishes it: a session, or `pending` while the ceremony page runs (poll with ""). */
    suspend fun finishPasskeyLogin(ceremonyId: String, credentialJson: String): PasskeyFinish
    /** Starts adding a passkey to the signed-in account. */
    suspend fun beginPasskeyRegistration(name: String): PasskeyCeremony
    /** Finishes adding one: the passkey, or `pending`. */
    suspend fun finishPasskeyRegistration(ceremonyId: String, credentialJson: String): PasskeyRegistered
    /** What an invite (its code or link token) is for. */
    suspend fun lookupInvite(tokenOrCode: String): InvitePreview
    /** Creates the invite's account and its first session. */
    suspend fun acceptInvite(
        tokenOrCode: String,
        username: String,
        password: String?,
        deviceName: String,
        acceptedTermsVersion: String,
        ageConfirmed: Boolean,
    ): Session
    /** Accepts the current terms version. */
    suspend fun acceptTerms(version: String)

    /**
     * One subscription to the event stream: emits until the stream ends, then completes; fails with
     * [AppError] when it breaks. [app.thehideout.core.gateway.GatewayStream] owns reconnecting.
     */
    fun subscribe(): Flow<GatewayEvent>

    companion object {
        const val PAGE = 50
    }
}
