package app.thehideout.ui.shell

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.input.TextFieldLineLimits
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.adaptive.layout.AnimatedPane
import androidx.compose.material3.adaptive.layout.ListDetailPaneScaffoldRole
import androidx.compose.material3.adaptive.navigation.NavigableListDetailPaneScaffold
import androidx.compose.material3.adaptive.navigation.rememberListDetailPaneScaffoldNavigator
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.thehideout.core.model.DmChannel
import app.thehideout.core.model.User
import app.thehideout.core.store.AppState
import app.thehideout.design.Radius
import app.thehideout.ui.components.*
import app.thehideout.ui.icons.Sym
import app.thehideout.ui.icons.SymIcon
import app.thehideout.ui.theme.Hideout
import kotlinx.coroutines.launch

/** Direct messages: the conversations (newest message first) beside, or before, the transcript. */
@Composable
fun DmsScreen(state: AppState, actions: ShellActions, modifier: Modifier = Modifier) {
    val navigator = rememberListDetailPaneScaffoldNavigator<Long>()
    val scope = rememberCoroutineScope()
    val open = state.homeDm?.let(state::dm)
    NavigableListDetailPaneScaffold(
        navigator = navigator,
        modifier = modifier,
        listPane = {
            AnimatedPane {
                DmList(
                    state, selected = open?.id,
                    onClick = { dm ->
                        actions.openHome()
                        actions.openChannel(dm.id)
                        scope.launch { navigator.navigateTo(ListDetailPaneScaffoldRole.Detail, dm.id) }
                    },
                    actions = actions,
                )
            }
        },
        detailPane = {
            AnimatedPane {
                if (open != null) {
                    TranscriptPane(
                        state, open.id, state.dmPeer(open)?.displayName ?: "Direct message", null, actions,
                        onBack = if (navigator.canNavigateBack()) ({ scope.launch { navigator.navigateBack() } }) else null,
                    )
                } else {
                    Box(Modifier.fillMaxSize().background(Hideout.colors.bg)) { EmptyState(Sym.Forum, "Pick a conversation", "Direct messages show up here.", Modifier.align(Alignment.Center)) }
                }
            }
        },
    )
}

@Composable
private fun DmList(state: AppState, selected: Long?, onClick: (DmChannel) -> Unit, actions: ShellActions = ShellActions()) {
    val c = Hideout.colors
    var showNewMessage by remember { mutableStateOf(false) }
    Column(Modifier.fillMaxSize().background(c.bg)) {
        Row(
            Modifier.fillMaxWidth().statusBarsPadding().padding(start = 20.dp, end = 12.dp, top = 8.dp, bottom = 8.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("Messages", style = MaterialTheme.typography.headlineMedium, color = c.text)
            IconButton(onClick = { showNewMessage = true }) { SymIcon(Sym.Add, "New message") }
        }
        if (state.dms.isEmpty()) {
            EmptyState(Sym.Forum, "No conversations", "Start one from a member's profile.")
        } else {
            LazyColumn(contentPadding = PaddingValues(horizontal = 8.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                items(state.dms.sortedByDescending { state.log(it.id)?.newestId ?: 0L }, key = { it.id }) { dm ->
                    val peer = state.dmPeer(dm)
                    val unread = state.unreadOf(dm.id)
                    val last = state.log(dm.id)?.messages?.lastOrNull()
                    Row(
                        Modifier.fillMaxWidth().clip(RoundedCornerShape(Radius.lg))
                            .background(if (dm.id == selected) c.accentSubtle else androidx.compose.ui.graphics.Color.Transparent)
                            .clickable { onClick(dm) }.heightIn(min = 64.dp).padding(horizontal = 12.dp, vertical = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(12.dp),
                    ) {
                        Avatar(peer?.id ?: dm.id, peer?.displayName ?: "?", size = 44.dp)
                        Column(Modifier.weight(1f)) {
                            Text(peer?.displayName ?: "Direct message", style = MaterialTheme.typography.titleSmall, color = if (dm.id == selected) c.accentText else c.text, fontWeight = if (unread.count > 0) FontWeight(700) else null)
                            if (last != null) Text(last.content.lines().first(), style = MaterialTheme.typography.bodyMedium, color = if (unread.count > 0) c.text else c.textMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                        if (unread.mentions > 0) MentionBadge(unread.mentions) else if (unread.count > 0) UnreadPill(unread.count)
                    }
                }
            }
        }
    }
    if (showNewMessage) {
        NewMessageSheet(state, onDismiss = { showNewMessage = false }, onPick = { userId -> showNewMessage = false; actions.startDm(userId) })
    }
}

/** DM-02: choose someone from your servers, searchable, to start a conversation. */
@Composable
private fun NewMessageSheet(state: AppState, onDismiss: () -> Unit, onPick: (Long) -> Unit) {
    val c = Hideout.colors
    val query = rememberTextFieldState()
    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = c.surface) {
        Column(Modifier.heightIn(min = 240.dp).padding(horizontal = 20.dp)) {
            Text("New message", style = MaterialTheme.typography.titleMedium, color = c.text, modifier = Modifier.padding(bottom = 12.dp))
            OutlinedTextField(
                state = query,
                modifier = Modifier.fillMaxWidth(),
                placeholder = { Text("Search people") },
                lineLimits = TextFieldLineLimits.SingleLine,
                shape = RoundedCornerShape(Radius.lg),
            )
            val q = query.text.toString()
            val people = state.knownUsers().filter { it.username.contains(q, ignoreCase = true) }
            LazyColumn(Modifier.padding(top = 8.dp)) {
                items(people, key = User::id) { u ->
                    Row(
                        Modifier.fillMaxWidth().clickable { onPick(u.id) }.padding(vertical = 10.dp),
                        horizontalArrangement = Arrangement.spacedBy(12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Avatar(u.id, u.displayName, size = 36.dp)
                        Text(u.displayName, style = MaterialTheme.typography.bodyLarge, color = c.text)
                    }
                }
            }
        }
    }
}
