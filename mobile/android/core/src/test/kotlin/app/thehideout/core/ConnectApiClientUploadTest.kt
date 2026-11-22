package app.thehideout.core

import app.thehideout.core.api.ApiConfig
import app.thehideout.core.api.AppError
import app.thehideout.core.api.ConnectApiClient
import app.thehideout.core.api.UploadSource
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.SocketEffect
import okhttp3.Headers.Companion.toHeaders
import org.junit.jupiter.api.AfterEach
import org.junit.jupiter.api.Assertions.*
import org.junit.jupiter.api.BeforeEach
import org.junit.jupiter.api.Test
import java.io.ByteArrayInputStream

/** `POST /attachments`: the plain-HTTP upload (`ConnectApiClient.uploadAttachment`), over a real socket. */
class ConnectApiClientUploadTest {
    private lateinit var server: MockWebServer

    @BeforeEach fun start() {
        server = MockWebServer()
        server.start()
    }

    @AfterEach fun stop() {
        server.close()
    }

    private fun client(): ConnectApiClient =
        ConnectApiClient(ApiConfig(server.url("/").toString().trimEnd('/'), { "tok" }, "android-test/1.0"))

    @Test fun `a successful upload sends the file, the headers and the query, and parses the pending attachment`() = runBlocking {
        server.enqueue(
            MockResponse.Builder()
                .code(200)
                .headers(mapOf("Content-Type" to "application/json").toHeaders())
                .body("""{"id":"123","filename":"a b.png","contentType":"image/png","size":"5","expiresAt":"2026-01-02T03:04:05Z"}""")
                .build(),
        )
        val progressed = mutableListOf<Long>()
        val pending = client().uploadAttachment(7, "a b.png", "image/png", 5, UploadSource { ByteArrayInputStream("hello".toByteArray()) }) { progressed += it }

        assertEquals(123L, pending.id)
        assertEquals("a b.png", pending.filename)
        assertEquals("image/png", pending.contentType)
        assertEquals(5L, pending.size)
        assertEquals(listOf(5L), progressed)

        val recorded = server.takeRequest()
        assertEquals("POST", recorded.method)
        assertEquals("/attachments?channel_id=7&filename=a%20b.png", recorded.target)
        assertEquals("Bearer tok", recorded.headers["authorization"])
        assertEquals("android-test/1.0", recorded.headers["x-thehideout-client"])
        assertEquals("image/png", recorded.headers["content-type"])
        assertEquals("5", recorded.headers["content-length"])
        assertEquals("hello", recorded.body!!.utf8())
    }

    @Test fun `a 413 maps to TooLarge`() = runBlocking {
        server.enqueue(MockResponse.Builder().code(413).body("""{"code":"invalid_argument","message":"files may be at most 1 bytes"}""").build())
        val e = assertThrows(AppError::class.java) { runBlocking { client().uploadAttachment(1, "x", "text/plain", 2, UploadSource { ByteArrayInputStream(byteArrayOf(1, 2)) }) {} } }
        assertEquals(AppError.TooLarge, e)
    }

    @Test fun `a response body that fails mid-read resumes with a retryable Unavailable instead of hanging`() = runBlocking {
        // A 200 whose socket is closed as the body starts: `r.body.bytes()` in the callback throws.
        server.enqueue(
            MockResponse.Builder()
                .code(200)
                .headers(mapOf("Content-Type" to "application/json").toHeaders())
                .body("""{"id":"123","filename":"x","contentType":"text/plain","size":"1","expiresAt":"2026-01-02T03:04:05Z"}""")
                .onResponseBody(SocketEffect.CloseSocket())
                .build(),
        )
        val e = assertThrows(AppError::class.java) {
            runBlocking {
                // Bounded: with the leak, the coroutine never resumes and this times out instead of returning.
                withTimeout(10_000) {
                    client().uploadAttachment(1, "x", "text/plain", 1, UploadSource { ByteArrayInputStream(byteArrayOf(1)) }) {}
                }
            }
        }
        assertTrue(e is AppError.Unavailable, "expected a transient Unavailable, got $e")
        assertTrue(e.isTransient, "the store's uploadWithRetry must retry it")
    }

    @Test fun `a 429 maps to RateLimited and a 401 to Unauthenticated`() = runBlocking {
        server.enqueue(MockResponse.Builder().code(429).body("""{"code":"resource_exhausted","message":"too many pending uploads"}""").build())
        val rateLimited = assertThrows(AppError::class.java) { runBlocking { client().uploadAttachment(1, "x", "text/plain", 1, UploadSource { ByteArrayInputStream(byteArrayOf(1)) }) {} } }
        assertEquals(AppError.RateLimited, rateLimited)
        server.enqueue(MockResponse.Builder().code(401).body("""{"code":"unauthenticated","message":"missing or malformed authorization header"}""").build())
        val unauthenticated = assertThrows(AppError::class.java) { runBlocking { client().uploadAttachment(1, "x", "text/plain", 1, UploadSource { ByteArrayInputStream(byteArrayOf(1)) }) {} } }
        assertEquals(AppError.Unauthenticated, unauthenticated)
    }
}
