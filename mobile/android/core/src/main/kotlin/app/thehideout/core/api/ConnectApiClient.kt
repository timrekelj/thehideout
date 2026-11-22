package app.thehideout.core.api

import app.thehideout.core.model.*
import com.connectrpc.ConnectException
import com.connectrpc.ProtocolClientConfig
import com.connectrpc.ResponseMessage
import com.connectrpc.extensions.GoogleJavaLiteProtobufStrategy
import com.connectrpc.impl.ProtocolClient
import com.connectrpc.okhttp.ConnectOkHttpClient
import com.connectrpc.protocols.NetworkProtocol
import com.thehideout.v1.AddReactionRequest
import com.thehideout.v1.AuthServiceClient
import com.thehideout.v1.DmServiceClient
import com.thehideout.v1.GatewayServiceClient
import com.thehideout.v1.GetMeRequest
import com.thehideout.v1.GuildServiceClient
import com.thehideout.v1.ListChannelsRequest
import com.thehideout.v1.ListDmsRequest
import com.thehideout.v1.ListGuildMembersRequest
import com.thehideout.v1.ListMessagesRequest
import com.thehideout.v1.ListMyGuildsRequest
import com.thehideout.v1.ListRolesRequest
import com.thehideout.v1.LoginRequest
import com.thehideout.v1.MessageServiceClient
import com.thehideout.v1.RemoveReactionRequest
import com.thehideout.v1.RoleServiceClient
import com.thehideout.v1.SendMessageRequest
import com.thehideout.v1.SubscribeRequest
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.async
import okhttp3.Interceptor
import okhttp3.OkHttpClient
import java.io.IOException
import java.util.concurrent.TimeUnit

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

/** The real [ApiClient]: connect-kotlin over OkHttp, with every failure mapped to [AppError]. */
class ConnectApiClient(config: ApiConfig, http: Pair<OkHttpClient, OkHttpClient> = buildHttpClients(config)) : ApiClient {
    private val client = ProtocolClient(
        ConnectOkHttpClient(http.first, http.second),
        ProtocolClientConfig(
            host = config.baseUrl,
            serializationStrategy = GoogleJavaLiteProtobufStrategy(),
            networkProtocol = NetworkProtocol.CONNECT,
        ),
    )
    private val auth = AuthServiceClient(client)
    private val guilds = GuildServiceClient(client)
    private val dms = DmServiceClient(client)
    private val messages = MessageServiceClient(client)
    private val roles = RoleServiceClient(client)
    private val gateway = GatewayServiceClient(client)

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
        return if (r.hasSecondFactorRequired()) {
            LoginResult.SecondFactor(r.secondFactorRequired.challengeId)
        } else {
            LoginResult.SignedIn(Session(r.token, r.user.toModel()))
        }
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

    override suspend fun logout() = Unit

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
