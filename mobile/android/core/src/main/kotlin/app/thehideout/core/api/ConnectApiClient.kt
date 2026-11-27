package app.thehideout.core.api

import app.thehideout.core.auth.InvitePreview
import app.thehideout.core.auth.PasskeyCeremony
import app.thehideout.core.auth.PasskeyFinish
import app.thehideout.core.auth.PasskeyRegistered
import app.thehideout.core.auth.SecondFactorMethod
import app.thehideout.core.model.*
import com.connectrpc.ConnectException
import com.connectrpc.MethodSpec
import com.connectrpc.ProtocolClientConfig
import com.connectrpc.ResponseMessage
import com.connectrpc.StreamType
import com.connectrpc.extensions.GoogleJavaLiteProtobufStrategy
import com.connectrpc.impl.ProtocolClient
import com.connectrpc.okhttp.ConnectOkHttpClient
import com.connectrpc.protocols.NetworkProtocol
import com.thehideout.v1.AcceptInviteRequest
import com.thehideout.v1.AcceptTermsRequest
import com.thehideout.v1.AddReactionRequest
import com.thehideout.v1.AuthServiceClient
import com.thehideout.v1.BeginPasskeyLoginRequest
import com.thehideout.v1.BeginPasskeyRegistrationRequest
import com.thehideout.v1.BeginSecondFactorPasskeyRequest
import com.thehideout.v1.CompleteSecondFactorRequest
import com.thehideout.v1.DeleteMessageRequest
import com.thehideout.v1.DmServiceClient
import com.thehideout.v1.EditMessageRequest
import com.thehideout.v1.EmojiServiceClient
import com.thehideout.v1.ListEmojisRequest
import com.thehideout.v1.FinishPasskeyLoginRequest
import com.thehideout.v1.FinishPasskeyRegistrationRequest
import com.thehideout.v1.FinishSecondFactorPasskeyRequest
import com.thehideout.v1.GatewayServiceClient
import com.thehideout.v1.GetMeRequest
import com.thehideout.v1.GuildServiceClient
import com.thehideout.v1.InviteServiceClient
import com.thehideout.v1.ListChannelsRequest
import com.thehideout.v1.ListDmsRequest
import com.thehideout.v1.ListGuildMembersRequest
import com.thehideout.v1.ListMessagesRequest
import com.thehideout.v1.ListMyGuildsRequest
import com.thehideout.v1.ListRolesRequest
import com.thehideout.v1.LoginRequest
import com.thehideout.v1.LookupInviteRequest
import com.thehideout.v1.MediaServiceClient
import com.thehideout.v1.MessageServiceClient
import com.thehideout.v1.RemoveReactionRequest
import com.thehideout.v1.RequestEmailCodeRequest
import com.thehideout.v1.RequestPasswordResetRequest
import com.thehideout.v1.ResetPasswordRequest
import com.thehideout.v1.RoleServiceClient
import com.thehideout.v1.SendMessageRequest
import com.thehideout.v1.SubscribeRequest
import com.thehideout.v1.VerifyEmailCodeRequest
import com.google.protobuf.ByteString
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.async
import kotlinx.coroutines.suspendCancellableCoroutine
import okhttp3.Call
import okhttp3.Callback
import okhttp3.Interceptor
import okhttp3.MediaType.Companion.toMediaTypeOrNull
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody
import okhttp3.Response
import okio.BufferedSink
import java.io.IOException
import java.util.concurrent.TimeUnit
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException
import kotlin.time.Duration
import kotlin.time.Duration.Companion.seconds

/** What the real client needs to know about where it runs. */
class ApiConfig(
    /** `https://host` or `http://10.0.2.2:8080`, no trailing slash. */
    val baseUrl: String,
    /** The session token, read per request (so sign-in and sign-out need no new client). */
    val token: () -> String?,
    /** `android/0.1.0 (Android 36)`: sent as `x-thehideout-client` so the server can refuse old builds. */
    val clientHeader: String,
)

/** Builds the shared OkHttp clients: unary calls time out at 30 s; the gateway stream never does (the watchdog does). */
fun buildHttpClients(config: ApiConfig, base: OkHttpClient = OkHttpClient()): Pair<OkHttpClient, OkHttpClient> {
    val headers = Interceptor { chain ->
        val b = chain.request().newBuilder().header("x-thehideout-client", config.clientHeader)
        config.token()?.let { b.header("authorization", "Bearer $it") }
        chain.proceed(b.build())
    }
    val unary = base.newBuilder().addInterceptor(headers).readTimeout(30, TimeUnit.SECONDS).build()
    val stream = base.newBuilder().addInterceptor(headers).readTimeout(0, TimeUnit.SECONDS).build()
    return unary to stream
}

/** Percent-encodes everything except RFC 3986 unreserved characters (desktop's `upload.rs::encode_component`). */
private fun encodeUploadComponent(s: String): String = buildString {
    for (b in s.toByteArray(Charsets.UTF_8)) {
        val c = b.toInt() and 0xFF
        if (c.toChar().isLetterOrDigit() && c < 128 || c.toChar() in "-._~") append(c.toChar())
        else append("%%%02X".format(c))
    }
}

/**
 * connect-kotlin's own default times every call out at 10 s, the gateway's Subscribe stream
 * included: it was being torn down and reopened every ~10 s regardless of the OkHttp stream
 * client's readTimeout(0) and GatewayStream's own 75 s silence watchdog. Unary calls keep that
 * 10 s default; a stream gets `null` so the library schedules no timeout for it at all (its own
 * watchdog is the only thing that can end it) — not `Duration.INFINITE`, which `simpleTimeouts`
 * cannot express (its two parameters are non-null) and which would still schedule a very long
 * real timer rather than none.
 */
internal val apiTimeoutOracle: (MethodSpec<*, *>) -> Duration? = { spec -> if (spec.streamType == StreamType.UNARY) 10.seconds else null }
/** The real [ApiClient]: connect-kotlin over OkHttp, with every failure mapped to [AppError]. */
class ConnectApiClient(private val config: ApiConfig, http: Pair<OkHttpClient, OkHttpClient> = buildHttpClients(config)) : ApiClient {
    /** The server this client was built for (tests check a cold start used the right one). */
    val baseUrl: String get() = config.baseUrl
    /** [baseUrl] trimmed, as the upload URL is built from it. */
    private val uploadBaseUrl = baseUrl.trimEnd('/')
    private val uploadHttp = http.first
    private val client = ProtocolClient(
        ConnectOkHttpClient(http.first, http.second),
        ProtocolClientConfig(
            host = config.baseUrl,
            serializationStrategy = GoogleJavaLiteProtobufStrategy(),
            networkProtocol = NetworkProtocol.CONNECT,
            timeoutOracle = apiTimeoutOracle,
        ),
    )
    private val auth = AuthServiceClient(client)
    private val guilds = GuildServiceClient(client)
    private val dms = DmServiceClient(client)
    private val messages = MessageServiceClient(client)
    private val roles = RoleServiceClient(client)
    private val emojiService = EmojiServiceClient(client)
    private val media = MediaServiceClient(client)
    private val gateway = GatewayServiceClient(client)
    private val invites = InviteServiceClient(client)

    private suspend fun <T> call(block: suspend () -> ResponseMessage<T>): T {
        val r = try {
            block()
        } catch (e: CancellationException) {
            throw e
        } catch (e: ConnectException) {
            throw e.toAppError()
        } catch (e: IOException) {
            throw AppError.Unavailable(e.message.orEmpty())
        }
        return when (r) {
            is ResponseMessage.Success -> r.message
            is ResponseMessage.Failure -> throw r.cause.toAppError()
        }
    }

    override suspend fun login(username: String, password: String, deviceName: String): LoginResult {
        val r = call { auth.login(LoginRequest.newBuilder().setUsername(username).setPassword(password).setDeviceName(deviceName).build()) }
        return loginResult(r.token, if (r.hasUser()) r.user else null, if (r.hasSecondFactorRequired()) r.secondFactorRequired else null)
    }

    override suspend fun me(): Me {
        val r = call { auth.getMe(GetMeRequest.getDefaultInstance()) }
        return Me(r.user.toModel(), if (r.hasTermsUpdateRequired()) r.termsUpdateRequired else null, r.termsVersion)
    }

    override suspend fun loadInitial(): Initial = coroutineScope {
        val me = async { me().user }
        val guildList = async { call { guilds.listMyGuilds(ListMyGuildsRequest.getDefaultInstance()) }.guildsList.map { it.toModel() } }
        val dmList = async { call { dms.listDms(ListDmsRequest.getDefaultInstance()) }.channelsList.map { it.toModel() } }
        val gs = guildList.await()
        val perGuild = gs.map { g -> async { g.id to listChannels(g.id) } }.map { it.await() }
        Initial(
            me = me.await(),
            guilds = gs,
            channels = perGuild.flatMap { it.second.channels },
            layouts = perGuild.associate { it.first to it.second.layout },
            dms = dmList.await(),
        )
    }

    override suspend fun listChannels(guild: Long): GuildChannels {
        val r = call { guilds.listChannels(ListChannelsRequest.newBuilder().setGuildId(guild).build()) }
        return GuildChannels(r.channelsList.map { it.toModel() }, r.layout.toModel())
    }

    override suspend fun listMessages(channel: Long, before: Long?, limit: Int): MessagePage {
        val req = ListMessagesRequest.newBuilder().setChannelId(channel).setLimit(limit)
        if (before != null) req.before = before
        val r = call { messages.listMessages(req.build()) }
        return MessagePage(r.messagesList.map { it.toModel() }, r.hasMore)
    }

    override suspend fun sendMessage(channel: Long, content: String, nonce: String, attachmentIds: List<Long>): Message {
        val req = SendMessageRequest.newBuilder().setChannelId(channel).setContent(content).setNonce(nonce).addAllAttachmentIds(attachmentIds)
        return call { messages.sendMessage(req.build()) }.message.toModel()
    }

    override suspend fun uploadAttachment(channel: Long, filename: String, contentType: String, size: Long, source: UploadSource, progress: (Long) -> Unit): PendingAttachment {
        val url = "$uploadBaseUrl/attachments?channel_id=$channel&filename=${encodeUploadComponent(filename)}"
        val body = object : RequestBody() {
            override fun contentType() = contentType.toMediaTypeOrNull()
            override fun contentLength() = size
            override fun writeTo(sink: BufferedSink) {
                source.open().use { input ->
                    val buffer = ByteArray(64 * 1024)
                    var sent = 0L
                    while (true) {
                        val read = input.read(buffer)
                        if (read == -1) break
                        sink.write(buffer, 0, read)
                        sent += read
                        progress(sent)
                    }
                }
            }
        }
        val request = Request.Builder().url(url).post(body).build()
        return suspendCancellableCoroutine { cont ->
            val call = uploadHttp.newCall(request)
            cont.invokeOnCancellation { call.cancel() }
            call.enqueue(object : Callback {
                override fun onFailure(call: Call, e: IOException) {
                    if (cont.isActive) cont.resumeWithException(AppError.Unavailable(e.message.orEmpty()))
                }

                override fun onResponse(call: Call, response: Response) {
                    // Reading the body can itself throw (truncated stream, reset connection): map that to a
                    // retryable Unavailable and resume, so the waiting coroutine never hangs in this callback.
                    val result = response.use { r ->
                        runCatching { r.body.bytes() }.fold(
                            onSuccess = { bytes ->
                                if (!r.isSuccessful) Result.failure(httpAttachmentError(r.code, bytes)) else runCatching { parsePendingAttachment(bytes) }
                            },
                            onFailure = { e -> Result.failure(AppError.Unavailable(e.message ?: "couldn't read the upload response")) },
                        )
                    }
                    if (!cont.isActive) return
                    result.fold(cont::resume, cont::resumeWithException)
                }
            })
        }
    }

    override suspend fun openDm(userId: Long): DmChannel {
        val r = call { dms.openDm(com.thehideout.v1.OpenDmRequest.newBuilder().setUserId(userId).build()) }
        return r.channel.toModel()
    }

    override suspend fun listMembers(guild: Long): GuildMembers {
        val r = call { guilds.listGuildMembers(ListGuildMembersRequest.newBuilder().setGuildId(guild).build()) }
        return GuildMembers(r.usersList.map { it.toModel() }, r.membersList.map { it.toModel() })
    }

    override suspend fun listRoles(guild: Long): GuildRoles {
        val r = call { roles.listRoles(ListRolesRequest.newBuilder().setGuildId(guild).build()) }
        return GuildRoles(r.rolesList.map { it.toModel() }, r.version)
    }

    override suspend fun listEmojis(guild: Long): List<Emoji> {
        val r = call { emojiService.listEmojis(ListEmojisRequest.newBuilder().setGuildId(guild).build()) }
        return r.emojisList.map { it.toModel() }
    }

    override suspend fun addReaction(channel: Long, message: Long, emoji: ReactionEmoji) {
        call { messages.addReaction(AddReactionRequest.newBuilder().setChannelId(channel).setMessageId(message).setEmoji(emoji.toProto()).build()) }
    }

    override suspend fun removeReaction(channel: Long, message: Long, emoji: ReactionEmoji) {
        call { messages.removeReaction(RemoveReactionRequest.newBuilder().setChannelId(channel).setMessageId(message).setEmoji(emoji.toProto()).build()) }
    }

    override suspend fun editMessage(channel: Long, message: Long, content: String): Message {
        val req = EditMessageRequest.newBuilder().setChannelId(channel).setMessageId(message).setContent(content)
        return call { messages.editMessage(req.build()) }.message.toModel()
    }

    override suspend fun deleteMessage(channel: Long, message: Long) {
        call { messages.deleteMessage(DeleteMessageRequest.newBuilder().setChannelId(channel).setMessageId(message).build()) }
    }

    override suspend fun logout() = Unit

    // ---- management: create/join servers, channels, sections, invites (phase 4a) ---------------

    override suspend fun createGuild(name: String): Guild {
        val r = call { guilds.createGuild(com.thehideout.v1.CreateGuildRequest.newBuilder().setName(name).build()) }
        return r.guild.toModel()
    }

    override suspend fun joinGuild(code: String): Guild {
        val r = call { guilds.joinGuild(com.thehideout.v1.JoinGuildRequest.newBuilder().setInviteCode(code).build()) }
        return r.guild.toModel()
    }

    override suspend fun createChannel(guild: Long, name: String, kind: ChannelKind, sectionId: Long?): Channel {
        val b = com.thehideout.v1.CreateChannelRequest.newBuilder().setGuildId(guild).setName(name)
            .setKind(if (kind == ChannelKind.Voice) com.thehideout.v1.ChannelKind.CHANNEL_KIND_VOICE else com.thehideout.v1.ChannelKind.CHANNEL_KIND_TEXT)
        sectionId?.let { b.sectionId = it }
        return call { guilds.createChannel(b.build()) }.channel.toModel()
    }

    override suspend fun createSection(guild: Long, name: String): SectionResult {
        val r = call { guilds.createSection(com.thehideout.v1.CreateSectionRequest.newBuilder().setGuildId(guild).setName(name).build()) }
        return SectionResult(r.section.toModel(), r.layout.toModel())
    }

    override suspend fun renameSection(guild: Long, section: Long, name: String): SectionResult {
        val r = call { guilds.updateSection(com.thehideout.v1.UpdateSectionRequest.newBuilder().setGuildId(guild).setSectionId(section).setName(name).build()) }
        return SectionResult(r.section.toModel(), r.layout.toModel())
    }

    override suspend fun deleteSection(guild: Long, section: Long): GuildLayout {
        val r = call { guilds.deleteSection(com.thehideout.v1.DeleteSectionRequest.newBuilder().setGuildId(guild).setSectionId(section).build()) }
        return r.layout.toModel()
    }

    override suspend fun rotateInviteCode(guild: Long): String {
        return call { guilds.rotateInviteCode(com.thehideout.v1.RotateInviteCodeRequest.newBuilder().setGuildId(guild).build()) }.inviteCode
    }

    override suspend fun createInvite(email: String, guildId: Long): Invite {
        val r = call { invites.createInvite(com.thehideout.v1.CreateInviteRequest.newBuilder().setEmail(email).setGuildId(guildId).build()) }
        return r.invite.toModel()
    }

    override suspend fun listInvites(guildId: Long): List<Invite> {
        val r = call { invites.listInvites(com.thehideout.v1.ListInvitesRequest.newBuilder().setGuildId(guildId).build()) }
        return r.invitesList.map { it.toModel() }
    }

    override suspend fun revokeInvite(inviteId: Long): Invite {
        return call { invites.revokeInvite(com.thehideout.v1.RevokeInviteRequest.newBuilder().setInviteId(inviteId).build()) }.invite.toModel()
    }

    override suspend fun resendInvite(inviteId: Long): Invite {
        return call { invites.resendInvite(com.thehideout.v1.ResendInviteRequest.newBuilder().setInviteId(inviteId).build()) }.invite.toModel()
    }

    // ---- management: server settings, roles, moderation, emojis (phase 4b-1) -------------------

    override suspend fun updateGuildName(guild: Long, name: String): Guild {
        val r = call { guilds.updateGuild(com.thehideout.v1.UpdateGuildRequest.newBuilder().setGuildId(guild).setName(name).build()) }
        return r.guild.toModel()
    }

    override suspend fun uploadGuildIcon(guild: Long, data: ByteArray, contentType: String): Guild {
        val req = com.thehideout.v1.UploadGuildIconRequest.newBuilder().setGuildId(guild).setData(ByteString.copyFrom(data)).setContentType(contentType).build()
        return call { media.uploadGuildIcon(req) }.guild.toModel()
    }

    override suspend fun removeGuildIcon(guild: Long): Guild {
        return call { media.removeGuildIcon(com.thehideout.v1.RemoveGuildIconRequest.newBuilder().setGuildId(guild).build()) }.guild.toModel()
    }

    override suspend fun deleteGuild(guild: Long, confirmName: String) {
        call { guilds.deleteGuild(com.thehideout.v1.DeleteGuildRequest.newBuilder().setGuildId(guild).setConfirmName(confirmName).build()) }
    }

    override suspend fun transferOwnership(guild: Long, user: Long, confirmName: String): Guild {
        val req = com.thehideout.v1.TransferOwnershipRequest.newBuilder().setGuildId(guild).setUserId(user).setConfirmName(confirmName).build()
        return call { guilds.transferOwnership(req) }.guild.toModel()
    }

    override suspend fun leaveGuild(guild: Long) {
        call { guilds.leaveGuild(com.thehideout.v1.LeaveGuildRequest.newBuilder().setGuildId(guild).build()) }
    }

    override suspend fun kickMember(guild: Long, user: Long) {
        call { guilds.kickMember(com.thehideout.v1.KickMemberRequest.newBuilder().setGuildId(guild).setUserId(user).build()) }
    }

    override suspend fun banMember(guild: Long, user: Long, reason: String, deleteMessageSeconds: Int?) {
        val b = com.thehideout.v1.BanMemberRequest.newBuilder().setGuildId(guild).setUserId(user).setReason(reason)
        deleteMessageSeconds?.let { b.deleteMessageSeconds = it }
        call { guilds.banMember(b.build()) }
    }

    override suspend fun unbanMember(guild: Long, user: Long) {
        call { guilds.unbanMember(com.thehideout.v1.UnbanMemberRequest.newBuilder().setGuildId(guild).setUserId(user).build()) }
    }

    override suspend fun listBans(guild: Long): List<Ban> {
        val r = call { guilds.listBans(com.thehideout.v1.ListBansRequest.newBuilder().setGuildId(guild).build()) }
        return r.bansList.map { it.toModel() }
    }

    override suspend fun createRole(guild: Long, name: String, color: String, permissions: Long): RoleResult {
        val req = com.thehideout.v1.CreateRoleRequest.newBuilder().setGuildId(guild).setName(name).setColor(color).setPermissions(permissions).build()
        val r = call { roles.createRole(req) }
        return RoleResult(r.role.toModel(), r.version)
    }

    override suspend fun updateRole(guild: Long, roleId: Long, name: String?, color: String?, permissions: Long?): Role {
        val b = com.thehideout.v1.UpdateRoleRequest.newBuilder().setGuildId(guild).setRoleId(roleId)
        name?.let { b.name = it }
        color?.let { b.color = it }
        permissions?.let { b.permissions = it }
        return call { roles.updateRole(b.build()) }.role.toModel()
    }

    override suspend fun deleteRole(guild: Long, roleId: Long): Long {
        return call { roles.deleteRole(com.thehideout.v1.DeleteRoleRequest.newBuilder().setGuildId(guild).setRoleId(roleId).build()) }.version
    }

    override suspend fun reorderRoles(guild: Long, expectedVersion: Long, roleIds: List<Long>): GuildRoles {
        val req = com.thehideout.v1.ReorderRolesRequest.newBuilder().setGuildId(guild).setExpectedVersion(expectedVersion).addAllRoleIds(roleIds).build()
        val r = call { roles.reorderRoles(req) }
        return GuildRoles(r.rolesList.map { it.toModel() }, r.version)
    }

    override suspend fun setMemberRoles(guild: Long, userId: Long, roleIds: List<Long>): Member {
        val req = com.thehideout.v1.SetMemberRolesRequest.newBuilder().setGuildId(guild).setUserId(userId).addAllRoleIds(roleIds).build()
        return call { roles.setMemberRoles(req) }.member.toModel()
    }

    override suspend fun createEmoji(guild: Long, name: String, data: ByteArray, contentType: String): Emoji {
        val req = com.thehideout.v1.CreateEmojiRequest.newBuilder().setGuildId(guild).setName(name).setData(ByteString.copyFrom(data)).setContentType(contentType).build()
        return call { emojiService.createEmoji(req) }.emoji.toModel()
    }

    override suspend fun renameEmoji(guild: Long, emojiId: Long, name: String): Emoji {
        val req = com.thehideout.v1.RenameEmojiRequest.newBuilder().setGuildId(guild).setEmojiId(emojiId).setName(name).build()
        return call { emojiService.renameEmoji(req) }.emoji.toModel()
    }

    override suspend fun deleteEmoji(guild: Long, emojiId: Long) {
        call { emojiService.deleteEmoji(com.thehideout.v1.DeleteEmojiRequest.newBuilder().setGuildId(guild).setEmojiId(emojiId).build()) }
    }

    // ---- authentication and onboarding ---------------------------------------------------------

    private fun com.thehideout.v1.PasskeyCeremony.toModel() = PasskeyCeremony(ceremonyId, ceremonyUrl, optionsJson)

    private fun loginResult(token: String, user: com.thehideout.v1.User?, challenge: com.thehideout.v1.SecondFactorChallenge?): LoginResult =
        if (challenge != null) {
            LoginResult.SecondFactor(challenge.challengeId, secondFactorMethods(challenge), challenge.expiresAt.toInstant())
        } else {
            LoginResult.SignedIn(Session(token, user!!.toModel()))
        }

    private fun secondFactorMethods(challenge: com.thehideout.v1.SecondFactorChallenge): Set<SecondFactorMethod> =
        challenge.methodsList.map {
            when (it) {
                com.thehideout.v1.SecondFactorMethod.SECOND_FACTOR_METHOD_TOTP -> SecondFactorMethod.Totp
                com.thehideout.v1.SecondFactorMethod.SECOND_FACTOR_METHOD_RECOVERY_CODE -> SecondFactorMethod.RecoveryCode
                com.thehideout.v1.SecondFactorMethod.SECOND_FACTOR_METHOD_PASSKEY -> SecondFactorMethod.Passkey
                else -> SecondFactorMethod.Unspecified
            }
        }.toSet()

    private fun com.thehideout.v1.InviteStatus.toModel(): app.thehideout.core.model.InviteStatus = when (this) {
        com.thehideout.v1.InviteStatus.INVITE_STATUS_PENDING -> app.thehideout.core.model.InviteStatus.Pending
        com.thehideout.v1.InviteStatus.INVITE_STATUS_ACCEPTED -> app.thehideout.core.model.InviteStatus.Accepted
        com.thehideout.v1.InviteStatus.INVITE_STATUS_EXPIRED -> app.thehideout.core.model.InviteStatus.Expired
        com.thehideout.v1.InviteStatus.INVITE_STATUS_REVOKED -> app.thehideout.core.model.InviteStatus.Revoked
        else -> app.thehideout.core.model.InviteStatus.Unspecified
    }

    override suspend fun requestEmailCode(email: String) {
        call { auth.requestEmailCode(RequestEmailCodeRequest.newBuilder().setEmail(email).build()) }
    }

    override suspend fun verifyEmailCode(email: String, code: String, deviceName: String): LoginResult {
        val r = call { auth.verifyEmailCode(VerifyEmailCodeRequest.newBuilder().setEmail(email).setCode(code).setDeviceName(deviceName).build()) }
        return loginResult(r.token, if (r.hasUser()) r.user else null, if (r.hasSecondFactorRequired()) r.secondFactorRequired else null)
    }

    override suspend fun completeSecondFactor(challengeId: String, totpCode: String, recoveryCode: String): Session {
        val r = call {
            auth.completeSecondFactor(
                CompleteSecondFactorRequest.newBuilder().setChallengeId(challengeId).setTotpCode(totpCode).setRecoveryCode(recoveryCode).build(),
            )
        }
        return Session(r.token, r.user.toModel())
    }

    override suspend fun beginSecondFactorPasskey(challengeId: String): PasskeyCeremony {
        val r = call { auth.beginSecondFactorPasskey(BeginSecondFactorPasskeyRequest.newBuilder().setChallengeId(challengeId).build()) }
        return r.ceremony.toModel()
    }

    override suspend fun finishSecondFactorPasskey(ceremonyId: String, credentialJson: String): PasskeyFinish {
        val r = call { auth.finishSecondFactorPasskey(FinishSecondFactorPasskeyRequest.newBuilder().setCeremonyId(ceremonyId).setCredentialJson(credentialJson).build()) }
        return PasskeyFinish(if (r.token.isEmpty()) null else Session(r.token, r.user.toModel()), r.pending)
    }

    override suspend fun requestPasswordReset(email: String) {
        call { auth.requestPasswordReset(RequestPasswordResetRequest.newBuilder().setEmail(email).build()) }
    }

    override suspend fun resetPassword(email: String, code: String, newPassword: String) {
        call { auth.resetPassword(ResetPasswordRequest.newBuilder().setEmail(email).setCode(code).setNewPassword(newPassword).build()) }
    }

    override suspend fun beginPasskeyLogin(username: String, deviceName: String): PasskeyCeremony {
        val r = call { auth.beginPasskeyLogin(BeginPasskeyLoginRequest.newBuilder().setUsername(username).setDeviceName(deviceName).build()) }
        return r.ceremony.toModel()
    }

    override suspend fun finishPasskeyLogin(ceremonyId: String, credentialJson: String): PasskeyFinish {
        val r = call { auth.finishPasskeyLogin(FinishPasskeyLoginRequest.newBuilder().setCeremonyId(ceremonyId).setCredentialJson(credentialJson).build()) }
        return PasskeyFinish(if (r.token.isEmpty()) null else Session(r.token, r.user.toModel()), r.pending)
    }

    override suspend fun beginPasskeyRegistration(name: String): PasskeyCeremony {
        val r = call { auth.beginPasskeyRegistration(BeginPasskeyRegistrationRequest.newBuilder().setName(name).build()) }
        return r.ceremony.toModel()
    }

    override suspend fun finishPasskeyRegistration(ceremonyId: String, credentialJson: String): PasskeyRegistered {
        val r = call {
            auth.finishPasskeyRegistration(FinishPasskeyRegistrationRequest.newBuilder().setCeremonyId(ceremonyId).setCredentialJson(credentialJson).build())
        }
        return PasskeyRegistered(if (r.hasPasskey()) r.passkey.name else "", r.pending)
    }

    override suspend fun lookupInvite(tokenOrCode: String): InvitePreview {
        val r = call { invites.lookupInvite(LookupInviteRequest.newBuilder().setTokenOrCode(tokenOrCode).build()) }
        return InvitePreview(
            maskedEmail = r.maskedEmail,
            inviter = if (r.hasInviter()) r.inviter.toModel() else null,
            guildNames = r.guildNamesList,
            expiresAt = r.expiresAt.toInstant(),
            status = r.status.toModel(),
            termsVersion = r.termsVersion,
            minimumAge = r.minimumAge,
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
        val b = AcceptInviteRequest.newBuilder()
            .setTokenOrCode(tokenOrCode)
            .setUsername(username)
            .setDeviceName(deviceName)
            .setAcceptedTermsVersion(acceptedTermsVersion)
            .setAgeConfirmed(ageConfirmed)
        if (password != null) b.setPassword(password)
        val r = call { invites.acceptInvite(b.build()) }
        return Session(r.token, r.user.toModel())
    }

    override suspend fun acceptTerms(version: String) {
        call { auth.acceptTerms(AcceptTermsRequest.newBuilder().setVersion(version).build()) }
    }

    override fun subscribe(): Flow<GatewayEvent> = flow {
        val stream = try {
            gateway.subscribe()
        } catch (e: ConnectException) {
            throw e.toAppError()
        }
        try {
            stream.sendAndClose(SubscribeRequest.getDefaultInstance()).onFailure { throw mapThrowable(it) }
            for (item in stream.responseChannel()) item.event.toModel()?.let { emit(it) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: ConnectException) {
            throw e.toAppError()
        } catch (e: IOException) {
            throw AppError.Unavailable(e.message.orEmpty())
        } finally {
            runCatching { stream.receiveClose() }
        }
    }

    private fun mapThrowable(t: Throwable): Throwable = if (t is ConnectException) t.toAppError() else t
}
