package app.thehideout.core

import app.thehideout.core.emoji.EmojiCategory
import app.thehideout.core.emoji.EmojiData
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class EmojiDataTest {
    @Test fun `search puts a name that starts with the query before one that merely contains it`() {
        val results = EmojiData.search("face")
        assertTrue(results.isNotEmpty())
        assertTrue(results.all { it.name.contains("face") })
        val firstContainsOnly = results.indexOfFirst { !it.name.startsWith("face") }
        val lastStartsWith = results.indexOfLast { it.name.startsWith("face") }
        assertTrue(firstContainsOnly == -1 || lastStartsWith < firstContainsOnly, "starts-with names come first")
    }

    @Test fun `an empty query returns the first page`() {
        assertEquals(24, EmojiData.search("", limit = 24).size)
    }

    @Test fun `every category has at least one entry`() {
        EmojiCategory.entries.forEach { assertTrue(EmojiData.inCategory(it).isNotEmpty(), "$it is empty") }
    }

    @Test fun `search is case-insensitive`() {
        assertEquals(EmojiData.search("FIRE"), EmojiData.search("fire"))
    }
}
