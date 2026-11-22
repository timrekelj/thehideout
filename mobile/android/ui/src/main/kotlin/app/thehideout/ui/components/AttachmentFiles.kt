package app.thehideout.ui.components

import android.app.DownloadManager
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import androidx.core.content.FileProvider
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.io.OutputStream

/** One client for every attachment download (separate from Coil's: this is a rare, user-initiated action). */
private val httpClient by lazy { OkHttpClient() }

/** The HTTP(S) call behind [streamAttachment]: the real [httpClient] by default, swappable in tests. */
internal fun interface AttachmentHttpFetch {
    fun call(url: String): okhttp3.Response
}

private val defaultHttpFetch = AttachmentHttpFetch { url -> httpClient.newCall(Request.Builder().url(url).build()).execute() }

/** The prefix of the fake backend's bundled-asset keys; see [AttachmentImage] and `mediaUrl`. */
internal const val ASSET_PREFIX = "file:///android_asset/"

/** A fetch, open or save that failed: `message` is safe to show the user directly. */
class AttachmentIoException(message: String) : IOException(message)

/** Streams `url`'s bytes into `out`: a bundled asset, or an HTTP(S) media URL. Throws [AttachmentIoException] on a non-2xx response or any I/O failure. */
private suspend fun streamAttachment(context: Context, url: String, out: OutputStream, httpFetch: AttachmentHttpFetch = defaultHttpFetch) = withContext(Dispatchers.IO) {
    try {
        if (url.startsWith(ASSET_PREFIX)) {
            context.assets.open(url.removePrefix(ASSET_PREFIX)).use { it.copyTo(out) }
        } else {
            httpFetch.call(url).use { resp ->
                if (!resp.isSuccessful) throw AttachmentIoException("The server answered with ${resp.code}.")
                resp.body.byteStream().copyTo(out)
            }
        }
    } catch (e: AttachmentIoException) {
        throw e
    } catch (e: IOException) {
        throw AttachmentIoException(e.message ?: "Couldn't reach the file.")
    }
}

/** The bytes of `url`. Only for small files (file cards, lightbox share): large downloads stream straight to their destination instead. Throws [AttachmentIoException] on failure. */
suspend fun fetchAttachmentBytes(context: Context, url: String): ByteArray = fetchAttachmentBytes(context, url, defaultHttpFetch)

/** [fetchAttachmentBytes], with the HTTP(S) call swappable (tests only). */
internal suspend fun fetchAttachmentBytes(context: Context, url: String, httpFetch: AttachmentHttpFetch): ByteArray = withContext(Dispatchers.IO) {
    val out = java.io.ByteArrayOutputStream()
    streamAttachment(context, url, out, httpFetch)
    out.toByteArray()
}

/** Streams `url` into the app's cache (`FileProvider`'s territory), named `filename`. Throws [AttachmentIoException] on failure. */
suspend fun fetchAttachmentToCache(context: Context, url: String, filename: String): File = fetchAttachmentToCache(context, url, filename, defaultHttpFetch)

/** [fetchAttachmentToCache], with the HTTP(S) call swappable (tests only). */
internal suspend fun fetchAttachmentToCache(context: Context, url: String, filename: String, httpFetch: AttachmentHttpFetch): File = withContext(Dispatchers.IO) {
    val dir = File(context.cacheDir, "attachments").apply { mkdirs() }
    val file = File(dir, filename)
    try {
        FileOutputStream(file).use { out -> streamAttachment(context, url, out, httpFetch) }
    } catch (e: Exception) {
        file.delete()
        throw e
    }
    file
}

/** `file`, as a `content://` URI through the app's `FileProvider` (ATT-04, the lightbox's share/save). */
fun attachmentContentUri(context: Context, file: File): Uri =
    FileProvider.getUriForFile(context, "${context.packageName}.fileprovider", file)

/** Opens `file` with the system's viewer for `mimeType`, through a `FileProvider` (ATT-04). */
fun openAttachmentWithSystem(context: Context, file: File, mimeType: String) {
    val uri = attachmentContentUri(context, file)
    val intent = Intent(Intent.ACTION_VIEW).apply {
        setDataAndType(uri, mimeType.ifBlank { "application/octet-stream" })
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    }
    context.startActivity(intent)
}

/**
 * Saves `url` to the device's Downloads (ATT-04, ATT-12): through `MediaStore` on API 29+ (streamed,
 * scoped storage; the pending row is removed if the write fails), [DownloadManager.enqueue] for an
 * HTTP(S) `url` below that (the system service downloads it, so the app needs no storage permission),
 * or — for the fake backend's bundled assets, which [DownloadManager] can't fetch itself — the app's
 * own external files dir (no permission either) registered with [DownloadManager.addCompletedDownload].
 * Throws [AttachmentIoException] on failure.
 */
suspend fun saveAttachmentToDownloads(context: Context, url: String, filename: String, mimeType: String) =
    saveAttachmentToDownloads(context, url, filename, mimeType, defaultHttpFetch)

/** [saveAttachmentToDownloads], with the HTTP(S) call swappable (tests only). */
@Suppress("DEPRECATION")
internal suspend fun saveAttachmentToDownloads(context: Context, url: String, filename: String, mimeType: String, httpFetch: AttachmentHttpFetch) {
    val mime = mimeType.ifBlank { "application/octet-stream" }
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
        withContext(Dispatchers.IO) {
            val resolver = context.contentResolver
            val values = ContentValues().apply {
                put(MediaStore.Downloads.DISPLAY_NAME, filename)
                put(MediaStore.Downloads.MIME_TYPE, mime)
                put(MediaStore.Downloads.IS_PENDING, 1)
            }
            val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
                ?: throw AttachmentIoException("Couldn't create the download.")
            try {
                resolver.openOutputStream(uri)?.use { out -> streamAttachment(context, url, out, httpFetch) }
                    ?: throw AttachmentIoException("Couldn't open the download for writing.")
                values.clear()
                values.put(MediaStore.Downloads.IS_PENDING, 0)
                resolver.update(uri, values, null, null)
            } catch (e: Exception) {
                resolver.delete(uri, null, null)
                throw e
            }
        }
    } else if (url.startsWith("http://") || url.startsWith("https://")) {
        withContext(Dispatchers.IO) {
            val manager = context.getSystemService(Context.DOWNLOAD_SERVICE) as DownloadManager
            val request = DownloadManager.Request(Uri.parse(url))
                .setDestinationInExternalPublicDir(Environment.DIRECTORY_DOWNLOADS, filename)
                .setMimeType(mime)
                .setNotificationVisibility(DownloadManager.Request.VISIBILITY_VISIBLE_NOTIFY_COMPLETED)
            try {
                manager.enqueue(request)
            } catch (e: Exception) {
                throw AttachmentIoException(e.message ?: "Couldn't start the download.")
            }
        }
    } else {
        withContext(Dispatchers.IO) {
            val dir = context.getExternalFilesDir(Environment.DIRECTORY_DOWNLOADS)
                ?: throw AttachmentIoException("No external storage is available.")
            dir.mkdirs()
            val file = File(dir, filename)
            try {
                FileOutputStream(file).use { out -> streamAttachment(context, url, out, httpFetch) }
            } catch (e: Exception) {
                file.delete()
                throw e
            }
            val manager = context.getSystemService(Context.DOWNLOAD_SERVICE) as DownloadManager
            manager.addCompletedDownload(filename, filename, true, mime, file.absolutePath, file.length(), true)
        }
    }
}

/** `name` (an attachment's display filename), stripped to characters safe for a local file name: for [openAttachmentWithSystem], [fetchAttachmentToCache] and [saveAttachmentToDownloads]. */
fun safeFileName(name: String): String = name.filter { it.isLetterOrDigit() || it in ".-_ " }.ifBlank { "file" }

/** Deletes cached attachment files older than a day, or past [maxBytes] total (oldest first): call once on app start. */
fun pruneAttachmentCache(context: Context, maxAgeMs: Long = 24 * 60 * 60 * 1000L, maxBytes: Long = 200L * 1024 * 1024) {
    val dir = File(context.cacheDir, "attachments")
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
