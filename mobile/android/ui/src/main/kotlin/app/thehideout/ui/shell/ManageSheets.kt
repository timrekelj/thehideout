package app.thehideout.ui.shell

import android.content.Intent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.foundation.clickable
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.unit.dp
import app.thehideout.core.model.Channel
import app.thehideout.core.model.ChannelKind
import app.thehideout.core.model.Guild
import app.thehideout.core.model.Invite
import app.thehideout.core.model.InviteStatus
import app.thehideout.core.model.Section
import app.thehideout.core.invite.InviteCode
import app.thehideout.ui.components.InviteCodeField
import app.thehideout.ui.icons.Sym
import app.thehideout.ui.icons.SymIcon
import app.thehideout.ui.theme.Hideout
import kotlinx.coroutines.launch
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter

/** Create server (DLG-01): a name and, on success, opens it. */
@Composable
fun CreateServerSheet(onDismiss: () -> Unit, createGuild: suspend (String) -> Result<Guild>, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    val scope = rememberCoroutineScope()
    var name by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = c.surface, modifier = modifier) {
        Column(Modifier.padding(horizontal = 20.dp, vertical = 12.dp).navigationBarsPadding(), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Text("Create server", style = MaterialTheme.typography.titleLarge, color = c.text)
            Text("Give your server a name. You can change it later.", style = MaterialTheme.typography.bodyMedium, color = c.textMuted)
            OutlinedTextField(
                value = name,
                onValueChange = { name = it; error = null },
                label = { Text("Server name") },
                placeholder = { Text("The Hideout") },
                isError = error != null,
                supportingText = error?.let { { Text(it, color = MaterialTheme.colorScheme.error) } },
                singleLine = true,
                modifier = Modifier.fillMaxWidth().testTag("create-server-name"),
            )
            Button(
                onClick = {
                    val typed = name.trim()
                    scope.launch {
                        busy = true
                        createGuild(typed).fold(
                            { onDismiss() },
                            { e -> busy = false; error = e.message ?: "Couldn't create the server." },
                        )
                    }
                },
                enabled = !busy && name.isNotBlank(),
                modifier = Modifier.fillMaxWidth().testTag("create-server-submit"),
            ) {
                if (busy) CircularProgressIndicator(Modifier.size(18.dp), color = c.accentFg, strokeWidth = 2.dp) else Text("Create")
            }
        }
    }
}

/** Join server with an invite code (DLG-02): the 8-box code field. */
@Composable
fun JoinServerSheet(onDismiss: () -> Unit, joinGuild: suspend (String) -> Result<Guild>, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    val scope = rememberCoroutineScope()
    var code by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val normalized = code.filter { it != '-' }
    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = c.surface, modifier = modifier) {
        Column(Modifier.padding(horizontal = 20.dp, vertical = 12.dp).navigationBarsPadding(), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Text("Join server", style = MaterialTheme.typography.titleLarge, color = c.text)
            Text("Paste an invite code from a friend.", style = MaterialTheme.typography.bodyMedium, color = c.textMuted)
            InviteCodeField(code, { code = it; error = null }, error = error)
            Button(
                onClick = {
                    scope.launch {
                        busy = true
                        joinGuild(code).fold(
                            { onDismiss() },
                            { e -> busy = false; error = e.message ?: "Couldn't join that server." },
                        )
                    }
                },
                enabled = !busy && InviteCode.isComplete(normalized),
                modifier = Modifier.fillMaxWidth().testTag("join-server-submit"),
            ) {
                if (busy) CircularProgressIndicator(Modifier.size(18.dp), color = c.accentFg, strokeWidth = 2.dp) else Text("Join")
            }
        }
    }
}

/** Create channel (SIDE-10): Text/Voice, name, section. Gated by MANAGE_CHANNELS by the caller. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CreateChannelSheet(
    guildId: Long,
    sections: List<Section>,
    onDismiss: () -> Unit,
    createChannel: suspend (Long, String, ChannelKind, Long?) -> Result<Channel>,
    modifier: Modifier = Modifier,
    initialSectionId: Long? = null,
) {
    val c = Hideout.colors
    val scope = rememberCoroutineScope()
    var kind by remember { mutableStateOf(ChannelKind.Text) }
    var name by remember { mutableStateOf("") }
    var sectionId by remember { mutableStateOf(initialSectionId) }
    var sectionMenu by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val sectionName = sections.firstOrNull { it.id == sectionId }?.name ?: "Top level"
    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = c.surface, modifier = modifier) {
        Column(Modifier.padding(horizontal = 20.dp, vertical = 12.dp).navigationBarsPadding(), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Text("Create channel", style = MaterialTheme.typography.titleLarge, color = c.text)
            Text("Channels keep a conversation in one place.", style = MaterialTheme.typography.bodyMedium, color = c.textMuted)
            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                SegmentedButton(
                    selected = kind == ChannelKind.Text,
                    onClick = { kind = ChannelKind.Text },
                    shape = SegmentedButtonDefaults.itemShape(0, 2),
                    icon = { SymIcon(Sym.Tag, null, size = 18.dp) },
                ) { Text("Text") }
                SegmentedButton(
                    selected = kind == ChannelKind.Voice,
                    onClick = { kind = ChannelKind.Voice },
                    shape = SegmentedButtonDefaults.itemShape(1, 2),
                    icon = { SymIcon(Sym.Headphones, null, size = 18.dp) },
                ) { Text("Voice") }
            }
            OutlinedTextField(
                value = name,
                onValueChange = { name = it; error = null },
                label = { Text("Channel name") },
                placeholder = { Text(if (kind == ChannelKind.Voice) "lounge" else "plans") },
                isError = error != null,
                supportingText = error?.let { { Text(it, color = MaterialTheme.colorScheme.error) } },
                singleLine = true,
                modifier = Modifier.fillMaxWidth().testTag("create-channel-name"),
            )
            if (sections.isNotEmpty()) {
                Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text("Section", style = MaterialTheme.typography.labelLarge, color = c.textMuted)
                    Box {
                        OutlinedTextField(
                            value = sectionName,
                            onValueChange = {},
                            readOnly = true,
                            trailingIcon = { SymIcon(Sym.ExpandMore, null) },
                            modifier = Modifier.fillMaxWidth().clickable { sectionMenu = true },
                            enabled = false,
                            colors = androidx.compose.material3.OutlinedTextFieldDefaults.colors(
                                disabledTextColor = c.text,
                                disabledBorderColor = c.controlBorder,
                                disabledTrailingIconColor = c.textMuted,
                            ),
                        )
                        DropdownMenu(expanded = sectionMenu, onDismissRequest = { sectionMenu = false }) {
                            DropdownMenuItem(text = { Text("Top level") }, onClick = { sectionId = null; sectionMenu = false })
                            sections.forEach { s ->
                                DropdownMenuItem(text = { Text(s.name) }, onClick = { sectionId = s.id; sectionMenu = false })
                            }
                        }
                    }
                }
            }
            Button(
                onClick = {
                    val typed = name.trim()
                    scope.launch {
                        busy = true
                        createChannel(guildId, typed, kind, sectionId).fold(
                            { onDismiss() },
                            { e -> busy = false; error = e.message ?: "Couldn't create the channel." },
                        )
                    }
                },
                enabled = !busy && name.isNotBlank(),
                modifier = Modifier.fillMaxWidth().testTag("create-channel-submit"),
            ) {
                if (busy) CircularProgressIndicator(Modifier.size(18.dp), color = c.accentFg, strokeWidth = 2.dp) else Text("Create")
            }
        }
    }
}

/** Create or rename a section (SIDE-14): one name field in an alert. */
@Composable
fun SectionNameDialog(
    title: String,
    initial: String,
    confirmLabel: String,
    onDismiss: () -> Unit,
    onConfirm: suspend (String) -> Result<*>,
) {
    val scope = rememberCoroutineScope()
    var name by remember { mutableStateOf(initial) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = {
            OutlinedTextField(
                value = name,
                onValueChange = { name = it; error = null },
                label = { Text("Section name") },
                isError = error != null,
                supportingText = error?.let { { Text(it, color = MaterialTheme.colorScheme.error) } },
                singleLine = true,
                modifier = Modifier.testTag("section-name-input"),
            )
        },
        confirmButton = {
            TextButton(
                enabled = !busy && name.isNotBlank(),
                onClick = {
                    val typed = name.trim()
                    scope.launch {
                        busy = true
                        onConfirm(typed).fold({ onDismiss() }, { e -> busy = false; error = e.message ?: "Couldn't save the section." })
                    }
                },
            ) { Text(confirmLabel) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

/** Delete section (SIDE-14): confirms that its channels move to the top level. */
@Composable
fun DeleteSectionDialog(name: String, onDismiss: () -> Unit, onConfirm: suspend () -> Result<*>) {
    val scope = rememberCoroutineScope()
    var busy by remember { mutableStateOf(false) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Delete section") },
        text = { Text("“$name” will be removed. Its channels move to the top of the list; nothing is deleted from them.") },
        confirmButton = {
            TextButton(enabled = !busy, onClick = { scope.launch { busy = true; onConfirm(); onDismiss() } }) { Text(if (busy) "Deleting…" else "Delete section") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

/** Invite people (DLG-03/04): the server's code and link to share, Reset code, and invite by email with the pending list. */
@Suppress("DEPRECATION") // LocalClipboardManager is deprecated for the suspend-based LocalClipboard; a plain synchronous copy fits a short code better than threading a CoroutineScope through
@Composable
fun InviteSheet(
    guild: Guild,
    canManageServer: Boolean,
    canInvite: Boolean,
    rotateInviteCode: suspend (Long) -> Result<String>,
    createInvite: suspend (String, Long) -> Result<Invite>,
    listInvites: suspend (Long) -> Result<List<Invite>>,
    revokeInvite: suspend (Long) -> Result<Invite>,
    resendInvite: suspend (Long) -> Result<Invite>,
    onDismiss: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val c = Hideout.colors
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    val clipboard = LocalClipboardManager.current
    val link = "https://thehideout.zabit.si/invite/${guild.inviteCode}"
    var resetConfirm by remember { mutableStateOf(false) }
    var resetBusy by remember { mutableStateOf(false) }
    var email by remember { mutableStateOf("") }
    var emailError by remember { mutableStateOf<String?>(null) }
    var emailBusy by remember { mutableStateOf(false) }
    var pending by remember { mutableStateOf<List<Invite>>(emptyList()) }
    var pendingError by remember { mutableStateOf<String?>(null) }
    var busyIds by remember { mutableStateOf(setOf<Long>()) }
    var refreshKey by remember { mutableStateOf(0) }

    LaunchedEffect(guild.id, refreshKey) {
        if (canInvite) listInvites(guild.id).fold({ pending = it.filter { i -> i.status == InviteStatus.Pending } }, { e -> pendingError = e.message })
    }

    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = c.surface, modifier = modifier) {
        LazyColumn(Modifier.padding(horizontal = 20.dp).navigationBarsPadding(), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            item {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text("Invite people", style = MaterialTheme.typography.titleLarge, color = c.text)
                    Text("Anyone with this code can join ${guild.name}.", style = MaterialTheme.typography.bodyMedium, color = c.textMuted)
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Text(
                            guild.inviteCode,
                            style = MaterialTheme.typography.titleMedium,
                            color = c.text,
                            modifier = Modifier.weight(1f).testTag("invite-code-value"),
                        )
                        IconButton(onClick = { clipboard.setText(AnnotatedString(guild.inviteCode)) }) { SymIcon(Sym.ContentCopy, "Copy code") }
                        IconButton(onClick = {
                            val send = Intent(Intent.ACTION_SEND).apply { type = "text/plain"; putExtra(Intent.EXTRA_TEXT, link) }
                            runCatching { context.startActivity(Intent.createChooser(send, null)) }
                        }) { SymIcon(Sym.Share, "Share the link") }
                    }
                    if (canManageServer) {
                        OutlinedButton(onClick = { resetConfirm = true }, modifier = Modifier.testTag("invite-reset")) {
                            SymIcon(Sym.Refresh, null, size = 18.dp)
                            Text(" Reset code…")
                        }
                    }
                }
            }
            if (canInvite) {
                item { HorizontalDivider() }
                item {
                    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Text("Invite by email", style = MaterialTheme.typography.titleMedium, color = c.text)
                        Text("They get an email to make an account and join ${guild.name}. Invites last 7 days.", style = MaterialTheme.typography.bodySmall, color = c.textMuted)
                        OutlinedTextField(
                            value = email,
                            onValueChange = { email = it; emailError = null },
                            label = { Text("Email") },
                            placeholder = { Text("friend@example.com") },
                            isError = emailError != null,
                            supportingText = emailError?.let { { Text(it, color = MaterialTheme.colorScheme.error) } },
                            singleLine = true,
                            modifier = Modifier.fillMaxWidth().testTag("invite-email-input"),
                        )
                        Button(
                            onClick = {
                                val typed = email.trim()
                                scope.launch {
                                    emailBusy = true
                                    createInvite(typed, guild.id).fold(
                                        { email = ""; emailBusy = false; refreshKey++ },
                                        { e -> emailBusy = false; emailError = e.message ?: "Couldn't send the invite." },
                                    )
                                }
                            },
                            enabled = !emailBusy && email.contains("@"),
                            modifier = Modifier.fillMaxWidth().testTag("invite-email-submit"),
                        ) {
                            if (emailBusy) CircularProgressIndicator(Modifier.size(18.dp), color = c.accentFg, strokeWidth = 2.dp) else Text("Send invite")
                        }
                    }
                }
                item { Text("Pending invites", style = MaterialTheme.typography.labelLarge, color = c.textMuted) }
                if (pendingError != null) item { Text(pendingError!!, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error) }
                else if (pending.isEmpty()) item { Text("No pending invites.", style = MaterialTheme.typography.bodySmall, color = c.textMuted) }
                items(pending, key = { it.id }) { invite ->
                    val busy = invite.id in busyIds
                    Row(Modifier.fillMaxWidth(), verticalAlignment = androidx.compose.ui.Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Column(Modifier.weight(1f)) {
                            Text(invite.email, style = MaterialTheme.typography.bodyMedium, color = c.text)
                            Text(
                                "Expires ${DateTimeFormatter.ofPattern("MMM d").withZone(ZoneId.systemDefault()).format(invite.expiresAt)}",
                                style = MaterialTheme.typography.bodySmall,
                                color = c.textMuted,
                            )
                        }
                        TextButton(enabled = !busy, onClick = {
                            scope.launch { busyIds = busyIds + invite.id; resendInvite(invite.id); busyIds = busyIds - invite.id; refreshKey++ }
                        }) { Text("Resend") }
                        TextButton(enabled = !busy, onClick = {
                            scope.launch { busyIds = busyIds + invite.id; revokeInvite(invite.id); busyIds = busyIds - invite.id; refreshKey++ }
                        }) { Text("Withdraw") }
                    }
                }
            }
            item { Box(Modifier.size(1.dp)) }
        }
    }

    if (resetConfirm) {
        AlertDialog(
            onDismissRequest = { if (!resetBusy) resetConfirm = false },
            title = { Text("Reset invite code") },
            text = { Text("${guild.name} gets a new code. The old code stops working, so anyone who hasn't joined yet will need the new one.") },
            confirmButton = {
                TextButton(enabled = !resetBusy, onClick = {
                    scope.launch { resetBusy = true; rotateInviteCode(guild.id); resetBusy = false; resetConfirm = false }
                }) { Text(if (resetBusy) "Resetting…" else "Reset code") }
            },
            dismissButton = { TextButton(onClick = { resetConfirm = false }) { Text("Cancel") } },
        )
    }
}
