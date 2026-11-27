package app.thehideout.ui.shell

import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SecondaryScrollableTabRow
import androidx.compose.material3.Switch
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import app.thehideout.core.model.Ban
import app.thehideout.core.model.Emoji
import app.thehideout.core.model.Perm
import app.thehideout.core.model.Role
import app.thehideout.core.model.User
import app.thehideout.core.store.AppState
import app.thehideout.design.Radius
import app.thehideout.ui.components.Avatar
import app.thehideout.ui.icons.Sym
import app.thehideout.ui.icons.SymIcon
import app.thehideout.ui.theme.Hideout
import coil3.compose.AsyncImage
import kotlinx.coroutines.launch

/** The server settings page nav (desktop's `ServerPage`). */
enum class ServerSettingsPage(val label: String) { Overview("Overview"), Roles("Roles"), Members("Members"), Bans("Bans"), Emojis("Emojis") }

/** The palette's 12 keys (`client/design`'s channel palette; "" is "no colour"). */
val PALETTE_KEYS = listOf("slate", "red", "orange", "amber", "yellow", "lime", "green", "teal", "cyan", "blue", "indigo", "pink")

/** One switch of the role editor (desktop's `PermRow`): the six core bits, then the voice ones. Connect/speak/video are a baseline everyone has and are not offered. */
private data class PermRow(val bit: Long, val label: String, val help: String)
private val PERMISSION_ROWS = listOf(
    PermRow(Perm.ADMINISTRATOR, "Administrator", "Grants every permission below. Give it only to people you trust completely."),
    PermRow(Perm.MANAGE_SERVER, "Manage server", "Rename the server and change its icon."),
    PermRow(Perm.MANAGE_CHANNELS, "Manage channels", "Create and edit channels and sections, and reorder the sidebar."),
    PermRow(Perm.MANAGE_ROLES, "Manage roles", "Create, edit, reorder and hand out roles below their own highest role."),
    PermRow(Perm.MANAGE_EMOJIS, "Manage emojis", "Add, rename and remove custom emojis."),
    PermRow(Perm.INVITE_PEOPLE, "Invite people", "Invite people by email, and withdraw or resend this server's invites."),
    PermRow(Perm.MUTE_MEMBERS, "Mute members", "Server-mute people in voice channels."),
    PermRow(Perm.MOVE_MEMBERS, "Move members", "Move people between voice channels."),
)

/** "Delete their recent messages" on the Ban dialog (desktop's `CLEANUP`). */
private val CLEANUP = listOf("Don't delete" to 0, "Last hour" to 3_600, "Last 24 hours" to 86_400, "Last 7 days" to 604_800)

/** A role's colour dot: its palette tone, or a hollow ring for no colour. */
@Composable
fun RoleDot(color: String, size: androidx.compose.ui.unit.Dp, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    val tone = c.channel(color)
    Box(
        modifier.size(size).clip(CircleShape)
            .then(if (tone != null) Modifier.background(tone.ink) else Modifier.background(Color.Transparent)),
    ) {
        if (tone == null) Box(Modifier.fillMaxSize().clip(CircleShape).background(Color.Transparent))
    }
}

/**
 * Full-screen server settings (SSET-*): a tab per page the signed-in user may open, gated exactly
 * like desktop ([AppState.canOpenServerSettings] mirrors the server's rules).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ServerSettingsScreen(state: AppState, guildId: Long, actions: ShellActions, onDismiss: () -> Unit, modifier: Modifier = Modifier, initialPage: ServerSettingsPage? = null) {
    val c = Hideout.colors
    val guild = state.guild(guildId) ?: run { onDismiss(); return }
    val pages = remember(state, guildId) {
        buildList {
            if (state.canManageServer(guildId)) add(ServerSettingsPage.Overview)
            if (state.canManageRoles(guildId)) add(ServerSettingsPage.Roles)
            if (state.canManageRoles(guildId) || state.canRemovePeople(guildId)) add(ServerSettingsPage.Members)
            if (state.canViewBans(guildId)) add(ServerSettingsPage.Bans)
            if (state.canManageEmojis(guildId)) add(ServerSettingsPage.Emojis)
        }
    }
    var page by remember(guildId) { mutableStateOf(initialPage ?: pages.firstOrNull() ?: ServerSettingsPage.Overview) }
    LaunchedEffect(pages) { if (page !in pages && pages.isNotEmpty()) page = pages.first() }

    Scaffold(
        modifier = modifier,
        containerColor = c.bg,
        topBar = {
            Column {
                TopAppBar(
                    title = { Text(guild.name, color = c.text) },
                    navigationIcon = { IconButton(onClick = onDismiss, modifier = Modifier.testTag("server-settings-close")) { SymIcon(Sym.ArrowBack, "Close") } },
                )
                if (pages.size > 1) {
                    SecondaryScrollableTabRow(selectedTabIndex = pages.indexOf(page).coerceAtLeast(0), containerColor = c.bg, contentColor = c.text) {
                        pages.forEach { p -> Tab(selected = p == page, onClick = { page = p }, text = { Text(p.label) }, modifier = Modifier.testTag("server-settings-tab-${p.name}")) }
                    }
                }
            }
        },
    ) { padding ->
        Box(Modifier.padding(padding).fillMaxSize()) {
            when (page) {
                ServerSettingsPage.Overview -> OverviewPage(state, guildId, actions, onDismiss)
                ServerSettingsPage.Roles -> RolesPage(state, guildId, actions)
                ServerSettingsPage.Members -> MembersPage(state, guildId, actions)
                ServerSettingsPage.Bans -> BansPage(state, guildId, actions)
                ServerSettingsPage.Emojis -> EmojisPage(state, guildId, actions)
            }
        }
    }
}

// ---- overview ----------------------------------------------------------------------------------

@Composable
private fun OverviewPage(state: AppState, guildId: Long, actions: ShellActions, onDismiss: () -> Unit) {
    val c = Hideout.colors
    val guild = state.guild(guildId) ?: return
    val mediaBase = app.thehideout.ui.components.LocalMediaBase.current
    val editable = state.canManageServer(guildId)
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    var name by remember(guildId) { mutableStateOf(guild.name) }
    var nameBusy by remember { mutableStateOf(false) }
    var nameError by remember { mutableStateOf<String?>(null) }
    var iconBusy by remember { mutableStateOf(false) }
    var iconError by remember { mutableStateOf<String?>(null) }
    var showTransfer by remember { mutableStateOf(false) }
    var showDelete by remember { mutableStateOf(false) }

    val pickIcon = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch {
            iconBusy = true
            iconError = null
            runCatching {
                val resolver = context.contentResolver
                val mime = resolver.getType(uri) ?: "image/jpeg"
                val bytes = resolver.openInputStream(uri)?.use { it.readBytes() } ?: throw IllegalStateException("can't read the image")
                if (bytes.size > 8 * 1024 * 1024) throw IllegalArgumentException("That image is over 8 MiB.")
                bytes to mime
            }.fold(
                { (bytes, mime) ->
                    actions.uploadGuildIcon(guildId, bytes, mime).fold({ iconBusy = false }, { e -> iconBusy = false; iconError = e.message ?: "Couldn't upload the icon." })
                },
                { e -> iconBusy = false; iconError = e.message ?: "Couldn't read that image." },
            )
        }
    }

    LazyColumn(Modifier.padding(horizontal = 20.dp, vertical = 16.dp), verticalArrangement = Arrangement.spacedBy(24.dp)) {
        item {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("Name", style = MaterialTheme.typography.titleMedium, color = c.text)
                Text("What people see at the top of the sidebar and in their server list.", style = MaterialTheme.typography.bodySmall, color = c.textMuted)
                Row(verticalAlignment = Alignment.Top, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedTextField(
                        value = name,
                        onValueChange = { name = it; nameError = null },
                        enabled = editable && !nameBusy,
                        isError = nameError != null,
                        supportingText = nameError?.let { { Text(it, color = MaterialTheme.colorScheme.error) } },
                        singleLine = true,
                        modifier = Modifier.weight(1f).testTag("server-name-input"),
                    )
                    Button(
                        enabled = editable && !nameBusy && name.trim() != guild.name && name.isNotBlank(),
                        onClick = {
                            scope.launch {
                                nameBusy = true
                                actions.renameGuild(guildId, name.trim()).fold({ nameBusy = false }, { e -> nameBusy = false; nameError = e.message ?: "Couldn't rename the server." })
                            }
                        },
                        modifier = Modifier.testTag("server-name-save"),
                    ) { if (nameBusy) CircularProgressIndicator(Modifier.size(18.dp), color = c.accentFg, strokeWidth = 2.dp) else Text("Save") }
                }
            }
        }
        item {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("Icon", style = MaterialTheme.typography.titleMedium, color = c.text)
                Text("A picture for the server rail. Without one the server shows its initials.", style = MaterialTheme.typography.bodySmall, color = c.textMuted)
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                    Box(Modifier.size(80.dp).clip(RoundedCornerShape(Radius.xl)).background(c.surfaceRaised), contentAlignment = Alignment.Center) {
                        if (guild.icon.isNotEmpty()) {
                            AsyncImage("$mediaBase${guild.icon}", contentDescription = guild.name, modifier = Modifier.fillMaxSize().clip(RoundedCornerShape(Radius.xl)))
                        } else {
                            Text(app.thehideout.design.AvatarStyle.initials(guild.name), color = c.text, style = MaterialTheme.typography.titleLarge)
                        }
                    }
                    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            OutlinedButton(
                                enabled = editable && !iconBusy,
                                onClick = { pickIcon.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)) },
                                modifier = Modifier.testTag("server-icon-upload"),
                            ) { Text(if (iconBusy) "Uploading…" else "Upload image…") }
                            if (guild.icon.isNotEmpty()) {
                                TextButton(
                                    enabled = editable && !iconBusy,
                                    onClick = {
                                        scope.launch {
                                            iconBusy = true
                                            actions.removeGuildIcon(guildId).fold({ iconBusy = false }, { e -> iconBusy = false; iconError = e.message ?: "Couldn't remove the icon." })
                                        }
                                    },
                                    modifier = Modifier.testTag("server-icon-remove"),
                                ) { Text("Remove") }
                            }
                        }
                        Text("PNG, JPEG, WebP or GIF, up to 8 MiB.", style = MaterialTheme.typography.bodySmall, color = c.textMuted)
                        iconError?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error) }
                    }
                }
            }
        }
        if (state.canDeleteGuild(guildId)) {
            item {
                Column(
                    Modifier.fillMaxWidth().clip(RoundedCornerShape(Radius.lg)).background(c.dangerBg).padding(16.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Text("Danger zone", style = MaterialTheme.typography.titleMedium, color = c.danger)
                    DangerRow("Transfer ownership", "Hand this server to another member. You become an ordinary member.") {
                        OutlinedButton(onClick = { showTransfer = true }, modifier = Modifier.testTag("server-transfer")) { Text("Transfer…") }
                    }
                    HorizontalDivider(color = c.dangerBorder)
                    DangerRow("Delete server", "Delete the server and everything in it for everyone. This can't be undone.") {
                        Button(
                            onClick = { showDelete = true },
                            colors = ButtonDefaults.buttonColors(containerColor = c.dangerSolid, contentColor = c.onDanger),
                            modifier = Modifier.testTag("server-delete"),
                        ) { Text("Delete server…") }
                    }
                }
            }
        }
    }

    if (showTransfer) {
        TransferOwnershipDialog(
            guild.name,
            state.transferCandidates(guildId),
            onDismiss = { showTransfer = false },
            onConfirm = { user, confirm -> actions.transferOwnership(guildId, user, confirm) },
        )
    }
    if (showDelete) {
        DeleteGuildDialog(guild.name, onDismiss = { showDelete = false }, onConfirm = { confirm -> actions.deleteGuild(guildId, confirm).also { r -> if (r.isSuccess) onDismiss() } })
    }
}

@Composable
private fun DangerRow(title: String, caption: String, action: @Composable () -> Unit) {
    val c = Hideout.colors
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge, color = c.text)
            Text(caption, style = MaterialTheme.typography.bodySmall, color = c.textMuted)
        }
        action()
    }
}

/** Hands the server to another member (owner only); types the server's name to confirm. */
@Composable
fun TransferOwnershipDialog(guildName: String, candidates: List<User>, onDismiss: () -> Unit, onConfirm: suspend (Long, String) -> Result<*>) {
    val scope = rememberCoroutineScope()
    var target by remember { mutableStateOf<User?>(null) }
    var menu by remember { mutableStateOf(false) }
    var confirmText by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    AlertDialog(
        onDismissRequest = { if (!busy) onDismiss() },
        title = { Text("Transfer ownership") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text("$guildName is handed over at once. You become an ordinary member.")
                Box {
                    OutlinedButton(onClick = { menu = true }, modifier = Modifier.fillMaxWidth().testTag("transfer-target")) { Text(target?.displayName ?: "Pick a member") }
                    DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                        candidates.forEach { u -> DropdownMenuItem(text = { Text(u.displayName) }, onClick = { target = u; menu = false }) }
                    }
                }
                OutlinedTextField(
                    value = confirmText,
                    onValueChange = { confirmText = it; error = null },
                    label = { Text("Type \"$guildName\" to confirm") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth().testTag("transfer-confirm-name"),
                )
                error?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall) }
            }
        },
        confirmButton = {
            TextButton(
                enabled = !busy && target != null && confirmText.trim() == guildName,
                modifier = Modifier.testTag("transfer-submit"),
                onClick = {
                    val user = target ?: return@TextButton
                    scope.launch {
                        busy = true
                        onConfirm(user.id, confirmText.trim()).fold({ onDismiss() }, { e -> busy = false; error = e.message ?: "Couldn't transfer the server." })
                    }
                },
            ) { Text(if (busy) "Transferring…" else "Transfer") }
        },
        dismissButton = { TextButton(enabled = !busy, onClick = onDismiss) { Text("Cancel") } },
    )
}

/** Deletes the server for good (owner only); types the server's name to confirm. */
@Composable
fun DeleteGuildDialog(guildName: String, onDismiss: () -> Unit, onConfirm: suspend (String) -> Result<*>) {
    val scope = rememberCoroutineScope()
    var confirmText by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    AlertDialog(
        onDismissRequest = { if (!busy) onDismiss() },
        title = { Text("Delete server") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text("$guildName and everything in it - channels, messages, roles, emojis and invites - will be deleted for everyone. This can't be undone.")
                OutlinedTextField(
                    value = confirmText,
                    onValueChange = { confirmText = it; error = null },
                    label = { Text("Type \"$guildName\" to confirm") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth().testTag("delete-guild-confirm-name"),
                )
                error?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall) }
            }
        },
        confirmButton = {
            TextButton(
                enabled = !busy && confirmText.trim() == guildName,
                modifier = Modifier.testTag("delete-guild-submit"),
                onClick = {
                    scope.launch {
                        busy = true
                        onConfirm(confirmText.trim()).fold({ onDismiss() }, { e -> busy = false; error = e.message ?: "Couldn't delete the server." })
                    }
                },
            ) { Text(if (busy) "Deleting…" else "Delete server") }
        },
        dismissButton = { TextButton(enabled = !busy, onClick = onDismiss) { Text("Cancel") } },
    )
}

/** Leaves a server (not its owner); no typed confirmation, unlike delete/transfer. */
@Composable
fun LeaveServerDialog(guildName: String, onDismiss: () -> Unit, onConfirm: suspend () -> Result<*>) {
    val scope = rememberCoroutineScope()
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    AlertDialog(
        onDismissRequest = { if (!busy) onDismiss() },
        title = { Text("Leave server") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("You'll need a new invite to come back to $guildName.")
                error?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall) }
            }
        },
        confirmButton = {
            TextButton(
                enabled = !busy,
                modifier = Modifier.testTag("leave-server-submit"),
                onClick = {
                    scope.launch {
                        busy = true
                        onConfirm().fold({ onDismiss() }, { e -> busy = false; error = e.message ?: "Couldn't leave the server." })
                    }
                },
            ) { Text(if (busy) "Leaving…" else "Leave") }
        },
        dismissButton = { TextButton(enabled = !busy, onClick = onDismiss) { Text("Cancel") } },
    )
}

// ---- roles --------------------------------------------------------------------------------------

@Composable
private fun RolesPage(state: AppState, guildId: Long, actions: ShellActions) {
    val c = Hideout.colors
    val roles = state.rolesOf(guildId)
    val can = state.canManageRoles(guildId)
    var editing by remember(guildId) { mutableStateOf<Role?>(null) }
    var creating by remember(guildId) { mutableStateOf(false) }
    var deleteTarget by remember(guildId) { mutableStateOf<Role?>(null) }
    val scope = rememberCoroutineScope()
    var busyMove by remember { mutableStateOf(false) }

    Column(Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text("Roles", style = MaterialTheme.typography.titleMedium, color = c.text)
                Text("The higher a role, the more it outranks. A member's name takes the colour of their highest role that has one.", style = MaterialTheme.typography.bodySmall, color = c.textMuted)
            }
            if (can) IconButton(onClick = { creating = true }, modifier = Modifier.testTag("role-new")) { SymIcon(Sym.Add, "New role") }
        }
        if (roles.isEmpty()) {
            Text("No roles yet. Create one to give members a colour and permissions.", style = MaterialTheme.typography.bodyMedium, color = c.textMuted, modifier = Modifier.padding(20.dp))
        }
        LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(horizontal = 12.dp, vertical = 4.dp)) {
            items(roles, key = { it.id }) { r ->
                val mine = state.canEditRole(guildId, r)
                val count = state.roleMembers(guildId, r.id)
                Row(
                    Modifier.fillMaxWidth().clickable(enabled = true) { editing = r }.padding(vertical = 10.dp, horizontal = 8.dp).testTag("role-row-${r.id}"),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    RoleDot(r.color, 12.dp)
                    Text(r.name, style = MaterialTheme.typography.bodyLarge, color = if (mine) c.text else c.textMuted, modifier = Modifier.weight(1f))
                    Text(count.toString(), style = MaterialTheme.typography.bodySmall, color = c.textMuted)
                    if (mine) {
                        IconButton(
                            enabled = !busyMove,
                            onClick = { scope.launch { busyMove = true; actions.moveRole(guildId, r.id, -1); busyMove = false } },
                            modifier = Modifier.size(32.dp).testTag("role-up-${r.id}"),
                        ) { SymIcon(Sym.ExpandMore, "Move ${r.name} up", modifier = Modifier.size(18.dp).graphicsLayer(rotationZ = 180f)) }
                        IconButton(
                            enabled = !busyMove,
                            onClick = { scope.launch { busyMove = true; actions.moveRole(guildId, r.id, 1); busyMove = false } },
                            modifier = Modifier.size(32.dp).testTag("role-down-${r.id}"),
                        ) { SymIcon(Sym.ExpandMore, "Move ${r.name} down", modifier = Modifier.size(18.dp)) }
                    } else {
                        SymIcon(Sym.Lock, "Above your highest role", tint = c.textFaint, size = 16.dp)
                    }
                }
            }
        }
    }

    if (creating) {
        RoleEditorSheet(
            title = "New role", role = null, editable = true,
            onDismiss = { creating = false },
            onSave = { name, color, perms -> actions.createRole(guildId, name, color, perms) },
            onDelete = null,
            canGrant = { bit -> state.canGrant(guildId, bit) },
        )
    }
    editing?.let { r ->
        val mine = state.canEditRole(guildId, r)
        RoleEditorSheet(
            title = r.name, role = r, editable = mine,
            onDismiss = { editing = null },
            onSave = { name, color, perms -> actions.updateRole(guildId, r.id, name, color, perms) },
            onDelete = { deleteTarget = r; editing = null },
            canGrant = { bit -> state.canGrant(guildId, bit) },
        )
    }
    deleteTarget?.let { r ->
        AlertDialog(
            onDismissRequest = { deleteTarget = null },
            title = { Text("Delete role") },
            text = { Text("“${r.name}” is removed from everyone who has it. This can't be undone.") },
            confirmButton = {
                TextButton(
                    modifier = Modifier.testTag("role-delete-confirm"),
                    onClick = { scope.launch { actions.deleteRole(guildId, r.id); deleteTarget = null } },
                ) { Text("Delete role") }
            },
            dismissButton = { TextButton(onClick = { deleteTarget = null }) { Text("Cancel") } },
        )
    }
}

/** Create/edit a role: name, one of 12 palette colours (or none), and the 8 togglable permissions. */
@Suppress("DEPRECATION") // rememberModalBottomSheetState(skipPartiallyExpanded): the suggested replacement is BottomSheetScaffold's, not ModalBottomSheet's
@Composable
private fun RoleEditorSheet(
    title: String,
    role: Role?,
    editable: Boolean,
    onDismiss: () -> Unit,
    onSave: suspend (name: String, color: String, perms: Long) -> Result<*>,
    onDelete: (() -> Unit)?,
    canGrant: (Long) -> Boolean,
) {
    val c = Hideout.colors
    val scope = rememberCoroutineScope()
    var name by remember { mutableStateOf(role?.name ?: "New role") }
    var color by remember { mutableStateOf(role?.color ?: "") }
    var perms by remember { mutableStateOf(role?.permissions ?: 0L) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val dirty = role == null || name.trim() != role.name || color != role.color || perms != role.permissions

    // Long content (name, colour, up to 8 permission switches): opens fully expanded so every
    // control, including Save at the bottom, is reachable without a drag gesture first.
    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = c.surface, sheetState = androidx.compose.material3.rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        LazyColumn(Modifier.padding(horizontal = 20.dp).navigationBarsPadding(), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            item { Text(title, style = MaterialTheme.typography.titleLarge, color = c.text) }
            if (!editable) {
                item { Text("This role is at or above your highest role, so you can only look at it.", style = MaterialTheme.typography.bodySmall, color = c.textMuted) }
            }
            item {
                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it },
                    label = { Text("Role name") },
                    enabled = editable,
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth().testTag("role-name-input"),
                )
            }
            item {
                Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text("Colour", style = MaterialTheme.typography.labelLarge, color = c.textMuted)
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        (listOf("") + PALETTE_KEYS).forEach { key ->
                            val selected = color == key
                            Box(
                                Modifier.size(32.dp).clip(CircleShape)
                                    .then(if (editable) Modifier.clickable { color = key } else Modifier)
                                    .then(if (selected) Modifier.background(c.hover) else Modifier)
                                    .testTag("role-colour-${key.ifEmpty { "none" }}"),
                                contentAlignment = Alignment.Center,
                            ) { RoleDot(key, 20.dp) }
                        }
                    }
                }
            }
            item {
                Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text("Permissions", style = MaterialTheme.typography.labelLarge, color = c.textMuted)
                    PERMISSION_ROWS.forEach { p ->
                        val on = Perm.has(perms, p.bit)
                        val holds = canGrant(p.bit)
                        Row(Modifier.fillMaxWidth().padding(vertical = 8.dp), verticalAlignment = Alignment.Top, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                            Column(Modifier.weight(1f)) {
                                Text(p.label, style = MaterialTheme.typography.bodyMedium, color = c.text)
                                Text(
                                    if (!holds && editable) "${p.help} You don't have this permission yourself." else p.help,
                                    style = MaterialTheme.typography.bodySmall,
                                    color = c.textMuted,
                                )
                                if (p.bit == Perm.ADMINISTRATOR && on) {
                                    Text("This role can do everything on this server.", style = MaterialTheme.typography.bodySmall, color = c.warning)
                                }
                            }
                            Switch(
                                checked = on,
                                onCheckedChange = { checked -> perms = if (checked) perms or p.bit else perms and p.bit.inv() },
                                enabled = editable && holds,
                                modifier = Modifier.testTag("perm-${p.label.lowercase().replace(' ', '-')}"),
                            )
                        }
                    }
                }
            }
            error?.let { item { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall) } }
            item {
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (onDelete != null) {
                        Button(
                            enabled = editable && !busy,
                            colors = ButtonDefaults.buttonColors(containerColor = c.dangerSolid, contentColor = c.onDanger),
                            onClick = onDelete,
                            modifier = Modifier.testTag("role-delete"),
                        ) { Text("Delete role") }
                        Spacer(Modifier.weight(1f))
                    }
                    TextButton(enabled = !busy, onClick = onDismiss) { Text(if (dirty && role != null) "Discard" else "Close") }
                    Button(
                        enabled = editable && !busy && dirty && name.isNotBlank(),
                        onClick = {
                            scope.launch {
                                busy = true
                                onSave(name.trim(), color, perms).fold({ onDismiss() }, { e -> busy = false; error = e.message ?: "Couldn't save the role." })
                            }
                        },
                        modifier = Modifier.testTag("role-save"),
                    ) { Text(if (busy) (if (role == null) "Creating…" else "Saving…") else if (role == null) "Create role" else "Save changes") }
                }
            }
            item { Spacer(Modifier.height(1.dp)) }
        }
    }
}

// ---- members ------------------------------------------------------------------------------------

@Composable
private fun MembersPage(state: AppState, guildId: Long, actions: ShellActions) {
    val c = Hideout.colors
    val ids = state.members[guildId].orEmpty()
    val scope = rememberCoroutineScope()
    var kickTarget by remember(guildId) { mutableStateOf<User?>(null) }
    var banTarget by remember(guildId) { mutableStateOf<User?>(null) }
    var menuFor by remember(guildId) { mutableStateOf<Long?>(null) }
    var busyUser by remember(guildId) { mutableStateOf<Long?>(null) }

    Column(Modifier.fillMaxSize()) {
        Text(
            if (state.canManageRoles(guildId)) "Give people roles for a name colour and permissions. You can only hand out roles below your own highest role." else "Remove people from the server. You can only act on members whose roles are below yours.",
            style = MaterialTheme.typography.bodySmall,
            color = c.textMuted,
            modifier = Modifier.padding(horizontal = 20.dp, vertical = 8.dp),
        )
        if (ids.isEmpty()) Text("No members yet.", style = MaterialTheme.typography.bodyMedium, color = c.textMuted, modifier = Modifier.padding(20.dp))
        LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(horizontal = 20.dp, vertical = 4.dp)) {
            items(ids, key = { it }) { userId ->
                val user = state.users[userId] ?: return@items
                val busy = busyUser == userId
                val held = state.rolesOf(guildId).filter { it.id in state.memberRoles[guildId]?.get(userId).orEmpty() }
                val assignable = state.assignableRoles(guildId, userId)
                Column(Modifier.fillMaxWidth().padding(vertical = 10.dp).testTag("member-roles-$userId")) {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        Avatar(user.id, user.displayName, size = 36.dp)
                        Column(Modifier.weight(1f)) {
                            val colorKey = state.nameColor(guildId, userId)
                            Text(user.displayName, style = MaterialTheme.typography.bodyLarge, color = if (colorKey.isEmpty()) c.text else c.channel(colorKey)?.text ?: c.text)
                        }
                        if (state.canKick(guildId, userId)) {
                            IconButton(enabled = !busy, onClick = { kickTarget = user }, modifier = Modifier.testTag("member-kick-$userId")) { SymIcon(Sym.Close, "Kick ${user.displayName}") }
                        }
                        if (state.canBan(guildId, userId)) {
                            IconButton(enabled = !busy, onClick = { banTarget = user }, modifier = Modifier.testTag("member-ban-$userId")) { SymIcon(Sym.Delete, "Ban ${user.displayName}") }
                        }
                    }
                    FlowRow(Modifier.padding(start = 48.dp, top = 6.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        held.forEach { r ->
                            val removable = state.canEditRole(guildId, r) && !busy
                            Row(
                                Modifier.clip(RoundedCornerShape(Radius.sm)).background(c.hover).padding(start = 8.dp, end = if (removable) 2.dp else 8.dp, top = 4.dp, bottom = 4.dp),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(6.dp),
                            ) {
                                RoleDot(r.color, 8.dp)
                                Text(r.name, style = MaterialTheme.typography.labelSmall, color = c.text)
                                if (removable) {
                                    IconButton(
                                        onClick = { scope.launch { busyUser = userId; actions.setMemberRole(guildId, userId, r.id, false); busyUser = null } },
                                        modifier = Modifier.size(20.dp).testTag("chip-x-$userId-${r.id}"),
                                    ) { SymIcon(Sym.Close, "Remove ${r.name}", modifier = Modifier.size(12.dp)) }
                                }
                            }
                        }
                        if (state.canManageRoles(guildId) && assignable.isNotEmpty()) {
                            Box {
                                OutlinedButton(
                                    enabled = !busy,
                                    onClick = { menuFor = userId },
                                    contentPadding = PaddingValues(horizontal = 10.dp, vertical = 4.dp),
                                    modifier = Modifier.height(28.dp).testTag("member-add-$userId"),
                                ) { Text("Role", style = MaterialTheme.typography.labelSmall) }
                                DropdownMenu(expanded = menuFor == userId, onDismissRequest = { menuFor = null }) {
                                    assignable.forEach { r ->
                                        DropdownMenuItem(
                                            text = { Text(r.name) },
                                            onClick = { menuFor = null; scope.launch { busyUser = userId; actions.setMemberRole(guildId, userId, r.id, true); busyUser = null } },
                                        )
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    kickTarget?.let { u ->
        AlertDialog(
            onDismissRequest = { kickTarget = null },
            title = { Text("Remove ${u.displayName}") },
            text = { Text("${u.displayName} is removed from the server. They can rejoin with an invite.") },
            confirmButton = {
                TextButton(
                    modifier = Modifier.testTag("kick-confirm"),
                    onClick = { scope.launch { actions.kickMember(guildId, u.id); kickTarget = null } },
                ) { Text("Remove") }
            },
            dismissButton = { TextButton(onClick = { kickTarget = null }) { Text("Cancel") } },
        )
    }
    banTarget?.let { u ->
        BanMemberDialog(u.displayName, onDismiss = { banTarget = null }, onConfirm = { reason, seconds -> actions.banMember(guildId, u.id, reason, seconds) })
    }
}

/** Ban a member: a reason and the recent-message cleanup window (MOD-*). */
@Composable
fun BanMemberDialog(userName: String, onDismiss: () -> Unit, onConfirm: suspend (String, Int?) -> Result<*>) {
    val scope = rememberCoroutineScope()
    var reason by remember { mutableStateOf("") }
    var cleanup by remember { mutableStateOf(0) }
    var cleanupMenu by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    AlertDialog(
        onDismissRequest = { if (!busy) onDismiss() },
        title = { Text("Ban $userName") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text("$userName is removed and can't rejoin with an invite until unbanned.")
                OutlinedTextField(
                    value = reason,
                    onValueChange = { reason = it },
                    label = { Text("Reason (optional)") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth().testTag("ban-reason"),
                )
                Column {
                    Text("Delete their recent messages", style = MaterialTheme.typography.labelLarge)
                    Box {
                        OutlinedButton(onClick = { cleanupMenu = true }, modifier = Modifier.fillMaxWidth().testTag("ban-cleanup")) { Text(CLEANUP[cleanup].first) }
                        DropdownMenu(expanded = cleanupMenu, onDismissRequest = { cleanupMenu = false }) {
                            CLEANUP.forEachIndexed { i, (label, _) -> DropdownMenuItem(text = { Text(label) }, onClick = { cleanup = i; cleanupMenu = false }) }
                        }
                    }
                }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall) }
            }
        },
        confirmButton = {
            TextButton(
                enabled = !busy,
                modifier = Modifier.testTag("ban-confirm"),
                onClick = {
                    scope.launch {
                        busy = true
                        val seconds = CLEANUP[cleanup].second.takeIf { it > 0 }
                        onConfirm(reason.trim(), seconds).fold({ onDismiss() }, { e -> busy = false; error = e.message ?: "Couldn't ban them." })
                    }
                },
            ) { Text(if (busy) "Banning…" else "Ban") }
        },
        dismissButton = { TextButton(enabled = !busy, onClick = onDismiss) { Text("Cancel") } },
    )
}

// ---- bans ---------------------------------------------------------------------------------------

@Composable
private fun BansPage(state: AppState, guildId: Long, actions: ShellActions) {
    val c = Hideout.colors
    val scope = rememberCoroutineScope()
    var loaded by remember(guildId) { mutableStateOf(false) }
    var error by remember(guildId) { mutableStateOf<String?>(null) }
    var busyUser by remember(guildId) { mutableStateOf<Long?>(null) }
    val bans = state.bansOf(guildId)

    LaunchedEffect(guildId) {
        actions.loadBans(guildId).fold({ loaded = true }, { e -> loaded = true; error = e.message ?: "Couldn't load the bans." })
    }

    Column(Modifier.fillMaxSize()) {
        Text("People who were banned can't join again until you unban them.", style = MaterialTheme.typography.bodySmall, color = c.textMuted, modifier = Modifier.padding(horizontal = 20.dp, vertical = 8.dp))
        error?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(horizontal = 20.dp)) }
        if (loaded && bans.isEmpty()) Text("No one is banned.", style = MaterialTheme.typography.bodyMedium, color = c.textMuted, modifier = Modifier.padding(20.dp))
        LazyColumn(contentPadding = PaddingValues(horizontal = 20.dp, vertical = 4.dp)) {
            items(bans, key = { it.user.id }) { b: Ban ->
                val busy = busyUser == b.user.id
                Row(Modifier.fillMaxWidth().padding(vertical = 10.dp).testTag("ban-${b.user.id}"), verticalAlignment = Alignment.Top, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    Avatar(b.user.id, b.user.displayName, size = 36.dp)
                    Column(Modifier.weight(1f)) {
                        Text(b.user.displayName, style = MaterialTheme.typography.bodyLarge, color = c.text)
                        Text(b.reason.ifBlank { "No reason given" }, style = MaterialTheme.typography.bodySmall, color = if (b.reason.isBlank()) c.textFaint else c.text)
                    }
                    TextButton(
                        enabled = !busy,
                        onClick = { scope.launch { busyUser = b.user.id; actions.unbanMember(guildId, b.user.id); busyUser = null } },
                        modifier = Modifier.testTag("unban-${b.user.id}"),
                    ) { Text("Unban") }
                }
            }
        }
    }
}

// ---- emojis -------------------------------------------------------------------------------------

@Composable
private fun EmojisPage(state: AppState, guildId: Long, actions: ShellActions) {
    val c = Hideout.colors
    val mediaBase = app.thehideout.ui.components.LocalMediaBase.current
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    val emojis = state.emojis[guildId].orEmpty()
    var renameTarget by remember(guildId) { mutableStateOf<Emoji?>(null) }
    var deleteTarget by remember(guildId) { mutableStateOf<Emoji?>(null) }
    var uploadError by remember(guildId) { mutableStateOf<String?>(null) }
    var pendingUpload by remember(guildId) { mutableStateOf<Pair<ByteArray, String>?>(null) }

    val pickEmoji = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch {
            runCatching {
                val resolver = context.contentResolver
                val mime = resolver.getType(uri) ?: "image/png"
                val bytes = resolver.openInputStream(uri)?.use { it.readBytes() } ?: throw IllegalStateException("can't read the image")
                if (bytes.size > 2 * 1024 * 1024) throw IllegalArgumentException("That image is over 2 MiB.")
                bytes to mime
            }.fold({ pendingUpload = it }, { e -> uploadError = e.message ?: "Couldn't read that image." })
        }
    }

    Column(Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            Text("${emojis.size} emoji" + if (emojis.size == 1) "" else "s", style = MaterialTheme.typography.titleMedium, color = c.text, modifier = Modifier.weight(1f))
            Button(onClick = { pickEmoji.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)) }, modifier = Modifier.testTag("emoji-add")) { Text("Add emoji…") }
        }
        uploadError?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(horizontal = 20.dp)) }
        if (emojis.isEmpty()) Text("No custom emoji yet.", style = MaterialTheme.typography.bodyMedium, color = c.textMuted, modifier = Modifier.padding(20.dp))
        LazyColumn(contentPadding = PaddingValues(horizontal = 20.dp, vertical = 4.dp)) {
            items(emojis, key = { it.id }) { e ->
                Row(Modifier.fillMaxWidth().padding(vertical = 8.dp).testTag("emoji-row-${e.id}"), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    AsyncImage("$mediaBase${e.image}", contentDescription = e.name, modifier = Modifier.size(28.dp))
                    Text(":${e.name}:", style = MaterialTheme.typography.bodyMedium, color = c.text, modifier = Modifier.weight(1f))
                    IconButton(onClick = { renameTarget = e }, modifier = Modifier.testTag("emoji-rename-${e.id}")) { SymIcon(Sym.Edit, "Rename ${e.name}") }
                    IconButton(onClick = { deleteTarget = e }, modifier = Modifier.testTag("emoji-delete-${e.id}")) { SymIcon(Sym.Delete, "Delete ${e.name}") }
                }
            }
        }
    }

    pendingUpload?.let { (bytes, mime) ->
        EmojiNameDialog(
            title = "Add emoji", initial = "",
            onDismiss = { pendingUpload = null },
            onConfirm = { name -> actions.createEmoji(guildId, name, bytes, mime) },
        )
    }
    renameTarget?.let { e ->
        EmojiNameDialog(
            title = "Rename emoji", initial = e.name,
            onDismiss = { renameTarget = null },
            onConfirm = { name -> actions.renameEmoji(guildId, e.id, name) },
        )
    }
    deleteTarget?.let { e ->
        AlertDialog(
            onDismissRequest = { deleteTarget = null },
            title = { Text("Delete emoji") },
            text = { Text("“:${e.name}:” is removed. Existing messages and reactions that use it show its name as text instead.") },
            confirmButton = {
                TextButton(modifier = Modifier.testTag("emoji-delete-confirm"), onClick = { scope.launch { actions.deleteEmoji(guildId, e.id); deleteTarget = null } }) { Text("Delete emoji") }
            },
            dismissButton = { TextButton(onClick = { deleteTarget = null }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun EmojiNameDialog(title: String, initial: String, onDismiss: () -> Unit, onConfirm: suspend (String) -> Result<*>) {
    val scope = rememberCoroutineScope()
    var name by remember { mutableStateOf(initial) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    AlertDialog(
        onDismissRequest = { if (!busy) onDismiss() },
        title = { Text(title) },
        text = {
            Column {
                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it.filter { ch -> ch.isLetterOrDigit() || ch == '_' }.lowercase(); error = null },
                    label = { Text("Name") },
                    singleLine = true,
                    modifier = Modifier.testTag("emoji-name-input"),
                )
                error?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall) }
            }
        },
        confirmButton = {
            TextButton(
                enabled = !busy && name.length in 2..32,
                modifier = Modifier.testTag("emoji-name-submit"),
                onClick = {
                    scope.launch {
                        busy = true
                        onConfirm(name).fold({ onDismiss() }, { e -> busy = false; error = e.message ?: "Couldn't save the emoji." })
                    }
                },
            ) { Text(if (busy) "Saving…" else "Save") }
        },
        dismissButton = { TextButton(enabled = !busy, onClick = onDismiss) { Text("Cancel") } },
    )
}
