package app.thehideout.core

import app.thehideout.core.fake.FakeBackend
import kotlinx.coroutines.runBlocking
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

/** `FakeBackend.seedAttachments`: every attachment kind, into the otherwise-empty #design channel. */
class FakeBackendAttachmentsTest {
    @Test
    fun `design has the eight seeded attachment messages`() = runBlocking {
        val fake = FakeBackend()
        val page = fake.listMessages(FakeBackend.DESIGN, before = null, limit = 50)
        assertEquals(8, page.items.size)
        // Newest first: the removed message was seeded last, the single dashboard image first.
        assertEquals(listOf(1, 1, 1, 1, 4, 3, 2, 1), page.items.map { it.attachments.size })
    }

    @Test
    fun `the removed attachment has no file and is marked removed`() = runBlocking {
        val fake = FakeBackend()
        val page = fake.listMessages(FakeBackend.DESIGN, before = null, limit = 50)
        val removed = page.items.first().attachments.single()
        assertTrue(removed.removed)
        assertTrue(removed.file.isEmpty())
        assertEquals("invoice.xlsx", removed.filename)
    }

    @Test
    fun `every other seeded attachment keys to a bundled fake-media asset`() = runBlocking {
        val fake = FakeBackend()
        val page = fake.listMessages(FakeBackend.DESIGN, before = null, limit = 50)
        val withFiles = page.items.flatMap { it.attachments }.filterNot { it.removed }
        assertFalse(withFiles.isEmpty())
        assertTrue(withFiles.all { it.file.startsWith("file:///android_asset/fake_media/") })
    }
}
