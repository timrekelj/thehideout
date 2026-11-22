package app.thehideout.core.store

import app.thehideout.core.api.UploadSource

/** Where one attached file is in its upload (ATT-09, the Kotlin twin of desktop's `UploadState`). */
enum class AttachUploadPhase { Queued, Uploading, Ready, Failed }

/** [AttachUploadPhase.Uploading]'s `sent`/`total`; [AttachUploadPhase.Ready]'s `pendingId` links `SendMessage`; [AttachUploadPhase.Failed]'s `reason` is shown in the chip. */
data class AttachUploadState(
    val phase: AttachUploadPhase,
    val sent: Long = 0,
    val total: Long = 0,
    val pendingId: Long? = null,
    val reason: String? = null,
)

/** One file the platform picker, camera or a shared/pasted image handed the composer; [source] streams its bytes (and streams them again on retry). */
data class PickedFile(
    val name: String,
    val contentType: String,
    val size: Long,
    /** Shown in the tray and, once sent, in the pending message: a `content://` URI Coil can load directly, or a bundled-asset key in tests/previews. */
    val preview: String?,
    val source: UploadSource,
)

/** One file in a channel's composer tray, from the moment it is picked to the moment it is sent or removed (desktop's `DraftFile`). */
data class DraftFile(
    val id: Long,
    val name: String,
    val contentType: String,
    val size: Long,
    val preview: String?,
    val source: UploadSource,
    val state: AttachUploadState,
)

/** One channel's files waiting to be sent (desktop's `Draft`). */
data class AttachDraft(val files: List<DraftFile> = emptyList(), val note: String? = null) {
    /** Blocks `Send`: at least one file has not reached [AttachUploadPhase.Ready] or [AttachUploadPhase.Failed] yet. */
    val uploading: Boolean get() = files.any { it.state.phase == AttachUploadPhase.Queued || it.state.phase == AttachUploadPhase.Uploading }
    val readyCount: Int get() = files.count { it.state.phase == AttachUploadPhase.Ready }
}

/** A sent file's local preview, kept on [Outgoing] until the server's message replaces it (desktop's `OutFile`). */
data class PendingPreview(val name: String, val contentType: String, val size: Long, val preview: String?)
