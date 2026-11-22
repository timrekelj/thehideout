package app.thehideout.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.input.TextFieldLineLimits
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
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
import kotlinx.coroutines.launch

/** One row of the picker's grid: a full-span section header, or an emoji cell. */
private sealed interface PickerCell {
    data class Header(val label: String) : PickerCell
    data class Cell(val entry: EmojiEntry) : PickerCell
}

/**
 * The full emoji picker (MSG-07): search, this server's custom emoji, and [EmojiCategory]'s eight
 * Unicode groups, in one scrolling grid with full-span section headers; a chip strip above jumps
 * the grid to a section. Passing an empty `serverEmojis` (as the DM composer does) is what keeps
 * it Unicode-only in DMs (DM-06) - there is no separate scope switch.
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
    val gridState = rememberLazyGridState()
    val scope = rememberCoroutineScope()
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
            if (q.isNotEmpty()) {
                val custom = serverEmojis.filter { it.name.contains(q, ignoreCase = true) }
                    .map { EmojiEntry.Custom(it.id, it.name, it.image, it.animated) }
                val unicode = EmojiData.search(q, limit = 60)
                EmojiGrid((custom + unicode).map { PickerCell.Cell(it) }, onPick, gridState)
            } else {
                val cells = remember(serverEmojis) { pickerCells(serverEmojis) }
                val sectionIndex = remember(cells) { cells.withIndex().filter { it.value is PickerCell.Header }.associate { (it.value as PickerCell.Header).label to it.index } }
                CategoryStrip(serverEmojis.isNotEmpty()) { label ->
                    sectionIndex[label]?.let { index -> scope.launch { gridState.animateScrollToItem(index) } }
                }
                EmojiGrid(cells, onPick, gridState)
            }
        }
    }
}

/** This server's emoji, then every [EmojiCategory], each with a header cell before its entries. */
private fun pickerCells(serverEmojis: List<Emoji>): List<PickerCell> = buildList {
    if (serverEmojis.isNotEmpty()) {
        add(PickerCell.Header("This server"))
        addAll(serverEmojis.map { PickerCell.Cell(EmojiEntry.Custom(it.id, it.name, it.image, it.animated)) })
    }
    EmojiCategory.entries.forEach { category ->
        add(PickerCell.Header(category.label))
        addAll(EmojiData.inCategory(category).map { PickerCell.Cell(it) })
    }
}

/** Jumps the grid to a section: "This server" (when there is one) plus every [EmojiCategory]. */
@Composable
private fun CategoryStrip(hasServerEmojis: Boolean, onPick: (String) -> Unit) {
    val c = Hideout.colors
    val labels = buildList {
        if (hasServerEmojis) add("This server")
        addAll(EmojiCategory.entries.map { it.label })
    }
    LazyRow(Modifier.fillMaxWidth().padding(bottom = 6.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        items(labels) { label ->
            Surface(onClick = { onPick(label) }, shape = RoundedCornerShape(Radius.md), color = c.surfaceOverlay) {
                Text(label, style = MaterialTheme.typography.labelSmall, color = c.text, modifier = Modifier.padding(horizontal = 10.dp, vertical = 6.dp))
            }
        }
    }
}

@Composable
private fun EmojiGrid(cells: List<PickerCell>, onPick: (EmojiEntry) -> Unit, state: androidx.compose.foundation.lazy.grid.LazyGridState) {
    val mediaBase = LocalMediaBase.current
    LazyVerticalGrid(
        columns = GridCells.Fixed(9),
        state = state,
        contentPadding = PaddingValues(vertical = 2.dp),
        modifier = Modifier.fillMaxWidth().heightIn(max = 420.dp),
    ) {
        items(
            cells,
            key = { cellKey(it) },
            span = { cell -> if (cell is PickerCell.Header) GridItemSpan(maxLineSpan) else GridItemSpan(1) },
        ) { cell ->
            when (cell) {
                is PickerCell.Header -> CategoryLabel(cell.label)
                is PickerCell.Cell -> Box(
                    Modifier.size(40.dp).clip(RoundedCornerShape(Radius.sm)).clickable { onPick(cell.entry) },
                    contentAlignment = Alignment.Center,
                ) {
                    when (val entry = cell.entry) {
                        is EmojiEntry.Unicode -> Text(entry.emoji, style = MaterialTheme.typography.titleLarge)
                        is EmojiEntry.Custom -> PickerEmojiImage(entry, mediaBase)
                    }
                }
            }
        }
    }
}

/** A server emoji's picture in the picker grid (MSG-09); a graceful `:name:` on a failed load. */
@Composable
private fun PickerEmojiImage(entry: EmojiEntry.Custom, mediaBase: String) {
    var failed by remember(entry.id) { mutableStateOf(false) }
    if (failed) {
        Text(":${entry.name}:", style = MaterialTheme.typography.labelSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
    } else {
        coil3.compose.AsyncImage(
            model = app.thehideout.core.attachment.mediaUrl(mediaBase, entry.image),
            contentDescription = ":${entry.name}:",
            modifier = Modifier.size(28.dp),
            onState = { if (it is coil3.compose.AsyncImagePainter.State.Error) failed = true },
        )
    }
}

@Composable
private fun CategoryLabel(text: String) {
    Text(text, style = MaterialTheme.typography.labelMedium, color = Hideout.colors.textMuted, modifier = Modifier.padding(top = 8.dp, bottom = 4.dp))
}

private fun cellKey(cell: PickerCell): String = when (cell) {
    is PickerCell.Header -> "h-${cell.label}"
    is PickerCell.Cell -> when (val e = cell.entry) {
        is EmojiEntry.Unicode -> "u-${e.emoji}"
        is EmojiEntry.Custom -> "c-${e.id}"
    }
}
