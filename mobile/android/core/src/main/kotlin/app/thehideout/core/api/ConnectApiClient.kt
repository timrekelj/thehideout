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
import com.thehideout.v1.MessageServiceClient
import com.thehideout.v1.RemoveReactionRequest
import com.thehideout.v1.RequestEmailCodeRequest
import com.thehideout.v1.RequestPasswordResetRequest
import com.thehideout.v1.ResetPasswordRequest
import com.thehideout.v1.RoleServiceClient
import com.thehideout.v1.SendMessageRequest
import com.thehideout.v1.SubscribeRequest
import com.thehideout.v1.VerifyEmailCodeRequest
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.async
import okhttp3.Interceptor
import okhttp3.OkHttpClient
import java.io.IOException
import java.util.concurrent.TimeUnit
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
