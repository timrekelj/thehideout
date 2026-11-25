package app.thehideout.core

import app.thehideout.core.api.AppError
import app.thehideout.core.api.PendingAttachment
import app.thehideout.core.api.UploadSource
import app.thehideout.core.attachment.DEFAULT_MAX_UPLOAD_BYTES
import app.thehideout.core.attachment.MAX_ATTACHMENTS
import app.thehideout.core.fake.FakeBackend.Companion.DEV
import app.thehideout.core.fake.FakeBackend.Companion.GENERAL
import app.thehideout.core.store.*
import org.junit.jupiter.api.Assertions.*
import org.junit.jupiter.api.Test
import java.io.ByteArrayInputStream
import java.time.Instant

private fun source(size: Int = 10): UploadSource = UploadSource { ByteArrayInputStream(ByteArray(size)) }

private fun picked(name: String, size: Long = 10, contentType: String = "image/png", preview: String? = "content://picked/$name") =
    PickedFile(name, contentType, size, preview, source(size.toInt()))

class AttachReducerTest {
    private val s0 = loadedState()
    private val T0 = Instant.parse("2026-11-22T10:00:00Z")

    // ---- admission ----

    @Test fun `a picked file is queued and asks the runner to upload it`() {
        val s = s0.reduce(Action.AttachFiles(GENERAL, listOf(picked("a.png"))))
        val draft = s.attachDraft(GENERAL)
        assertEquals(1, draft.files.size)
        assertEquals(AttachUploadPhase.Queued, draft.files.single().state.phase)
        assertNull(draft.note)
    }

    @Test fun `up to the cap is admitted, the rest note the limit and are not queued`() {
        val files = (1..MAX_ATTACHMENTS + 2).map { picked("f$it.png") }
        val s = s0.reduce(Action.AttachFiles(GENERAL, files))
        val draft = s.attachDraft(GENERAL)
        assertEquals(MAX_ATTACHMENTS, draft.files.size)
        assertEquals("You can attach up to $MAX_ATTACHMENTS files to a message", draft.note)
    }

    @Test fun `a file over the limit is refused and named in the note, others still admitted`() {
        // The note reflects the last file processed: put the oversized one last so it survives in the assertion.
        val s = s0.reduce(Action.AttachFiles(GENERAL, listOf(picked("ok.png"), picked("big.bin", size = DEFAULT_MAX_UPLOAD_BYTES + 1))))
        val draft = s.attachDraft(GENERAL)
        assertEquals(1, draft.files.size)
        assertEquals("ok.png", draft.files.single().name)
        val note = draft.note ?: ""
        assertTrue(note.startsWith("big.bin is"), note)
    }

    // ---- progress, success, failure ----

    @Test fun `progress only moves an uploading file, clamped to its total`() {
        var s = s0.reduce(Action.AttachFiles(GENERAL, listOf(picked("a.png", size = 100))))
        val id = s.attachDraft(GENERAL).files.single().id
        // Still queued: a progress tick before the runner started it changes nothing.
        s = s.reduce(Action.AttachProgress(id, 50, 100))
        assertEquals(AttachUploadPhase.Queued, s.attachDraft(GENERAL).files.single().state.phase)

        s = s.reduce(Action.AttachStarted(id))
        s = s.reduce(Action.AttachProgress(id, 40, 100))
        assertEquals(40L, s.attachDraft(GENERAL).files.single().state.sent)
    }

    @Test fun `a successful upload turns Ready and keeps the pending id for Send`() {
        var s = s0.reduce(Action.AttachFiles(GENERAL, listOf(picked("a.png"))))
        val id = s.attachDraft(GENERAL).files.single().id
        s = s.reduce(Action.AttachStarted(id))
        s = s.reduce(Action.AttachUploaded(id, Result.success(PendingAttachment(555, "a.png", "image/png", 10, T0))))
        val f = s.attachDraft(GENERAL).files.single()
        assertEquals(AttachUploadPhase.Ready, f.state.phase)
        assertEquals(555L, f.state.pendingId)
    }

    @Test fun `a failed upload turns Failed with a readable reason`() {
        var s = s0.reduce(Action.AttachFiles(GENERAL, listOf(picked("a.png"))))
        val id = s.attachDraft(GENERAL).files.single().id
        s = s.reduce(Action.AttachUploaded(id, Result.failure(AppError.TooLarge)))
        val f = s.attachDraft(GENERAL).files.single()
        assertEquals(AttachUploadPhase.Failed, f.state.phase)
        assertEquals("Too large", f.state.reason)
    }

    // ---- remove and retry ----

    @Test fun `removing a file drops it and cancels it if it was still going up`() {
        var s = s0.reduce(Action.AttachFiles(GENERAL, listOf(picked("a.png"))))
        val id = s.attachDraft(GENERAL).files.single().id
        val r = Reducer.reduce(s, Action.RemoveAttachment(id))
        assertTrue(r.state.attachDraft(GENERAL).files.isEmpty())
        assertEquals(listOf(Effect.CancelUpload(id)), r.effects)
    }

    @Test fun `removing a ready file does not ask for a cancel`() {
        var s = s0.reduce(Action.AttachFiles(GENERAL, listOf(picked("a.png"))))
        val id = s.attachDraft(GENERAL).files.single().id
        s = s.reduce(Action.AttachUploaded(id, Result.success(PendingAttachment(1, "a.png", "image/png", 10, T0))))
        val r = Reducer.reduce(s, Action.RemoveAttachment(id))
        assertTrue(r.effects.isEmpty())
    }

    @Test fun `retrying a failed file queues it again with an upload effect`() {
        var s = s0.reduce(Action.AttachFiles(GENERAL, listOf(picked("a.png"))))
        val id = s.attachDraft(GENERAL).files.single().id
        s = s.reduce(Action.AttachUploaded(id, Result.failure(AppError.Unavailable("x"))))
        val r = Reducer.reduce(s, Action.RetryAttachment(id))
        assertEquals(AttachUploadPhase.Queued, r.state.attachDraft(GENERAL).files.single().state.phase)
        assertEquals(1, r.effects.size)
        assertTrue(r.effects.single() is Effect.UploadAttachment)
    }

    @Test fun `retrying a file that is not failed does nothing`() {
        var s = s0.reduce(Action.AttachFiles(GENERAL, listOf(picked("a.png"))))
        val id = s.attachDraft(GENERAL).files.single().id
        val r = Reducer.reduce(s, Action.RetryAttachment(id))
        assertEquals(s, r.state)
        assertTrue(r.effects.isEmpty())
    }

    // ---- send ----

    @Test fun `send is blocked while a file is queued or uploading, even with text`() {
        val s = s0.reduce(Action.AttachFiles(GENERAL, listOf(picked("a.png"))))
        val r = Reducer.reduce(s, Action.Send("hello", T0))
        assertEquals(s, r.state)
        assertTrue(r.effects.isEmpty())
    }

    @Test fun `send with no text but a ready file goes with no content`() {
        var s = s0.reduce(Action.AttachFiles(GENERAL, listOf(picked("a.png"))))
        val id = s.attachDraft(GENERAL).files.single().id
        s = s.reduce(Action.AttachUploaded(id, Result.success(PendingAttachment(42, "a.png", "image/png", 10, T0))))
        val r = Reducer.reduce(s, Action.Send("", T0))
        assertEquals(1, r.state.outgoing.size)
        assertEquals(listOf(42L), r.state.outgoing.single().attachmentIds)
        assertEquals("a.png", r.state.outgoing.single().attachments.single().name)
        assertTrue(r.state.attachDraft(GENERAL).files.isEmpty(), "the sent file leaves the tray")
        assertEquals(listOf(Effect.SendMessage(GENERAL, "", r.state.outgoing.single().nonce, listOf(42L))), r.effects)
    }

    @Test fun `sending leaves a failed file in the tray instead of attaching it`() {
        var s = s0.reduce(Action.AttachFiles(GENERAL, listOf(picked("a.png"), picked("b.png"))))
        val (okId, failId) = s.attachDraft(GENERAL).files.map { it.id }
        s = s.reduce(Action.AttachUploaded(okId, Result.success(PendingAttachment(1, "a.png", "image/png", 10, T0))))
        s = s.reduce(Action.AttachUploaded(failId, Result.failure(AppError.TooLarge)))
        val r = Reducer.reduce(s, Action.Send("hi", T0))
        assertEquals(listOf(1L), r.state.outgoing.single().attachmentIds)
        val left = r.state.attachDraft(GENERAL).files
        assertEquals(1, left.size)
        assertEquals(AttachUploadPhase.Failed, left.single().state.phase)
    }

    @Test fun `with neither text nor a ready file, send does nothing`() {
        val r = Reducer.reduce(s0, Action.Send("", T0))
        assertEquals(s0, r.state)
    }

    @Test fun `attachments in one channel never block Send in another`() {
        val s = s0.reduce(Action.AttachFiles(DEV, listOf(picked("a.png"))))
        val r = Reducer.reduce(s, Action.Send("hi", T0))
        assertEquals(1, r.state.outgoing.size)
    }
}
