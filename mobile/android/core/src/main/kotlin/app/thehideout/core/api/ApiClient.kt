package app.thehideout.core.api

import app.thehideout.core.model.*
import kotlinx.coroutines.flow.Flow

data class Session(val token: String, val me: User)

sealed interface LoginResult {
    data class SignedIn(val session: Session) : LoginResult
    data class SecondFactor(val challengeId: String) : LoginResult
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
    suspend fun listMembers(guild: Long): GuildMembers
    suspend fun listRoles(guild: Long): GuildRoles
    suspend fun addReaction(channel: Long, message: Long, emoji: ReactionEmoji)
    suspend fun removeReaction(channel: Long, message: Long, emoji: ReactionEmoji)
    suspend fun logout()

    /**
     * One subscription to the event stream: emits until the stream ends, then completes; fails with
     * [AppError] when it breaks. [app.thehideout.core.gateway.GatewayStream] owns reconnecting.
     */
    fun subscribe(): Flow<GatewayEvent>

    companion object {
        const val PAGE = 50
    }
}
