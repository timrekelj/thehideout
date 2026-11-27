package app.thehideout.ui.shell

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
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
import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import app.thehideout.core.model.User
import app.thehideout.core.store.AppState
import app.thehideout.ui.components.Avatar
import app.thehideout.ui.components.GroupLabel
import app.thehideout.ui.components.HideoutCard
import app.thehideout.ui.icons.Sym
import app.thehideout.ui.icons.SymIcon
import app.thehideout.ui.theme.Hideout
import app.thehideout.ui.theme.Look

private sealed interface YouRoute {
    data object Appearance : YouRoute
    data object Security : YouRoute
    data object Notifications : YouRoute
    data object About : YouRoute
}

/** [YouRoute] is not `Parcelable`; `rememberSaveable`'s default `Saver` would crash trying to
 * marshal it. Every case here is a no-argument object, so a plain name round-trips it. */
private val YouRouteSaver = Saver<YouRoute?, String>(
    save = { it?.let { r -> r::class.simpleName } ?: "" },
    restore = { name ->
        when (name) {
            YouRoute.Appearance::class.simpleName -> YouRoute.Appearance
            YouRoute.Security::class.simpleName -> YouRoute.Security
            YouRoute.Notifications::class.simpleName -> YouRoute.Notifications
            YouRoute.About::class.simpleName -> YouRoute.About
            else -> null
        }
    },
)

/** You: the profile and the settings entry points (SET-05). */
@Composable
fun YouScreen(
    state: AppState,
    look: Look,
    onLookChange: (Look) -> Unit,
    actions: ShellActions,
    versionName: String,
    serverUrl: String,
    onOpenUrl: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val c = Hideout.colors
    var route by rememberSaveable(stateSaver = YouRouteSaver) { mutableStateOf<YouRoute?>(null) }
    Box(modifier.fillMaxSize()) {
        YouList(state.me, look, versionName, onSignOut = actions.signOut, onRoute = { route = it }, modifier = Modifier.fillMaxSize())
        when (route) {
            YouRoute.Appearance -> AppearanceScreen(look, onLookChange, onBack = { route = null }, modifier = Modifier.fillMaxSize())
            YouRoute.Security -> SecurityScreen(state, actions, serverUrl, onBack = { route = null }, modifier = Modifier.fillMaxSize())
            YouRoute.Notifications -> NotificationsScreen(state, actions, onBack = { route = null }, modifier = Modifier.fillMaxSize())
            YouRoute.About -> AboutScreen(versionName, serverUrl, onOpenUrl, onBack = { route = null }, modifier = Modifier.fillMaxSize())
            null -> Unit
        }
    }
}

@Composable
private fun YouList(me: User?, look: Look, versionName: String, onSignOut: () -> Unit, onRoute: (YouRoute) -> Unit, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    Column(modifier.background(c.bg).statusBarsPadding().verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
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
                SettingRow(Sym.Palette, "Appearance", "Accent, background, density, text size") { onRoute(YouRoute.Appearance) }
                HorizontalDivider(color = c.borderSubtle)
                SettingRow(Sym.Notifications, "Notifications", "Servers, channels and previews") { onRoute(YouRoute.Notifications) }
                HorizontalDivider(color = c.borderSubtle)
                SettingRow(Sym.Lock, "Security", "Password, two-factor and passkeys") { onRoute(YouRoute.Security) }
            }
        }
        GroupLabel("Account")
        HideoutCard(Modifier.padding(horizontal = 16.dp).fillMaxWidth()) {
            Column {
                SettingRow(Sym.Stack, "About", "thehideout $versionName") { onRoute(YouRoute.About) }
                HorizontalDivider(color = c.borderSubtle)
                SettingRow(Sym.Close, "Sign out", null, destructive = true, onClick = onSignOut)
            }
        }
    }
}

@Composable
private fun SettingRow(sym: Sym, title: String, subtitle: String?, destructive: Boolean = false, onClick: () -> Unit) {
    val c = Hideout.colors
    val tint = if (destructive) c.danger else c.accentText
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).heightIn(min = 64.dp).padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        SymIcon(sym, null, tint = tint)
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge, color = if (destructive) c.danger else c.text)
            if (subtitle != null) Text(subtitle, style = MaterialTheme.typography.bodySmall, color = c.textMuted)
        }
        if (!destructive) SymIcon(Sym.ChevronRight, null, tint = c.textFaint)
    }
}
