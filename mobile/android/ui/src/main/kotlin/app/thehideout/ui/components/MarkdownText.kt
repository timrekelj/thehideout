package app.thehideout.ui.components

import android.widget.Toast
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
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
import androidx.compose.foundation.text.InlineTextContent
import androidx.compose.foundation.text.appendInlineContent
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.Placeholder
import androidx.compose.ui.text.PlaceholderVerticalAlign
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.thehideout.core.emoji.EMOJI_INLINE_DP
import app.thehideout.core.emoji.EMOJI_JUMBO_DP
import app.thehideout.core.emoji.JumboEmoji
import app.thehideout.core.emoji.jumboEmojis
import app.thehideout.core.attachment.mediaUrl
import app.thehideout.core.markdown.Block
import app.thehideout.core.markdown.Inline
import app.thehideout.core.markdown.Markdown
import app.thehideout.core.model.Emoji
import app.thehideout.design.DesignFonts
import app.thehideout.design.Radius
import app.thehideout.ui.theme.Hideout
import coil3.compose.AsyncImage
import coil3.compose.AsyncImagePainter

/**
 * Message text: the parsed blocks of [Markdown] drawn with the type ramp. Mentions are accent-washed
 * (stronger when it is [me]), links open in the browser, spoilers hide behind a tap. Custom emoji
 * tokens [customEmojis] knows draw as pictures (MD-06); a message that is nothing but emoji (at
 * most 27) draws jumbo-sized instead of the normal blocks (MD-07).
 */
@Composable
fun MarkdownText(
    text: String,
    modifier: Modifier = Modifier,
    me: String? = null,
    color: Color = Hideout.colors.text,
    customEmojis: Map<Long, Emoji> = emptyMap(),
) {
    val blocks = remember(text) { Markdown.parse(text) }
    val c = Hideout.colors
    val revealed = remember(text) { mutableStateListOf<Int>() }
    val mediaBase = LocalMediaBase.current
    val jumbo = remember(blocks, customEmojis) { jumboEmojis(blocks) }
    if (jumbo != null) {
        JumboRow(jumbo, customEmojis, mediaBase, modifier)
        return
    }
    val inlineContent = rememberEmojiInlineContent(customEmojis, mediaBase)
    Column(modifier, verticalArrangement = Arrangement.spacedBy(4.dp)) {
        for (b in blocks) when (b) {
            is Block.Paragraph -> Text(rememberInline(b.inlines, me, revealed, customEmojis), style = MaterialTheme.typography.bodyLarge, color = color, inlineContent = inlineContent)
            is Block.Heading -> Text(
                rememberInline(b.inlines, me, revealed, customEmojis),
                style = if (b.level == 1) MaterialTheme.typography.titleLarge else MaterialTheme.typography.titleMedium,
                color = color,
                inlineContent = inlineContent,
            )
            is Block.Quote -> Row(Modifier.height(IntrinsicSize.Min)) {
                Box(Modifier.width(3.dp).fillMaxHeight().clip3(c.borderStrong))
                Text(
                    rememberInline(b.inlines, me, revealed, customEmojis),
                    style = MaterialTheme.typography.bodyLarge, color = c.textMuted, modifier = Modifier.padding(start = 10.dp),
                    inlineContent = inlineContent,
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
                        Text(rememberInline(item, me, revealed, customEmojis), style = MaterialTheme.typography.bodyLarge, color = color, inlineContent = inlineContent)
                    }
                }
            }
        }
    }
}

private fun Modifier.clip3(color: Color) = this.then(Modifier.background(color, RoundedCornerShape(2.dp)))
private fun Modifier.clip4(shape: RoundedCornerShape, color: Color) = this.then(Modifier.background(color, shape))

private fun emojiContentId(id: Long) = "emoji-$id"

/** Inline content keyed by [emojiContentId], one per custom emoji the text may use (MD-06). */
@Composable
private fun rememberEmojiInlineContent(customEmojis: Map<Long, Emoji>, mediaBase: String): Map<String, InlineTextContent> =
    remember(customEmojis, mediaBase) {
        customEmojis.values.associate { emoji ->
            emojiContentId(emoji.id) to InlineTextContent(
                Placeholder(EMOJI_INLINE_DP.sp, EMOJI_INLINE_DP.sp, PlaceholderVerticalAlign.TextCenter),
            ) {
                InlineCustomEmoji(emoji, mediaBase, EMOJI_INLINE_DP.dp)
            }
        }
    }

/** One custom emoji picture: a graceful `:name:` on a failed load, `:name:` on a long press. */
@Composable
internal fun InlineCustomEmoji(emoji: Emoji, mediaBase: String, size: androidx.compose.ui.unit.Dp) {
    val context = LocalContext.current
    var failed by remember(emoji.id) { mutableStateOf(false) }
    if (failed) {
        Text(":${emoji.name}:", style = MaterialTheme.typography.labelSmall, color = Hideout.colors.textMuted)
    } else {
        AsyncImage(
            model = mediaUrl(mediaBase, emoji.image),
            contentDescription = ":${emoji.name}:",
            modifier = Modifier.size(size).pointerInput(emoji.id) {
                detectTapGestures(onLongPress = { Toast.makeText(context, ":${emoji.name}:", Toast.LENGTH_SHORT).show() })
            },
            onState = { if (it is AsyncImagePainter.State.Error) failed = true },
        )
    }
}

/** A message of only emoji (MD-07): Unicode glyphs and custom emoji pictures, drawn large. */
@Composable
private fun JumboRow(items: List<JumboEmoji>, customEmojis: Map<Long, Emoji>, mediaBase: String, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    FlowRow(modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        items.forEach { item ->
            when (item) {
                is JumboEmoji.Unicode -> Text(item.value, fontSize = EMOJI_JUMBO_DP.sp, lineHeight = (EMOJI_JUMBO_DP + 8).sp, color = c.text)
                is JumboEmoji.Custom -> {
                    val emoji = customEmojis[item.id]
                    if (emoji != null) InlineCustomEmoji(emoji, mediaBase, EMOJI_JUMBO_DP.dp)
                    else Text(":${item.name}:", style = MaterialTheme.typography.bodyLarge, color = c.textMuted)
                }
            }
        }
    }
}

@Composable
private fun rememberInline(inlines: List<Inline>, me: String?, revealed: MutableList<Int>, customEmojis: Map<Long, Emoji>): AnnotatedString {
    val c = Hideout.colors
    val mono = DesignFonts.mono
    val style = MaterialTheme.typography.bodyLarge
    val hidden = revealed.toList()
    return remember(inlines, me, c, hidden, customEmojis) {
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
                        val emoji = customEmojis[n.id]
                        if (emoji != null) {
                            appendInlineContent(emojiContentId(emoji.id), ":${n.name}:")
                        } else {
                            // Unknown to this guild (deleted, or a DM with no server emoji): desktop
                            // shows the same literal `:name:` text.
                            withStyle(SpanStyle(color = c.textMuted)) { append(":${n.name}:") }
                        }
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
