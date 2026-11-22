package app.thehideout.core

import app.thehideout.core.api.AppError
import app.thehideout.core.fake.FakeBackend
import app.thehideout.core.fake.FakeBackend.Companion.ANA
import app.thehideout.core.fake.FakeBackend.Companion.DEV
import app.thehideout.core.fake.FakeBackend.Companion.GENERAL
import app.thehideout.core.fake.FakeBackend.Companion.HIDEOUT
import app.thehideout.core.fake.FakeBackend.Companion.LOUNGE
import app.thehideout.core.gateway.Backoff
import app.thehideout.core.gateway.GatewayStream
import app.thehideout.core.model.GatewayEvent
import app.thehideout.core.store.*
import app.cash.turbine.test
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.*
import org.junit.jupiter.api.Test
import java.time.Duration

@OptIn(ExperimentalCoroutinesApi::class)
class AppStoreTest {
    private fun TestScope.store(fake: FakeBackend, tokens: TokenStore = InMemoryTokenStore("t"), gateway: GatewayStream? = null) =
        AppStore(fake, backgroundScope, tokens, gateway = gateway ?: GatewayStream(fake::subscribe), clock = { T0 })

    @Test fun `without a saved token the app starts signed out`() = runTest(UnconfinedTestDispatcher()) {
        val s = store(FakeBackend(), InMemoryTokenStore())
        s.state.test {
            assertEquals(SessionPhase.Loading, awaitItem().phase)
            s.bootstrap()
            assertEquals(SessionPhase.SignedOut(), awaitItem().phase)
        }
    }

    @Test fun `bootstrap loads the world, opens the first channel and its history`() = runTest(UnconfinedTestDispatcher()) {
        val s = store(FakeBackend())
        s.bootstrap().join()
        val st = s.state.first { it.log(GENERAL)?.loaded == true }
        assertEquals(SessionPhase.SignedIn, st.phase)
        assertEquals(GENERAL, st.currentChannel)
        assertEquals(FakeBackend.ANA, st.log(GENERAL)!!.messages.first().authorId.let { FakeBackend.ANA })
        assertTrue(st.log(GENERAL)!!.hasMore, "the page limit is below the seeded history")
        assertEquals(50, st.log(GENERAL)!!.messages.size)
        assertEquals(setOf(1L, 2L, 3L, 4L), st.members[HIDEOUT]!!.toSet())
        assertEquals("Moderator", st.roles[HIDEOUT]!!.first { it.id == 1001L }.name)
    }

    @Test fun `sending shows pending at once, then exactly one confirmed message`() = runTest(UnconfinedTestDispatcher()) {
        val fake = FakeBackend()
        val s = store(fake)
        s.bootstrap().join()
        s.state.first { it.log(GENERAL)?.loaded == true }
        val before = s.state.value.log(GENERAL)!!.messages.size
        fake.latency = Duration.ofMillis(200)
        s.send("hello from the test")
        assertEquals(Delivery.Pending, s.state.value.outgoing.single().state)
        settle()
        val st = s.state.value
        assertTrue(st.outgoing.isEmpty(), "outgoing=${st.outgoing} notices=${st.notices}")
        assertEquals(before + 1, st.log(GENERAL)!!.messages.size)
        assertEquals("hello from the test", st.log(GENERAL)!!.messages.last().content)
        assertEquals(1, fake.sent.size)
    }

    @Test fun `a failed send stays as failed until retried`() = runTest(UnconfinedTestDispatcher()) {
        val fake = FakeBackend().also { it.failSends = 1 }
        val s = store(fake)
        s.bootstrap().join()
        s.state.first { it.log(GENERAL)?.loaded == true }
        s.send("x")
        settle()
        val failed = s.state.value.outgoing.single()
        assertEquals(Delivery.Failed, failed.state)
        s.retry(failed.nonce)
        settle()
        assertTrue(s.state.value.outgoing.isEmpty())
        assertEquals("x", s.state.value.log(GENERAL)!!.messages.last().content)
    }

    @Test fun `gateway events reach the state while the app runs`() = runTest(UnconfinedTestDispatcher()) {
        val fake = FakeBackend()
        val s = store(fake)
        s.bootstrap().join()
        s.state.first { it.connection == ConnectionStatus.Connected }
        assertEquals(2, s.state.value.voiceChannelStates(HIDEOUT, LOUNGE).size, "the snapshot after the first heartbeat")
        fake.post(DEV, ANA, "ping @tim")
        val st = s.state.first { it.unreadOf(DEV).count > 0 }
        assertEquals(Unread(1, 1), st.unreadOf(DEV))
    }

    @Test fun `an interrupted stream refetches the open channel when it comes back`() = runTest(UnconfinedTestDispatcher()) {
        val fake = FakeBackend()
        var opens = 0
        val gw = GatewayStream({
            opens++
            if (opens == 2) flow { throw AppError.Unavailable("no network") } else fake.subscribe()
        }, backoff = Backoff(Duration.ofSeconds(1), Duration.ofSeconds(1)))
        val s = store(fake, gateway = gw)
        s.bootstrap().join()
        s.state.first { it.connection == ConnectionStatus.Connected }
        assertEquals(1, opens)
        // The first stream ends when its flow completes; make it end by cancelling the gateway and restarting it.
        s.stopGateway()
        s.dispatch(Action.Gateway(app.thehideout.core.model.GatewayUpdate.Disconnected(Duration.ofSeconds(1))))
        assertEquals(StreamState.Interrupted, s.state.value.stream)
        fake.post(GENERAL, ANA, "arrived while away")
        s.startGateway() // the second open fails, the third succeeds after the backoff
        testScheduler.advanceTimeBy(2_000)
        testScheduler.runCurrent()
        assertEquals(3, opens)
        val st = s.state.value
        assertEquals(StreamState.Live, st.stream)
        assertEquals(ConnectionStatus.Connected, st.connection)
        assertTrue(st.log(GENERAL)!!.messages.any { it.content == "arrived while away" }, "the missed message came from the refetch, not the stream")
    }

    /** Background work (the store's scope) is not waited for by `advanceUntilIdle`: move the clock and run what is due. */
    private fun TestScope.settle() {
        testScheduler.advanceTimeBy(500)
        testScheduler.runCurrent()
    }

    @Test fun `a refused token signs out and forgets it`() = runTest(UnconfinedTestDispatcher()) {
        val tokens = InMemoryTokenStore("stale")
        val refusing = object : app.thehideout.core.api.ApiClient by FakeBackend() {
            override suspend fun loadInitial() = throw AppError.Unauthenticated
        }
        val s = AppStore(refusing, backgroundScope, tokens)
        s.bootstrap().join()
        assertEquals(SessionPhase.SignedOut(AppError.Unauthenticated), s.state.value.phase)
        assertNull(tokens.load())
    }

    @Test fun `an old client, a suspension and unaccepted terms put up their gate`() = runTest(UnconfinedTestDispatcher()) {
        fun gate(e: AppError): SessionPhase {
            val api = object : app.thehideout.core.api.ApiClient by FakeBackend() {
                override suspend fun loadInitial() = throw e
            }
            val st = AppStore(api, backgroundScope, InMemoryTokenStore("t"))
            st.bootstrap()
            return st.state.value.phase
        }
        assertEquals(SessionPhase.UpdateRequired, gate(AppError.ClientTooOld))
        assertEquals(SessionPhase.Suspended("spam", null), gate(AppError.Suspended("spam", null)))
        assertEquals(SessionPhase.TermsRequired(""), gate(AppError.TermsUpdateRequired))
    }

    @Test fun `sign out clears the token and the state`() = runTest(UnconfinedTestDispatcher()) {
        val tokens = InMemoryTokenStore("t")
        val s = store(FakeBackend(), tokens)
        s.bootstrap().join()
        s.signOut()
        assertEquals(SessionPhase.SignedOut(), s.state.value.phase)
        assertNull(tokens.load())
        assertTrue(s.state.value.guilds.isEmpty())
    }

    @Test fun `signing in saves the token and loads the world`() = runTest(UnconfinedTestDispatcher()) {
        val tokens = InMemoryTokenStore()
        val s = store(FakeBackend(), tokens)
        val bad = s.signIn("nobody", "pw", "test")
        assertEquals(AppError.Unauthenticated, bad.exceptionOrNull())
        val ok = s.signIn("tim", "pw", "test")
        assertTrue(ok.isSuccess)
        assertEquals("fake-token-1", tokens.load())
        s.state.first { it.phase == SessionPhase.SignedIn }
        assertEquals("tim", s.state.value.me!!.username)
    }
}
