package app.thehideout.ui.components

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.spring
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.thehideout.design.AvatarStyle
import app.thehideout.design.Radius
import app.thehideout.ui.theme.Hideout

/** The accent pill with an unread count ("9+" above nine): quiet, for channels with new messages. */
@Composable
fun UnreadPill(count: Int, modifier: Modifier = Modifier) {
    if (count <= 0) return
    val c = Hideout.colors
    CountPill(if (count > 99) "99+" else count.toString(), c.accent, c.accentFg, modifier, "$count unread")
}

/** The red pill for mentions of you. */
@Composable
fun MentionBadge(count: Int, modifier: Modifier = Modifier) {
    if (count <= 0) return
    val c = Hideout.colors
    CountPill(if (count > 99) "99+" else count.toString(), c.dangerSolid, c.onDanger, modifier, "$count mentions")
}

@Composable
private fun CountPill(text: String, bg: androidx.compose.ui.graphics.Color, fg: androidx.compose.ui.graphics.Color, modifier: Modifier, description: String) {
    Box(
        modifier
            .semantics { contentDescription = description }
            .heightIn(min = 20.dp).widthIn(min = 20.dp)
            .clip(CircleShape).background(bg).padding(horizontal = 6.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(text, color = fg, fontSize = 11.sp, lineHeight = 14.sp, fontWeight = FontWeight(650), textAlign = TextAlign.Center, maxLines = 1)
    }
}

/**
 * A server in the picker: its icon (initials on a tile; the selected one in the accent, with its
 * corners tightening), a mention badge on the corner and an unread dot. The selected tile keeps a
 * pill at the leading edge, as desktop's rail does.
 */
@Composable
fun ServerBadge(
    name: String,
    selected: Boolean,
    modifier: Modifier = Modifier,
    unread: Int = 0,
    mentions: Int = 0,
    size: Dp = 48.dp,
    onClick: (() -> Unit)? = null,
) {
    val c = Hideout.colors
    val corner by animateDpAsState(if (selected) Radius.xl else size / 2 - 4.dp, spring(dampingRatio = 0.7f, stiffness = 400f), label = "corner")
    val bg by animateColorAsState(if (selected) c.accent else c.surfaceRaised, label = "bg")
    val fg = if (selected) c.accentFg else c.text
    Box(modifier.size(size + 12.dp), contentAlignment = Alignment.Center) {
        if (selected) {
            Box(
                Modifier.align(Alignment.CenterStart).size(width = 4.dp, height = 24.dp)
                    .clip(RoundedCornerShape(topEnd = 4.dp, bottomEnd = 4.dp)).background(c.accent),
            )
        }
        Box(
            Modifier.size(size).clip(RoundedCornerShape(corner)).background(bg)
                .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
                .semantics { contentDescription = name },
            contentAlignment = Alignment.Center,
        ) {
            Text(AvatarStyle.initials(name), color = fg, fontSize = (size.value * 0.36f).sp, fontWeight = FontWeight(650), maxLines = 1)
        }
        if (mentions > 0) {
            MentionBadge(mentions, Modifier.align(Alignment.BottomEnd).offset(x = (-2).dp, y = (-2).dp))
        } else if (unread > 0 && !selected) {
            Box(Modifier.align(Alignment.CenterStart).size(8.dp).clip(CircleShape).background(c.text))
        }
    }
}
