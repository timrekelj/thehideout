package app.thehideout.ui.shell

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import app.thehideout.core.model.User
import app.thehideout.ui.components.Avatar
import app.thehideout.ui.components.GroupLabel
import app.thehideout.ui.components.HideoutCard
import app.thehideout.ui.gallery.LookSheet
import app.thehideout.ui.icons.Sym
import app.thehideout.ui.icons.SymIcon
import app.thehideout.ui.theme.Hideout
import app.thehideout.ui.theme.Look

/** You: the profile and the settings entry points (Appearance is live already; the rest arrives with its phase). */
@Composable
fun YouScreen(me: User?, look: Look, onLookChange: (Look) -> Unit, onSignOut: () -> Unit, versionName: String, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    var appearance by rememberSaveable { mutableStateOf(false) }
    Column(modifier.fillMaxSize().background(c.bg).statusBarsPadding().verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
        Text("You", style = MaterialTheme.typography.headlineMedium, color = c.text, modifier = Modifier.padding(start = 20.dp, end = 20.dp, top = 16.dp, bottom = 12.dp))
        if (me != null) {
            HideoutCard(Modifier.padding(horizontal = 16.dp).fillMaxWidth()) {
                Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                    Avatar(me.id, me.displayName, size = 64.dp)
                    Column {
                        Text(me.displayName, style = MaterialTheme.typography.headlineSmall, color = c.text)
                        me.email?.let { Text(it, style = MaterialTheme.typography.bodyMedium, color = c.textMuted) }
                        if (me.isAdmin) Text("Site admin", style = MaterialTheme.typography.labelMedium, color = c.accentText)
                    }
                }
            }
        }
        GroupLabel("Settings")
        HideoutCard(Modifier.padding(horizontal = 16.dp).fillMaxWidth()) {
            Column {
                SettingRow(Sym.Palette, "Appearance", "Accent, background, density, text size") { appearance = true }
                HorizontalDivider(color = c.borderSubtle)
                SettingRow(Sym.Notifications, "Notifications", "Arrives with push support", enabled = false)
                HorizontalDivider(color = c.borderSubtle)
                SettingRow(Sym.Lock, "Security", "Password, two-factor and passkeys", enabled = false)
            }
        }
        GroupLabel("Account")
        HideoutCard(Modifier.padding(horizontal = 16.dp).fillMaxWidth()) {
            Column {
                SettingRow(Sym.Stack, "About", "thehideout $versionName", enabled = false)
                HorizontalDivider(color = c.borderSubtle)
                SettingRow(Sym.Close, "Sign out", null, destructive = true, onClick = onSignOut)
            }
        }
    }
    if (appearance) LookSheet(look, onLookChange) { appearance = false }
}

@Composable
private fun SettingRow(sym: Sym, title: String, subtitle: String?, enabled: Boolean = true, destructive: Boolean = false, onClick: () -> Unit = {}) {
    val c = Hideout.colors
    val tint = when { destructive -> c.danger; enabled -> c.accentText; else -> c.textFaint }
    Row(
        Modifier.fillMaxWidth().then(if (enabled) Modifier.clickable(onClick = onClick) else Modifier).heightIn(min = 64.dp).padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        SymIcon(sym, null, tint = tint)
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge, color = if (destructive) c.danger else if (enabled) c.text else c.textMuted)
            if (subtitle != null) Text(subtitle, style = MaterialTheme.typography.bodySmall, color = c.textMuted)
        }
        if (enabled && !destructive) SymIcon(Sym.ChevronRight, null, tint = c.textFaint)
    }
}
