package app.thehideout.core

import app.thehideout.core.cache.FileCache
import app.thehideout.core.cache.SessionCache
import app.thehideout.core.fake.FakeBackend
import app.thehideout.core.model.*
import app.thehideout.core.store.Action
import app.thehideout.core.store.AppState
import app.thehideout.core.store.Reducer
import kotlinx.coroutines.runBlocking
import org.junit.jupiter.api.Assertions.*
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.io.TempDir
import java.io.File

class FileCacheTest {
    @TempDir lateinit var dir: File

    private val rich = Message(
        id = 9, channelId = 1, guildId = 2, authorId = 3, content = "hello **world**", createdAt = T0, editedAt = T0.plusSeconds(5), nonce = "n-1",
        reactions = listOf(Reaction(ReactionEmoji.Unicode("🎉"), 2, true), Reaction(ReactionEmoji.Custom(77), 1, false)),
        attachments = listOf(Attachment(1, "a.png", "image/png", 1234, "key", 100, 50, "thumb")),
        embeds = listOf(
            Embed(
                "https://example.com", "T", "D", "Site", "thumb", 10, 20, EmbedKind.Rich, "x", "Author", "https://a", "icon",
                listOf(EmbedMedia(EmbedMediaKind.Gifv, "f", "poster", 3, 4, 1000, 99)), T0,
            ),
        ),
    )

    @Test fun `a message survives the round trip through protobuf bytes with everything on it`() {
        assertEquals(rich, rich.toProto().let { com.thehideout.v1.Message.parseFrom(it.toByteArray()) }.toModel())
        val dm = rich.copy(guildId = null, editedAt = null, reactions = emptyList(), attachments = emptyList(), embeds = emptyList())
        assertEquals(dm, dm.toProto().toModel())
    }

    @Test fun `lists and channels round trip through the session cache`() {
        val init = runBlocking { FakeBackend().loadInitial() }
        val cache = SessionCache(FileCache(dir))
        val state = Reducer.reduce(AppState(), Action.InitialLoaded(init)).state
        cache.saveLists(state)
        cache.saveChannel(FakeBackend.GENERAL, listOf(rich))
        val back = SessionCache(FileCache(dir)).loadInitial()!!
        assertEquals(init.me.copy(email = init.me.email), back.me)
        assertEquals(init.guilds, back.guilds)
        assertEquals(init.channels, back.channels)
        assertEquals(init.dms, back.dms)
        assertEquals(init.layouts, back.layouts)
        assertEquals(listOf(rich), SessionCache(FileCache(dir)).loadMessages(FakeBackend.GENERAL))
    }

    @Test fun `only the newest hundred messages of a channel are kept`() {
        val cache = SessionCache(FileCache(dir))
        cache.saveChannel(1, (1L..250L).map { msg(it, 1, 2) })
        val back = cache.loadMessages(1)!!
        assertEquals(100, back.size)
        assertEquals(151L, back.first().id)
        assertEquals(250L, back.last().id)
    }

    @Test fun `missing and corrupt files read as nothing`() {
        val cache = SessionCache(FileCache(dir))
        assertNull(cache.loadMessages(5))
        assertNull(cache.loadInitial())
        File(dir, "channel-5.pb").writeBytes(byteArrayOf(0x0a, 0x7f, 0x01))
        assertNull(cache.loadMessages(5))
        assertFalse(File(dir, "channel-5.pb").exists(), "a file that does not parse is dropped")
    }

    @Test fun `writes are atomic and leave no temp file`() {
        val files = FileCache(dir)
        files.write("k", listOf(rich.toProto()))
        files.write("k", listOf(rich.toProto(), rich.toProto()))
        assertEquals(listOf("k.pb"), dir.list()!!.sorted())
        assertEquals(2, files.read("k") { com.thehideout.v1.Message.parseDelimitedFrom(it) }!!.size)
    }

    @Test fun `the cache stays under its budget by dropping the least recently used files`() {
        val files = FileCache(dir, maxBytes = 3_000)
        val big = rich.copy(content = "x".repeat(1000)).toProto()
        files.write("a", listOf(big)); File(dir, "a.pb").setLastModified(1_000)
        files.write("b", listOf(big)); File(dir, "b.pb").setLastModified(2_000)
        files.write("c", listOf(big)); File(dir, "c.pb").setLastModified(3_000)
        files.write("d", listOf(big))
        assertTrue(files.sizeBytes() <= 3_000)
        assertFalse(File(dir, "a.pb").exists(), "the oldest went first")
        assertTrue(File(dir, "d.pb").exists())
    }

    @Test fun `sign-out drops everything`() {
        val cache = SessionCache(FileCache(dir))
        cache.saveChannel(1, listOf(rich))
        cache.clear()
        assertEquals(0, dir.list()!!.size)
    }

    @Test fun `keys cannot escape the directory`() {
        val files = FileCache(dir)
        files.write("../../evil", listOf(rich.toProto()))
        assertTrue(dir.list()!!.all { !it.contains("/") })
        assertEquals(1, dir.list()!!.size)
    }
}
