package app.thehideout.core

import app.thehideout.core.markdown.Block
import app.thehideout.core.markdown.Inline
import app.thehideout.core.markdown.Markdown
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test

class MarkdownTest {
    private fun t(s: String) = Inline.Text(s)

    @Test fun `inline marks nest and stay literal when unmatched`() {
        assertEquals(listOf(Inline.Bold(listOf(t("a "), Inline.Italic(listOf(t("b")))))), Markdown.inline("**a *b***"))
        assertEquals(listOf(t("2 * 3 = 6")), Markdown.inline("2 * 3 = 6"))
        assertEquals(listOf(t("a "), Inline.Code("b*c"), t(" d")), Markdown.inline("a `b*c` d"))
        assertEquals(listOf(Inline.Underline(listOf(t("u"))), t(" "), Inline.Strike(listOf(t("s"))), t(" "), Inline.Spoiler(listOf(t("x")))), Markdown.inline("__u__ ~~s~~ ||x||"))
        assertEquals(listOf(t("**not bold")), Markdown.inline("**not bold"))
    }

    @Test fun `links, bare urls and mentions`() {
        assertEquals(listOf(Inline.Link(listOf(t("the wiki")), "https://example.com/w")), Markdown.inline("[the wiki](https://example.com/w)"))
        assertEquals(listOf(t("see "), Inline.Link(listOf(t("https://example.com/c")), "https://example.com/c"), t(".")), Markdown.inline("see https://example.com/c."))
        assertEquals(listOf(t("hi "), Inline.Mention("tim"), t("!")), Markdown.inline("hi @tim!"))
        assertEquals(listOf(t("mail x@tim.com")), Markdown.inline("mail x@tim.com"), "an at sign inside a word is not a mention")
        assertEquals(listOf(t("[x](javascript:alert(1))")), Markdown.inline("[x](javascript:alert(1))"), "only http(s) links")
    }

    @Test fun `escapes keep marks literal`() {
        assertEquals(listOf(t("*x*")), Markdown.inline("\\*x\\*"))
    }

    @Test fun `blocks`() {
        val b = Markdown.parse("# Title\nintro\n\n- one\n- two\n\n1. a\n2. b\n\n> quoted\n> twice\n\n```sh\ncargo build\n```\nafter")
        assertEquals(
            listOf(
                Block.Heading(1, listOf(t("Title"))),
                Block.Paragraph(listOf(t("intro"))),
                Block.ListBlock(false, listOf(listOf(t("one")), listOf(t("two")))),
                Block.ListBlock(true, listOf(listOf(t("a")), listOf(t("b")))),
                Block.Quote(listOf(t("quoted\ntwice"))),
                Block.CodeBlock("sh", "cargo build"),
                Block.Paragraph(listOf(t("after"))),
            ),
            b,
        )
    }

    @Test fun `an unterminated fence runs to the end`() {
        assertEquals(listOf(Block.CodeBlock("", "a\nb")), Markdown.parse("```\na\nb"))
    }
}
