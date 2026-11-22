package app.thehideout.ui.components

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import app.thehideout.design.Radius
import app.thehideout.ui.icons.Sym
import app.thehideout.ui.icons.SymIcon
import app.thehideout.ui.theme.Hideout

/** A grouped surface: `surface` on the page's `bg`, hairline border, crisp corners. */
@Composable
fun HideoutCard(modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    val c = Hideout.colors
    Surface(
        modifier = modifier,
        color = c.surface,
        shape = RoundedCornerShape(Radius.lg),
        border = BorderStroke(1.dp, c.borderSubtle),
        content = content,
    )
}

/** A small caps-ish heading above a group. */
@Composable
fun GroupLabel(text: String, modifier: Modifier = Modifier) {
    Text(
        text.uppercase(),
        modifier = modifier.padding(start = 20.dp, end = 20.dp, top = 18.dp, bottom = 8.dp),
        style = MaterialTheme.typography.labelMedium,
        color = Hideout.colors.textMuted,
    )
}

/** Nothing here yet: an icon, a line, an explanation. */
@Composable
fun EmptyState(sym: Sym, title: String, body: String, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    Column(
        modifier.fillMaxWidth().padding(32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        SymIcon(sym, null, size = 40.dp, tint = c.textFaint)
        Text(title, style = MaterialTheme.typography.titleMedium, color = c.text, textAlign = TextAlign.Center)
        Text(body, style = MaterialTheme.typography.bodyMedium, color = c.textMuted, textAlign = TextAlign.Center)
    }
}
