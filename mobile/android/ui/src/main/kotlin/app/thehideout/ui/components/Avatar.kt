package app.thehideout.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.thehideout.design.AvatarStyle
import app.thehideout.design.Radius
import app.thehideout.ui.theme.Hideout

/**
 * A user's picture: their initials on one of the accent's eight fills, picked by desktop's FNV-1a
 * rule over the id, so a person has the same colour on every device. Corners follow `Radius.avatar`.
 */
@Composable
fun Avatar(id: Long, name: String, modifier: Modifier = Modifier, size: Dp = 40.dp) {
    val accent = Hideout.spec.accent
    val fill = remember(id, accent) { AvatarStyle.fill(id.toString(), accent) }
    Box(
        modifier = modifier.size(size).clip(RoundedCornerShape(Radius.avatar(size))).background(fill),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = AvatarStyle.initials(name),
            color = AvatarStyle.ink,
            fontSize = (size.value * 0.4f).sp,
            fontWeight = FontWeight(600),
            maxLines = 1,
        )
    }
}

/** Overlapping avatars (who is in a voice channel), each ringed in the surface they sit on. */
@Composable
fun AvatarStack(people: List<Pair<Long, String>>, modifier: Modifier = Modifier, size: Dp = 24.dp, max: Int = 4) {
    val ring = Hideout.colors.bg
    Row(modifier) {
        people.take(max).forEachIndexed { i, (id, name) ->
            Avatar(
                id, name, size = size,
                modifier = Modifier.offset(x = -(size * 0.25f) * i).border(2.dp, ring, RoundedCornerShape(Radius.avatar(size))),
            )
        }
    }
}
