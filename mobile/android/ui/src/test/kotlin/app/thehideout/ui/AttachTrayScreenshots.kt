package app.thehideout.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import app.thehideout.core.api.UploadSource
import app.thehideout.core.store.AttachDraft
import app.thehideout.core.store.AttachUploadPhase
import app.thehideout.core.store.AttachUploadState
import app.thehideout.core.store.DraftFile
import app.thehideout.ui.components.AttachTray
import app.thehideout.ui.theme.Hideout
import org.junit.Test
import org.robolectric.annotation.Config
import java.io.ByteArrayInputStream

private const val ASSET = "file:///android_asset/fake_media/"
private const val PHONE_Q = "w411dp-h891dp-xxhdpi"
private const val TABLET_Q = "w1280dp-h800dp-xhdpi"

private fun source(): UploadSource = UploadSource { ByteArrayInputStream(ByteArray(16)) }

private fun file(id: Long, name: String, contentType: String, size: Long, preview: String? = null, state: AttachUploadState) =
    DraftFile(id, name, contentType, size, preview, source(), state)

/** An image mid-upload (~45%), a finished-but-sent video at 100%, and a text file still waiting. */
private val uploadingDraft = AttachDraft(
    listOf(
        file(1, "portrait-1.jpg", "image/jpeg", 4_812_544, ASSET + "portrait-1.jpg", AttachUploadState(AttachUploadPhase.Uploading, sent = 2_156_036, total = 4_812_544)),
        file(2, "clip-1.mp4", "video/mp4", 590_746, null, AttachUploadState(AttachUploadPhase.Uploading, sent = 590_746, total = 590_746)),
        file(3, "field-notes.txt", "text/plain", 18_204, null, AttachUploadState(AttachUploadPhase.Queued)),
    ),
)

/** Everything reached the server: thumbnails, names and sizes, remove buttons only. */
private val readyDraft = AttachDraft(
    listOf(
        file(1, "portrait-1.jpg", "image/jpeg", 4_812_544, ASSET + "portrait-1.jpg", AttachUploadState(AttachUploadPhase.Ready, sent = 4_812_544, total = 4_812_544, pendingId = 11)),
        file(2, "report.pdf", "application/pdf", 182_300, null, AttachUploadState(AttachUploadPhase.Ready, sent = 182_300, total = 182_300, pendingId = 12)),
        file(3, "landscape-2.jpg", "image/jpeg", 3_204_118, ASSET + "landscape-2.jpg", AttachUploadState(AttachUploadPhase.Ready, sent = 3_204_118, total = 3_204_118, pendingId = 13)),
    ),
)

/** One upload failed (danger chip, reason, "Try again") next to a ready one, with the note row below. */
private val failedDraft = AttachDraft(
    listOf(
        file(1, "clip-1.mp4", "video/mp4", 590_746, null, AttachUploadState(AttachUploadPhase.Failed, reason = "Upload failed")),
        file(2, "portrait-1.jpg", "image/jpeg", 4_812_544, ASSET + "portrait-1.jpg", AttachUploadState(AttachUploadPhase.Ready, sent = 4_812_544, total = 4_812_544, pendingId = 11)),
    ),
    note = "big-video.mp4 is 68.0 MB, over the 50.0 MB limit",
)

/** The composer's attachment tray in each upload phase — uploading with progress, ready, and
 *  failed with "Try again" plus the note row (ATT-09) — dark/light on phone and tablet. */
class AttachTrayScreenshots : ScreenshotTest() {
    @Composable
    private fun Frame(content: @Composable () -> Unit) {
        val c = Hideout.colors
        Box(Modifier.background(c.bg).padding(16.dp)) { content() }
    }

    private fun run(screen: String, device: Device, variant: Variant, draft: AttachDraft) {
        compose.setThemed(variant) { Frame { AttachTray(draft, onRemove = {}, onRetry = {}) } }
        compose.capture(screen, device, variant)
    }

    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun uploadingPhoneDark() = run("attach-tray-uploading", Device.Phone, Variant.Dark, uploadingDraft)
    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun uploadingPhoneLight() = run("attach-tray-uploading", Device.Phone, Variant.Light, uploadingDraft)
    @Config(sdk = [35], qualifiers = TABLET_Q) @Test fun uploadingTabletDark() = run("attach-tray-uploading", Device.Tablet, Variant.Dark, uploadingDraft)
    @Config(sdk = [35], qualifiers = TABLET_Q) @Test fun uploadingTabletLight() = run("attach-tray-uploading", Device.Tablet, Variant.Light, uploadingDraft)

    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun readyPhoneDark() = run("attach-tray-ready", Device.Phone, Variant.Dark, readyDraft)
    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun readyPhoneLight() = run("attach-tray-ready", Device.Phone, Variant.Light, readyDraft)
    @Config(sdk = [35], qualifiers = TABLET_Q) @Test fun readyTabletDark() = run("attach-tray-ready", Device.Tablet, Variant.Dark, readyDraft)
    @Config(sdk = [35], qualifiers = TABLET_Q) @Test fun readyTabletLight() = run("attach-tray-ready", Device.Tablet, Variant.Light, readyDraft)

    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun failedPhoneDark() = run("attach-tray-failed", Device.Phone, Variant.Dark, failedDraft)
    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun failedPhoneLight() = run("attach-tray-failed", Device.Phone, Variant.Light, failedDraft)
    @Config(sdk = [35], qualifiers = TABLET_Q) @Test fun failedTabletDark() = run("attach-tray-failed", Device.Tablet, Variant.Dark, failedDraft)
    @Config(sdk = [35], qualifiers = TABLET_Q) @Test fun failedTabletLight() = run("attach-tray-failed", Device.Tablet, Variant.Light, failedDraft)
}
