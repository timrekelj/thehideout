package app.thehideout.ui

import android.os.Build
import androidx.test.core.app.ApplicationProvider
import app.thehideout.ui.components.AttachmentHttpFetch
import app.thehideout.ui.components.AttachmentIoException
import app.thehideout.ui.components.fetchAttachmentBytes
import app.thehideout.ui.components.fetchAttachmentToCache
import app.thehideout.ui.components.saveAttachmentToDownloads
import kotlinx.coroutines.runBlocking
import okhttp3.MediaType.Companion.toMediaTypeOrNull
import okhttp3.Protocol
import okhttp3.Request
import okhttp3.Response
import okhttp3.ResponseBody.Companion.toResponseBody
import org.junit.Assert.assertFalse
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.io.IOException

private fun fakeResponse(url: String, code: Int, body: String = "") = Response.Builder()
    .request(Request.Builder().url(url).build())
    .protocol(Protocol.HTTP_1_1)
    .code(code)
    .message(if (code == 404) "Not Found" else "OK")
    .body(body.toResponseBody("text/plain".toMediaTypeOrNull()))
    .build()

/** `AttachmentFiles.kt`'s failure paths (an I/O failure, or a non-2xx response), with the HTTP(S) call injected so nothing touches the network. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class AttachmentFilesTest {
    private val context = ApplicationProvider.getApplicationContext<android.app.Application>()

    @Test fun `fetchAttachmentBytes throws on an IOException`() {
        val fetch = AttachmentHttpFetch { throw IOException("boom") }
        val e = runCatching { runBlocking { fetchAttachmentBytes(context, "https://example.test/f", fetch) } }.exceptionOrNull()
        assert(e is AttachmentIoException) { "expected AttachmentIoException, got $e" }
    }

    @Test fun `fetchAttachmentBytes throws on a 404`() {
        val fetch = AttachmentHttpFetch { url -> fakeResponse(url, 404) }
        val e = runCatching { runBlocking { fetchAttachmentBytes(context, "https://example.test/f", fetch) } }.exceptionOrNull()
        assert(e is AttachmentIoException && e.message!!.contains("404")) { "expected a 404 message, got $e" }
    }

    @Test fun `fetchAttachmentToCache deletes the partial file on failure`() {
        val fetch = AttachmentHttpFetch { throw IOException("boom") }
        assertThrowsIo { runBlocking { fetchAttachmentToCache(context, "https://example.test/f", "x.bin", fetch) } }
        assertFalse(java.io.File(context.cacheDir, "attachments/x.bin").exists())
    }

    @Test fun `saveAttachmentToDownloads on a 404 does not leave a pending MediaStore row`() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return
        val fetch = AttachmentHttpFetch { url -> fakeResponse(url, 404) }
        assertThrowsIo {
            runBlocking { saveAttachmentToDownloads(context, "https://example.test/f", "y.bin", "application/octet-stream", fetch) }
        }
    }

    private fun assertThrowsIo(block: () -> Unit) {
        val e = runCatching(block).exceptionOrNull()
        assert(e is AttachmentIoException) { "expected AttachmentIoException, got $e" }
    }
}
