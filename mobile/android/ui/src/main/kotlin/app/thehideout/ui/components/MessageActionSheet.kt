package app.thehideout.ui.components

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import android.content.ClipData
import android.content.ClipboardManager
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import app.thehideout.core.emoji.EmojiEntry
import app.thehideout.core.model.Emoji
import app.thehideout.core.model.Message
import app.thehideout.core.model.ReactionEmoji
import app.thehideout.ui.icons.Sym
import app.thehideout.ui.icons.SymIcon
import app.thehideout.ui.theme.Hideout

private val QUICK_REACTIONS = listOf("👍", "❤️", "😂", "😮", "😢", "🎉")

/**
 * The long-press message menu (MSG-01/MSG-02): a quick-reaction row, React (more, opens the full
 * picker), Reply, Copy text, Edit and Delete (own, or Delete with MANAGE_MESSAGES).
 */
@Composable
fun MessageActionSheet(
    message: Message,
    canEdit: Boolean,
    canDelete: Boolean,
    onDismiss: () -> Unit,
    onReact: (ReactionEmoji) -> Unit,
    onReply: () -> Unit,
    onEdit: () -> Unit,
    onDelete: () -> Unit,
    modifier: Modifier = Modifier,
    serverEmojis: List<Emoji> = emptyList(),
) {
    val c = Hideout.colors
    val context = LocalContext.current
    var showEmojiPicker by remember { mutableStateOf(false) }
    var confirmDelete by remember { mutableStateOf(false) }

    ModalBottomSheet(onDismissRequest = onDismiss, modifier = modifier, containerColor = c.surface) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            QUICK_REACTIONS.forEach { emoji ->
                Text(
                    emoji,
                    style = MaterialTheme.typography.headlineSmall,
                    modifier = Modifier.size(36.dp).clip(CircleShape)
                        .clickable { onReact(ReactionEmoji.Unicode(emoji)); onDismiss() },
                )
            }
            SymIcon(
                Sym.Add, "More reactions",
                modifier = Modifier.size(36.dp).clip(CircleShape).clickable { showEmojiPicker = true }.padding(6.dp),
            )
        }
        ActionRow(Sym.Reply, "Reply") { onReply(); onDismiss() }
        ActionRow(Sym.ContentCopy, "Copy text") {
            val manager = context.getSystemService(ClipboardManager::class.java)
            manager?.setPrimaryClip(ClipData.newPlainText("message", message.content))
            onDismiss()
        }
        if (canEdit) ActionRow(Sym.Edit, "Edit message") { onEdit(); onDismiss() }
        if (canDelete) ActionRow(Sym.Delete, "Delete message", danger = true) { confirmDelete = true }
    }

    if (showEmojiPicker) {
        EmojiPickerSheet(
            onDismiss = { showEmojiPicker = false },
            onPick = { entry ->
                if (entry is EmojiEntry.Unicode) onReact(ReactionEmoji.Unicode(entry.emoji))
                if (entry is EmojiEntry.Custom) onReact(ReactionEmoji.Custom(entry.id))
                showEmojiPicker = false
                onDismiss()
            },
            serverEmojis = serverEmojis,
        )
    }

    if (confirmDelete) {
        AlertDialog(
            onDismissRequest = { confirmDelete = false },
            title = { Text("Delete message") },
            text = { Text("This message will be deleted for everyone.") },
            confirmButton = { TextButton(onClick = { confirmDelete = false; onDelete(); onDismiss() }) { Text("Delete", color = c.danger) } },
            dismissButton = { TextButton(onClick = { confirmDelete = false }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun ActionRow(icon: Sym, label: String, danger: Boolean = false, onClick: () -> Unit) {
    val c = Hideout.colors
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = 20.dp, vertical = 14.dp),
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        SymIcon(icon, null, tint = if (danger) c.danger else c.text)
        Text(label, style = MaterialTheme.typography.bodyLarge, color = if (danger) c.danger else c.text)
    }
}
