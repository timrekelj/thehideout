package app.thehideout.ui.shell

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.Text
import androidx.compose.material3.adaptive.navigationsuite.NavigationSuiteDefaults
import androidx.compose.material3.adaptive.navigationsuite.NavigationSuiteScaffold
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import app.thehideout.core.store.AppState
import app.thehideout.core.store.Place
import app.thehideout.ui.icons.Sym
import app.thehideout.ui.icons.SymIcon
import app.thehideout.ui.theme.Hideout
import app.thehideout.ui.theme.Look

enum class Destination(val label: String, val sym: Sym) {
    Servers("Servers", Sym.Dns),
    Messages("DMs", Sym.Forum),
    Mentions("Mentions", Sym.AlternateEmail),
    You("You", Sym.Person),
}

/**
 * The signed-in app: a navigation bar on phones and a rail on wider windows for the same four
 * destinations. Servers and DMs are list-detail screens; Mentions waits for its feed; You holds
 * the profile and the look.
 */
@Composable
fun SignedInShell(
    state: AppState,
    look: Look,
    onLookChange: (Look) -> Unit,
    actions: ShellActions,
    versionName: String,
    serverUrl: String = "",
    onOpenUrl: (String) -> Unit = {},
    modifier: Modifier = Modifier,
    initial: Destination = Destination.Servers,
) {
    val c = Hideout.colors
    var current by rememberSaveable { mutableStateOf(initial) }
    val navActions = remember(actions) {
        actions.copy(navigateToDms = { current = Destination.Messages }, navigateToServers = { current = Destination.Servers })
    }
    // Unread for the open channel only counts while the tab that would show it is actually on
    // screen: `currentChannel` stays selected in the Servers tab (for a guild) or the DMs tab (for
    // home) even after switching to another one, so the core store needs this separately to avoid
    // treating a channel as "read" while a different tab is showing.
    val transcriptOnScreen = (current == Destination.Servers && state.place is Place.InGuild) ||
        (current == Destination.Messages && state.place == Place.Home)
    LaunchedEffect(transcriptOnScreen) { actions.setChannelForeground(transcriptOnScreen) }
    val dmUnread = state.homeUnread
    val serverUnread = state.guilds.sumOf { state.guildUnread(it.id).count }
    val mentionCount = state.guilds.sumOf { state.guildUnread(it.id).mentions } + state.homeUnread.mentions
    NavigationSuiteScaffold(
        modifier = modifier,
        containerColor = c.bg,
        contentColor = c.text,
        navigationSuiteColors = NavigationSuiteDefaults.colors(
            navigationBarContainerColor = c.surface,
            navigationBarContentColor = c.text,
            navigationRailContainerColor = c.surface,
            navigationRailContentColor = c.text,
        ),
        navigationSuiteItems = {
            Destination.entries.forEach { d ->
                item(
                    selected = d == current,
                    onClick = { current = d },
                    icon = {
                        val count = when (d) {
                            Destination.Servers -> serverUnread
                            Destination.Messages -> dmUnread.count
                            Destination.Mentions -> mentionCount
                            Destination.You -> 0
                        }
                        if (count > 0) BadgedBox(badge = { Badge(containerColor = c.accent, contentColor = c.accentFg) { Text(if (count > 99) "99+" else count.toString()) } }) { SymIcon(d.sym, null) } else SymIcon(d.sym, null)
                    },
                    label = { Text(d.label) },
                )
            }
        },
    ) {
        Box(Modifier.fillMaxSize().background(c.bg)) {
            when (current) {
                Destination.Servers -> ServersScreen(state, navActions)
                Destination.Messages -> DmsScreen(state, navActions)
                Destination.Mentions -> MentionsScreen(state, navActions)
                Destination.You -> YouScreen(state, look, onLookChange, actions, versionName, serverUrl, onOpenUrl)
            }
        }
    }
}
