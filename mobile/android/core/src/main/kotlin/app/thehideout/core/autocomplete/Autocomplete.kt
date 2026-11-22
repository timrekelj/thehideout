package app.thehideout.core.autocomplete

import app.thehideout.core.emoji.EmojiEntry
import app.thehideout.core.model.Emoji
import app.thehideout.core.model.User
import app.thehideout.core.emoji.EmojiData

/** What the composer is suggesting, derived from the text before the caret. */
sealed interface Trigger {
    data class Mention(val query: String, val start: Int) : Trigger
    data class EmojiShortcode(val query: String, val start: Int) : Trigger
    data object None : Trigger
}

object Autocomplete {
    private val WORD_BOUNDARY = Regex("[\\s]")

    /**
     * What the composer should suggest for [text] with the caret at [caret]: an unbroken run of
     * non-whitespace back from the caret that starts with `@` (mention) or `:` (emoji shortcode).
     * Mobile-only nicety; desktop has no `@` autocomplete (MSG-12).
     */
    fun trigger(text: String, caret: Int): Trigger {
        if (caret <= 0 || caret > text.length) return Trigger.None
        var start = caret - 1
        while (start >= 0 && !WORD_BOUNDARY.matches(text[start].toString())) {
            when (text[start]) {
                ':' -> return Trigger.EmojiShortcode(text.substring(start + 1, caret), start)
                '@' -> return Trigger.Mention(text.substring(start + 1, caret), start)
            }
            start--
        }
        return Trigger.None
    }

    /** Members whose username starts with [query] (case-insensitive), up to [limit], as desktop's `@` chip does. */
    fun mentionMatches(query: String, members: List<User>, limit: Int = 8): List<User> {
        if (query.isEmpty()) return members.take(limit)
        return members.filter { it.username.startsWith(query, ignoreCase = true) }.sortedBy { it.username.lowercase() }.take(limit)
    }

    /** `:sm` -> up to 8 rows, server emoji first (MSG-10). */
    fun emojiMatches(query: String, serverEmojis: List<Emoji>, limit: Int = 8): List<EmojiEntry> {
        val custom = serverEmojis.filter { it.name.contains(query, ignoreCase = true) }
            .sortedBy { it.name.lowercase() }
            .map { EmojiEntry.Custom(it.id, it.name, it.image, it.animated) }
        val unicode = EmojiData.search(query)
        return (custom + unicode).take(limit)
    }
}
