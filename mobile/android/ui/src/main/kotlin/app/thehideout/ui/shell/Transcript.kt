package app.thehideout.ui.shell

import androidx.compose.foundation.background
import androidx.compose.foundation.focusable
import androidx.compose.ui.focus.focusRequester
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.text.input.TextFieldLineLimits
import androidx.compose.foundation.text.input.clearText
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.material3.CenterAlignedTopAppBar
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.LoadingIndicator
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.thehideout.core.model.Channel
import app.thehideout.core.model.ChannelKind
import app.thehideout.core.model.Message
import app.thehideout.core.store.AppState
import app.thehideout.core.store.Delivery
import app.thehideout.core.store.Marker
import app.thehideout.ui.components.*
import app.thehideout.ui.components.LocalToday
import app.thehideout.ui.icons.Sym
import app.thehideout.ui.icons.SymIcon
import app.thehideout.ui.theme.Hideout
import java.time.Duration
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId

/** One entry of a transcript, newest first (the list is reversed). */
internal sealed interface TranscriptItem {
    data class Row(val ui: MessageUi) : TranscriptItem
    data class Day(val date: LocalDate) : TranscriptItem
    data object New : TranscriptItem
}

/**
 * Lays a channel's messages out as rows: heads and follow-ups (a run by one author within five
 * minutes), day dividers, the "New" divider, then this client's pending and failed sends last.
 * Returned oldest first.
 */
internal fun transcriptItems(state: AppState, channel: Long, zone: ZoneId): List<TranscriptItem> {
    val log = state.log(channel) ?: return emptyList()
    val me = state.me?.username
    val out = mutableListOf<TranscriptItem>()
    val marker = state.marker as? Marker.After
    var prev: Message? = null
    val after = if (marker?.channel == channel) marker.id else null
    val hasMarker = marker?.channel == channel
    if (hasMarker && after == null && log.messages.isNotEmpty()) out += TranscriptItem.New
    for (m in log.messages) {
        val day = m.createdAt.atZone(zone).toLocalDate()
        val p = prev
        val newDay = p == null || p.createdAt.atZone(zone).toLocalDate() != day
        if (newDay) out += TranscriptItem.Day(day)
        val head = p == null || newDay || p.authorId != m.authorId || Duration.between(p.createdAt, m.createdAt) > Duration.ofMinutes(5)
        out += TranscriptItem.Row(MessageUi(m, state.users[m.authorId], head, me))
        if (hasMarker && after == m.id) out += TranscriptItem.New
        prev = m
    }
    for (o in state.outgoing.filter { it.channelId == channel }) {
        val head = prev?.authorId != state.myId
        val message = Message(0, channel, null, state.myId, o.content, o.at, nonce = o.nonce)
        out += TranscriptItem.Row(MessageUi(message, state.me, head, me, o.state))
        prev = message
    }
    return out
}

/** The channel (or DM) on screen: a top bar, the messages, and the composer. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TranscriptPane(
    state: AppState,
    channelId: Long,
    title: String,
    subtitle: String?,
    actions: ShellActions,
    modifier: Modifier = Modifier,
    onBack: (() -> Unit)? = null,
) {
    val c = Hideout.colors
    val zone = app.thehideout.ui.components.LocalTimeZone.current
    val items = remember(state.logs[channelId], state.outgoing, state.marker, state.users, zone) { transcriptItems(state, channelId, zone) }
    val log = state.log(channelId)
    val today = LocalToday.current ?: LocalDate.now(zone)
    val listState = rememberLazyListState()
    // Opening a channel reads it; the keyboard comes up when the reader taps the composer, not before.
    // The adaptive scaffold moves focus into a pane it navigates to, and the only focusable thing
    // in touch mode is the composer's text field: park focus on an inert target instead.
    val sink = remember { androidx.compose.ui.focus.FocusRequester() }
    LaunchedEffect(channelId) { sink.requestFocus() }
    Scaffold(
        modifier = modifier.focusRequester(sink).focusable(),
        containerColor = c.bg,
        contentWindowInsets = WindowInsets.safeDrawing,
        topBar = {
            TopAppBar(
                title = {
                    Column {
                        Text(title, style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        if (!subtitle.isNullOrBlank()) Text(subtitle, style = MaterialTheme.typography.bodySmall, color = c.textMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                },
                navigationIcon = { if (onBack != null) IconButton(onClick = onBack) { SymIcon(Sym.ArrowBack, "Back") } },
                actions = { IconButton(onClick = {}) { SymIcon(Sym.Search, "Search") } },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = c.bg, scrolledContainerColor = c.surface),
            )
        },
        bottomBar = { Composer(title, actions.send) },
    ) { padding ->
        Box(Modifier.fillMaxSize().padding(padding)) {
            if (log == null || (!log.loaded && items.isEmpty())) {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { LoadingIndicator() }
            } else {
                // Reversed so the newest message sits at the bottom and older pages load upward.
                val reversed = remember(items) { items.asReversed() }
                LazyColumn(
                    state = listState,
                    reverseLayout = true,
                    contentPadding = PaddingValues(vertical = 8.dp),
                ) {
                    items(reversed.size, key = null) { i ->
                        when (val item = reversed[i]) {
                            is TranscriptItem.Row -> MessageRow(item.ui, onRetry = { item.ui.message.nonce.takeIf { it.isNotEmpty() }?.let(actions.retry) })
                            is TranscriptItem.Day -> DayDivider(item.date, today)
                            TranscriptItem.New -> NewDivider()
                        }
                    }
                    if (log.hasMore) item { Box(Modifier.fillMaxWidth().padding(16.dp), contentAlignment = Alignment.Center) { LoadingIndicator() } }
                }
                LoadOlderWhenReached(listState, reversed.size, log.hasMore) { actions.loadOlder(channelId) }
            }
        }
    }
}

@Composable
private fun LoadOlderWhenReached(state: LazyListState, count: Int, hasMore: Boolean, load: () -> Unit) {
    val atTop by remember(state, count) { derivedStateOf { state.layoutInfo.visibleItemsInfo.lastOrNull()?.index?.let { it >= count - 3 } ?: false } }
    LaunchedEffect(atTop, hasMore) { if (atTop && hasMore) load() }
}

/** The message field. Attachments, emoji and mentions arrive with the chat phase. */
@Composable
private fun Composer(channelName: String, onSend: (String) -> Unit) {
    val c = Hideout.colors
    val text = rememberTextFieldState()
    Row(
        Modifier.fillMaxWidth().background(c.bg).navigationBarsPadding().imePadding().padding(horizontal = 12.dp, vertical = 8.dp),
        verticalAlignment = Alignment.Bottom,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        OutlinedTextField(
            state = text,
            modifier = Modifier.weight(1f),
            placeholder = { Text("Message $channelName") },
            lineLimits = TextFieldLineLimits.MultiLine(1, 5),
            shape = androidx.compose.foundation.shape.RoundedCornerShape(app.thehideout.design.Radius.xl),
        )
        FilledIconButton(
            onClick = { onSend(text.text.toString()); text.clearText() },
            enabled = text.text.isNotBlank(),
            modifier = Modifier.padding(bottom = 6.dp),
            shapes = androidx.compose.material3.IconButtonDefaults.shapes(),
        ) { SymIcon(Sym.Send, "Send") }
    }
}
