package app.thehideout.core.gateway

import app.thehideout.core.api.AppError
import app.thehideout.core.model.GatewayEvent
import app.thehideout.core.model.GatewayUpdate
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.channels.ReceiveChannel
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.produceIn
import kotlinx.coroutines.withTimeoutOrNull
import java.time.Duration

/** Exponential backoff: [initial], doubling on each consecutive failure, never above [max]. */
data class Backoff(val initial: Duration = Duration.ofMillis(500), val max: Duration = Duration.ofSeconds(30)) {
    fun delayFor(attempt: Int): Duration {
        var d = initial
        repeat(attempt) { d = d.multipliedBy(2); if (d >= max) return max }
        return if (d > max) max else d
    }
}

/**
 * Keeps one subscription to the event stream alive: opens it, forwards its events, declares it dead
 * when it stays silent for [silence] (the server sends a heartbeat every 30 s), and reconnects with
 * [backoff]. Every drop is reported as [GatewayUpdate.Disconnected], which marks the stream
 * interrupted for the store: the server does not replay, so the store refetches on the next
 * [GatewayUpdate.Connected].
 *
 * Cold: collecting starts the loop, cancelling the collector stops it. Run it only while the app is
 * in the foreground (or a voice service is alive).
 */
class GatewayStream(
    private val open: () -> Flow<GatewayEvent>,
    private val silence: Duration = Duration.ofSeconds(75),
    private val backoff: Backoff = Backoff(),
) {
    fun updates(): Flow<GatewayUpdate> = flow {
        var attempt = 0
        while (true) {
            emit(GatewayUpdate.Connecting)
            var connected = false
            try {
                coroutineScope {
                    // A failing source must not fail this scope before the events it delivered are read.
                    val events: ReceiveChannel<Item> = open().map<GatewayEvent, Item> { Item.Event(it) }.catch { emit(Item.Failed(it)) }.produceIn(this)
                    try {
                        while (true) {
                            val item = withTimeoutOrNull(silence.toMillis()) { events.receiveCatching() }
                                ?: throw StreamSilent
                            if (item.isClosed) break
                            val next = item.getOrThrow()
                            if (next is Item.Failed) throw next.cause
                            if (!connected) {
                                connected = true
                                attempt = 0
                                emit(GatewayUpdate.Connected)
                            }
                            emit(GatewayUpdate.Event((next as Item.Event).event))
                        }
                    } finally {
                        events.cancel()
                    }
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: AppError.Unauthenticated) {
                emit(GatewayUpdate.AuthExpired)
                return@flow
            } catch (_: Throwable) {
                // Dropped, refused, silent: all the same to the caller; it retries.
            }
            val wait = backoff.delayFor(attempt++)
            emit(GatewayUpdate.Disconnected(wait))
            delay(wait.toMillis())
        }
    }

    private sealed interface Item {
        data class Event(val event: GatewayEvent) : Item
        data class Failed(val cause: Throwable) : Item
    }

    private data object StreamSilent : Exception("no events for too long") {
        private fun readResolve(): Any = StreamSilent
    }
}
