package app.thehideout.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.media3.common.MediaItem
import androidx.media3.common.Player
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.ui.compose.PlayerSurface
import app.thehideout.core.attachment.CARD_W
import app.thehideout.core.attachment.FileKind
import app.thehideout.core.attachment.MEDIA_MAX_H
import app.thehideout.core.attachment.MEDIA_MAX_W
import app.thehideout.core.attachment.clockLabel
import app.thehideout.core.attachment.fileKind
import app.thehideout.core.attachment.fitSize
import app.thehideout.core.attachment.humanSize
import app.thehideout.core.attachment.imageGrid
import app.thehideout.core.attachment.mediaUrl
import app.thehideout.core.attachment.opensInline
import app.thehideout.core.model.Attachment
import app.thehideout.design.Radius
import app.thehideout.ui.icons.Sym
import app.thehideout.ui.icons.SymIcon
import app.thehideout.ui.theme.Hideout
import coil3.compose.AsyncImagePainter
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

/** What the lightbox shows: one picture or video of a message, by its media key. */
data class LightboxItem(
    val key: String,
    val name: String,
    val width: Int,
    val height: Int,
    val isVideo: Boolean,
    val contentType: String = "",
)

/** Opens the lightbox on `items`, starting at `startIndex` (ATT-05). */
data class LightboxRequest(val items: List<LightboxItem>, val startIndex: Int)

private fun Attachment.toLightboxItem(isVideo: Boolean = false) =
    LightboxItem(file, filename, width ?: 0, height ?: 0, isVideo, contentType)

/** An attachment picture with a sized placeholder and a tap-to-retry error state (ATT-01). */
@Composable
fun AttachmentImage(
    url: String,
    contentDescription: String?,
    modifier: Modifier = Modifier,
    contentScale: ContentScale = ContentScale.Crop,
) {
    // A plain AsyncImage, not SubcomposeAsyncImage: this sits inside a Row sized by
    // IntrinsicSize.Min upstream (MessageRow), which SubcomposeLayout-based layouts can't be
    // measured under. The sized placeholder and retry overlay are plain (non-subcompose) Boxes.
    var retry by remember(url) { mutableIntStateOf(0) }
    var state by remember(url, retry) { mutableStateOf<AsyncImagePainter.State>(AsyncImagePainter.State.Empty) }
    Box(modifier) {
        androidx.compose.runtime.key(retry) {
            coil3.compose.AsyncImage(
                model = url,
                contentDescription = contentDescription,
                modifier = Modifier.fillMaxSize(),
                contentScale = contentScale,
                onState = { state = it },
            )
        }
        when (state) {
            is AsyncImagePainter.State.Error -> Box(
                Modifier.fillMaxSize().background(MediaColors.back).clickable { retry++ },
                contentAlignment = Alignment.Center,
            ) { SymIcon(Sym.Refresh, "Retry", tint = MediaColors.ink) }
            is AsyncImagePainter.State.Loading, is AsyncImagePainter.State.Empty ->
                Box(Modifier.fillMaxSize().background(MediaColors.back))
            is AsyncImagePainter.State.Success -> Unit
        }
    }
}

/**
 * The pictures of one message as a grid (ATT-01): one keeps its aspect, two sit side by side,
 * three put the first tall on the left, four or more are a 2x2 with "+N" on the last tile.
 */
@Composable
fun AttachmentImageGrid(images: List<Attachment>, mediaBase: String, modifier: Modifier = Modifier, onOpen: (Int) -> Unit = {}) {
    if (images.isEmpty()) return
    val shown = images.take(4)
    val single = images.first().let { a -> a.width?.let { w -> a.height?.let { h -> w to h } } }
    androidx.compose.ui.layout.Layout(
        modifier = modifier,
        content = {
            shown.forEachIndexed { i, att ->
                Box(Modifier.clip(RoundedCornerShape(Radius.lg)).clickable { onOpen(i) }) {
                    AttachmentImage(
                        url = mediaUrl(mediaBase, att.thumbnail?.takeIf { it.isNotEmpty() } ?: att.file),
                        contentDescription = "View ${att.filename}",
                        modifier = Modifier.fillMaxSize(),
                    )
                    if (i == shown.lastIndex && images.size > shown.size) {
                        Box(Modifier.fillMaxSize().background(MediaColors.dim), contentAlignment = Alignment.Center) {
                            Text("+${images.size - shown.size + 1}", color = MediaColors.ink, style = MaterialTheme.typography.headlineSmall)
                        }
                    }
                }
            }
        },
    ) { measurables, constraints ->
        // The width adapts to the column (never past MEDIA_MAX_W): a custom Layout, not
        // BoxWithConstraints, since this sits inside a Row sized by IntrinsicSize.Min upstream
        // (MessageRow), which SubcomposeLayout-based layouts can't be measured under.
        val maxW = minOf(constraints.maxWidth.toDp().value, MEDIA_MAX_W)
        val (gridSize, cells) = imageGrid(shown.size, single, maxW)
        val (gw, gh) = gridSize
        val placeables = measurables.mapIndexed { i, m -> m.measure(androidx.compose.ui.unit.Constraints.fixed(cells[i].w.dp.roundToPx(), cells[i].h.dp.roundToPx())) }
        layout(gw.dp.roundToPx(), gh.dp.roundToPx()) {
            placeables.forEachIndexed { i, p -> p.placeRelative(cells[i].x.dp.roundToPx(), cells[i].y.dp.roundToPx()) }
        }
    }
}

/** Play/pause, position, duration and mute, read off an [ExoPlayer] (or an audio-only one). */
private class MediaPlayerState {
    var isPlaying by mutableStateOf(false)
    var positionMs by mutableLongStateOf(0L)
    var durationMs by mutableLongStateOf(0L)
    var muted by mutableStateOf(false)
}

@Composable
private fun rememberMediaPlayerState(player: ExoPlayer?): MediaPlayerState {
    val state = remember { MediaPlayerState() }
    DisposableEffect(player) {
        if (player == null) return@DisposableEffect onDispose {}
        val listener = object : Player.Listener {
            override fun onIsPlayingChanged(isPlaying: Boolean) {
                state.isPlaying = isPlaying
            }
            override fun onEvents(p: Player, events: Player.Events) {
                state.durationMs = p.duration.coerceAtLeast(0L)
                state.positionMs = p.currentPosition.coerceAtLeast(0L)
                state.muted = p.volume == 0f
            }
        }
        player.addListener(listener)
        onDispose { player.removeListener(listener) }
    }
    LaunchedEffect(player, state.isPlaying) {
        while (isActive && player != null && state.isPlaying) {
            state.positionMs = player.currentPosition.coerceAtLeast(0L)
            delay(200)
        }
    }
    return state
}

/** A draggable progress track (desktop's scrub bar): tap or drag anywhere to seek. */
@Composable
private fun ScrubBar(fraction: Float, modifier: Modifier = Modifier, trackColor: Color, fillColor: Color, onSeek: (Float) -> Unit) {
    Box(
        modifier
            .height(28.dp)
            .fillMaxWidth()
            .pointerInput(onSeek) {
                val width = size.width.toFloat().coerceAtLeast(1f)
                coroutineScope {
                    launch { detectTapGestures { offset -> onSeek((offset.x / width).coerceIn(0f, 1f)) } }
                    launch { detectDragGestures { change, _ -> onSeek((change.position.x / width).coerceIn(0f, 1f)) } }
                }
            },
        contentAlignment = Alignment.CenterStart,
    ) {
        Box(Modifier.fillMaxWidth().height(4.dp).clip(RoundedCornerShape(Radius.full)).background(trackColor)) {
            Box(Modifier.fillMaxWidth(fraction.coerceIn(0f, 1f)).height(4.dp).clip(RoundedCornerShape(Radius.full)).background(fillColor))
        }
    }
}

/** A video attachment: a poster with a play button, in-place playback with a bar (ATT-02). */
@Composable
fun AttachmentVideoCard(attachment: Attachment, mediaBase: String, modifier: Modifier = Modifier, onExpand: () -> Unit = {}) {
    val natural = attachment.width?.let { ww -> attachment.height?.let { hh -> ww to hh } }
    androidx.compose.ui.layout.Layout(
        modifier = modifier,
        content = { AttachmentVideoCardSized(attachment, mediaBase, onExpand) },
    ) { measurables, constraints ->
        // A custom Layout, not BoxWithConstraints: see AttachmentImageGrid's measure lambda.
        val maxW = minOf(constraints.maxWidth.toDp().value, MEDIA_MAX_W)
        val (w, h) = fitSize(natural, maxW, MEDIA_MAX_H)
        val fixed = androidx.compose.ui.unit.Constraints.fixed(w.dp.roundToPx(), h.dp.roundToPx())
        val placeable = measurables.single().measure(fixed)
        layout(placeable.width, placeable.height) { placeable.placeRelative(0, 0) }
    }
}

@Composable
private fun AttachmentVideoCardSized(attachment: Attachment, mediaBase: String, onExpand: () -> Unit) {
    val context = LocalContext.current
    var player by remember(attachment.file) { mutableStateOf<ExoPlayer?>(null) }
    var started by remember(attachment.file) { mutableStateOf(false) }
    DisposableEffect(attachment.file) { onDispose { player?.release(); player = null } }
    val ui = rememberMediaPlayerState(player)

    fun ensurePlayer(): ExoPlayer = player ?: ExoPlayer.Builder(context).build().apply {
        setMediaItem(MediaItem.fromUri(mediaUrl(mediaBase, attachment.file)))
        prepare()
        playWhenReady = true
        player = this
        started = true
    }

    Box(Modifier.fillMaxSize().clip(RoundedCornerShape(Radius.lg)).background(MediaColors.back)) {
        if (!started) {
            attachment.thumbnail?.takeIf { it.isNotEmpty() }?.let {
                AttachmentImage(mediaUrl(mediaBase, it), "Video poster", Modifier.fillMaxSize(), ContentScale.Fit)
            }
            Box(
                Modifier.size(48.dp).align(Alignment.Center).clip(RoundedCornerShape(Radius.full))
                    .background(MediaColors.dim).clickable { ensurePlayer() },
                contentAlignment = Alignment.Center,
            ) { SymIcon(Sym.PlayArrow, "Play video", tint = MediaColors.ink) }
        } else {
            val p = player
            if (p != null) {
                PlayerSurface(player = p, modifier = Modifier.fillMaxSize().clickable { if (ui.isPlaying) p.pause() else p.play() })
            }
            Row(
                Modifier.fillMaxWidth().height(36.dp).align(Alignment.BottomCenter).background(MediaColors.dim)
                    .padding(horizontal = 6.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                IconButton(onClick = { if (ui.isPlaying) p?.pause() else p?.play() }) {
                    SymIcon(if (ui.isPlaying) Sym.Pause else Sym.PlayArrow, if (ui.isPlaying) "Pause" else "Play", tint = MediaColors.ink)
                }
                ScrubBar(
                    fraction = if (ui.durationMs > 0) ui.positionMs.toFloat() / ui.durationMs else 0f,
                    modifier = Modifier.weight(1f),
                    trackColor = MediaColors.ink.copy(alpha = 0.3f),
                    fillColor = MediaColors.ink,
                    onSeek = { f -> p?.seekTo((f * ui.durationMs).toLong()) },
                )
                Text(
                    "${clockLabel(ui.positionMs / 1000.0)} / ${clockLabel(ui.durationMs / 1000.0)}",
                    style = MaterialTheme.typography.labelSmall,
                    color = MediaColors.ink,
                )
                IconButton(onClick = { p?.volume = if (ui.muted) 1f else 0f }) {
                    SymIcon(if (ui.muted) Sym.VolumeOff else Sym.VolumeUp, if (ui.muted) "Unmute" else "Mute", tint = MediaColors.ink)
                }
                IconButton(onClick = onExpand) { SymIcon(Sym.Fullscreen, "Expand", tint = MediaColors.ink) }
            }
        }
    }
}

/** A compact play/pause/scrub row for an audio attachment (ATT-03). */
@Composable
fun AttachmentAudioRow(attachment: Attachment, mediaBase: String, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var player by remember(attachment.file) { mutableStateOf<ExoPlayer?>(null) }
    DisposableEffect(attachment.file) { onDispose { player?.release(); player = null } }
    val ui = rememberMediaPlayerState(player)

    fun ensurePlayer(): ExoPlayer = player ?: ExoPlayer.Builder(context).build().apply {
        setMediaItem(MediaItem.fromUri(mediaUrl(mediaBase, attachment.file)))
        prepare()
        player = this
    }

    Row(
        modifier.fillMaxWidth().widthIn(max = CARD_W.dp).clip(RoundedCornerShape(Radius.lg)).background(c.hover)
            .padding(horizontal = 8.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        IconButton(onClick = { val p = ensurePlayer(); if (p.isPlaying) p.pause() else p.play() }) {
            SymIcon(if (ui.isPlaying) Sym.Pause else Sym.PlayArrow, if (ui.isPlaying) "Pause" else "Play", tint = c.accentText)
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(attachment.filename, style = MaterialTheme.typography.labelLarge, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
            ScrubBar(
                fraction = if (ui.durationMs > 0) ui.positionMs.toFloat() / ui.durationMs else 0f,
                trackColor = c.pressed,
                fillColor = c.accent,
                onSeek = { f -> player?.seekTo((f * ui.durationMs).toLong()) },
            )
            Text(
                if (ui.durationMs > 0) "${clockLabel(ui.positionMs / 1000.0)} / ${clockLabel(ui.durationMs / 1000.0)}" else humanSize(attachment.size),
                style = MaterialTheme.typography.labelSmall,
                color = c.textMuted,
            )
        }
        IconButton(onClick = {
            scope.launch {
                val bytes = fetchAttachmentBytes(context, mediaUrl(mediaBase, attachment.file))
                saveAttachmentToDownloads(context, bytes, safeFileName(attachment.filename), attachment.contentType)
            }
        }) { SymIcon(Sym.Download, "Download", tint = c.textMuted) }
    }
}

/** A downloadable file: its type icon, name and size, "Open" and "Download" (ATT-04). */
@Composable
fun AttachmentFileCard(attachment: Attachment, mediaBase: String, kind: FileKind, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var busy by remember { mutableStateOf(false) }
    val icon = when (kind) {
        FileKind.Image -> Sym.Image
        FileKind.Video -> Sym.Videocam
        FileKind.Audio -> Sym.MusicNote
        FileKind.Pdf -> Sym.PictureAsPdf
        FileKind.Archive -> Sym.FolderZip
        FileKind.Text, FileKind.Doc -> Sym.Description
        FileKind.Code -> Sym.Code
        FileKind.Other -> Sym.Draft
    }
    Row(
        modifier.fillMaxWidth().widthIn(max = CARD_W.dp).clip(RoundedCornerShape(Radius.lg)).background(c.hover)
            .padding(10.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(36.dp).clip(RoundedCornerShape(Radius.md)).background(c.accentSubtle), contentAlignment = Alignment.Center) {
            SymIcon(icon, null, tint = c.accentText)
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
            Text(attachment.filename, style = MaterialTheme.typography.labelLarge, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(humanSize(attachment.size), style = MaterialTheme.typography.labelSmall, color = c.textMuted)
        }
        if (busy) {
            CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
        } else {
            if (kind.opensInline()) {
                IconButton(onClick = {
                    scope.launch {
                        busy = true
                        val bytes = fetchAttachmentBytes(context, mediaUrl(mediaBase, attachment.file))
                        openAttachmentWithSystem(context, bytes, safeFileName(attachment.filename), attachment.contentType)
                        busy = false
                    }
                }) { SymIcon(Sym.OpenInNew, "Open", tint = c.textMuted) }
            }
            IconButton(onClick = {
                scope.launch {
                    busy = true
                    val bytes = fetchAttachmentBytes(context, mediaUrl(mediaBase, attachment.file))
                    saveAttachmentToDownloads(context, bytes, safeFileName(attachment.filename), attachment.contentType)
                    busy = false
                }
            }) { SymIcon(Sym.Download, "Download ${attachment.filename}", tint = c.textMuted) }
        }
    }
}

/** "File removed by an admin", where the file was (ATT-04). */
@Composable
fun RemovedAttachmentPlaceholder(modifier: Modifier = Modifier) {
    val c = Hideout.colors
    Row(
        modifier.fillMaxWidth().widthIn(max = CARD_W.dp).height(36.dp).clip(RoundedCornerShape(Radius.lg)).background(c.hover)
            .padding(horizontal = 10.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        SymIcon(Sym.Draft, null, tint = c.textFaint, size = 16.dp)
        Text("File removed by an admin", style = MaterialTheme.typography.labelMedium, color = c.textMuted)
    }
}

/**
 * What hangs under a message: its image grid, then its video, audio, file and removed attachments
 * in order (desktop's `attachments()`, `ui/attach.rs`).
 */
@Composable
fun MessageAttachments(
    attachments: List<Attachment>,
    modifier: Modifier = Modifier,
    onOpenLightbox: (LightboxRequest) -> Unit = {},
) {
    if (attachments.isEmpty()) return
    val mediaBase = LocalMediaBase.current
    val images = attachments.filter { !it.removed && it.file.isNotEmpty() && fileKind(it.contentType, it.filename) == FileKind.Image }
    Column(modifier.padding(top = 4.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        if (images.isNotEmpty()) {
            AttachmentImageGrid(
                images = images,
                mediaBase = mediaBase,
                onOpen = { i -> onOpenLightbox(LightboxRequest(images.map { it.toLightboxItem() }, i)) },
            )
        }
        for (att in attachments) {
            if (images.any { it.id == att.id }) continue
            if (att.removed || att.file.isEmpty()) {
                RemovedAttachmentPlaceholder()
                continue
            }
            when (val kind = fileKind(att.contentType, att.filename)) {
                FileKind.Video -> AttachmentVideoCard(att, mediaBase, onExpand = { onOpenLightbox(LightboxRequest(listOf(att.toLightboxItem(isVideo = true)), 0)) })
                FileKind.Audio -> AttachmentAudioRow(att, mediaBase)
                else -> AttachmentFileCard(att, mediaBase, kind)
            }
        }
    }
}
