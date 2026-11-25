package app.thehideout.core.emoji

import app.thehideout.core.markdown.Block
import app.thehideout.core.markdown.Inline
import java.text.BreakIterator

/** One emoji of a jumbo message (MD-07): a Unicode grapheme cluster, or a custom emoji token. */
sealed interface JumboEmoji {
    data class Unicode(val value: String) : JumboEmoji
    data class Custom(val id: Long, val name: String, val animated: Boolean) : JumboEmoji
}

/** Most emoji a message of only emoji may have and still draw jumbo (desktop's `JUMBO_MAX`). */
const val JUMBO_MAX = 27

/** Side of a custom emoji in a line of text, in dp (desktop's `EMOJI_INLINE`). */
const val EMOJI_INLINE_DP = 20f
/** Side of a custom emoji (and the font size of a Unicode one) in a jumbo message, in dp (desktop's `EMOJI_JUMBO`). */
const val EMOJI_JUMBO_DP = 48f

/**
 * The emoji of a message that is nothing but emoji and spaces: at least one and at most
 * [JUMBO_MAX], with no other markup around them (a single paragraph of only [Inline.Text] made of
 * emoji and whitespace, and [Inline.CustomEmoji]). Anything else - bold, a link, a mention, more
 * than one block - returns `null`, same as desktop's `jumbo`.
 */
fun jumboEmojis(blocks: List<Block>): List<JumboEmoji>? {
    val only = (blocks.singleOrNull() as? Block.Paragraph)?.inlines ?: return null
    val out = mutableListOf<JumboEmoji>()
    for (node in only) {
        when (node) {
            is Inline.Text -> {
                val clusters = graphemeClusters(node.text)
                for (cluster in clusters) {
                    if (cluster.isBlank()) continue
                    if (!isEmojiCluster(cluster)) return null
                    if (out.size >= JUMBO_MAX) return null
                    out += JumboEmoji.Unicode(cluster)
                }
            }
            is Inline.CustomEmoji -> {
                if (out.size >= JUMBO_MAX) return null
                out += JumboEmoji.Custom(node.id, node.name, node.animated)
            }
            else -> return null
        }
    }
    return out.takeIf { it.isNotEmpty() }
}

/** Splits `text` into user-perceived characters (so a flag or a ZWJ sequence stays one cluster). */
private fun graphemeClusters(text: String): List<String> {
    val it = BreakIterator.getCharacterInstance()
    it.setText(text)
    val out = mutableListOf<String>()
    var start = it.first()
    var end = it.next()
    while (end != BreakIterator.DONE) {
        out += text.substring(start, end)
        start = end
        end = it.next()
    }
    return out
}

/**
 * Whether every code point of `cluster` belongs to an emoji building block: a pictograph, a
 * symbol commonly presented as emoji, a regional indicator (flags), a skin-tone modifier, the
 * zero-width joiner, a variation selector, or the keycap combiner. A curated approximation (like
 * [EmojiData]), not full Unicode emoji-property data: good enough to tell "only emoji" apart from
 * ordinary text without bundling a Unicode database.
 */
private fun isEmojiCluster(cluster: String): Boolean {
    val cps = cluster.codePoints().toArray()
    if (cps.isEmpty()) return false
    // '#'/'*'/a digit is only an emoji as a keycap sequence (base + U+20E3), never alone.
    if (cps[0] == 0x23 || cps[0] == 0x2A || cps[0] in 0x30..0x39) return cps.size > 1 && cps.last() == 0x20E3
    return cps.all { isEmojiCodePoint(it) }
}

private fun isEmojiCodePoint(cp: Int): Boolean = when (cp) {
    0x200D, 0xFE0F, 0x20E3 -> true // ZWJ, variation selector, keycap combiner
    in 0x1F1E6..0x1F1FF -> true // regional indicators (flags)
    in 0x1F3FB..0x1F3FF -> true // skin tone modifiers
    in 0x2600..0x27BF -> true // misc symbols, dingbats
    in 0x2300..0x23FF -> true // misc technical (e.g. hourglass, watch)
    in 0x2B00..0x2BFF -> true // arrows/stars
    in 0x1F000..0x1FAFF -> true // emoji & pictographs, supplemental symbols
    in 0x3030..0x303D -> true
    0x2122, 0x2139, 0x2194, 0x2195, 0x2196, 0x2197, 0x2198, 0x2199 -> true
    else -> false
}
