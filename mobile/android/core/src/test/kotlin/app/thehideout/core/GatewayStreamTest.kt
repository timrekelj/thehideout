package app.thehideout.core

import app.thehideout.core.api.AppError
import app.thehideout.core.gateway.Backoff
import app.thehideout.core.gateway.GatewayStream
import app.thehideout.core.model.GatewayEvent
import app.thehideout.core.model.GatewayUpdate
import app.cash.turbine.test
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.*
import org.junit.jupiter.api.Test
import java.time.Duration

@OptIn(kotlinx.coroutines.ExperimentalCoroutinesApi::class)
class GatewayStreamTest {
    private val beat = GatewayEvent.Heartbeat

    @Test fun `backoff doubles from the first retry and stops at the cap`() {
        val b = Backoff(Duration.ofMillis(500), Duration.ofSeconds(30))
        assertEquals(listOf(500L, 1000, 2000, 4000, 8000, 16000, 30000, 30000), (0..7).map { b.delayFor(it).toMillis() })
        assertEquals(30_000L, b.delayFor(200).toMillis(), "no overflow far out")
    }

    @Test fun `forwards events after Connecting and Connected`() = runTest {
        val stream = GatewayStream({ flow { emit(beat); emit(GatewayEvent.GuildRemoved(1)); delay(10_000_000) } })
        stream.updates().test {
            assertEquals(GatewayUpdate.Connecting, awaitItem())
            assertEquals(GatewayUpdate.Connected, awaitItem())
            assertEquals(GatewayUpdate.Event(beat), awaitItem())
            assertEquals(GatewayUpdate.Event(GatewayEvent.GuildRemoved(1)), awaitItem())
            cancelAndIgnoreRemainingEvents()
        }
    }

    @Test fun `a stream that ends is reported and reopened after the backoff`() = runTest {
        var opens = 0
        val stream = GatewayStream({ opens++; flowOf(beat) }, backoff = Backoff(Duration.ofSeconds(1), Duration.ofSeconds(8)))
        stream.updates().test {
            assertEquals(GatewayUpdate.Connecting, awaitItem())
            assertEquals(GatewayUpdate.Connected, awaitItem())
            assertEquals(GatewayUpdate.Event(beat), awaitItem())
            assertEquals(GatewayUpdate.Disconnected(Duration.ofSeconds(1)), awaitItem(), "connected cleanly: attempts start over")
            assertEquals(GatewayUpdate.Connecting, awaitItem())
            assertEquals(2, opens)
            cancelAndIgnoreRemainingEvents()
        }
    }

    @Test fun `failures back off exponentially and a clean connect resets the ladder`() = runTest {
        var opens = 0
        val stream = GatewayStream(
            {
                when (opens++) {
                    0, 1, 2 -> flow { throw AppError.Unavailable("refused") }
                    3 -> flow { emit(beat); throw AppError.Unavailable("reset") }
                    else -> flow { delay(10_000_000) }
                }
            },
            backoff = Backoff(Duration.ofSeconds(1), Duration.ofSeconds(60)),
        )
        stream.updates().test {
            val waits = mutableListOf<Long>()
            repeat(3) {
                assertEquals(GatewayUpdate.Connecting, awaitItem())
                waits += (awaitItem() as GatewayUpdate.Disconnected).retryIn.seconds
            }
            assertEquals(listOf(1L, 2L, 4L), waits)
            assertEquals(GatewayUpdate.Connecting, awaitItem())
            assertEquals(GatewayUpdate.Connected, awaitItem())
            assertEquals(GatewayUpdate.Event(beat), awaitItem())
            assertEquals(GatewayUpdate.Disconnected(Duration.ofSeconds(1)), awaitItem(), "a clean connect reset the ladder")
            cancelAndIgnoreRemainingEvents()
        }
    }

    @Test fun `a silent stream is declared dead by the watchdog`() = runTest {
        var opens = 0
        val stream = GatewayStream(
            { opens++; flow { emit(beat); delay(10_000_000) } },
            silence = Duration.ofSeconds(75),
            backoff = Backoff(Duration.ofSeconds(2), Duration.ofSeconds(8)),
        )
        stream.updates().test {
            assertEquals(GatewayUpdate.Connecting, awaitItem())
            assertEquals(GatewayUpdate.Connected, awaitItem())
            assertEquals(GatewayUpdate.Event(beat), awaitItem())
            val dead = awaitItem()
            assertEquals(GatewayUpdate.Disconnected(Duration.ofSeconds(2)), dead)
            assertEquals(75_000L, testScheduler.currentTime, "declared dead exactly after the silence window")
            assertEquals(GatewayUpdate.Connecting, awaitItem())
            cancelAndIgnoreRemainingEvents()
        }
        assertEquals(2, opens)
    }

    @Test fun `heartbeats keep a quiet stream alive`() = runTest {
        val stream = GatewayStream({ flow { repeat(5) { delay(30_000); emit(beat) }; delay(10_000_000) } }, silence = Duration.ofSeconds(75))
        stream.updates().test {
            assertEquals(GatewayUpdate.Connecting, awaitItem())
            assertEquals(GatewayUpdate.Connected, awaitItem())
            repeat(5) { assertEquals(GatewayUpdate.Event(beat), awaitItem()) }
            assertEquals(150_000L, testScheduler.currentTime)
            cancelAndIgnoreRemainingEvents()
        }
    }

    @Test fun `a refused token ends the stream with AuthExpired and does not retry`() = runTest {
        var opens = 0
        val open: () -> Flow<GatewayEvent> = { opens++; flow { throw AppError.Unauthenticated } }
        GatewayStream(open).updates().test {
            assertEquals(GatewayUpdate.Connecting, awaitItem())
            assertEquals(GatewayUpdate.AuthExpired, awaitItem())
            awaitComplete()
        }
        assertEquals(1, opens)
    }
}
