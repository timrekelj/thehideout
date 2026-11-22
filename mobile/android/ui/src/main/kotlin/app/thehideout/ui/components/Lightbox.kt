package app.thehideout.ui.components

import android.content.Intent
import android.widget.FrameLayout
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.gestures.rememberTransformableState
import androidx.compose.foundation.gestures.transformable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.media3.common.MediaItem
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.ui.PlayerView
import app.thehideout.core.attachment.mediaUrl
import app.thehideout.ui.icons.Sym
import app.thehideout.ui.icons.SymIcon
import kotlinx.coroutines.launch
import kotlin.math.abs
import kotlin.math.roundToInt

/**
 * The full-screen media viewer (ATT-05): swipes between a message's pictures and videos, pinch
 * and double-tap zoom with pan on pictures, swipe down to dismiss, and native Media3 controls on
 * video. `onDismiss` fires on the close button, the back gesture, or a swipe down past the threshold.
 */
@Composable
fun AttachmentLightbox(request: LightboxRequest, onDismiss: () -> Unit) {
    val mediaBase = LocalMediaBase.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val pagerState = rememberPagerState(initialPage = request.startIndex.coerceIn(0, request.items.lastIndex)) { request.items.size }
    var dragOffsetY by remember { mutableFloatStateOf(0f) }
    val dismissFraction by remember { derivedStateOf { (abs(dragOffsetY) / DISMISS_DISTANCE).coerceIn(0f, 1f) } }
    var busy by remember { mutableStateOf(false) }
    // Only a page at scale 1 may be dragged to dismiss: a vertical pan while zoomed in pans the
    // picture instead (ZoomableLightboxMedia's own `transformable`, gated by `canPan`).
    var pageScale by remember { mutableFloatStateOf(1f) }
    LaunchedEffect(pagerState.currentPage) { pageScale = 1f }

    Dialog(
        onDismissRequest = onDismiss,
        properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false),
    ) {
        Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha = 1f - dismissFraction * 0.85f))) {
            HorizontalPager(
                state = pagerState,
                modifier = Modifier
                    .fillMaxSize()
                    .offset { IntOffset(0, dragOffsetY.roundToInt()) }
                    .pointerInput(Unit) {
                        detectVerticalDragGestures(
                            onDragEnd = {
                                if (abs(dragOffsetY) > DISMISS_DISTANCE) onDismiss() else dragOffsetY = 0f
                            },
                        ) { _, amount -> if (pageScale <= 1f) dragOffsetY += amount }
                    },
            ) { page ->
                ZoomableLightboxMedia(
                    item = request.items[page],
                    mediaBase = mediaBase,
                    isActive = pagerState.currentPage == page,
                    onScaleChange = { if (page == pagerState.currentPage) pageScale = it },
                )
            }

            val current = request.items[pagerState.currentPage]
            Row(
                Modifier.align(Alignment.TopCenter).fillMaxWidth().statusBarsPadding().padding(horizontal = 8.dp, vertical = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                Text(
                    if (request.items.size > 1) "${current.name} · ${pagerState.currentPage + 1} of ${request.items.size}" else current.name,
                    color = MediaColors.ink,
                    style = MaterialTheme.typography.labelLarge,
                    modifier = Modifier.weight(1f).padding(start = 8.dp),
                )
                IconButton(enabled = !busy, onClick = {
                    scope.launch {
                        busy = true
                        try {
                            val file = fetchAttachmentToCache(context, mediaUrl(mediaBase, current.key), safeFileName(current.name))
                            val uri = attachmentContentUri(context, file)
                            val share = Intent(Intent.ACTION_SEND).apply {
                                type = current.contentType.ifBlank { "application/octet-stream" }
                                putExtra(Intent.EXTRA_STREAM, uri)
                                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                            }
                            runCatching { context.startActivity(Intent.createChooser(share, null)) }
                        } catch (e: Exception) {
                            attachmentErrorToast(context, e)
                        } finally {
                            busy = false
                        }
                    }
                }) { SymIcon(Sym.Share, "Share", tint = MediaColors.ink) }
                IconButton(enabled = !busy, onClick = {
                    scope.launch {
                        busy = true
                        try {
                            saveAttachmentToDownloads(context, mediaUrl(mediaBase, current.key), safeFileName(current.name), current.contentType)
                        } catch (e: Exception) {
                            attachmentErrorToast(context, e)
                        } finally {
                            busy = false
                        }
                    }
                }) { SymIcon(Sym.Download, "Save", tint = MediaColors.ink) }
                IconButton(onClick = onDismiss) { SymIcon(Sym.Close, "Close", tint = MediaColors.ink) }
            }
            if (busy) {
                Box(Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(bottom = 16.dp)) {
                    CircularProgressIndicator(color = MediaColors.ink)
                }
            }
        }
    }
}

/**
 * Pinch and double-tap zoom with pan on a picture; a plain, controller-driven surface on a video
 * (no pinch or double-tap zoom there — scrubbing and seeking are the controller's job).
 */
@Composable
private fun ZoomableLightboxMedia(item: LightboxItem, mediaBase: String, isActive: Boolean, onScaleChange: (Float) -> Unit) {
    var scale by remember(item.key) { mutableFloatStateOf(1f) }
    var offset by remember(item.key) { mutableStateOf(Offset.Zero) }
    val zoomable = !item.isVideo
    Box(
        Modifier
            .fillMaxSize()
            .then(
                if (zoomable) {
                    Modifier
                        // canPan = { scale != 1f }: a single-finger drag only pans once zoomed in, so an
                        // unzoomed swipe reaches the HorizontalPager above instead of being eaten here.
                        .transformable(
                            state = rememberTransformableState { _, zoom, pan, _ ->
                                val next = (scale * zoom).coerceIn(1f, 5f)
                                scale = next
                                offset = if (next <= 1f) Offset.Zero else offset + pan
                                onScaleChange(next)
                            },
                            canPan = { scale != 1f },
                        )
                        .pointerInput(item.key) {
                            detectTapGestures(onDoubleTap = {
                                scale = if (scale > 1f) 1f else 2.5f
                                offset = Offset.Zero
                                onScaleChange(scale)
                            })
                        }
                } else {
                    Modifier
                },
            )
            .graphicsLayer {
                scaleX = scale
                scaleY = scale
                translationX = offset.x
                translationY = offset.y
            },
        contentAlignment = Alignment.Center,
    ) {
        if (item.isVideo) {
            LightboxVideo(mediaUrl(mediaBase, item.key), item.startPositionMs, isActive)
        } else {
            AttachmentImage(mediaUrl(mediaBase, item.key), item.name, Modifier.fillMaxSize(), ContentScale.Fit)
        }
    }
}

/**
 * A video surface with Media3's standard controller (play/pause, scrub, time) via `PlayerView`:
 * starts at `startPositionMs` (handed off from the inline card's "Expand"), and plays only while
 * `isActive` (the pager's current page), so swiping away pauses it and only one player plays at once.
 */
@Composable
private fun LightboxVideo(url: String, startPositionMs: Long, isActive: Boolean) {
    val context = LocalContext.current
    val player = remember(url) {
        ExoPlayer.Builder(context).build().apply {
            setMediaItem(MediaItem.fromUri(url))
            seekTo(startPositionMs)
            prepare()
        }
    }
    DisposableEffect(player) {
        onDispose {
            ActiveAttachmentPlayer.released(player)
            player.release()
        }
    }
    LaunchedEffect(player, isActive) {
        if (isActive) ActiveAttachmentPlayer.play(player) else player.pause()
    }
    AndroidView(
        modifier = Modifier.fillMaxSize(),
        factory = {
            PlayerView(it).apply {
                layoutParams = FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT)
                this.player = player
                useController = true
            }
        },
    )
}

private const val DISMISS_DISTANCE = 300f
