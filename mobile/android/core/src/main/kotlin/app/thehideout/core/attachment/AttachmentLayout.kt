package app.thehideout.core.attachment

import java.util.Locale
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

/** What a file is, by its content type then its extension (desktop's `file_kind`, `attachments.rs`). */
enum class FileKind { Image, Video, Audio, Pdf, Archive, Text, Code, Doc, Other }

/** Files one message may carry (ATT-09, desktop's `attach::MAX_FILES`). */
const val MAX_ATTACHMENTS = 10

/** The server's default per-upload cap (`server/src/media/attachments.rs::DEFAULT_MAX_UPLOAD_BYTES`), used until a real server answers otherwise. */
const val DEFAULT_MAX_UPLOAD_BYTES = 50L * 1024 * 1024

/** Uploads of one draft running at once; the rest wait as [app.thehideout.core.store.AttachUploadPhase.Queued]. */
const val MAX_CONCURRENT_UPLOADS = 3

/** The kind of a file by its content type, then its filename's extension. */
fun fileKind(contentType: String, filename: String): FileKind {
    val ct = contentType.lowercase()
    val ext = filename.substringAfterLast('.', "").lowercase()
    return when {
        ct.startsWith("image/") -> FileKind.Image
        ct.startsWith("video/") || ext in setOf("mp4", "mov", "webm", "m4v") -> FileKind.Video
        ct.startsWith("audio/") || ext in setOf("mp3", "m4a", "wav", "flac", "ogg", "aac") -> FileKind.Audio
        ct == "application/pdf" || ext == "pdf" -> FileKind.Pdf
        ct.contains("zip") || ct.contains("tar") || ct.contains("compressed") || ct.contains("7z") ||
            ext in setOf("zip", "tar", "gz", "tgz", "7z", "rar", "bz2", "xz") -> FileKind.Archive
        ct.contains("json") || ct.contains("xml") || ct.contains("javascript") ||
            ext in setOf("rs", "js", "ts", "py", "json", "toml", "yaml", "yml", "html", "css", "sh", "c", "cpp", "go") -> FileKind.Code
        ct.startsWith("text/") || ext in setOf("txt", "md", "csv", "log") -> FileKind.Text
        ct.contains("word") || ct.contains("officedocument") || ct.contains("opendocument") ||
            ext in setOf("doc", "docx", "odt", "xls", "xlsx", "ppt", "pptx") -> FileKind.Doc
        else -> FileKind.Other
    }
}

/** The content type a file of this name is uploaded as, when the picker couldn't say (desktop's `attach::content_type_of`). */
fun contentTypeOf(filename: String): String = when (filename.substringAfterLast('.', "").lowercase()) {
    "png" -> "image/png"
    "jpg", "jpeg" -> "image/jpeg"
    "webp" -> "image/webp"
    "gif" -> "image/gif"
    "heic" -> "image/heic"
    "heif" -> "image/heif"
    "mp4", "m4v" -> "video/mp4"
    "mov" -> "video/quicktime"
    "webm" -> "video/webm"
    "mp3" -> "audio/mpeg"
    "m4a" -> "audio/mp4"
    "wav" -> "audio/wav"
    "ogg" -> "audio/ogg"
    "flac" -> "audio/flac"
    "pdf" -> "application/pdf"
    "zip" -> "application/zip"
    "json" -> "application/json"
    "txt", "log", "md" -> "text/plain"
    "csv" -> "text/csv"
    else -> "application/octet-stream"
}

/** Whether the server serves it inline, so "Open" shows it rather than downloading it. */
fun FileKind.opensInline(): Boolean = this == FileKind.Image || this == FileKind.Video || this == FileKind.Audio || this == FileKind.Pdf

/** "12 B", "3.4 KB", "5.0 MB", "1.2 GB" (powers of 1024, one decimal from KB up). */
fun humanSize(bytes: Long): String {
    if (bytes < 1024) return "$bytes B"
    val units = listOf("KB", "MB", "GB", "TB")
    var v = bytes / 1024.0
    var unit = 0
    while (v >= 1024.0 && unit + 1 < units.size) {
        v /= 1024.0
        unit++
    }
    return String.format(Locale.ROOT, "%.1f %s", v, units[unit])
}

/** "0:07", "12:03", "1:02:03". */
fun clockLabel(seconds: Double): String {
    val s = if (seconds.isFinite()) max(seconds, 0.0).toLong() else 0L
    val h = s / 3600
    val m = s / 60 % 60
    val sec = s % 60
    return if (h > 0) String.format(Locale.ROOT, "%d:%02d:%02d", h, m, sec) else String.format(Locale.ROOT, "%d:%02d", m, sec)
}

/** The fake backend's bundled-asset keys pass through [mediaUrl] unprefixed; nothing else may. */
const val FAKE_ASSET_PREFIX = "file:///android_asset/"

/**
 * `{base}/media/{key}`, unless `key` already names one of the fake backend's bundled assets
 * ([FAKE_ASSET_PREFIX]). Any other scheme (`file://`, `content://`, a foreign host) is rejected
 * rather than passed through, so a malicious media key can't act as a tracking pixel or read a
 * local file in production. A pending message's local preview ([app.thehideout.core.store.PendingPreview])
 * never goes through this: it is rendered straight from its own `content://` URI, a separate path
 * from anything a server could put in an `Attachment`.
 */
fun mediaUrl(baseUrl: String, key: String): String = when {
    key.startsWith(FAKE_ASSET_PREFIX) -> key
    key.contains("://") -> throw IllegalArgumentException("refusing to load an attachment key with its own scheme: $key")
    else -> "${baseUrl.trimEnd('/')}/media/$key"
}

/** Widest and tallest an inline picture or video grows, in dp. */
const val MEDIA_MAX_W = 400f
const val MEDIA_MAX_H = 300f

/** The gap between grid tiles, in dp. */
const val GRID_GAP = 4f

/** Height of a two-or-more-column grid, in dp. */
const val GRID_H = 240f

/** The widest a file card or audio row grows, in dp. */
const val CARD_W = 340f

/** `(w, h)` scaled down (never up past natural) to fit `maxW` x `maxH`, keeping its aspect. */
fun fitSize(natural: Pair<Int, Int>?, maxW: Float, maxH: Float): Pair<Float, Float> {
    val (nw, nh) = if (natural != null && natural.first > 0 && natural.second > 0) {
        natural.first.toFloat() to natural.second.toFloat()
    } else {
        maxW to maxW * 9f / 16f
    }
    var scale = min(maxW / nw, maxH / nh).coerceAtMost(1f)
    // A thumbnail is never a speck: a tiny picture still gets a legible box.
    if (nw * scale < 96f && nh * scale < 96f) {
        scale = min(min(96f / nw, 96f / nh), min(maxW / nw, maxH / nh))
    }
    return (nw * scale).roundToDp() to (nh * scale).roundToDp()
}

private fun Float.roundToDp(): Float = roundToInt().toFloat()

/** A tile's place in a grid, in dp from the grid's top-left corner. */
data class Cell(val x: Float, val y: Float, val w: Float, val h: Float)

/**
 * Where `n` pictures sit: one alone keeps its aspect (within [MEDIA_MAX_W] x [MEDIA_MAX_H]); two
 * sit side by side; three put the first tall on the left; four or more are a 2x2 (a fifth and
 * beyond are reached through the lightbox, and the fourth tile says how many more there are).
 * Returns the grid's size and its cells.
 */
fun imageGrid(n: Int, single: Pair<Int, Int>? = null, maxW: Float = MEDIA_MAX_W): Pair<Pair<Float, Float>, List<Cell>> {
    if (n == 0) return (0f to 0f) to emptyList()
    if (n == 1) {
        val (w, h) = fitSize(single, maxW, MEDIA_MAX_H)
        return (w to h) to listOf(Cell(0f, 0f, w, h))
    }
    val w = maxW
    val half = (w - GRID_GAP) / 2f
    val cells = when (n) {
        2 -> listOf(
            Cell(0f, 0f, half, GRID_H),
            Cell(half + GRID_GAP, 0f, half, GRID_H),
        )
        3 -> {
            val small = (GRID_H - GRID_GAP) / 2f
            listOf(
                Cell(0f, 0f, half, GRID_H),
                Cell(half + GRID_GAP, 0f, half, small),
                Cell(half + GRID_GAP, small + GRID_GAP, half, small),
            )
        }
        else -> {
            val small = (GRID_H - GRID_GAP) / 2f
            val right = half + GRID_GAP
            val low = small + GRID_GAP
            listOf(
                Cell(0f, 0f, half, small),
                Cell(right, 0f, half, small),
                Cell(0f, low, half, small),
                Cell(right, low, half, small),
            )
        }
    }
    return (w to GRID_H) to cells
}
