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

/** One client for every attachment download (separate from Coil's: this is a rare, user-initiated action). */
private val httpClient by lazy { OkHttpClient() }

/** The bytes of `url`: a bundled asset (the fake backend's `file:///android_asset/...` keys) or an HTTP(S) media URL. */
suspend fun fetchAttachmentBytes(context: Context, url: String): ByteArray = withContext(Dispatchers.IO) {
    if (url.startsWith("file:///android_asset/")) {
        context.assets.open(url.removePrefix("file:///android_asset/")).use { it.readBytes() }
    } else {
        httpClient.newCall(Request.Builder().url(url).build()).execute().use { resp ->
            checkNotNull(resp.body) { "empty response" }.bytes()
        }
    }
}

/** Opens `bytes` with the system's viewer for `mimeType`, through a `FileProvider` (ATT-04). */
fun openAttachmentWithSystem(context: Context, bytes: ByteArray, filename: String, mimeType: String) {
    val dir = File(context.cacheDir, "attachments").apply { mkdirs() }
    val file = File(dir, filename)
    file.writeBytes(bytes)
    val uri = FileProvider.getUriForFile(context, "${context.packageName}.fileprovider", file)
    val intent = Intent(Intent.ACTION_VIEW).apply {
        setDataAndType(uri, mimeType.ifBlank { "application/octet-stream" })
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    }
    runCatching { context.startActivity(intent) }
}

/**
 * Saves `bytes` to the device's Downloads, through `MediaStore` on API 29+ (scoped storage) or a
 * direct file registered with [DownloadManager] below it (ATT-04, ATT-12). Returns whether it worked.
 */
@Suppress("DEPRECATION")
suspend fun saveAttachmentToDownloads(context: Context, bytes: ByteArray, filename: String, mimeType: String): Boolean =
    withContext(Dispatchers.IO) {
        runCatching {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                val resolver = context.contentResolver
                val values = ContentValues().apply {
                    put(MediaStore.Downloads.DISPLAY_NAME, filename)
                    put(MediaStore.Downloads.MIME_TYPE, mimeType.ifBlank { "application/octet-stream" })
                    put(MediaStore.Downloads.IS_PENDING, 1)
                }
                val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values) ?: return@runCatching false
                resolver.openOutputStream(uri)?.use { it.write(bytes) }
                values.clear()
                values.put(MediaStore.Downloads.IS_PENDING, 0)
                resolver.update(uri, values, null, null)
                true
            } else {
                @Suppress("DEPRECATION")
                val downloads = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS)
                downloads.mkdirs()
                val file = File(downloads, filename)
                file.writeBytes(bytes)
                val manager = context.getSystemService(Context.DOWNLOAD_SERVICE) as DownloadManager
                manager.addCompletedDownload(filename, filename, true, mimeType.ifBlank { "application/octet-stream" }, file.absolutePath, bytes.size.toLong(), true)
                true
            }
        }.getOrDefault(false)
    }

/** A `mediaUrl`'s filename hint for [openAttachmentWithSystem], which needs a plain file name, not a key. */
fun safeFileName(name: String): String = name.filter { it.isLetterOrDigit() || it in ".-_ " }.ifBlank { "file" }
