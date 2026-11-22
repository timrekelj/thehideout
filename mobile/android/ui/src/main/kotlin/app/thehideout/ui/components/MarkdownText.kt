package app.thehideout.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import app.thehideout.core.markdown.Block
import app.thehideout.core.markdown.Inline
import app.thehideout.core.markdown.Markdown
import app.thehideout.design.DesignFonts
import app.thehideout.design.Radius
import app.thehideout.ui.theme.Hideout

/**
 * Message text: the parsed blocks of [Markdown] drawn with the type ramp. Mentions are accent-washed
 * (stronger when it is [me]), links open in the browser, spoilers hide behind a tap.
 */
@Composable
fun MarkdownText(text: String, modifier: Modifier = Modifier, me: String? = null, color: Color = Hideout.colors.text) {
    val blocks = remember(text) { Markdown.parse(text) }
    val c = Hideout.colors
    val revealed = remember(text) { mutableStateListOf<Int>() }
    Column(modifier, verticalArrangement = Arrangement.spacedBy(4.dp)) {
        for (b in blocks) when (b) {
            is Block.Paragraph -> Text(rememberInline(b.inlines, me, revealed), style = MaterialTheme.typography.bodyLarge, color = color)
            is Block.Heading -> Text(
                rememberInline(b.inlines, me, revealed),
                style = if (b.level == 1) MaterialTheme.typography.titleLarge else MaterialTheme.typography.titleMedium,
                color = color,
            )
            is Block.Quote -> Row(Modifier.height(IntrinsicSize.Min)) {
                Box(Modifier.width(3.dp).fillMaxHeight().clip3(c.borderStrong))
                Text(
                    rememberInline(b.inlines, me, revealed),
                    style = MaterialTheme.typography.bodyLarge, color = c.textMuted, modifier = Modifier.padding(start = 10.dp),
                )
            }
            is Block.CodeBlock -> Box(
                Modifier.fillMaxWidth().clip4(RoundedCornerShape(Radius.md), c.surfaceOverlay).horizontalScroll(rememberScrollState()).padding(horizontal = 12.dp, vertical = 10.dp),
            ) {
                Text(b.code, style = Hideout.type.code.toTextStyle(), color = c.text, softWrap = false)
            }
            is Block.ListBlock -> Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                b.items.forEachIndexed { i, item ->
                    Row {
                        if (b.ordered) Text("${i + 1}.", style = MaterialTheme.typography.bodyLarge, color = c.textMuted, modifier = Modifier.width(22.dp))
                        else Box(Modifier.width(22.dp).height(21.dp), contentAlignment = androidx.compose.ui.Alignment.CenterStart) { Box(Modifier.padding(start = 7.dp).size(5.dp).background(c.textMuted, androidx.compose.foundation.shape.CircleShape)) }
                        Text(rememberInline(item, me, revealed), style = MaterialTheme.typography.bodyLarge, color = color)
                    }
                }
            }
        }
    }
}

private fun Modifier.clip3(color: Color) = this.then(Modifier.background(color, RoundedCornerShape(2.dp)))
private fun Modifier.clip4(shape: RoundedCornerShape, color: Color) = this.then(Modifier.background(color, shape))

@Composable
private fun rememberInline(inlines: List<Inline>, me: String?, revealed: MutableList<Int>): AnnotatedString {
    val c = Hideout.colors
    val mono = DesignFonts.mono
    val style = MaterialTheme.typography.bodyLarge
    val hidden = revealed.toList()
    return remember(inlines, me, c, hidden) {
        buildAnnotatedString {
            var spoilers = 0
            fun walk(list: List<Inline>) {
                for (n in list) when (n) {
                    is Inline.Text -> append(n.text)
                    is Inline.Bold -> withStyle(SpanStyle(fontWeight = FontWeight(650))) { walk(n.children) }
                    is Inline.Italic -> withStyle(SpanStyle(fontStyle = FontStyle.Italic)) { walk(n.children) }
                    is Inline.Underline -> withStyle(SpanStyle(textDecoration = TextDecoration.Underline)) { walk(n.children) }
                    is Inline.Strike -> withStyle(SpanStyle(textDecoration = TextDecoration.LineThrough, color = c.textMuted)) { walk(n.children) }
                    is Inline.Code -> withStyle(SpanStyle(fontFamily = mono, fontSize = style.fontSize * 0.92f, background = c.layer2)) { append(" ${n.code} ") }
                    is Inline.Link -> withLink(
                        LinkAnnotation.Url(n.url, TextLinkStyles(SpanStyle(color = c.accentText, textDecoration = TextDecoration.Underline))),
                    ) { walk(n.children) }
                    is Inline.Mention -> {
                        val mine = me != null && n.name.equals(me, ignoreCase = true)
                        withStyle(
                            SpanStyle(
                                color = c.accentText,
                                background = if (mine) c.accent.copy(alpha = 0.28f) else c.accentWash,
                                fontWeight = FontWeight(if (mine) 650 else 600),
                            ),
                        ) { append("@${n.name}") }
                    }
                    is Inline.CustomEmoji -> {
                        // Rendered as its name until the guild emoji's image is decoded (Coil, MSG-09);
                        // desktop shows the same `:name:` text for a token it cannot resolve.
                        withStyle(SpanStyle(color = c.textMuted)) { append(":${n.name}:") }
                    }
                    is Inline.Spoiler -> {
                        val id = spoilers++
                        val shown = id in hidden
                        withLink(
                            LinkAnnotation.Clickable(
                                tag = "spoiler-$id",
                                styles = TextLinkStyles(if (shown) SpanStyle(background = c.layer2) else SpanStyle(color = c.layer3, background = c.layer3)),
                                linkInteractionListener = { if (id in revealed) revealed.remove(id) else revealed.add(id) },
                            ),
                        ) { walk(n.children) }
                    }
                }
            }
            walk(inlines)
        }
    }
}
