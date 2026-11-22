package app.thehideout.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.input.TextFieldLineLimits
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.thehideout.core.emoji.EmojiCategory
import app.thehideout.core.emoji.EmojiData
import app.thehideout.core.emoji.EmojiEntry
import app.thehideout.core.model.Emoji
import app.thehideout.design.Radius
import app.thehideout.ui.theme.Hideout

/**
 * The full emoji picker (MSG-07): search, this server's custom emoji, nine Unicode categories.
 * `scope` narrows it to Unicode only in DMs (MSG-06/DM-06, no custom emoji to show there).
 */
@Composable
fun EmojiPickerSheet(
    onDismiss: () -> Unit,
    onPick: (EmojiEntry) -> Unit,
    modifier: Modifier = Modifier,
    serverEmojis: List<Emoji> = emptyList(),
) {
    val c = Hideout.colors
    val query = rememberTextFieldState()
    ModalBottomSheet(onDismissRequest = onDismiss, modifier = modifier, containerColor = c.surface) {
        Column(Modifier.heightIn(min = 320.dp, max = 520.dp).padding(horizontal = 12.dp)) {
            OutlinedTextField(
                state = query,
                modifier = Modifier.fillMaxWidth().padding(bottom = 8.dp),
                placeholder = { Text("Search emoji") },
                lineLimits = TextFieldLineLimits.SingleLine,
                shape = RoundedCornerShape(Radius.lg),
            )
            val q = query.text.toString()
            val custom = serverEmojis.filter { it.name.contains(q, ignoreCase = true) }
                .map { EmojiEntry.Custom(it.id, it.name, it.image, it.animated) }
            if (q.isNotEmpty()) {
                val unicode = EmojiData.search(q, limit = 60)
                EmojiGrid(custom + unicode, onPick)
            } else {
                Column {
                    if (serverEmojis.isNotEmpty()) {
                        CategoryLabel("This server")
                        EmojiGrid(serverEmojis.map { EmojiEntry.Custom(it.id, it.name, it.image, it.animated) }, onPick, rows = 2)
                    }
                    EmojiCategory.entries.forEach { category ->
                        CategoryLabel(category.label)
                        EmojiGrid(EmojiData.inCategory(category), onPick, rows = 3)
                    }
                }
            }
        }
    }
}

@Composable
private fun CategoryLabel(text: String) {
    Text(text, style = MaterialTheme.typography.labelMedium, color = Hideout.colors.textMuted, modifier = Modifier.padding(top = 8.dp, bottom = 4.dp))
}

@Composable
private fun EmojiGrid(entries: List<EmojiEntry>, onPick: (EmojiEntry) -> Unit, rows: Int = 8) {
    LazyVerticalGrid(
        columns = GridCells.Fixed(9),
        contentPadding = PaddingValues(vertical = 2.dp),
        modifier = Modifier.heightIn(max = (rows * 44).dp),
    ) {
        items(entries, key = { entryKey(it) }) { entry ->
            Box(
                Modifier.size(40.dp).clip(RoundedCornerShape(Radius.sm)).clickable { onPick(entry) },
                contentAlignment = Alignment.Center,
            ) {
                when (entry) {
                    is EmojiEntry.Unicode -> Text(entry.emoji, style = MaterialTheme.typography.titleLarge)
                    is EmojiEntry.Custom -> Text(":${entry.name}:", style = MaterialTheme.typography.labelSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
        }
    }
}

private fun entryKey(e: EmojiEntry): String = when (e) {
    is EmojiEntry.Unicode -> "u-${e.emoji}"
    is EmojiEntry.Custom -> "c-${e.id}"
}
