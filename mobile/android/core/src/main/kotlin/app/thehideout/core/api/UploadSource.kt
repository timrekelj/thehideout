package app.thehideout.core.api

import java.io.InputStream

/**
 * Opens the bytes of a file to attach, streamed rather than read whole into memory. The Android
 * layer wraps a `ContentResolver` stream into this at the call site, so `:core` stays plain Kotlin.
 * [open] may be called again on retry; each call must return a fresh stream from the start.
 */
fun interface UploadSource {
    fun open(): InputStream
}
