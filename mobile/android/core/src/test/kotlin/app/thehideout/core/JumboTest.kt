package app.thehideout.core

import app.thehideout.core.emoji.JUMBO_MAX
import app.thehideout.core.emoji.JumboEmoji
import app.thehideout.core.emoji.jumboEmojis
import app.thehideout.core.markdown.Markdown
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

/** Ports desktop's `body.rs` jumbo tests (MD-07): a message of only emoji, Unicode and custom, draws large. */
class JumboTest {
    private fun jumbo(text: String) = jumboEmojis(Markdown.parse(text))

    @Test fun `plain text is never jumbo`() {
        assertNull(jumbo("hello"))
        assertNull(jumbo(""))
    }

    @Test fun `one unicode emoji is jumbo`() {
        val result = jumbo("😀")
        assertEquals(listOf(JumboEmoji.Unicode("😀")), result)
    }

    @Test fun `unicode emoji separated by spaces are each their own cluster`() {
        val result = jumbo("😀 😀")
        assertEquals(2, result?.size)
    }

    @Test fun `mixing emoji with ordinary text is not jumbo`() {
        assertNull(jumbo("😀 hello"))
    }

    @Test fun `a custom emoji token alone is jumbo`() {
        val result = jumbo("<:wave:42>")
        assertEquals(listOf(JumboEmoji.Custom(42, "wave", false)), result)
    }

    @Test fun `an animated custom emoji keeps its flag`() {
        val result = jumbo("<a:party:7>")
        assertEquals(listOf(JumboEmoji.Custom(7, "party", true)), result)
    }

    @Test fun `unicode and custom emoji mix freely, up to the cap`() {
        val result = jumbo("😀 <:wave:1> 😂")
        assertEquals(3, result?.size)
    }

    @Test fun `bold or a link around emoji is never jumbo`() {
        assertNull(jumbo("**😀**"))
        assertNull(jumbo("[😀](https://example.com)"))
    }

    @Test fun `more than one block is never jumbo`() {
        assertNull(jumbo("😀\n\n😀"))
    }

    @Test fun `more than JUMBO_MAX emoji is never jumbo`() {
        val many = "😀 ".repeat(JUMBO_MAX + 1).trim()
        assertNull(jumbo(many))
        val atCap = "😀 ".repeat(JUMBO_MAX).trim()
        assertEquals(JUMBO_MAX, jumbo(atCap)?.size)
    }

    @Test fun `a bare digit or hash is never treated as an emoji`() {
        assertNull(jumbo("#1"))
        assertTrue(jumbo("3️⃣") != null) // the actual keycap sequence "3️⃣" is fine
    }
}
