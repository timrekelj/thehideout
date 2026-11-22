package app.thehideout.core

import app.thehideout.core.attachment.FileKind
import app.thehideout.core.attachment.GRID_GAP
import app.thehideout.core.attachment.GRID_H
import app.thehideout.core.attachment.clockLabel
import app.thehideout.core.attachment.fileKind
import app.thehideout.core.attachment.fitSize
import app.thehideout.core.attachment.humanSize
import app.thehideout.core.attachment.imageGrid
import app.thehideout.core.attachment.mediaUrl
import app.thehideout.core.attachment.opensInline
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertThrows
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

/** Ports desktop's `attachments.rs` tests: sizes read in binary units, clocks, kinds, grid maths. */
class AttachmentLayoutTest {
    @Test
    fun `sizes read in binary units`() {
        assertEquals("0 B", humanSize(0))
        assertEquals("1023 B", humanSize(1023))
        assertEquals("1.0 KB", humanSize(1024))
        assertEquals("5.0 MB", humanSize(5 * 1024 * 1024L))
        assertEquals("50.0 GB", humanSize(50L * 1024 * 1024 * 1024))
    }

    @Test
    fun clocks() {
        assertEquals("0:07", clockLabel(7.4))
        assertEquals("12:03", clockLabel(723.0))
        assertEquals("1:02:03", clockLabel(3723.0))
        assertEquals("0:00", clockLabel(Double.NaN))
    }

    @Test
    fun `kinds come from the type then the name`() {
        assertEquals(FileKind.Image, fileKind("image/png", "a"))
        assertEquals(FileKind.Video, fileKind("application/octet-stream", "clip.MP4"))
        assertEquals(FileKind.Pdf, fileKind("application/pdf", "x"))
        assertEquals(FileKind.Archive, fileKind("application/zip", "x"))
        assertEquals(FileKind.Text, fileKind("", "notes.txt"))
        assertEquals(FileKind.Other, fileKind("", "blob"))
        assertTrue(FileKind.Pdf.opensInline() && !FileKind.Archive.opensInline())
    }

    @Test
    fun `code and document kinds come from the extension too`() {
        assertEquals(FileKind.Code, fileKind("", "main.rs"))
        assertEquals(FileKind.Code, fileKind("application/json", "x"))
        assertEquals(FileKind.Code, fileKind("", "notes.yaml"))
        assertEquals(FileKind.Doc, fileKind("", "report.docx"))
        assertEquals(FileKind.Doc, fileKind("application/msword", "x"))
        assertEquals(FileKind.Doc, fileKind("", "sheet.xlsx"))
    }

    @Test
    fun `a single picture keeps its aspect within the limits`() {
        assertEquals(400f to 225f, fitSize(1600 to 900, 400f, 300f))
        assertEquals(150f to 300f, fitSize(600 to 1200, 400f, 300f))
        assertEquals(200f to 100f, fitSize(200 to 100, 400f, 300f))
        assertEquals(400f to 225f, fitSize(null, 400f, 300f))
    }

    @Test
    fun `a tiny picture is scaled up to a legible box, keeping its aspect`() {
        // 20x10 scaled by the max-dimension factor so neither side reads as a speck: 96x48.
        assertEquals(96f to 48f, fitSize(20 to 10, 400f, 300f))
        // A tiny square grows to exactly 96x96.
        assertEquals(96f to 96f, fitSize(10 to 10, 400f, 300f))
    }

    @Test
    fun `grids follow the two up three and two by two rules`() {
        val (size2, two) = imageGrid(2)
        assertEquals(400f to GRID_H, size2)
        assertEquals(2, two.size)
        assertEquals(GRID_H, two[0].h)
        assertEquals(two[0].w + GRID_GAP, two[1].x)

        val (_, three) = imageGrid(3)
        assertEquals(GRID_H, three[0].h, "the first is tall")
        assertEquals(GRID_H, three[1].h + GRID_GAP + three[2].h, "two stack at the right")

        for (n in listOf(4, 7)) {
            val (_, cells) = imageGrid(n)
            assertEquals(4, cells.size, "never more than four tiles")
            assertEquals(cells[0].w, cells[3].w)
            assertEquals(cells[0].h + GRID_GAP, cells[2].y)
        }
        assertTrue(imageGrid(0).second.isEmpty())
    }

    @Test
    fun `media urls join the base and the key, or pass only the fake backend's bundled assets through`() {
        assertEquals("http://h:1/media/abc", mediaUrl("http://h:1", "abc"))
        assertEquals("http://h:1/media/abc", mediaUrl("http://h:1/", "abc"))
        assertEquals("file:///android_asset/x.png", mediaUrl("http://h:1", "file:///android_asset/x.png"))
    }

    @Test
    fun `a key with a foreign scheme is refused, not passed through`() {
        assertThrows(IllegalArgumentException::class.java) { mediaUrl("http://h:1", "https://evil.example/track.png") }
        assertThrows(IllegalArgumentException::class.java) { mediaUrl("http://h:1", "content://com.evil/leak") }
        assertThrows(IllegalArgumentException::class.java) { mediaUrl("http://h:1", "file:///etc/passwd") }
    }
}
