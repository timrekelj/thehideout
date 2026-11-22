package app.thehideout.core.cache

import app.thehideout.core.model.*
import com.google.protobuf.MessageLite
import java.io.File
import java.io.IOException

/**
 * A small file cache of serialized protobuf-lite bytes (not Room: the data is a mirror the server
 * owns and the gateway keeps current). One file per list and per channel, atomic writes, size
 * capped, dropped on sign-out. Failures are never fatal: a cache that cannot read answers `null`.
 *
 * Layout under [dir]: `index.pb`-style files named by key, each a length-delimited list of messages.
 */
class FileCache(private val dir: File, private val maxBytes: Long = DEFAULT_MAX_BYTES) {
    init {
        dir.mkdirs()
    }

    /** Writes [items] under [key]: write to a temp file, then rename over the old one. */
    fun <T : MessageLite> write(key: String, items: List<T>) {
        try {
            val target = file(key)
            val tmp = File(dir, target.name + ".tmp")
            tmp.outputStream().buffered().use { out -> items.forEach { it.writeDelimitedTo(out) } }
            if (!tmp.renameTo(target)) {
                target.delete()
                if (!tmp.renameTo(target)) tmp.delete()
            }
            trim()
        } catch (_: IOException) {
        }
    }

    /** Reads what [write] stored; `null` when there is nothing (or it does not parse). */
    fun <T : MessageLite> read(key: String, parse: (java.io.InputStream) -> T?): List<T>? {
        val f = file(key)
        if (!f.isFile) return null
        return try {
            f.inputStream().buffered().use { input ->
                buildList {
                    while (true) add(parse(input) ?: break)
                }
            }.also { f.setLastModified(System.currentTimeMillis()) }
        } catch (_: IOException) {
            f.delete()
            null
        } catch (_: RuntimeException) {
            // InvalidProtocolBufferException is an IOException; this is the belt for a truncated file.
            f.delete()
            null
        }
    }

    /** Drops everything (sign-out). */
    fun clear() {
        dir.listFiles()?.forEach { it.delete() }
    }

    fun sizeBytes(): Long = dir.listFiles()?.sumOf { it.length() } ?: 0L

    /** Evicts the least recently used files until the cache is under its budget. */
    private fun trim() {
        var files = dir.listFiles()?.filter { it.isFile }?.sortedBy { it.lastModified() } ?: return
        var total = files.sumOf { it.length() }
        for (f in files) {
            if (total <= maxBytes) break
            total -= f.length()
            f.delete()
        }
    }

    private fun file(key: String) = File(dir, key.replace(Regex("[^A-Za-z0-9._-]"), "_") + ".pb")

    companion object {
        const val DEFAULT_MAX_BYTES = 64L * 1024 * 1024
        const val MESSAGES_PER_CHANNEL = 100
    }
}

/**
 * The store-level view of the cache: the lists and the newest messages of recently opened channels,
 * as protobuf-lite blobs, loaded before the first network answer.
 */
class SessionCache(private val files: FileCache) {
    fun saveLists(state: app.thehideout.core.store.AppState) {
        state.me?.let { files.write("me", listOf(it.toProto())) }
        files.write("guilds", state.guilds.map { it.toProto() })
        files.write("channels", state.channels.values.flatten().map { it.toProto() })
        files.write("dms", state.dms.map { it.toProto() })
        state.layouts.forEach { (g, l) -> files.write("layout-$g", listOf(l.toProto())) }
    }

    fun saveChannel(channel: Long, messages: List<Message>) {
        files.write("channel-$channel", messages.takeLast(FileCache.MESSAGES_PER_CHANNEL).map { it.toProto() })
    }

    fun loadMessages(channel: Long): List<Message>? =
        files.read("channel-$channel") { com.thehideout.v1.Message.parseDelimitedFrom(it) }?.map { it.toModel() }

    /** The lists as they were when last saved, as if the first network answer had just come; `null` without a complete cache. */
    fun loadInitial(): app.thehideout.core.api.Initial? {
        val me = files.read("me") { com.thehideout.v1.User.parseDelimitedFrom(it) }?.firstOrNull()?.toModel() ?: return null
        val guilds = files.read("guilds") { com.thehideout.v1.Guild.parseDelimitedFrom(it) }?.map { it.toModel() } ?: return null
        val channels = files.read("channels") { com.thehideout.v1.Channel.parseDelimitedFrom(it) }?.map { it.toModel() } ?: return null
        val dms = files.read("dms") { com.thehideout.v1.DmChannel.parseDelimitedFrom(it) }?.map { it.toModel() } ?: emptyList()
        val layouts = guilds.mapNotNull { g ->
            files.read("layout-${g.id}") { com.thehideout.v1.GuildLayout.parseDelimitedFrom(it) }?.firstOrNull()?.let { g.id to it.toModel() }
        }.toMap()
        return app.thehideout.core.api.Initial(me, guilds, channels, layouts, dms)
    }

    fun clear() = files.clear()
}
