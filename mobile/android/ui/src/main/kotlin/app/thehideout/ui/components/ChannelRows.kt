package app.thehideout.ui.components

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.thehideout.core.model.Channel
import app.thehideout.core.model.ChannelIconRef
import app.thehideout.core.model.ChannelKind
import app.thehideout.core.model.VoiceState
import app.thehideout.core.store.Unread
import app.thehideout.design.Radius
import app.thehideout.ui.icons.Sym
import app.thehideout.ui.icons.SymIcon
import app.thehideout.ui.theme.Hideout

/** A touch row is never shorter than 48 dp, whatever the density's row height says. */
private val MinTouch = 48.dp

/**
 * A channel in the list: its icon in the channel's palette colour (Phosphor glyphs arrive with
 * DS-04, so text channels wear a hash and voice channels a speaker; an emoji icon is drawn as
 * text), the name (strong while unread) and the unread or mention badge. A voice channel lists who
 * is in it underneath.
 */
@Composable
fun ChannelRow(
    channel: Channel,
    selected: Boolean,
    unread: Unread,
    modifier: Modifier = Modifier,
    voice: List<VoiceState> = emptyList(),
    names: (Long) -> Pair<Long, String> = { it to "user $it" },
    onClick: () -> Unit = {},
) {
    val c = Hideout.colors
    val bg by animateColorAsState(if (selected) c.accentSubtle else Color.Transparent, label = "row")
    val ink = c.channel(channel.color)?.ink ?: if (selected) c.accentText else c.textMuted
    val hasUnread = unread.count > 0 && !selected
    Column(
        modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(Radius.lg))
            .background(bg)
            .semantics(mergeDescendants = true) { this.selected = selected; this.role = Role.Tab; contentDescription = channel.name }
            .clickable(onClick = onClick),
    ) {
        Row(
            Modifier.fillMaxWidth().heightIn(min = MinTouch).padding(horizontal = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            ChannelGlyph(channel, ink)
            Text(
                channel.name,
                modifier = Modifier.weight(1f),
                style = MaterialTheme.typography.bodyLarge,
                color = when {
                    selected -> c.accentText
                    hasUnread -> c.text
                    else -> c.textMuted
                },
                fontWeight = if (hasUnread || selected) FontWeight(650) else null,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            if (unread.mentions > 0) MentionBadge(unread.mentions) else if (hasUnread) UnreadPill(unread.count)
        }
        voice.forEach { v ->
            val (id, name) = names(v.userId)
            Row(
                Modifier.fillMaxWidth().padding(start = 44.dp, end = 12.dp).heightIn(min = 32.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Avatar(id, name, size = 22.dp)
                Text(name, modifier = Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium, color = c.textMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (v.selfMute || v.serverMute) SymIcon(Sym.MicOff, "muted", size = 16.dp, tint = c.textFaint)
                if (v.selfDeaf) SymIcon(Sym.Headphones, "deafened", size = 16.dp, tint = c.textFaint)
            }
        }
    }
}

@Composable
private fun ChannelGlyph(channel: Channel, ink: Color) {
    when (val icon = channel.icon) {
        is ChannelIconRef.Unicode -> Text(icon.emoji, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.size(24.dp))
        else -> SymIcon(if (channel.kind == ChannelKind.Voice) Sym.VolumeUp else Sym.Tag, null, size = 22.dp, tint = ink)
    }
}

/** A section's header: its name, a chevron that turns when it folds, and the unread total while folded. */
@Composable
fun SectionHeader(name: String, open: Boolean, modifier: Modifier = Modifier, unread: Unread = Unread(), onToggle: () -> Unit = {}) {
    val c = Hideout.colors
    val turn by animateFloatAsState(if (open) 0f else -90f, label = "chevron")
    Row(
        modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(Radius.md))
            .semantics(mergeDescendants = true) { contentDescription = "$name, ${if (open) "expanded" else "collapsed"}" }
            .clickable(onClick = onToggle)
            .heightIn(min = 40.dp)
            .padding(horizontal = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        SymIcon(Sym.ExpandMore, null, size = 18.dp, tint = c.textFaint, modifier = Modifier.rotate(turn))
        Text(name.uppercase(), modifier = Modifier.weight(1f), style = MaterialTheme.typography.labelMedium, color = c.textMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
        if (!open) {
            if (unread.mentions > 0) MentionBadge(unread.mentions) else if (unread.count > 0) UnreadPill(unread.count)
        }
    }
}
