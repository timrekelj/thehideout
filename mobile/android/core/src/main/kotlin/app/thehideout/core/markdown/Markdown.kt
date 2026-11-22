package app.thehideout.core.markdown

/** Message markup: the subset the desktop client renders (`**bold**`, `*italic*`, `__underline__`, `~~strike~~`, `` `code` ``, `||spoiler||`, links, mentions, fences, quotes, lists, headings). */
sealed interface Inline {
    data class Text(val text: String) : Inline
    data class Bold(val children: List<Inline>) : Inline
    data class Italic(val children: List<Inline>) : Inline
    data class Underline(val children: List<Inline>) : Inline
    data class Strike(val children: List<Inline>) : Inline
    data class Spoiler(val children: List<Inline>) : Inline
    data class Code(val code: String) : Inline
    data class Link(val children: List<Inline>, val url: String) : Inline
    /** `@name`, `name` without the at sign. */
    data class Mention(val name: String) : Inline
    /** `<:name:id>` or, when [animated], `<a:name:id>` (MD-06). */
    data class CustomEmoji(val name: String, val id: Long, val animated: Boolean) : Inline
}

sealed interface Block {
    data class Paragraph(val inlines: List<Inline>) : Block
    data class Heading(val level: Int, val inlines: List<Inline>) : Block
    data class Quote(val inlines: List<Inline>) : Block
    data class CodeBlock(val language: String, val code: String) : Block
    data class ListBlock(val ordered: Boolean, val items: List<List<Inline>>) : Block
}

object Markdown {
    fun parse(text: String): List<Block> {
        val blocks = mutableListOf<Block>()
        val lines = text.split('\n')
        var i = 0
        val para = mutableListOf<String>()
        fun flush() {
            if (para.isNotEmpty()) blocks += Block.Paragraph(inline(para.joinToString("\n")))
            para.clear()
        }
        while (i < lines.size) {
            val line = lines[i]
            when {
                line.startsWith("```") -> {
                    flush()
                    val lang = line.removePrefix("```").trim()
                    val code = mutableListOf<String>()
                    i++
                    while (i < lines.size && !lines[i].startsWith("```")) code += lines[i++]
                    blocks += Block.CodeBlock(lang, code.joinToString("\n"))
                }
                line.startsWith("> ") -> {
                    flush()
                    val quote = mutableListOf<String>()
                    while (i < lines.size && lines[i].startsWith("> ")) quote += lines[i++].removePrefix("> ")
                    i--
                    blocks += Block.Quote(inline(quote.joinToString("\n")))
                }
                HEADING.matches(line) -> {
                    flush()
                    val m = HEADING.matchEntire(line)!!
                    blocks += Block.Heading(m.groupValues[1].length, inline(m.groupValues[2]))
                }
                BULLET.matches(line) || ORDERED.matches(line) -> {
                    flush()
                    val ordered = ORDERED.matches(line)
                    val items = mutableListOf<List<Inline>>()
                    while (i < lines.size && (if (ordered) ORDERED else BULLET).matches(lines[i])) {
                        items += inline((if (ordered) ORDERED else BULLET).matchEntire(lines[i])!!.groupValues[1])
                        i++
                    }
                    i--
                    blocks += Block.ListBlock(ordered, items)
                }
                line.isBlank() -> flush()
                else -> para += line
            }
            i++
        }
        flush()
        return blocks
    }

    private val HEADING = Regex("^(#{1,3}) (.+)$")
    private val BULLET = Regex("^[-*] (.+)$")
    private val ORDERED = Regex("^\\d+\\. (.+)$")
    private val URL = Regex("^https?://[^\\s<>()]+[^\\s<>().,;:!?'\"]")
    private val MENTION = Regex("^@([\\p{L}\\p{N}_]+)")
    private val CUSTOM_EMOJI = Regex("^<(a)?:([\\p{L}\\p{N}_]+):(\\d+)>")

    /** Parses inline markup; unmatched delimiters stay literal. */
    fun inline(s: String): List<Inline> {
        val out = mutableListOf<Inline>()
        val text = StringBuilder()
        fun flush() {
            if (text.isNotEmpty()) out += Inline.Text(text.toString())
            text.clear()
        }
        var i = 0
        while (i < s.length) {
            val c = s[i]
            val rest = s.substring(i)
            fun span(open: String, make: (List<Inline>) -> Inline): Boolean {
                if (!rest.startsWith(open)) return false
                var end = s.indexOf(open, i + open.length)
                if (end <= i + open.length) return false
                // In a longer run of the same mark the closing pair is its last two: `**a *b***`.
                while (end + open.length < s.length && s[end + open.length] == open[0] && open.length > 1) end++
                flush()
                out += make(inline(s.substring(i + open.length, end)))
                i = end + open.length
                return true
            }
            when {
                c == '\\' && i + 1 < s.length && s[i + 1] in "\\`*_~|[]@" -> { text.append(s[i + 1]); i += 2 }
                c == '`' -> {
                    val end = s.indexOf('`', i + 1)
                    if (end > i + 1) { flush(); out += Inline.Code(s.substring(i + 1, end)); i = end + 1 } else { text.append(c); i++ }
                }
                span("**") { Inline.Bold(it) } -> Unit
                span("__") { Inline.Underline(it) } -> Unit
                span("~~") { Inline.Strike(it) } -> Unit
                span("||") { Inline.Spoiler(it) } -> Unit
                (c == '*' || c == '_') && span(c.toString()) { Inline.Italic(it) } -> Unit
                c == '[' -> {
                    val close = s.indexOf("](", i + 1)
                    val end = if (close > 0) s.indexOf(')', close + 2) else -1
                    val url = if (end > 0) s.substring(close + 2, end) else ""
                    if (end > 0 && (url.startsWith("http://") || url.startsWith("https://"))) {
                        flush(); out += Inline.Link(inline(s.substring(i + 1, close)), url); i = end + 1
                    } else { text.append(c); i++ }
                }
                c == 'h' && URL.containsMatchIn(rest) && URL.find(rest)?.range?.first == 0 -> {
                    val url = URL.find(rest)!!.value
                    flush(); out += Inline.Link(listOf(Inline.Text(url)), url); i += url.length
                }
                c == '@' && (i == 0 || !s[i - 1].isLetterOrDigit()) && MENTION.containsMatchIn(rest) -> {
                    val name = MENTION.find(rest)!!.groupValues[1]
                    flush(); out += Inline.Mention(name); i += 1 + name.length
                }
                c == '<' && CUSTOM_EMOJI.matchAt(rest, 0) != null -> {
                    val m = CUSTOM_EMOJI.matchAt(rest, 0)!!
                    flush(); out += Inline.CustomEmoji(m.groupValues[2], m.groupValues[3].toLong(), m.groupValues[1] == "a"); i += m.value.length
                }
                else -> { text.append(c); i++ }
            }
        }
        flush()
        return out
    }
}
