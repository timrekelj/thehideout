package app.thehideout.ui.shell

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import app.thehideout.core.model.Role
import app.thehideout.core.model.User
import app.thehideout.core.store.AppState
import app.thehideout.ui.components.Avatar
import app.thehideout.ui.theme.Hideout

/** Members grouped under role headers (MEM-01/MEM-02), tapping one opens a small profile sheet (PROF-01). */
@Composable
fun MembersSheet(state: AppState, guildId: Long, onDismiss: () -> Unit, onMessage: (Long) -> Unit, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    val ids = state.members[guildId].orEmpty()
    val roles = state.roles[guildId].orEmpty()
    val memberRoles = state.memberRoles[guildId].orEmpty()
    var profile by remember { mutableStateOf<User?>(null) }

    data class Group(val role: Role?, val users: List<User>)
    val groups = remember(ids, roles, memberRoles, state.users) {
        val byRole = ids.mapNotNull { state.users[it] }.groupBy { u ->
            val mine = memberRoles[u.id].orEmpty()
            roles.filter { it.id in mine }.maxByOrNull { it.position }
        }
        val named = roles.sortedByDescending { it.position }.mapNotNull { r -> byRole[r]?.let { Group(r, it.sortedBy { u -> u.username.lowercase() }) } }
        val plain = byRole[null]?.sortedBy { it.username.lowercase() }.orEmpty()
        named + if (plain.isNotEmpty()) listOf(Group(null, plain)) else emptyList()
    }

    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = c.surface, modifier = modifier) {
        Text("Members · ${ids.size}", style = MaterialTheme.typography.titleMedium, color = c.text, modifier = Modifier.padding(horizontal = 20.dp, vertical = 8.dp))
        LazyColumn(contentPadding = PaddingValues(horizontal = 8.dp, vertical = 4.dp)) {
            groups.forEach { group ->
                item(key = "header-${group.role?.id ?: 0}") {
                    Text(
                        group.role?.name ?: "Members",
                        style = MaterialTheme.typography.labelMedium,
                        color = group.role?.let { c.channel(it.color)?.text } ?: c.textMuted,
                        modifier = Modifier.padding(horizontal = 12.dp, vertical = 6.dp),
                    )
                }
                group.users.forEach { u ->
                    item(key = "user-${u.id}") {
                        Row(
                            Modifier.fillMaxWidth().testTag("member-row-${u.id}").clickable { profile = u }.padding(horizontal = 12.dp, vertical = 8.dp),
                            horizontalArrangement = Arrangement.spacedBy(12.dp),
                        ) {
                            Avatar(u.id, u.displayName, size = 36.dp)
                            Text(u.displayName, style = MaterialTheme.typography.bodyLarge, color = group.role?.let { c.channel(it.color)?.text } ?: c.text)
                        }
                    }
                }
            }
        }
    }

    profile?.let { u ->
        ModalBottomSheet(onDismissRequest = { profile = null }, containerColor = c.surface) {
            Column(Modifier.heightIn(min = 160.dp).padding(horizontal = 20.dp, vertical = 12.dp)) {
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    Avatar(u.id, u.displayName, size = 56.dp)
                    Column {
                        Text(u.displayName, style = MaterialTheme.typography.titleLarge, color = c.text)
                        Text("@${u.username}", style = MaterialTheme.typography.bodyMedium, color = c.textMuted)
                    }
                }
                if (u.id != state.myId) {
                    Button(onClick = { onMessage(u.id) }, modifier = Modifier.padding(top = 16.dp)) { Text("Message") }
                }
            }
        }
    }
}
