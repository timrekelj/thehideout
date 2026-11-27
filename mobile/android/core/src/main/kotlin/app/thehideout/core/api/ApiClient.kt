package app.thehideout.core.api

import app.thehideout.core.auth.InvitePreview
import app.thehideout.core.auth.PasskeyCeremony
import app.thehideout.core.auth.PasskeyFinish
import app.thehideout.core.auth.PasskeyRegistered
import app.thehideout.core.model.*
import app.thehideout.core.security.DeviceSession
import app.thehideout.core.security.Passkey
import app.thehideout.core.security.SecurityInfo
import app.thehideout.core.security.TotpSetup
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
data class RoleResult(val role: Role, val version: Long)

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
    /** Joins the guild its invite code points to (server-generated, case-sensitive, no fixed shape). */
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

    // ---- management: server settings, roles, moderation, emojis (phase 4b-1) -------------------

    /** Renames a guild; MANAGE_SERVER required. */
    suspend fun updateGuildName(guild: Long, name: String): Guild
    /** Sets a guild's icon (server crops to a centre square and resizes); MANAGE_SERVER required. */
    suspend fun uploadGuildIcon(guild: Long, data: ByteArray, contentType: String): Guild
    /** Removes a guild's icon (no-op if none); MANAGE_SERVER required. */
    suspend fun removeGuildIcon(guild: Long): Guild
    /** Deletes a guild with everything in it; owner only, `confirmName` must equal the guild's name. */
    suspend fun deleteGuild(guild: Long, confirmName: String)
    /** Hands the guild to another member; owner only, `confirmName` must equal the guild's name. */
    suspend fun transferOwnership(guild: Long, user: Long, confirmName: String): Guild
    /** Leaves a guild; the owner cannot (transfer or delete first). */
    suspend fun leaveGuild(guild: Long)
    /** Removes a member; KICK_MEMBERS and a top role above theirs required. */
    suspend fun kickMember(guild: Long, user: Long)
    /** Bans a member (or former member); BAN_MEMBERS and the same hierarchy rule as [kickMember].
     * [deleteMessageSeconds] also deletes their recent messages (at most 604800, 7 days). */
    suspend fun banMember(guild: Long, user: Long, reason: String, deleteMessageSeconds: Int?)
    /** Lifts a ban; BAN_MEMBERS required. */
    suspend fun unbanMember(guild: Long, user: Long)
    /** The guild's bans, newest first; BAN_MEMBERS required. */
    suspend fun listBans(guild: Long): List<Ban>
    /** Creates a role at the bottom of the hierarchy; MANAGE_ROLES required. */
    suspend fun createRole(guild: Long, name: String, color: String, permissions: Long): RoleResult
    /** Updates a role; unset fields are left unchanged. MANAGE_ROLES required. */
    suspend fun updateRole(guild: Long, roleId: Long, name: String?, color: String?, permissions: Long?): Role
    /** Deletes a role and removes it from every member; MANAGE_ROLES required. Returns the new roles version. */
    suspend fun deleteRole(guild: Long, roleId: Long): Long
    /** Sets the full role order; MANAGE_ROLES required. `ABORTED` if [expectedVersion] is stale. */
    suspend fun reorderRoles(guild: Long, expectedVersion: Long, roleIds: List<Long>): GuildRoles
    /** Replaces the set of roles held by a member; MANAGE_ROLES required. */
    suspend fun setMemberRoles(guild: Long, userId: Long, roleIds: List<Long>): Member
    /** Creates a custom emoji from at most 2 MiB of PNG/JPEG/WebP/GIF; MANAGE_EMOJIS required. */
    suspend fun createEmoji(guild: Long, name: String, data: ByteArray, contentType: String): Emoji
    /** Renames a custom emoji; MANAGE_EMOJIS required. */
    suspend fun renameEmoji(guild: Long, emojiId: Long, name: String): Emoji
    /** Deletes a custom emoji; existing reactions/messages using it render as text. MANAGE_EMOJIS required. */
    suspend fun deleteEmoji(guild: Long, emojiId: Long)

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

    // ---- account security (phase 5): email, password, two-factor, passkeys, devices -------------

    /** How the caller's account is protected, for the Security page's section summaries. */
    suspend fun getSecurity(): SecurityInfo
    /** Changes the caller's password (empty [currentPassword] for accounts without one yet); signs out their other sessions. */
    suspend fun changePassword(currentPassword: String, newPassword: String)
    /** Starts changing (or adding) the caller's email: emails a code to [newEmail]. */
    suspend fun changeEmail(newEmail: String, currentPassword: String)
    /** Finishes [changeEmail] with the emailed code. */
    suspend fun confirmEmailChange(code: String): User
    /** Starts enabling an authenticator app: a fresh secret, valid for 10 minutes. */
    suspend fun beginTotpSetup(): TotpSetup
    /** Enables the authenticator app with a current code from it; the 10 recovery codes, shown only now. */
    suspend fun confirmTotp(code: String): List<String>
    /** Disables the authenticator app (and deletes the recovery codes) with a code from it or a recovery code. */
    suspend fun disableTotp(code: String)
    /** Replaces the recovery codes (old ones stop working); the new 10, shown only now. */
    suspend fun regenerateRecoveryCodes(code: String): List<String>
    /** The caller's passkeys, oldest first. */
    suspend fun listPasskeys(): List<Passkey>
    /** Renames one of the caller's passkeys. */
    suspend fun renamePasskey(id: Long, name: String): Passkey
    /** Deletes one of the caller's passkeys. */
    suspend fun deletePasskey(id: Long)
    /** The caller's signed-in sessions (devices), newest first. */
    suspend fun listSessions(): List<DeviceSession>
    /** Signs out one of the caller's sessions (may be the current one). */
    suspend fun revokeSession(id: Long)
    /** Signs out every session of the caller except the current one; returns how many. */
    suspend fun revokeOtherSessions(): Int
    /** Emails a 6-digit code DeleteAccount accepts instead of a password (passwordless accounts). */
    suspend fun requestAccountDeletionCode()
    /**
     * Deletes the caller's account: [password], or [emailCode] for accounts without one, plus
     * [totpCode] when two-factor authentication is on. Throws [AppError.OwnsGuilds] while the
     * caller still owns guilds.
     */
    suspend fun deleteAccount(password: String?, emailCode: String?, totpCode: String?)
    /** Queues a data export; the masked address the download link will be emailed to. Rate limited to once per 24h. */
    suspend fun exportData(): String

    // ---- notification settings (MOB-02) -----------------------------------------------------------

    /** The caller's notification settings (defaults when never changed). */
    suspend fun getNotificationSettings(): NotificationSettings
    /** Changes settings; unset fields are unchanged. Returns the full settings. */
    suspend fun updateNotificationSettings(
        dmsEnabled: Boolean? = null,
        guildDefault: NotificationLevel? = null,
        showPreviews: Boolean? = null,
        guilds: List<NotificationOverride> = emptyList(),
        channels: List<NotificationOverride> = emptyList(),
    ): NotificationSettings

    /**
     * One subscription to the event stream: emits until the stream ends, then completes; fails with
     * [AppError] when it breaks. [app.thehideout.core.gateway.GatewayStream] owns reconnecting.
     */
    fun subscribe(): Flow<GatewayEvent>

    companion object {
        const val PAGE = 50
    }
}
