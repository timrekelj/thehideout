package app.thehideout.core

import app.thehideout.core.emoji.jumboEmojis
import app.thehideout.core.fake.FakeBackend
import app.thehideout.core.markdown.Markdown
import app.thehideout.core.model.EmbedKind
import kotlinx.coroutines.runBlocking
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertNotNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

/** `FakeBackend`'s custom emoji and embed seeds (MSG-09, ATT-06): bundled assets, a jumbo message, one embed per kind/provider. */
class FakeBackendEmbedsEmojiTest {
    @Test
    fun `the hideout has three custom emoji, one animated, keyed to bundled assets`() = runBlocking {
        val fake = FakeBackend()
        val emojis = fake.listEmojis(FakeBackend.HIDEOUT)
        assertEquals(3, emojis.size)
        assertTrue(emojis.all { it.image.startsWith("file:///android_asset/fake_emoji/") })
        assertEquals(1, emojis.count { it.animated })
        assertEquals(setOf("wave", "thumbs_up", "party_parrot"), emojis.map { it.name }.toSet())
    }

    @Test
    fun `design has an inline custom emoji message and a jumbo-only one`() = runBlocking {
        val fake = FakeBackend()
        val page = fake.listMessages(FakeBackend.DESIGN, before = null, limit = 50)
        val inline = page.items.first { it.content.contains("<:thumbs_up:") }
        val blocksInline = Markdown.parse(inline.content)
        assertEquals(null, jumboEmojis(blocksInline), "mixed text and emoji is not jumbo")

        val jumboMessage = page.items.first { it.content.contains("<a:party_parrot:") }
        val jumbo = jumboEmojis(Markdown.parse(jumboMessage.content))
        assertNotNull(jumbo)
        assertEquals(3, jumbo?.size)
        assertTrue(jumbo!!.all { it is app.thehideout.core.emoji.JumboEmoji.Custom && it.animated })
    }

    @Test
    fun `design has a link, an image, a video and one rich embed per provider`() = runBlocking {
        val fake = FakeBackend()
        val page = fake.listMessages(FakeBackend.DESIGN, before = null, limit = 50)
        val embeds = page.items.flatMap { it.embeds }
        assertEquals(7, embeds.size)
        assertEquals(1, embeds.count { it.kind == EmbedKind.Link })
        assertEquals(1, embeds.count { it.kind == EmbedKind.Image })
        assertEquals(1, embeds.count { it.kind == EmbedKind.Video })
        assertEquals(setOf("x", "tiktok", "instagram", "reddit"), embeds.filter { it.kind == EmbedKind.Rich }.map { it.provider }.toSet())
    }
}
