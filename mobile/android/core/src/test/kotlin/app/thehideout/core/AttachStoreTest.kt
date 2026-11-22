package app.thehideout.core

import app.thehideout.core.api.AppError
import app.thehideout.core.api.ApiClient
import app.thehideout.core.api.PendingAttachment
import app.thehideout.core.api.UploadSource
import app.thehideout.core.attachment.MAX_CONCURRENT_UPLOADS
import app.thehideout.core.fake.FakeBackend
import app.thehideout.core.fake.FakeBackend.Companion.GENERAL
import app.thehideout.core.gateway.GatewayStream
import app.thehideout.core.store.*
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.*
import org.junit.jupiter.api.Test
import java.io.ByteArrayInputStream
import java.time.Duration
import java.time.Instant
import java.util.concurrent.atomic.AtomicInteger

private fun source(): UploadSource = UploadSource { ByteArrayInputStream(ByteArray(10)) }
private fun picked(name: String) = PickedFile(name, "image/png", 10, "content://$name", source())

/** Concurrency, automatic retry on a transient failure, and cancel-on-remove for the upload runner (`AppStore.run`). */
@OptIn(ExperimentalCoroutinesApi::class)
class AttachStoreTest {
    private fun TestScope.store(api: ApiClient) =
        AppStore(api, backgroundScope, InMemoryTokenStore("t"), gateway = GatewayStream(api::subscribe), clock = { Instant.now() })

    @Test fun `no more than the concurrency limit uploads at once`() = runTest(UnconfinedTestDispatcher()) {
        // Every call blocks on `release`, so exactly as many reach `uploadAttachment` as the gate allows.
        val entered = AtomicInteger(0)
        val release = kotlinx.coroutines.CompletableDeferred<Unit>()
        val gated = object : ApiClient by FakeBackend() {
            override suspend fun uploadAttachment(channel: Long, filename: String, contentType: String, size: Long, source: UploadSource, progress: (Long) -> Unit): PendingAttachment {
                entered.incrementAndGet()
                release.await()
                return PendingAttachment(entered.get().toLong(), filename, contentType, size, Instant.now())
            }
        }
        val s = store(gated)
        s.bootstrap().join()
        s.attachFiles(GENERAL, (1..MAX_CONCURRENT_UPLOADS + 2).map { picked("f$it.png") })
        testScheduler.runCurrent()
        assertEquals(MAX_CONCURRENT_UPLOADS, entered.get(), "only the concurrency limit reaches the network call")
        val draft = s.state.value.attachDraft(GENERAL)
        assertEquals(MAX_CONCURRENT_UPLOADS, draft.files.count { it.state.phase == AttachUploadPhase.Uploading })
        assertEquals(2, draft.files.count { it.state.phase == AttachUploadPhase.Queued })

        release.complete(Unit)
        testScheduler.runCurrent()
        assertEquals(MAX_CONCURRENT_UPLOADS + 2, entered.get(), "the queued ones got their turn once a slot freed up")
        assertTrue(s.state.value.attachDraft(GENERAL).files.all { it.state.phase == AttachUploadPhase.Ready })
    }

    @Test fun `a transient failure is retried automatically and the file still turns Ready`() = runTest(UnconfinedTestDispatcher()) {
        val attempts = AtomicInteger(0)
        val flaky = object : ApiClient by FakeBackend() {
            override suspend fun uploadAttachment(channel: Long, filename: String, contentType: String, size: Long, source: UploadSource, progress: (Long) -> Unit): PendingAttachment {
                if (attempts.getAndIncrement() < 2) throw AppError.Unavailable("connection reset")
                return PendingAttachment(1, filename, contentType, size, Instant.now())
            }
        }
        val s = store(flaky)
        s.bootstrap().join()
        s.attachFiles(GENERAL, listOf(picked("a.png")))
        testScheduler.advanceTimeBy(60_000)
        testScheduler.runCurrent()
        val f = s.state.value.attachDraft(GENERAL).files.single()
        assertEquals(AttachUploadPhase.Ready, f.state.phase)
        assertEquals(3, attempts.get(), "two failures, then the attempt that succeeds")
    }

    @Test fun `a non-transient failure is not retried`() = runTest(UnconfinedTestDispatcher()) {
        val attempts = AtomicInteger(0)
        val refusing = object : ApiClient by FakeBackend() {
            override suspend fun uploadAttachment(channel: Long, filename: String, contentType: String, size: Long, source: UploadSource, progress: (Long) -> Unit): PendingAttachment {
                attempts.incrementAndGet()
                throw AppError.TooLarge
            }
        }
        val s = store(refusing)
        s.bootstrap().join()
        s.attachFiles(GENERAL, listOf(picked("a.png")))
        testScheduler.advanceTimeBy(60_000)
        testScheduler.runCurrent()
        assertEquals(AttachUploadPhase.Failed, s.state.value.attachDraft(GENERAL).files.single().state.phase)
        assertEquals(1, attempts.get())
    }

    @Test fun `removing a file while it uploads cancels the transfer`() = runTest(UnconfinedTestDispatcher()) {
        val cancelled = AtomicInteger(0)
        val slow = object : ApiClient by FakeBackend() {
            override suspend fun uploadAttachment(channel: Long, filename: String, contentType: String, size: Long, source: UploadSource, progress: (Long) -> Unit): PendingAttachment {
                try {
                    kotlinx.coroutines.delay(10_000)
                } catch (e: kotlinx.coroutines.CancellationException) {
                    cancelled.incrementAndGet()
                    throw e
                }
                return PendingAttachment(1, filename, contentType, size, Instant.now())
            }
        }
        val s = store(slow)
        s.bootstrap().join()
        s.attachFiles(GENERAL, listOf(picked("a.png")))
        testScheduler.runCurrent()
        val id = s.state.value.attachDraft(GENERAL).files.single().id
        s.removeAttachment(id)
        testScheduler.runCurrent()
        assertTrue(s.state.value.attachDraft(GENERAL).files.isEmpty())
        assertEquals(1, cancelled.get())
    }
}
