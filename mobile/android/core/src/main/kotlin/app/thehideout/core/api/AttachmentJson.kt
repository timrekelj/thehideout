package app.thehideout.core.api

import java.time.Instant
import java.time.format.DateTimeParseException

/** A pending upload: `POST /attachments` answers one; `SendMessageRequest.attachment_ids` links it to a message. */
data class PendingAttachment(val id: Long, val filename: String, val contentType: String, val size: Long, val expiresAt: Instant)

/** The server's error body for the plain-HTTP attachment endpoint: `{"code": "...", "message": "..."}`. */
data class HttpErrorBody(val code: String?, val message: String?)

/**
 * A tiny recursive-descent JSON reader: just enough to parse `PendingAttachment` and error bodies
 * (flat objects of strings, numbers, booleans and null; protobuf-JSON mapping sends 64-bit integers
 * as strings). Not a general-purpose parser: no streaming, no comments, no trailing commas.
 */
private class JsonReader(private val s: String) {
    private var i = 0

    fun readValue(): Any? {
        skipWs()
        return when {
            i >= s.length -> throw JsonParseException("unexpected end of input")
            s[i] == '{' -> readObject()
            s[i] == '[' -> readArray()
            s[i] == '"' -> readString()
            s.startsWith("true", i) -> { i += 4; true }
            s.startsWith("false", i) -> { i += 5; false }
            s.startsWith("null", i) -> { i += 4; null }
            else -> readNumber()
        }
    }

    private fun readObject(): Map<String, Any?> {
        expect('{')
        val out = LinkedHashMap<String, Any?>()
        skipWs()
        if (peek() == '}') { i++; return out }
        while (true) {
            skipWs()
            val key = readString()
            skipWs()
            expect(':')
            out[key] = readValue()
            skipWs()
            when (peek()) {
                ',' -> i++
                '}' -> { i++; return out }
                else -> throw JsonParseException("expected ',' or '}' at $i")
            }
        }
    }

    private fun readArray(): List<Any?> {
        expect('[')
        val out = mutableListOf<Any?>()
        skipWs()
        if (peek() == ']') { i++; return out }
        while (true) {
            out += readValue()
            skipWs()
            when (peek()) {
                ',' -> i++
                ']' -> { i++; return out }
                else -> throw JsonParseException("expected ',' or ']' at $i")
            }
        }
    }

    private fun readString(): String {
        expect('"')
        val sb = StringBuilder()
        while (true) {
            if (i >= s.length) throw JsonParseException("unterminated string")
            val c = s[i++]
            when {
                c == '"' -> return sb.toString()
                c == '\\' -> {
                    if (i >= s.length) throw JsonParseException("unterminated escape")
                    when (val e = s[i++]) {
                        '"' -> sb.append('"')
                        '\\' -> sb.append('\\')
                        '/' -> sb.append('/')
                        'b' -> sb.append('\b')
                        'f' -> sb.append('\u000C')
                        'n' -> sb.append('\n')
                        'r' -> sb.append('\r')
                        't' -> sb.append('\t')
                        'u' -> {
                            if (i + 4 > s.length) throw JsonParseException("bad unicode escape")
                            sb.append(s.substring(i, i + 4).toInt(16).toChar())
                            i += 4
                        }
                        else -> throw JsonParseException("bad escape '\\$e'")
                    }
                }
                else -> sb.append(c)
            }
        }
    }

    private fun readNumber(): Double {
        val start = i
        if (peek() == '-') i++
        while (i < s.length && (s[i].isDigit() || s[i] in ".eE+-")) i++
        if (i == start) throw JsonParseException("expected a value at $i")
        return s.substring(start, i).toDouble()
    }

    private fun peek(): Char = if (i < s.length) s[i] else '\u0000'

    private fun expect(c: Char) {
        skipWs()
        if (peek() != c) throw JsonParseException("expected '$c' at $i")
        i++
    }

    private fun skipWs() {
        while (i < s.length && s[i].isWhitespace()) i++
    }
}

class JsonParseException(message: String) : Exception(message)

/** Parses a flat JSON object into a map (string keys, `String`/`Double`/`Boolean`/`null`/nested values). */
fun parseJsonObject(text: String): Map<String, Any?> {
    @Suppress("UNCHECKED_CAST")
    return JsonReader(text).readValue() as? Map<String, Any?> ?: throw JsonParseException("expected an object")
}

/** `"123"` or `123`, as the protobuf-JSON mapping sends 64-bit integers either way. */
private fun Any?.asLong(): Long? = when (this) {
    is String -> toLongOrNull()
    is Double -> toLong()
    else -> null
}

private fun Any?.asString(): String? = this as? String

/** The protobuf-JSON mapping of `PendingAttachment` (`attachment.proto`): camelCase or snake_case, ids and sizes as strings or numbers. */
fun parsePendingAttachment(body: ByteArray): PendingAttachment {
    val text = body.toString(Charsets.UTF_8)
    val obj = try {
        parseJsonObject(text)
    } catch (e: Exception) {
        throw AppError.Other("bad upload response: ${e.message}")
    }
    val id = (obj["id"] ?: obj["Id"]).asLong() ?: throw AppError.Other("bad upload response: missing id")
    val filename = (obj["filename"] ?: obj["Filename"]).asString().orEmpty()
    val contentType = (obj["contentType"] ?: obj["content_type"]).asString().orEmpty()
    val size = (obj["size"] ?: obj["Size"]).asLong() ?: 0L
    val expiresAtRaw = (obj["expiresAt"] ?: obj["expires_at"]).asString()
    val expiresAt = try {
        expiresAtRaw?.let(Instant::parse) ?: Instant.EPOCH
    } catch (e: DateTimeParseException) {
        Instant.EPOCH
    }
    return PendingAttachment(id, filename, contentType, size, expiresAt)
}

/** The server's `{"code": "...", "message": "..."}` error body (`attachments.rs`'s `HttpError`); tolerant of a body that isn't JSON. */
fun parseHttpErrorBody(body: ByteArray): HttpErrorBody {
    val text = body.toString(Charsets.UTF_8)
    val obj = runCatching { parseJsonObject(text) }.getOrNull() ?: return HttpErrorBody(null, text.take(200).ifBlank { null })
    return HttpErrorBody(obj["code"].asString(), obj["message"].asString())
}

/** Maps the attachment endpoint's plain-HTTP status and body (see `server/src/media/attachments.rs`) to [AppError]. */
fun httpAttachmentError(status: Int, body: ByteArray): AppError {
    val b = parseHttpErrorBody(body)
    val message = b.message ?: "upload failed"
    return when (status) {
        401 -> AppError.Unauthenticated
        403 -> AppError.PermissionDenied
        404 -> AppError.NotFound
        408 -> AppError.Unavailable(message)
        411, 400 -> AppError.InvalidArgument(message)
        413 -> AppError.TooLarge
        429 -> AppError.RateLimited
        in 500..599, 503 -> AppError.Unavailable(message)
        else -> AppError.Other(message)
    }
}
