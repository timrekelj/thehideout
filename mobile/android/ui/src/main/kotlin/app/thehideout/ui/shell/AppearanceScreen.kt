package app.thehideout.ui.shell

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import app.thehideout.ui.components.Avatar
import app.thehideout.ui.components.HideoutCard
import app.thehideout.ui.gallery.LookControls
import app.thehideout.ui.icons.Sym
import app.thehideout.ui.icons.SymIcon
import app.thehideout.ui.theme.Hideout
import app.thehideout.ui.theme.Look

/**
 * SET-01: every look axis, with a live preview — the sample conversation re-themes with every
 * touch, the same way the whole app does, because [Hideout.colors] is read straight from [look].
 */
@Composable
fun AppearanceScreen(look: Look, onLookChange: (Look) -> Unit, onBack: () -> Unit, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    BackHandler(onBack = onBack)
    Column(modifier.fillMaxSize().background(c.bg).statusBarsPadding().verticalScroll(rememberScrollState())) {
        Row(Modifier.fillMaxWidth().padding(start = 4.dp, end = 20.dp, top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack) { SymIcon(Sym.ArrowBack, "Back") }
            Text("Appearance", style = MaterialTheme.typography.headlineSmall, color = c.text)
        }
        PreviewCard()
        LookControls(look, onLookChange, modifier = Modifier.padding(top = 8.dp), scrollable = false)
    }
}

/** A small, static sample conversation: every colour and type token a look changes, in one place. */
@Composable
private fun PreviewCard() {
    val c = Hideout.colors
    HideoutCard(Modifier.padding(horizontal = 16.dp).fillMaxWidth()) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Avatar(2L, "bruno", size = 36.dp)
                Column {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                        Text("bruno", style = MaterialTheme.typography.labelLarge, color = c.accentText)
                        Text("12:04", style = MaterialTheme.typography.bodySmall, color = c.textFaint)
                    }
                    Text("the new ramen place on Elm opens at noon, you in?", style = MaterialTheme.typography.bodyLarge, color = c.text)
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Avatar(1L, "tim", size = 36.dp)
                Column {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                        Text("tim", style = MaterialTheme.typography.labelLarge, color = c.accentText)
                        Text("12:06", style = MaterialTheme.typography.bodySmall, color = c.textFaint)
                    }
                    Text("count me in, I'll bring the speaker", style = MaterialTheme.typography.bodyLarge, color = c.text)
                }
            }
        }
    }
}
