package app.thehideout.core

import app.thehideout.core.fake.FakeBackend
import app.thehideout.core.model.*
import app.thehideout.core.store.*
import kotlinx.coroutines.runBlocking
import java.time.Instant

/** A signed-in state as the fake world delivers it, with The Hideout open on #general. */
fun loadedState(): AppState = runBlocking {
    val fake = FakeBackend()
    val init = fake.loadInitial()
    Reducer.reduce(AppState(), Action.InitialLoaded(init)).state
}

val T0: Instant = Instant.parse("2026-11-22T10:00:00Z")

fun msg(id: Long, channel: Long, author: Long, content: String = "m$id", nonce: String = "", guild: Long? = FakeBackend.HIDEOUT) =
    Message(id, channel, guild, author, content, T0.plusSeconds(id), nonce = nonce)

fun AppState.reduce(a: Action): AppState = Reducer.reduce(this, a).state

fun AppState.event(e: GatewayEvent): AppState = reduce(Action.Gateway(GatewayUpdate.Event(e)))

/** Marks #general loaded with [ids] (oldest first), as a first page would. */
fun AppState.withLog(channel: Long, ids: List<Long>, author: Long = FakeBackend.ANA, hasMore: Boolean = false): AppState {
    val guild = if (channel == FakeBackend.DM_ANA || channel == FakeBackend.DM_BRUNO) null else FakeBackend.HIDEOUT
    val log = ChannelLog(
        messages = ids.map { msg(it, channel, author, guild = guild) },
        oldestId = ids.firstOrNull(),
        hasMore = hasMore,
        loaded = true,
    )
    return copy(logs = logs + (channel to log))
}
