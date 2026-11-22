package app.thehideout.ui.shell

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.background
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.pointer.pointerInput
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
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.text.input.TextFieldLineLimits
import androidx.compose.foundation.text.input.clearText
import androidx.compose.foundation.text.input.setTextAndPlaceCursorAtEnd
import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.material3.CenterAlignedTopAppBar
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.LoadingIndicator
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.thehideout.core.autocomplete.Autocomplete
import app.thehideout.core.autocomplete.Trigger
import app.thehideout.core.emoji.EmojiEntry
import app.thehideout.core.model.Emoji
import app.thehideout.core.model.Message
import app.thehideout.core.model.ReactionEmoji
import app.thehideout.core.model.User
import app.thehideout.core.store.AppState
import app.thehideout.core.store.ConnectionStatus
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

/** The channel (or DM) on screen: a top bar, the reconnect banner, the messages, and the composer. */
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
    members: List<User> = emptyList(),
    serverEmojis: List<Emoji> = emptyList(),
    canManageMessages: Boolean = false,
    onOpenMembers: (() -> Unit)? = null,
) {
    val c = Hideout.colors
    val zone = app.thehideout.ui.components.LocalTimeZone.current
    val items = remember(state.logs[channelId], state.outgoing, state.marker, state.users, zone) { transcriptItems(state, channelId, zone) }
    val log = state.log(channelId)
    val today = LocalToday.current ?: LocalDate.now(zone)
    val listState = rememberLazyListState()
    val sink = remember { androidx.compose.ui.focus.FocusRequester() }
    LaunchedEffect(channelId) { sink.requestFocus() }

    var actionMessage by remember { mutableStateOf<Message?>(null) }
    var editing by remember(channelId) { mutableStateOf<Message?>(null) }
    val text = rememberSaveable(channelId, saver = TextFieldState.Saver) { TextFieldState(state.draft(channelId)) }
    val composerFocus = remember(channelId) { androidx.compose.ui.focus.FocusRequester() }
    LaunchedEffect(channelId) { snapshotFlow { text.text.toString() }.collect { if (editing == null) actions.setDraft(channelId, it) } }

    Scaffold(
        modifier = modifier.focusRequester(sink).focusable(),
        containerColor = c.bg,
        contentWindowInsets = WindowInsets.safeDrawing,
        topBar = {
            Column {
                TopAppBar(
                    title = {
                        Column {
                            Text(title, style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            if (!subtitle.isNullOrBlank()) Text(subtitle, style = MaterialTheme.typography.bodySmall, color = c.textMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                    },
                    navigationIcon = { if (onBack != null) IconButton(onClick = onBack) { SymIcon(Sym.ArrowBack, "Back") } },
                    actions = {
                        if (onOpenMembers != null) IconButton(onClick = onOpenMembers) { SymIcon(Sym.Person, "Members") }
                        IconButton(onClick = {}) { SymIcon(Sym.Search, "Search") }
                    },
                    colors = TopAppBarDefaults.topAppBarColors(containerColor = c.bg, scrolledContainerColor = c.surface),
                )
                ReconnectBanner(state.connection)
            }
        },
        bottomBar = {
            Composer(
                channelName = title,
                text = text,
                members = members,
                serverEmojis = serverEmojis,
                editing = editing,
                focusRequester = composerFocus,
                onCancelEdit = { editing = null; text.setTextAndPlaceCursorAtEnd(state.draft(channelId)) },
                onSend = { body ->
                    val e = editing
                    if (e != null) {
                        actions.editMessage(channelId, e.id, body)
                        editing = null
                        text.setTextAndPlaceCursorAtEnd(state.draft(channelId))
                    } else {
                        actions.send(body)
                        text.clearText()
                    }
                },
            )
        },
    ) { padding ->
        Box(Modifier.fillMaxSize().padding(padding)) {
            if (log == null || (!log.loaded && items.isEmpty())) {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { LoadingIndicator() }
            } else if (items.isEmpty()) {
                EmptyState(Sym.Tag, "This is the start", "No messages here yet. Say hello.", Modifier.align(Alignment.Center))
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
                            is TranscriptItem.Row -> MessageRow(
                                item.ui,
                                modifier = Modifier.pointerInput(item.ui.message.id) {
                                    detectTapGestures(onLongPress = { if (item.ui.message.id != 0L) actionMessage = item.ui.message })
                                },
                                onReact = { r -> actions.react(channelId, item.ui.message.id, r.emoji) },
                                onRetry = { item.ui.message.nonce.takeIf { it.isNotEmpty() }?.let(actions.retry) },
                            )
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

    actionMessage?.let { m ->
        val mine = m.authorId == state.myId
        MessageActionSheet(
            message = m,
            canEdit = mine,
            canDelete = mine || canManageMessages,
            onDismiss = { actionMessage = null },
            onReact = { emoji -> actions.react(channelId, m.id, emoji) },
            onReply = {
                val name = state.users[m.authorId]?.username
                if (name != null) {
                    text.setTextAndPlaceCursorAtEnd("@$name ${text.text}")
                    composerFocus.requestFocus()
                }
            },
            onEdit = { editing = m; text.setTextAndPlaceCursorAtEnd(m.content) },
            onDelete = { actions.deleteMessage(channelId, m.id) },
            serverEmojis = serverEmojis,
        )
    }
}

/** The connection pill: hidden while live, shown while connecting or reconnecting (TRANS-09). */
@Composable
private fun ReconnectBanner(status: ConnectionStatus) {
    val c = Hideout.colors
    AnimatedVisibility(visible = status != ConnectionStatus.Connected) {
        Surface(color = c.surfaceOverlay, modifier = Modifier.fillMaxWidth()) {
            Text(
                if (status == ConnectionStatus.Reconnecting) "Reconnecting…" else "Connecting…",
                style = MaterialTheme.typography.labelMedium,
                color = c.textMuted,
                modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp),
            )
        }
    }
}

@Composable
private fun LoadOlderWhenReached(state: LazyListState, count: Int, hasMore: Boolean, load: () -> Unit) {
    val atTop by remember(state, count) { derivedStateOf { state.layoutInfo.visibleItemsInfo.lastOrNull()?.index?.let { it >= count - 3 } ?: false } }
    LaunchedEffect(atTop, hasMore) { if (atTop && hasMore) load() }
}

/**
 * The message field: multi-line, `@mention`/`:emoji` autocomplete above it (MSG-10, simplified to
 * assume the caret sits at the end of the text while typing), an emoji button opening the picker,
 * and an inline edit mode (own message) with Cancel/Save instead of Send.
 */
@Composable
private fun Composer(
    channelName: String,
    text: TextFieldState,
    members: List<User>,
    serverEmojis: List<Emoji>,
    editing: Message?,
    focusRequester: androidx.compose.ui.focus.FocusRequester,
    onCancelEdit: () -> Unit,
    onSend: (String) -> Unit,
) {
    val c = Hideout.colors
    var showEmojiPicker by remember { mutableStateOf(false) }
    val current = text.text.toString()
    val trigger = remember(current) { Autocomplete.trigger(current, current.length) }

    Column {
        when (trigger) {
            is Trigger.Mention -> {
                val matches = Autocomplete.mentionMatches(trigger.query, members)
                if (matches.isNotEmpty()) {
                    AutocompleteStrip(matches.map { it.username }) { name ->
                        text.setTextAndPlaceCursorAtEnd(current.substring(0, trigger.start) + "@$name ")
                    }
                }
            }
            is Trigger.EmojiShortcode -> {
                val matches = Autocomplete.emojiMatches(trigger.query, serverEmojis)
                if (matches.isNotEmpty()) {
                    AutocompleteStrip(matches.map { entryLabel(it) }) { label ->
                        val picked = matches.first { entryLabel(it) == label }
                        val replacement = when (picked) {
                            is EmojiEntry.Unicode -> picked.emoji + " "
                            is EmojiEntry.Custom -> customEmojiToken(picked) + " "
                        }
                        text.setTextAndPlaceCursorAtEnd(current.substring(0, trigger.start) + replacement)
                    }
                }
            }
            Trigger.None -> Unit
        }
        if (editing != null) {
            Row(Modifier.fillMaxWidth().background(c.surfaceRaised).padding(horizontal = 16.dp, vertical = 6.dp), horizontalArrangement = Arrangement.SpaceBetween) {
                Text("Editing message", style = MaterialTheme.typography.labelMedium, color = c.textMuted)
                TextButton(onClick = onCancelEdit) { Text("Cancel") }
            }
        }
        Row(
            Modifier.fillMaxWidth().background(c.bg).navigationBarsPadding().imePadding().padding(horizontal = 12.dp, vertical = 8.dp),
            verticalAlignment = Alignment.Bottom,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            IconButton(onClick = { showEmojiPicker = true }, modifier = Modifier.padding(bottom = 2.dp)) { SymIcon(Sym.Mood, "Emoji") }
            OutlinedTextField(
                state = text,
                modifier = Modifier.weight(1f).testTag("composer-field").focusRequester(focusRequester),
                placeholder = { Text(if (editing != null) "Edit message" else "Message $channelName") },
                lineLimits = TextFieldLineLimits.MultiLine(1, 5),
                shape = androidx.compose.foundation.shape.RoundedCornerShape(app.thehideout.design.Radius.xl),
            )
            FilledIconButton(
                onClick = { if (text.text.isNotBlank()) onSend(text.text.toString()) },
                enabled = text.text.isNotBlank(),
                modifier = Modifier.padding(bottom = 6.dp),
                shapes = androidx.compose.material3.IconButtonDefaults.shapes(),
            ) { SymIcon(if (editing != null) Sym.Check else Sym.Send, if (editing != null) "Save" else "Send") }
        }
    }

    if (showEmojiPicker) {
        EmojiPickerSheet(
            onDismiss = { showEmojiPicker = false },
            onPick = { entry ->
                val insert = when (entry) {
                    is EmojiEntry.Unicode -> entry.emoji
                    is EmojiEntry.Custom -> customEmojiToken(entry)
                }
                text.setTextAndPlaceCursorAtEnd("$current$insert")
                showEmojiPicker = false
            },
            serverEmojis = serverEmojis,
        )
    }
}

private fun entryLabel(e: EmojiEntry): String = when (e) {
    is EmojiEntry.Unicode -> "${e.emoji} ${e.name}"
    is EmojiEntry.Custom -> "${e.name}"
}

/** The `<:name:id>` / `<a:name:id>` token desktop's composer inserts and its markdown parses (MD-06). */
private fun customEmojiToken(e: EmojiEntry.Custom): String = "<${if (e.animated) "a" else ""}:${e.name}:${e.id}>"

/** A row of tappable suggestions above the keyboard (member names or emoji), scrollable when it overflows. */
@Composable
private fun AutocompleteStrip(labels: List<String>, onPick: (String) -> Unit) {
    val c = Hideout.colors
    androidx.compose.foundation.lazy.LazyRow(
        Modifier.fillMaxWidth().background(c.surfaceRaised).padding(horizontal = 8.dp, vertical = 6.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        items(labels) { label ->
            Surface(
                onClick = { onPick(label) },
                modifier = Modifier.testTag("autocomplete-$label"),
                shape = androidx.compose.foundation.shape.RoundedCornerShape(app.thehideout.design.Radius.md),
                color = c.surfaceOverlay,
            ) {
                Text(label, style = MaterialTheme.typography.labelMedium, color = c.text, modifier = Modifier.padding(horizontal = 10.dp, vertical = 6.dp))
            }
        }
    }
}
