package app.thehideout.ui.components

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocal
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.thehideout.core.embed.EmbedRef
import app.thehideout.core.model.Emoji
import app.thehideout.core.model.Message
import app.thehideout.core.model.Reaction
import app.thehideout.core.model.ReactionEmoji
import app.thehideout.core.model.User
import app.thehideout.core.store.Delivery
import app.thehideout.design.Radius
import app.thehideout.ui.icons.Sym
import app.thehideout.ui.icons.SymIcon
import app.thehideout.ui.theme.Hideout
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale

/** The day "Today" means; `null` = the device's clock. Tests pin it. */
val LocalToday = compositionLocalOf<LocalDate?> { null }

/** The zone times are shown in (the device's, unless a test pins one). */
val LocalTimeZone = compositionLocalOf { ZoneId.systemDefault() }

/** "10:42" in the device's locale, in [LocalTimeZone]. */
@Composable
fun clockTime(at: Instant): String {
    val zone = LocalTimeZone.current
    return DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(Locale.getDefault()).format(at.atZone(zone))
}

/** What a message row shows beyond the [Message]: who wrote it, whether it starts a group, whether it mentions you. */
data class MessageUi(
    val message: Message,
    val author: User?,
    /** The first row of a run by one author: avatar, name and time. Follow-ups just indent. */
    val head: Boolean = true,
    /** The signed-in user's name, to highlight mentions of them. */
    val me: String? = null,
    val delivery: Delivery? = null,
)

/**
 * One message in a transcript. Heads show the avatar, name and time; follow-ups line up under the
 * text. Mentions of you wash the row in the accent; a pending send dims, a failed one offers retry.
 */
@Composable
fun MessageRow(
    ui: MessageUi,
    modifier: Modifier = Modifier,
    onReact: (Reaction) -> Unit = {},
    onRetry: () -> Unit = {},
    onOpenLightbox: (LightboxRequest) -> Unit = {},
    serverEmojis: List<Emoji> = emptyList(),
    collapsedEmbeds: Set<EmbedRef> = emptySet(),
    onToggleEmbedCollapsed: (Int) -> Unit = {},
) {
    val c = Hideout.colors
    val m = ui.message
    val customEmojis = remember(serverEmojis) { serverEmojis.associateBy { it.id } }
    val mentionsMe = ui.me != null && app.thehideout.core.store.mentions(m.content, ui.me)
    val failed = ui.delivery == Delivery.Failed
    val metrics = Hideout.metrics
    val (padTop, padBottom) = if (ui.head) metrics.msgPadHead else metrics.msgPadFollow
    Row(
        modifier
            .fillMaxWidth()
            .height(IntrinsicSize.Min)
            .then(if (mentionsMe) Modifier.background(c.accentWash) else if (failed) Modifier.background(c.dangerBg) else Modifier)
            .padding(top = padTop + 3.dp, bottom = padBottom + 3.dp, end = 16.dp),
    ) {
        Box(Modifier.width(3.dp).fillMaxHeight().background(if (mentionsMe) c.accent else androidx.compose.ui.graphics.Color.Transparent))
        Box(Modifier.width(13.dp))
        Box(Modifier.width(40.dp)) {
            if (ui.head) Avatar(m.authorId, ui.author?.displayName ?: "?", size = 40.dp)
        }
        Column(Modifier.weight(1f).padding(start = 12.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            if (ui.head) {
                Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text(ui.author?.displayName ?: "user ${m.authorId}", style = MaterialTheme.typography.titleSmall, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(clockTime(m.createdAt), style = MaterialTheme.typography.bodySmall, color = c.textFaint)
                }
            }
            Box(Modifier.alpha(if (ui.delivery == Delivery.Pending) 0.6f else 1f)) {
                MarkdownText(m.content, me = ui.me, customEmojis = customEmojis)
            }
            if (m.editedAt != null) Text("edited", style = MaterialTheme.typography.bodySmall, color = c.textFaint)
            if (m.attachments.isNotEmpty()) MessageAttachments(m.attachments, onOpenLightbox = onOpenLightbox)
            m.embeds.forEachIndexed { index, embed ->
                EmbedCard(
                    embed,
                    collapsed = EmbedRef(m.id, index) in collapsedEmbeds,
                    onToggleCollapsed = { onToggleEmbedCollapsed(index) },
                    onOpenLightbox = onOpenLightbox,
                )
            }
            if (m.reactions.isNotEmpty()) {
                FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp), modifier = Modifier.padding(top = 4.dp)) {
                    m.reactions.forEach { ReactionChip(it, customEmojis = customEmojis, onClick = { onReact(it) }) }
                }
            }
            if (ui.delivery == Delivery.Pending) Text("Sending…", style = MaterialTheme.typography.bodySmall, color = c.textFaint)
            if (failed) {
                Row(Modifier.clickable(onClick = onRetry).padding(vertical = 4.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text("Not sent.", style = MaterialTheme.typography.bodySmall, color = c.danger)
                    Text("Tap to retry", style = MaterialTheme.typography.bodySmall, color = c.danger, fontWeight = FontWeight(650))
                }
            }
        }
    }
}

/** An emoji and its count; yours is outlined in the accent. */
@Composable
fun ReactionChip(r: Reaction, modifier: Modifier = Modifier, customEmojis: Map<Long, Emoji> = emptyMap(), onClick: () -> Unit = {}) {
    val c = Hideout.colors
    val mediaBase = LocalMediaBase.current
    Surface(
        onClick = onClick,
        modifier = modifier,
        shape = RoundedCornerShape(Radius.md),
        color = if (r.me) c.accentSubtle else c.surfaceRaised,
        border = BorderStroke(1.dp, if (r.me) c.accent else c.borderSubtle),
    ) {
        Row(Modifier.padding(horizontal = 10.dp, vertical = 5.dp), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
            when (val e = r.emoji) {
                is ReactionEmoji.Unicode -> Text(e.emoji, style = MaterialTheme.typography.bodyLarge)
                is ReactionEmoji.Custom -> {
                    val emoji = customEmojis[e.id]
                    if (emoji != null) InlineCustomEmoji(emoji, mediaBase, 20.dp) else Text(":${e.id}:", style = MaterialTheme.typography.bodyLarge)
                }
            }
            Text(r.count.toString(), style = MaterialTheme.typography.labelMedium, color = if (r.me) c.accentText else c.textMuted, fontWeight = FontWeight(600))
        }
    }
}

/** "Today", "Yesterday" or a date, between runs of messages from different days. */
@Composable
fun DayDivider(date: LocalDate, today: LocalDate, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    val label = when (date) {
        today -> "Today"
        today.minusDays(1) -> "Yesterday"
        else -> DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM).withLocale(Locale.getDefault()).format(date)
    }
    Row(modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Box(Modifier.weight(1f).height(1.dp).background(c.border))
        Text(label, style = MaterialTheme.typography.labelMedium, color = c.textMuted)
        Box(Modifier.weight(1f).height(1.dp).background(c.border))
    }
}

/** The "New" divider before the first unread message. */
@Composable
fun NewDivider(modifier: Modifier = Modifier) {
    val c = Hideout.colors
    Row(modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Box(Modifier.weight(1f).height(1.dp).background(c.danger))
        Text("New", style = MaterialTheme.typography.labelMedium, color = c.danger, fontWeight = FontWeight(650))
    }
}
