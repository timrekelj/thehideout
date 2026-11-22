package app.thehideout.ui

import android.app.Application
import android.net.Uri
import androidx.test.core.app.ApplicationProvider
import app.thehideout.ui.components.pickedFileFrom
import app.thehideout.ui.components.pruneUploadCache
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.io.File

/**
 * Resolving a picked URI into a [app.thehideout.core.store.PickedFile] (`pickedFileFrom`): the
 * picker → upload path's first step, with a real file behind a `file://` URI.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class AttachPickerTest {
    private val context = ApplicationProvider.getApplicationContext<Application>()

    @Test fun `a picked text file resolves to name, type, size, preview and a re-openable source`() = runBlocking {
        val f = File(context.cacheDir, "picker-note.txt").apply { writeText("hello picker") }
        val picked = pickedFileFrom(context, Uri.fromFile(f))
        assertEquals("picker-note.txt", picked.name)
        assertEquals("text/plain", picked.contentType)
        assertEquals(12L, picked.size)
        assertEquals(Uri.fromFile(f).toString(), picked.preview)
        assertEquals("hello picker", picked.source.open().use { it.readBytes().toString(Charsets.UTF_8) })
    }

    @Test fun `a picked image is copied to the upload cache so its EXIF can be scrubbed, and the source streams the copy`() = runBlocking {
        val f = File(context.cacheDir, "picker-photo.png").apply { writeBytes(byteArrayOf(0x89.toByte(), 0x50, 0x4E, 0x47, 1, 2, 3, 4)) }
        val picked = pickedFileFrom(context, Uri.fromFile(f))
        assertEquals("picker-photo.png", picked.name)
        assertEquals("image/png", picked.contentType)
        assertEquals(8L, picked.size)
        // The source streams the scrubbed cache copy, not the original: a retry re-reads it from the start.
        assertEquals(8L, picked.source.open().use { it.readBytes().size.toLong() })
        val copy = File(context.cacheDir, "uploads").listFiles().orEmpty().singleOrNull { it.name.endsWith("picker-photo.png") }
        assertTrue("the EXIF-scrub copy exists in the uploads cache", copy != null)
    }

    @Test fun `pruneUploadCache deletes stale scrub copies`() {
        val dir = File(context.cacheDir, "uploads").apply { mkdirs() }
        val stale = File(dir, "u1-old.txt").apply { writeText("old"); setLastModified(System.currentTimeMillis() - 48 * 60 * 60 * 1000L) }
        val fresh = File(dir, "u2-new.txt").apply { writeText("new") }
        pruneUploadCache(context)
        assertTrue(!stale.exists())
        assertTrue(fresh.exists())
    }
}
