package app.thehideout.ui.shell

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.adaptive.layout.AnimatedPane
import androidx.compose.material3.adaptive.layout.ListDetailPaneScaffoldRole
import androidx.compose.material3.adaptive.navigation.NavigableListDetailPaneScaffold
import androidx.compose.material3.adaptive.navigation.rememberListDetailPaneScaffoldNavigator
import androidx.compose.runtime.Composable
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.toMutableStateList
import androidx.compose.runtime.snapshots.SnapshotStateList
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import app.thehideout.core.model.ChannelKind
import app.thehideout.core.store.AppState
import app.thehideout.core.store.Place
import app.thehideout.core.store.Unread
import app.thehideout.design.Radius
import app.thehideout.ui.components.*
import app.thehideout.ui.icons.Sym
import app.thehideout.ui.icons.SymIcon
import app.thehideout.ui.theme.Hideout
import kotlinx.coroutines.launch

/**
 * Servers: a server picker over the channel list (sections folded as you like) and, beside it on
 * wide screens or after a tap on a phone, the transcript. A list-detail scaffold, so back is the
 * system's (predictive on Android 14+) and the panes reflow with the window.
 */
@Composable
fun ServersScreen(state: AppState, actions: ShellActions, modifier: Modifier = Modifier) {
    val navigator = rememberListDetailPaneScaffoldNavigator<Long>()
    val scope = rememberCoroutineScope()
    val guild = state.currentGuild
    val channel = guild?.let { g -> state.currentChannel?.let { state.channel(g.id, it) } }
    NavigableListDetailPaneScaffold(
        navigator = navigator,
        modifier = modifier,
        listPane = {
            AnimatedPane {
                ChannelListPane(state, actions) { id ->
                    actions.openChannel(id)
                    if (state.channel(guild?.id ?: 0, id)?.kind != ChannelKind.Voice) scope.launch { navigator.navigateTo(ListDetailPaneScaffoldRole.Detail, id) }
                }
            }
        },
        detailPane = {
            AnimatedPane {
                if (guild != null && channel != null) {
                    TranscriptPane(
                        state, channel.id, "# ${channel.name}", channel.topic.ifBlank { guild.name }, actions,
                        onBack = if (navigator.canNavigateBack()) ({ scope.launch { navigator.navigateBack() } }) else null,
                    )
                } else {
                    Box(Modifier.fillMaxSize().background(Hideout.colors.bg)) { EmptyState(Sym.Tag, "Pick a channel", "Its messages show up here.", Modifier.align(Alignment.Center)) }
                }
            }
        },
    )
}

@Composable
private fun ChannelListPane(state: AppState, actions: ShellActions, onChannel: (Long) -> Unit) {
    val c = Hideout.colors
    val guild = state.currentGuild
    // Folded sections, per guild, kept across rotation.
    val folded = rememberSaveable(saver = androidx.compose.runtime.saveable.listSaver<SnapshotStateList<Long>, Long>(save = { it.toList() }, restore = { it.toMutableStateList() })) { mutableListOfLong() }
    Column(Modifier.fillMaxSize().background(c.bg)) {
        Column(Modifier.statusBarsPadding()) {
            LazyRow(contentPadding = PaddingValues(horizontal = 8.dp), horizontalArrangement = Arrangement.spacedBy(0.dp), verticalAlignment = Alignment.CenterVertically) {
                items(state.guilds, key = { it.id }) { g ->
                    val railTile = app.thehideout.ui.theme.Hideout.metrics.railTile
                    val u = state.guildUnread(g.id)
                    ServerBadge(g.name, selected = state.place == Place.InGuild(g.id), unread = u.count, mentions = u.mentions, size = railTile, onClick = { actions.openGuild(g.id) })
                }
            }
            if (guild != null) {
                Text(guild.name, style = MaterialTheme.typography.headlineSmall, color = c.text, modifier = Modifier.padding(horizontal = 20.dp, vertical = 4.dp))
            }
        }
        if (guild == null) {
            EmptyState(Sym.Dns, "No servers yet", "Create one or join with an invite.")
        } else {
            val layout = state.layouts[guild.id]
            val channels = state.channelsOf(guild.id).associateBy { it.id }
            LazyColumn(contentPadding = PaddingValues(horizontal = 8.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(app.thehideout.ui.theme.Hideout.metrics.rowGap)) {
                layout?.normalized()?.entries?.forEach { entry ->
                    val section = layout.sections.firstOrNull { it.id == entry.sectionId }
                    val open = section == null || section.id !in folded
                    if (section != null) {
                        val total = entry.channelIds.fold(Unread()) { a, id -> state.unreadOf(id).let { Unread(a.count + it.count, a.mentions + it.mentions) } }
                        item(key = "section-${section.id}") {
                            SectionHeader(section.name, open, unread = total) { if (section.id in folded) folded.remove(section.id) else folded.add(section.id) }
                        }
                    }
                    if (open) entry.channelIds.mapNotNull { channels[it] }.forEach { ch ->
                        item(key = "channel-${ch.id}") {
                            ChannelRow(
                                ch,
                                selected = ch.id == state.currentChannel && ch.kind == ChannelKind.Text,
                                unread = state.unreadOf(ch.id),
                                voice = if (ch.kind == ChannelKind.Voice) state.voiceChannelStates(guild.id, ch.id) else emptyList(),
                                names = { id -> id to state.userName(id) },
                                onClick = { onChannel(ch.id) },
                            )
                        }
                    }
                }
            }
        }
    }
}

private fun mutableListOfLong(): SnapshotStateList<Long> = mutableListOf<Long>().toMutableStateList()
