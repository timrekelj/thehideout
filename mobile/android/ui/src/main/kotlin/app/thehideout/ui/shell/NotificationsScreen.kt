package app.thehideout.ui.shell

import androidx.activity.compose.BackHandler
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
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ButtonGroupDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.ToggleButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import app.thehideout.core.model.ChannelKind
import app.thehideout.core.model.NotificationLevel
import app.thehideout.core.model.NotificationOverride
import app.thehideout.core.model.NotificationSettings
import app.thehideout.core.notification.isMuted
import app.thehideout.core.store.AppState
import app.thehideout.ui.components.GroupLabel
import app.thehideout.ui.components.HideoutCard
import app.thehideout.ui.icons.Sym
import app.thehideout.ui.icons.SymIcon
import app.thehideout.ui.theme.Hideout
import kotlinx.coroutines.launch
import java.time.Instant

private val MUTED_FOREVER: Instant = Instant.parse("2099-01-01T00:00:00Z")

private fun levelLabel(level: NotificationLevel): String = when (level) {
    NotificationLevel.All -> "All messages"
    NotificationLevel.Mentions -> "Mentions only"
    NotificationLevel.Nothing -> "Nothing"
    NotificationLevel.Inherit, NotificationLevel.Unspecified -> "Default"
}

/** MOB-02: global defaults plus per-server and per-channel overrides, synced via `notification_settings_updated`. */
@Composable
fun NotificationsScreen(state: AppState, actions: ShellActions, onBack: () -> Unit, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    BackHandler(onBack = onBack)
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(Unit) {
        if (state.notificationSettings == null) actions.loadNotificationSettings().fold({}, { e -> error = e.message })
    }
    val settings = state.notificationSettings
    var guildRoute by rememberSaveable { mutableStateOf<Long?>(null) }

    Box(modifier.fillMaxSize()) {
        Column(Modifier.fillMaxSize().background(c.bg).statusBarsPadding().verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
            Row(Modifier.fillMaxWidth().padding(start = 4.dp, end = 20.dp, top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                IconButton(onClick = onBack) { SymIcon(Sym.ArrowBack, "Back") }
                Text("Notifications", style = MaterialTheme.typography.headlineSmall, color = c.text)
            }
            if (error != null) Text(error!!, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(horizontal = 20.dp))
            if (settings == null) {
                if (error == null) Box(Modifier.fillMaxWidth().padding(32.dp), contentAlignment = Alignment.Center) { LoadingIndicator() }
                return@Column
            }

            GroupLabel("Direct messages")
            HideoutCard(Modifier.padding(horizontal = 16.dp).fillMaxWidth()) {
                ToggleRow("Notify me for DMs", "Off mutes every direct message", settings.dmsEnabled) { v ->
                    scope.launch { actions.updateNotificationSettings(v, null, null, emptyList(), emptyList()).fold({}, { e -> error = e.message }) }
                }
            }

            GroupLabel("Servers, by default")
            HideoutCard(Modifier.padding(horizontal = 16.dp).fillMaxWidth()) {
                Column {
                    Column(Modifier.padding(vertical = 8.dp)) {
                        LevelSegmented(
                            listOf(NotificationLevel.All, NotificationLevel.Mentions, NotificationLevel.Nothing),
                            settings.guildDefault,
                            tagPrefix = "global-",
                        ) { lvl -> scope.launch { actions.updateNotificationSettings(null, lvl, null, emptyList(), emptyList()).fold({}, { e -> error = e.message }) } }
                    }
                    HorizontalDivider(color = c.borderSubtle)
                    ToggleRow("Show message previews", "Off shows \"New message\" instead of the text", settings.showPreviews) { v ->
                        scope.launch { actions.updateNotificationSettings(null, null, v, emptyList(), emptyList()).fold({}, { e -> error = e.message }) }
                    }
                }
            }

            GroupLabel("Per server")
            HideoutCard(Modifier.padding(horizontal = 16.dp).fillMaxWidth()) {
                Column {
                    state.guilds.forEachIndexed { i, g ->
                        if (i > 0) HorizontalDivider(color = c.borderSubtle)
                        val override = settings.guilds.firstOrNull { it.id == g.id }
                        val label = if (override?.isMuted(Instant.now()) == true) "Muted" else override?.let { levelLabel(it.level) } ?: "Default"
                        OverrideRow(Sym.Dns, g.name, label, onClick = { guildRoute = g.id })
                    }
                    if (state.guilds.isEmpty()) Text("No servers yet.", color = c.textMuted, modifier = Modifier.padding(16.dp))
                }
            }
        }

        guildRoute?.let { gid ->
            val guild = state.guild(gid)
            if (guild != null) {
                BackHandler { guildRoute = null }
                GuildNotificationsScreen(
                    guildName = guild.name,
                    channels = state.channelsOf(gid),
                    settings = settings ?: NotificationSettings(true, NotificationLevel.Mentions, true),
                    guildId = gid,
                    actions = actions,
                    onBack = { guildRoute = null },
                    onError = { error = it },
                    modifier = Modifier.fillMaxSize(),
                )
            }
        }
    }
}

@Composable
private fun GuildNotificationsScreen(
    guildName: String,
    channels: List<app.thehideout.core.model.Channel>,
    settings: NotificationSettings,
    guildId: Long,
    actions: ShellActions,
    onBack: () -> Unit,
    onError: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val c = Hideout.colors
    val scope = rememberCoroutineScope()
    var channelDialog by remember { mutableStateOf<Long?>(null) }
    val guildOverride = settings.guilds.firstOrNull { it.id == guildId }
    Column(modifier.fillMaxSize().background(c.bg).statusBarsPadding().verticalScroll(rememberScrollState())) {
        Row(Modifier.fillMaxWidth().padding(start = 4.dp, end = 20.dp, top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack) { SymIcon(Sym.ArrowBack, "Back") }
            Text(guildName, style = MaterialTheme.typography.headlineSmall, color = c.text)
        }
        GroupLabel("This server")
        HideoutCard(Modifier.padding(horizontal = 16.dp).fillMaxWidth()) {
            Column {
                Column(Modifier.padding(vertical = 8.dp)) {
                    LevelSegmented(
                        listOf(NotificationLevel.Inherit, NotificationLevel.All, NotificationLevel.Mentions, NotificationLevel.Nothing),
                        guildOverride?.level ?: NotificationLevel.Inherit,
                        tagPrefix = "guild-",
                    ) { lvl ->
                        scope.launch {
                            actions.updateNotificationSettings(null, null, null, listOf(NotificationOverride(guildId, lvl, guildOverride?.mutedUntil)), emptyList())
                                .fold({}, { e -> onError(e.message ?: "Couldn't save.") })
                        }
                    }
                }
                HorizontalDivider(color = c.borderSubtle)
                ToggleRow("Muted", "No pushes from this server until you turn it back on", guildOverride?.isMuted(Instant.now()) == true) { muted ->
                    scope.launch {
                        val until = if (muted) MUTED_FOREVER else null
                        actions.updateNotificationSettings(null, null, null, listOf(NotificationOverride(guildId, guildOverride?.level ?: NotificationLevel.Inherit, until)), emptyList())
                            .fold({}, { e -> onError(e.message ?: "Couldn't save.") })
                    }
                }
            }
        }
        GroupLabel("Channels")
        HideoutCard(Modifier.padding(horizontal = 16.dp).fillMaxWidth()) {
            Column {
                channels.forEachIndexed { i, ch ->
                    if (i > 0) HorizontalDivider(color = c.borderSubtle)
                    val override = settings.channels.firstOrNull { it.id == ch.id }
                    val label = if (override?.isMuted(Instant.now()) == true) "Muted" else override?.let { levelLabel(it.level) } ?: "Default"
                    OverrideRow(if (ch.kind == ChannelKind.Voice) Sym.Headphones else Sym.Tag, ch.name, label, onClick = { channelDialog = ch.id })
                }
                if (channels.isEmpty()) Text("No channels yet.", color = c.textMuted, modifier = Modifier.padding(16.dp))
            }
        }
    }
    channelDialog?.let { chId ->
        val ch = channels.firstOrNull { it.id == chId }
        if (ch == null) {
            // The channel was deleted (or its list hasn't loaded this one) while the dialog was open.
            channelDialog = null
        } else {
            val override = settings.channels.firstOrNull { it.id == chId }
            ChannelOverrideDialog(
                channelName = ch.name,
                current = override,
                onDismiss = { channelDialog = null },
                onSave = { lvl, mutedUntil ->
                    actions.updateNotificationSettings(null, null, null, emptyList(), listOf(NotificationOverride(chId, lvl, mutedUntil)))
                },
            )
        }
    }
}

@Composable
private fun ChannelOverrideDialog(channelName: String, current: NotificationOverride?, onDismiss: () -> Unit, onSave: suspend (NotificationLevel, Instant?) -> Result<*>) {
    var level by remember { mutableStateOf(current?.level ?: NotificationLevel.Inherit) }
    var muted by remember { mutableStateOf(current?.isMuted(Instant.now()) == true) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    AlertDialog(
        onDismissRequest = { if (!busy) onDismiss() },
        title = { Text("#$channelName") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                listOf(NotificationLevel.Inherit, NotificationLevel.All, NotificationLevel.Mentions, NotificationLevel.Nothing).forEach { lvl ->
                    Row(
                        Modifier.fillMaxWidth().clickable { level = lvl }.semantics { role = Role.RadioButton }.testTag("channel-${lvl.name}"),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        RadioButton(selected = level == lvl, onClick = { level = lvl })
                        Text(levelLabel(lvl))
                    }
                }
                HorizontalDivider(Modifier.padding(vertical = 8.dp))
                Row(Modifier.fillMaxWidth().clickable { muted = !muted }, verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text("Muted", modifier = Modifier.weight(1f))
                    Switch(checked = muted, onCheckedChange = { muted = it })
                }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall) }
            }
        },
        confirmButton = {
            TextButton(
                enabled = !busy,
                onClick = {
                    scope.launch {
                        busy = true
                        onSave(level, if (muted) MUTED_FOREVER else null).fold({ onDismiss() }, { e -> busy = false; error = e.message ?: "Couldn't save." })
                    }
                },
            ) { Text("Save") }
        },
        dismissButton = { TextButton(enabled = !busy, onClick = onDismiss) { Text("Cancel") } },
    )
}

@Composable
private fun LevelSegmented(options: List<NotificationLevel>, selected: NotificationLevel, tagPrefix: String = "", onPick: (NotificationLevel) -> Unit) {
    Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween)) {
        options.forEachIndexed { i, option ->
            val shapes = when (i) {
                0 -> ButtonGroupDefaults.connectedLeadingButtonShapes()
                options.lastIndex -> ButtonGroupDefaults.connectedTrailingButtonShapes()
                else -> ButtonGroupDefaults.connectedMiddleButtonShapes()
            }
            ToggleButton(
                checked = option == selected,
                onCheckedChange = { onPick(option) },
                modifier = Modifier.weight(1f).semantics { role = Role.RadioButton }.testTag("$tagPrefix${option.name}"),
                shapes = shapes,
            ) { Text(levelLabel(option), maxLines = 1) }
        }
    }
}

@Composable
private fun ToggleRow(title: String, subtitle: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    val c = Hideout.colors
    Row(
        Modifier.fillMaxWidth().clickable { onChange(!checked) }.padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge, color = c.text)
            Text(subtitle, style = MaterialTheme.typography.bodySmall, color = c.textMuted)
        }
        Switch(checked = checked, onCheckedChange = onChange)
    }
}

@Composable
private fun OverrideRow(sym: Sym, title: String, value: String, onClick: () -> Unit) {
    val c = Hideout.colors
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).heightIn(min = 56.dp).padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        SymIcon(sym, null, tint = c.textMuted, size = 20.dp)
        Text(title, style = MaterialTheme.typography.bodyLarge, color = c.text, modifier = Modifier.weight(1f))
        Text(value, style = MaterialTheme.typography.bodyMedium, color = c.textMuted)
        SymIcon(Sym.ChevronRight, null, tint = c.textFaint)
    }
}
