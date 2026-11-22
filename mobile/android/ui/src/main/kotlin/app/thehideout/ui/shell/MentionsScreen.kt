package app.thehideout.ui.shell

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.thehideout.core.store.AppState
import app.thehideout.core.store.MentionEntry
import app.thehideout.ui.components.Avatar
import app.thehideout.ui.components.EmptyState
import app.thehideout.ui.components.clockTime
import app.thehideout.ui.icons.Sym
import app.thehideout.ui.theme.Hideout

/**
 * A local feed of messages that mention the signed-in user (section 4's MENTIONS tab): tapping
 * one jumps to its channel. Built from every channel whose history is loaded, newest first; a
 * channel not yet opened on this device will not show its older mentions until it is visited.
 */
@Composable
fun MentionsScreen(state: AppState, actions: ShellActions, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    val feed = state.mentionFeed()
    Column(modifier.background(c.bg)) {
        Text("Mentions", style = MaterialTheme.typography.headlineMedium, color = c.text, modifier = Modifier.statusBarsPadding().padding(start = 20.dp, end = 20.dp, top = 16.dp, bottom = 8.dp))
        if (feed.isEmpty()) {
            EmptyState(Sym.AlternateEmail, "No mentions yet", "Messages that mention you collect here.")
        } else {
            LazyColumn(contentPadding = PaddingValues(horizontal = 8.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                items(feed, key = { it.message.id }) { entry -> MentionRow(state, entry, actions) }
            }
        }
    }
}

@Composable
private fun MentionRow(state: AppState, entry: MentionEntry, actions: ShellActions) {
    val c = Hideout.colors
    val m = entry.message
    val author = state.users[m.authorId]
    val channelName = m.guildId?.let { g -> state.channel(g, entry.channelId)?.let { "#${it.name}" } }
        ?: "Direct message"
    val guildId = m.guildId
    Row(
        Modifier.fillMaxWidth().clickable {
            if (guildId != null) { actions.openGuild(guildId); actions.openChannel(entry.channelId); actions.navigateToServers() }
            else { actions.openHome(); actions.openChannel(entry.channelId); actions.navigateToDms() }
        }.padding(horizontal = 12.dp, vertical = 10.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Avatar(m.authorId, author?.displayName ?: "?", size = 40.dp)
        Column(Modifier) {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(author?.displayName ?: "user ${m.authorId}", style = MaterialTheme.typography.titleSmall, color = c.text)
                Text(channelName, style = MaterialTheme.typography.bodySmall, color = c.textMuted)
                Text(clockTime(m.createdAt), style = MaterialTheme.typography.bodySmall, color = c.textFaint)
            }
            Text(m.content.lines().first(), style = MaterialTheme.typography.bodyMedium, color = c.textMuted, maxLines = 2, overflow = TextOverflow.Ellipsis)
        }
    }
}
