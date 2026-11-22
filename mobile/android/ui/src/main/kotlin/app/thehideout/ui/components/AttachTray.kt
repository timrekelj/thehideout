package app.thehideout.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.thehideout.core.attachment.FileKind
import app.thehideout.core.attachment.fileKind
import app.thehideout.core.attachment.humanSize
import app.thehideout.core.store.AttachDraft
import app.thehideout.core.store.AttachUploadPhase
import app.thehideout.core.store.DraftFile
import app.thehideout.core.store.PendingPreview
import app.thehideout.design.Radius
import app.thehideout.ui.icons.Sym
import app.thehideout.ui.icons.SymIcon
import app.thehideout.ui.theme.Hideout
import coil3.compose.AsyncImage

private fun trayIcon(kind: FileKind) = when (kind) {
    FileKind.Image -> Sym.Image
    FileKind.Video -> Sym.Videocam
    FileKind.Audio -> Sym.MusicNote
    FileKind.Pdf -> Sym.PictureAsPdf
    FileKind.Archive -> Sym.FolderZip
    FileKind.Text, FileKind.Doc -> Sym.Description
    FileKind.Code -> Sym.Code
    FileKind.Other -> Sym.Draft
}

/**
 * The composer's tray: a chip per attached file with its thumbnail or type icon, name, size,
 * upload progress, remove, and "Try again" on a failed one (ATT-09, desktop's `attach::tray`).
 * One wide [LazyRow] so it scrolls horizontally when it overflows; a note (too many, too large)
 * sits below it.
 */
@Composable
fun AttachTray(draft: AttachDraft, onRemove: (Long) -> Unit, onRetry: (Long) -> Unit, modifier: Modifier = Modifier) {
    if (draft.files.isEmpty() && draft.note == null) return
    val c = Hideout.colors
    Column(modifier.fillMaxWidth()) {
        if (draft.files.isNotEmpty()) {
            LazyRow(
                Modifier.fillMaxWidth().testTag("attach-tray"),
                contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 8.dp, vertical = 4.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                items(draft.files, key = { it.id }) { file -> AttachChip(file, onRemove = { onRemove(file.id) }, onRetry = { onRetry(file.id) }) }
            }
        }
        draft.note?.let { note ->
            Row(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 2.dp), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                SymIcon(Sym.Warning, null, tint = c.danger, size = 14.dp)
                Text(note, style = MaterialTheme.typography.labelSmall, color = c.danger)
            }
        }
    }
}

/**
 * A pending message's local file previews (ATT-09): what was sent, shown until the server's own
 * message replaces the pending row. An image loads straight from its own `content://` URI — never
 * through [app.thehideout.core.attachment.mediaUrl], which only trusts server media keys; anything
 * else is a small chip with its type icon, name and size. Fixed sizes only: this sits inside a row
 * measured by `IntrinsicSize.Min` upstream, which subcompose layouts can't be measured under.
 */
@Composable
fun PendingAttachmentPreviews(previews: List<PendingPreview>, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    FlowRow(modifier.padding(top = 4.dp), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        previews.forEach { p ->
            val kind = fileKind(p.contentType, p.name)
            if (kind == FileKind.Image && p.preview != null) {
                AsyncImage(
                    p.preview,
                    contentDescription = p.name,
                    modifier = Modifier.size(96.dp).clip(RoundedCornerShape(Radius.lg)).background(c.surfaceOverlay),
                    contentScale = ContentScale.Crop,
                )
            } else {
                Row(
                    Modifier.widthIn(max = 220.dp).clip(RoundedCornerShape(Radius.lg)).background(c.hover).padding(horizontal = 8.dp, vertical = 6.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    SymIcon(trayIcon(kind), null, tint = c.accentText, size = 16.dp)
                    Text(p.name, style = MaterialTheme.typography.labelLarge, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(humanSize(p.size), style = MaterialTheme.typography.labelSmall, color = c.textMuted)
                }
            }
        }
    }
}

@Composable
private fun AttachChip(file: DraftFile, onRemove: () -> Unit, onRetry: () -> Unit, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    val kind = fileKind(file.contentType, file.name)
    val failed = file.state.phase == AttachUploadPhase.Failed
    Row(
        modifier
            .testTag("attach-chip-${file.id}")
            .width(220.dp)
            .clip(RoundedCornerShape(Radius.lg))
            .background(if (failed) c.dangerBg else c.surfaceRaised)
            .padding(6.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(32.dp).clip(RoundedCornerShape(Radius.md)), contentAlignment = Alignment.Center) {
            if (kind == FileKind.Image && file.preview != null) {
                AsyncImage(file.preview, contentDescription = null, modifier = Modifier.size(32.dp).background(c.surfaceOverlay), contentScale = ContentScale.Crop)
            } else {
                Box(Modifier.size(32.dp).background(c.accentSubtle), contentAlignment = Alignment.Center) {
                    SymIcon(trayIcon(kind), null, tint = c.accentText, size = 16.dp)
                }
            }
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
            Text(file.name, style = MaterialTheme.typography.labelLarge, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
            val sub = when (file.state.phase) {
                AttachUploadPhase.Uploading -> "${humanSize(file.state.sent)} / ${humanSize(file.state.total)}"
                AttachUploadPhase.Queued -> "Waiting…"
                AttachUploadPhase.Ready -> humanSize(file.size)
                AttachUploadPhase.Failed -> file.state.reason ?: "Failed"
            }
            Text(sub, style = MaterialTheme.typography.labelSmall, color = if (failed) c.danger else c.textMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (file.state.phase == AttachUploadPhase.Uploading || file.state.phase == AttachUploadPhase.Queued) {
                val frac = if (file.state.total <= 0) 0f else (file.state.sent.toFloat() / file.state.total).coerceIn(0f, 1f)
                Row(Modifier.fillMaxWidth().height(3.dp).clip(RoundedCornerShape(Radius.full)).background(c.pressed)) {
                    Box(Modifier.fillMaxWidth(frac).height(3.dp).clip(RoundedCornerShape(Radius.full)).background(c.accent))
                }
            }
        }
        if (failed) {
            IconButton(onClick = onRetry, modifier = Modifier.size(28.dp)) { SymIcon(Sym.Refresh, "Try again", tint = c.textMuted, size = 16.dp) }
        }
        IconButton(onClick = onRemove, modifier = Modifier.size(28.dp)) { SymIcon(Sym.Close, "Remove ${file.name}", tint = c.textMuted, size = 16.dp) }
    }
}
