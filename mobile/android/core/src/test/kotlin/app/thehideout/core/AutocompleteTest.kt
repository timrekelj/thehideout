package app.thehideout.core

import app.thehideout.core.autocomplete.Autocomplete
import app.thehideout.core.autocomplete.Trigger
import app.thehideout.core.emoji.EmojiEntry
import app.thehideout.core.model.Emoji
import app.thehideout.core.model.User
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import java.time.Instant

class AutocompleteTest {
    private fun user(id: Long, name: String) = User(id, name, Instant.EPOCH)

    @Test fun `a colon starts an emoji trigger with everything after it as the query`() {
        assertEquals(Trigger.EmojiShortcode("sm", 6), Autocomplete.trigger("hello :sm", 9))
    }

    @Test fun `an at sign starts a mention trigger with everything after it as the query`() {
        assertEquals(Trigger.Mention("a", 4), Autocomplete.trigger("hey @a", 6))
    }

    @Test fun `a mention trigger still fires right after the at sign, with an empty query`() {
        assertEquals(Trigger.Mention("", 0), Autocomplete.trigger("@", 1))
    }

    @Test fun `no colon or at sign before the caret is no trigger`() {
        assertEquals(Trigger.None, Autocomplete.trigger("hello there", 11))
        assertEquals(Trigger.None, Autocomplete.trigger("", 0))
    }

    @Test fun `a space closes the trigger`() {
        assertEquals(Trigger.None, Autocomplete.trigger(":smile ", 7))
    }

    @Test fun `mention matches are prefix and case-insensitive`() {
        val members = listOf(user(1, "bruno"), user(2, "ana"), user(3, "Anton"))
        assertEquals(listOf("ana", "Anton"), Autocomplete.mentionMatches("an", members).map { it.username })
    }

    @Test fun `an empty query returns every member up to the limit`() {
        val members = listOf(user(1, "bruno"), user(2, "ana"), user(3, "anton"))
        assertEquals(3, Autocomplete.mentionMatches("", members).size)
        assertEquals(2, Autocomplete.mentionMatches("", members, limit = 2).size)
    }

    @Test fun `emoji matches put server emoji first, then unicode, by query`() {
        val custom = listOf(Emoji(10, 1, "smol", false, "img", 1, Instant.EPOCH))
        val matches = Autocomplete.emojiMatches("sm", custom)
        assertTrue(matches.first() is EmojiEntry.Custom)
        assertEquals("smol", (matches.first() as EmojiEntry.Custom).name)
        assertTrue(matches.drop(1).all { it is EmojiEntry.Unicode })
    }
}
