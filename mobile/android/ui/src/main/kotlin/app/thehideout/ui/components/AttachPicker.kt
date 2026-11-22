package app.thehideout.ui.components

import android.content.ContentResolver
import android.content.Context
import android.net.Uri
import android.provider.MediaStore
import android.provider.OpenableColumns
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import androidx.exifinterface.media.ExifInterface
import app.thehideout.core.attachment.contentTypeOf
import app.thehideout.core.store.PickedFile
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.io.IOException

/** `name`, stripped to characters safe for a local file name (shared with the download side, [safeFileName]). */
private fun Context.cacheSubdir(name: String): File = File(cacheDir, name).apply { mkdirs() }

/** The display name and byte size `OpenableColumns` reports for [uri], falling back to its last path segment and -1. */
private fun queryNameAndSize(resolver: ContentResolver, uri: Uri): Pair<String, Long> {
    resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)?.use { cursor ->
        if (cursor.moveToFirst()) {
            val nameAt = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
            val sizeAt = cursor.getColumnIndex(OpenableColumns.SIZE)
            val name = if (nameAt >= 0) cursor.getString(nameAt) else null
            val size = if (sizeAt >= 0 && !cursor.isNull(sizeAt)) cursor.getLong(sizeAt) else -1L
            if (name != null) return name to size
        }
    }
    return (uri.lastPathSegment ?: "file") to -1L
}

/** MIME types [ExifInterface] can rewrite in place; everything else streams untouched. */
private fun supportsExifRewrite(mime: String) = mime == "image/jpeg" || mime == "image/png" || mime == "image/webp"

/** Every EXIF GPS tag, cleared so a picked photo cannot leak where it was taken (the platform picker does not strip this itself). */
private val GPS_TAGS = listOf(
    ExifInterface.TAG_GPS_LATITUDE, ExifInterface.TAG_GPS_LATITUDE_REF,
    ExifInterface.TAG_GPS_LONGITUDE, ExifInterface.TAG_GPS_LONGITUDE_REF,
    ExifInterface.TAG_GPS_ALTITUDE, ExifInterface.TAG_GPS_ALTITUDE_REF,
    ExifInterface.TAG_GPS_TIMESTAMP, ExifInterface.TAG_GPS_DATESTAMP,
    ExifInterface.TAG_GPS_PROCESSING_METHOD, ExifInterface.TAG_GPS_AREA_INFORMATION,
    ExifInterface.TAG_GPS_DEST_LATITUDE, ExifInterface.TAG_GPS_DEST_LATITUDE_REF,
    ExifInterface.TAG_GPS_DEST_LONGITUDE, ExifInterface.TAG_GPS_DEST_LONGITUDE_REF,
)

/**
 * One file the user just picked, captured or shared, resolved to a [PickedFile] the store can
 * upload: its declared name, MIME type (guessed from the name when the provider doesn't say) and
 * size, a `content://` preview, and a [app.thehideout.core.api.UploadSource]. A JPEG/PNG/WebP is
 * copied into the app's cache first so its GPS EXIF tags can be stripped before anything is
 * uploaded (EXIF *orientation* is left alone: the server honours it when it makes a thumbnail).
 * The copy is streamed, not read whole into memory; so is the upload itself.
 */
suspend fun pickedFileFrom(context: Context, uri: Uri): PickedFile = withContext(Dispatchers.IO) {
    val resolver = context.contentResolver
    val (name, declaredSize) = queryNameAndSize(resolver, uri)
    val mime = resolver.getType(uri)?.takeIf { it.isNotBlank() && it != "application/octet-stream" } ?: contentTypeOf(name)
    if (supportsExifRewrite(mime)) {
        val scrubbed = File(context.cacheSubdir("uploads"), "u${System.nanoTime()}-${safeFileName(name)}")
        try {
            resolver.openInputStream(uri)?.use { input -> FileOutputStream(scrubbed).use { input.copyTo(it) } }
                ?: throw IOException("can't open $uri")
            runCatching {
                val exif = ExifInterface(scrubbed.absolutePath)
                if (GPS_TAGS.any { exif.getAttribute(it) != null }) {
                    GPS_TAGS.forEach { exif.setAttribute(it, null) }
                    exif.saveAttributes()
                }
            }
            val size = scrubbed.length()
            PickedFile(name, mime, size, uri.toString()) { FileInputStream(scrubbed) }
        } catch (e: IOException) {
            scrubbed.delete()
            throw e
        }
    } else {
        val size = if (declaredSize >= 0) declaredSize else resolver.openAssetFileDescriptor(uri, "r")?.use { it.length }.takeIf { it != null && it >= 0 } ?: 0L
        PickedFile(name, mime, size, uri.toString()) { resolver.openInputStream(uri) ?: throw IOException("can't reopen $uri") }
    }
}

/** Resolves every [uris] in order (sequentially: each may copy a file, so this is not parallelised). */
suspend fun pickedFilesFrom(context: Context, uris: List<Uri>): List<PickedFile> = uris.map { pickedFileFrom(context, it) }

/** Deletes cache copies [pickedFileFrom] made to strip EXIF, older than a day or past [maxBytes] (oldest first). */
fun pruneUploadCache(context: Context, maxAgeMs: Long = 24 * 60 * 60 * 1000L, maxBytes: Long = 100L * 1024 * 1024) {
    val dir = File(context.cacheDir, "uploads")
    val files = dir.listFiles() ?: return
    val now = System.currentTimeMillis()
    val (stale, fresh) = files.partition { now - it.lastModified() > maxAgeMs }
    stale.forEach { it.delete() }
    var total = fresh.sumOf { it.length() }
    for (f in fresh.sortedBy { it.lastModified() }) {
        if (total <= maxBytes) break
        total -= f.length()
        f.delete()
    }
}

/** The "+" menu's three pickers, each handing its result to [onPicked] as resolved [PickedFile]s. */
class AttachPickers internal constructor(
    val openPhotos: () -> Unit,
    val openFiles: () -> Unit,
    val openCameraPhoto: () -> Unit,
    val openCameraVideo: () -> Unit,
)

/** A fresh `content://` URI in the app's cache (`FileProvider`'s territory) for the camera to write into. */
private fun newCameraUri(context: Context, extension: String): Uri {
    val file = File(context.cacheSubdir("camera"), "capture-${System.nanoTime()}.$extension")
    return attachmentContentUri(context, file)
}

/**
 * Wires the Android Photo Picker (multi-select images and videos), `ACTION_OPEN_DOCUMENT`
 * (multi-select, any type) and the system camera (photo or video, through a `FileProvider`) to
 * [onPicked] (ATT-09, COMP-05). Each launcher resolves its result off the main thread before
 * calling back, so [onPicked] always sees ready [PickedFile]s.
 */
@Composable
fun rememberAttachPickers(onPicked: (List<PickedFile>) -> Unit): AttachPickers {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    fun resolveAndEmit(uris: List<Uri>) {
        if (uris.isEmpty()) return
        scope.launch { onPicked(pickedFilesFrom(context, uris)) }
    }

    val photos = rememberLauncherForActivityResult(ActivityResultContracts.PickMultipleVisualMedia()) { resolveAndEmit(it) }
    val files = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { resolveAndEmit(it) }
    var cameraPhotoUri by remember { mutableStateOf<Uri?>(null) }
    var cameraVideoUri by remember { mutableStateOf<Uri?>(null) }
    val takePhoto = rememberLauncherForActivityResult(ActivityResultContracts.TakePicture()) { ok -> if (ok) cameraPhotoUri?.let { resolveAndEmit(listOf(it)) } }
    val takeVideo = rememberLauncherForActivityResult(ActivityResultContracts.CaptureVideo()) { ok -> if (ok) cameraVideoUri?.let { resolveAndEmit(listOf(it)) } }

    return remember(context) {
        AttachPickers(
            openPhotos = { photos.launch(androidx.activity.result.PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageAndVideo)) },
            openFiles = { files.launch(arrayOf("*/*")) },
            openCameraPhoto = {
                val uri = newCameraUri(context, "jpg")
                cameraPhotoUri = uri
                runCatching { takePhoto.launch(uri) }
            },
            openCameraVideo = {
                val uri = newCameraUri(context, "mp4")
                cameraVideoUri = uri
                runCatching { takeVideo.launch(uri) }
            },
        )
    }
}
