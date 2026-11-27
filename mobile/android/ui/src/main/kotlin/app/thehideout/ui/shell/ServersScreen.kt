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
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.adaptive.layout.AnimatedPane
import androidx.compose.material3.adaptive.layout.ListDetailPaneScaffoldRole
import androidx.compose.material3.adaptive.navigation.NavigableListDetailPaneScaffold
import androidx.compose.material3.adaptive.navigation.rememberListDetailPaneScaffoldNavigator
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
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
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import app.thehideout.core.model.ChannelKind
import app.thehideout.core.model.Perm
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
    // A channel already open when this pane (re)mounts - on launch, or because a Mentions tap or a
    // profile's "Message" switched here - brings the detail pane forward on a phone, same as a tap
    // on a channel row does; nothing to open shows the list.
    LaunchedEffect(channel?.id) { if (channel != null) navigator.navigateTo(ListDetailPaneScaffoldRole.Detail, channel.id) }
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
                    var showMembers by remember { mutableStateOf(false) }
                    val members = state.members[guild.id].orEmpty().mapNotNull { state.users[it] }
                    TranscriptPane(
                        state, channel.id, "# ${channel.name}", channel.topic.ifBlank { guild.name }, actions,
                        onBack = if (navigator.canNavigateBack()) ({ scope.launch { navigator.navigateBack() } }) else null,
                        members = members,
                        serverEmojis = state.emojis[guild.id].orEmpty(),
                        canManageMessages = Perm.has(state.perms(guild.id), Perm.MANAGE_MESSAGES),
                        onOpenMembers = { showMembers = true },
                    )
                    if (showMembers) {
                        MembersSheet(state, guild.id, onDismiss = { showMembers = false }, onMessage = { userId ->
                            showMembers = false
                            actions.startDm(userId)
                            actions.navigateToDms()
                        })
                    }
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
    var addMenu by remember { mutableStateOf(false) }
    var showCreateServer by remember { mutableStateOf(false) }
    var showJoinServer by remember { mutableStateOf(false) }
    var guildMenu by remember { mutableStateOf(false) }
    var showCreateChannel by remember { mutableStateOf(false) }
    var showCreateSection by remember { mutableStateOf(false) }
    var showInvite by remember { mutableStateOf(false) }
    var sectionMenuFor by remember { mutableStateOf<Long?>(null) }
    var renameSectionFor by remember { mutableStateOf<app.thehideout.core.model.Section?>(null) }
    var deleteSectionFor by remember { mutableStateOf<app.thehideout.core.model.Section?>(null) }
    var createChannelSectionId by remember { mutableStateOf<Long?>(null) }
    // A server switch invalidates any sheet/dialog left open on the previous one (its section or
    // channel ids belong to that guild, not whichever is now current).
    LaunchedEffect(guild?.id) {
        addMenu = false
        showCreateServer = false
        showJoinServer = false
        guildMenu = false
        showCreateChannel = false
        showCreateSection = false
        showInvite = false
        sectionMenuFor = null
        renameSectionFor = null
        deleteSectionFor = null
        createChannelSectionId = null
    }
    val canManageChannels = guild != null && Perm.has(state.perms(guild.id), Perm.MANAGE_CHANNELS)
    val canManageServer = guild != null && Perm.has(state.perms(guild.id), Perm.MANAGE_SERVER)
    val canInvite = guild != null && Perm.has(state.perms(guild.id), Perm.INVITE_PEOPLE)
    Column(Modifier.fillMaxSize().background(c.bg)) {
        Column(Modifier.statusBarsPadding()) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                LazyRow(
                    modifier = Modifier.weight(1f),
                    contentPadding = PaddingValues(horizontal = 8.dp),
                    horizontalArrangement = Arrangement.spacedBy(0.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    items(state.guilds, key = { it.id }) { g ->
                        val railTile = app.thehideout.ui.theme.Hideout.metrics.railTile
                        val u = state.guildUnread(g.id)
                        ServerBadge(g.name, selected = state.place == Place.InGuild(g.id), unread = u.count, mentions = u.mentions, size = railTile, onClick = { actions.openGuild(g.id) })
                    }
                }
                Box {
                    IconButton(onClick = { addMenu = true }, modifier = Modifier.testTag("add-server")) { SymIcon(Sym.Add, "Create or join a server") }
                    DropdownMenu(expanded = addMenu, onDismissRequest = { addMenu = false }) {
                        DropdownMenuItem(text = { Text("Create server") }, onClick = { addMenu = false; showCreateServer = true })
                        DropdownMenuItem(text = { Text("Join server") }, onClick = { addMenu = false; showJoinServer = true })
                    }
                }
            }
            if (guild != null) {
                Row(Modifier.padding(horizontal = 12.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(guild.name, style = MaterialTheme.typography.headlineSmall, color = c.text, modifier = Modifier.weight(1f).padding(start = 8.dp))
                    if (canInvite || canManageChannels) {
                        Box {
                            IconButton(onClick = { guildMenu = true }, modifier = Modifier.testTag("server-menu")) { SymIcon(Sym.MoreVert, "Server menu") }
                            DropdownMenu(expanded = guildMenu, onDismissRequest = { guildMenu = false }) {
                                if (canInvite) DropdownMenuItem(text = { Text("Invite people") }, onClick = { guildMenu = false; showInvite = true })
                                if (canManageChannels) {
                                    DropdownMenuItem(text = { Text("Create channel") }, onClick = { guildMenu = false; createChannelSectionId = null; showCreateChannel = true })
                                    DropdownMenuItem(text = { Text("Create section") }, onClick = { guildMenu = false; showCreateSection = true })
                                }
                            }
                        }
                    }
                }
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
                            SectionHeader(
                                section.name, open, unread = total,
                                onToggle = { if (section.id in folded) folded.remove(section.id) else folded.add(section.id) },
                                trailing = if (canManageChannels) {
                                    {
                                        Box {
                                            IconButton(onClick = { sectionMenuFor = section.id }, modifier = Modifier.size(32.dp).testTag("section-menu-${section.id}")) { SymIcon(Sym.MoreVert, "Section menu") }
                                            DropdownMenu(expanded = sectionMenuFor == section.id, onDismissRequest = { sectionMenuFor = null }) {
                                                DropdownMenuItem(text = { Text("Create channel here") }, onClick = { sectionMenuFor = null; createChannelSectionId = section.id; showCreateChannel = true })
                                                DropdownMenuItem(text = { Text("Rename") }, onClick = { sectionMenuFor = null; renameSectionFor = section })
                                                DropdownMenuItem(text = { Text("Delete") }, onClick = { sectionMenuFor = null; deleteSectionFor = section })
                                            }
                                        }
                                    }
                                } else null,
                            )
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

    if (showCreateServer) CreateServerSheet(onDismiss = { showCreateServer = false }, createGuild = actions.createGuild)
    if (showJoinServer) JoinServerSheet(onDismiss = { showJoinServer = false }, joinGuild = actions.joinGuild)
    if (guild != null) {
        if (showCreateChannel) {
            CreateChannelSheet(
                guild.id,
                sections = state.layouts[guild.id]?.sections.orEmpty(),
                onDismiss = { showCreateChannel = false },
                createChannel = actions.createChannel,
                initialSectionId = createChannelSectionId,
            )
        }
        if (showCreateSection) {
            SectionNameDialog(
                "Create section", "", "Create",
                onDismiss = { showCreateSection = false },
                onConfirm = { name -> actions.createSection(guild.id, name) },
            )
        }
        renameSectionFor?.let { section ->
            SectionNameDialog(
                "Rename section", section.name, "Save",
                onDismiss = { renameSectionFor = null },
                onConfirm = { name -> actions.renameSection(guild.id, section.id, name) },
            )
        }
        deleteSectionFor?.let { section ->
            DeleteSectionDialog(section.name, onDismiss = { deleteSectionFor = null }, onConfirm = { actions.deleteSection(guild.id, section.id) })
        }
        if (showInvite) {
            InviteSheet(
                guild, canManageServer, canInvite,
                rotateInviteCode = actions.rotateInviteCode,
                createInvite = actions.createInvite,
                listInvites = actions.listInvites,
                revokeInvite = actions.revokeInvite,
                resendInvite = actions.resendInvite,
                onDismiss = { showInvite = false },
            )
        }
    }
}

private fun mutableListOfLong(): SnapshotStateList<Long> = mutableListOf<Long>().toMutableStateList()
