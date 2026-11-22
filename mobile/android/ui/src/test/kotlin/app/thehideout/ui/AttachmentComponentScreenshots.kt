package app.thehideout.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import app.thehideout.core.model.Attachment
import app.thehideout.ui.components.AttachmentAudioRow
import app.thehideout.ui.components.AttachmentFileCard
import app.thehideout.ui.components.AttachmentVideoCard
import app.thehideout.ui.components.RemovedAttachmentPlaceholder
import app.thehideout.core.attachment.FileKind
import app.thehideout.ui.theme.Hideout
import org.junit.Test
import org.robolectric.annotation.Config

private const val ASSET = "file:///android_asset/fake_media/"
private val video = Attachment(1, "demo.mp4", "video/mp4", 590_746, ASSET + "clip-1.mp4", 960, 540, thumbnail = ASSET + "clip-1-poster.jpg")
private val audio = Attachment(2, "voice-note.m4a", "audio/mp4", 98_808, ASSET + "tone-1.m4a")
private val file = Attachment(3, "report.pdf", "application/pdf", 182_300, ASSET + "notes-1.txt")

private const val PHONE_Q = "w411dp-h891dp-xxhdpi"
private const val TABLET_Q = "w1280dp-h800dp-xhdpi"

/** The video card, audio row, file card and removed placeholder, each alone (ATT-02..04). */
class AttachmentComponentScreenshots : ScreenshotTest() {
    @Composable
    private fun Frame(content: @Composable () -> Unit) {
        val c = Hideout.colors
        Box(Modifier.background(c.bg).padding(16.dp).width(340.dp)) { content() }
    }

    private fun run(screen: String, device: Device, variant: Variant, content: @Composable () -> Unit) {
        compose.setThemed(variant) { Frame(content) }
        compose.capture(screen, device, variant)
    }

    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun videoCardPhoneDark() = run("attachment-video-card", Device.Phone, Variant.Dark) { AttachmentVideoCard(video, "") }
    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun videoCardPhoneLight() = run("attachment-video-card", Device.Phone, Variant.Light) { AttachmentVideoCard(video, "") }
    @Config(sdk = [35], qualifiers = TABLET_Q) @Test fun videoCardTabletDark() = run("attachment-video-card", Device.Tablet, Variant.Dark) { AttachmentVideoCard(video, "") }
    @Config(sdk = [35], qualifiers = TABLET_Q) @Test fun videoCardTabletLight() = run("attachment-video-card", Device.Tablet, Variant.Light) { AttachmentVideoCard(video, "") }

    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun audioRowPhoneDark() = run("attachment-audio-row", Device.Phone, Variant.Dark) { AttachmentAudioRow(audio, "") }
    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun audioRowPhoneLight() = run("attachment-audio-row", Device.Phone, Variant.Light) { AttachmentAudioRow(audio, "") }
    @Config(sdk = [35], qualifiers = TABLET_Q) @Test fun audioRowTabletDark() = run("attachment-audio-row", Device.Tablet, Variant.Dark) { AttachmentAudioRow(audio, "") }
    @Config(sdk = [35], qualifiers = TABLET_Q) @Test fun audioRowTabletLight() = run("attachment-audio-row", Device.Tablet, Variant.Light) { AttachmentAudioRow(audio, "") }

    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun fileCardPhoneDark() = run("attachment-file-card", Device.Phone, Variant.Dark) { AttachmentFileCard(file, "", FileKind.Pdf) }
    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun fileCardPhoneLight() = run("attachment-file-card", Device.Phone, Variant.Light) { AttachmentFileCard(file, "", FileKind.Pdf) }
    @Config(sdk = [35], qualifiers = TABLET_Q) @Test fun fileCardTabletDark() = run("attachment-file-card", Device.Tablet, Variant.Dark) { AttachmentFileCard(file, "", FileKind.Pdf) }
    @Config(sdk = [35], qualifiers = TABLET_Q) @Test fun fileCardTabletLight() = run("attachment-file-card", Device.Tablet, Variant.Light) { AttachmentFileCard(file, "", FileKind.Pdf) }

    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun removedPhoneDark() = run("attachment-removed", Device.Phone, Variant.Dark) { RemovedAttachmentPlaceholder() }
    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun removedPhoneLight() = run("attachment-removed", Device.Phone, Variant.Light) { RemovedAttachmentPlaceholder() }
    @Config(sdk = [35], qualifiers = TABLET_Q) @Test fun removedTabletDark() = run("attachment-removed", Device.Tablet, Variant.Dark) { RemovedAttachmentPlaceholder() }
    @Config(sdk = [35], qualifiers = TABLET_Q) @Test fun removedTabletLight() = run("attachment-removed", Device.Tablet, Variant.Light) { RemovedAttachmentPlaceholder() }
}
