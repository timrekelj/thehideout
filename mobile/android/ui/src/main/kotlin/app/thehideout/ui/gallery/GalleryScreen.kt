package app.thehideout.ui.gallery

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.ContainedLoadingIndicator
import androidx.compose.material3.ElevatedButton
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.InputChip
import androidx.compose.material3.LargeFlexibleTopAppBar
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.CircularWavyProgressIndicator
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedIconButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.ToggleButton
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.ButtonGroupDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.thehideout.core.store.Unread
import app.thehideout.design.*
import app.thehideout.ui.components.*
import app.thehideout.ui.icons.Sym
import app.thehideout.ui.icons.SymIcon
import app.thehideout.ui.theme.Hideout
import app.thehideout.ui.theme.Look
import java.time.LocalDate
import androidx.compose.foundation.text.input.TextFieldLineLimits

/** One piece of the gallery: a title, an optional note, and the content it demonstrates. */
data class GallerySection(val title: String, val note: String? = null, val content: @Composable () -> Unit)

/** The gallery's sections, in the order [GalleryScreen] shows them. Shared with the screenshot
 *  tests, which capture them in a few bounded groups instead of the whole page in one bitmap. */
val gallerySections: List<GallerySection> = listOf(
    GallerySection("Surfaces", "bg climbs to surface, raised, overlay") { Surfaces() },
    GallerySection("Text and accent") { TextAndAccent() },
    GallerySection("Status") { StatusColors() },
    GallerySection("Channel palette", "ink for icons, text-safe ink for words") { ChannelPalette() },
    GallerySection("Type", "Instrument Sans, JetBrains Mono for code") { TypeRamp() },
    GallerySection("Buttons") { Buttons() },
    GallerySection("Button groups and toggles") { ButtonGroups() },
    GallerySection("Fields") { Fields() },
    GallerySection("Chips and badges") { ChipsAndBadges() },
    GallerySection("Selection") { Selection() },
    GallerySection("Progress") { Progress() },
    GallerySection("Avatars", "deterministic fills, desktop's FNV-1a rule") { Avatars() },
    GallerySection("Servers") { Servers() },
    GallerySection("Channels") { Channels() },
    GallerySection("Messages") { Messages() },
)

/** A plain, non-lazy stack of some of [gallerySections] with no screen chrome (no top bar, FAB or
 *  look sheet): lets the screenshot tests capture the gallery in a few bounded pieces instead of
 *  one huge bitmap. */
@Composable
fun GallerySectionsPage(sections: List<GallerySection>, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    Column(modifier.fillMaxWidth().background(c.bg)) {
        sections.forEach { Section(it.title, it.note, it.content) }
    }
}

/**
 * The debug gallery: the mobile twin of `client/gallery`. Everything built so far, drawn with the
 * tokens of the look in force, and a switcher for the look itself, so a design change is judged in
 * one scroll. Reached from the debug launcher entry or `gallery=true`.
 */
@Composable
fun GalleryScreen(look: Look, onLookChange: (Look) -> Unit, modifier: Modifier = Modifier, initialSheetOpen: Boolean = false) {
    val c = Hideout.colors
    val scroll = TopAppBarDefaults.exitUntilCollapsedScrollBehavior()
    var sheet by rememberSaveable { mutableStateOf(initialSheetOpen) }
    Scaffold(
        modifier = modifier.nestedScroll(scroll.nestedScrollConnection),
        containerColor = c.bg,
        topBar = {
            LargeFlexibleTopAppBar(
                title = { Text("Gallery") },
                subtitle = { Text(lookSummary(look, Hideout.isDark)) },
                actions = {
                    IconButton(onClick = { sheet = true }) { SymIcon(Sym.Palette, "Change the look") }
                },
                scrollBehavior = scroll,
            )
        },
        floatingActionButton = {
            ExtendedFloatingActionButton(
                onClick = { sheet = true },
                icon = { SymIcon(Sym.Tune, null) },
                text = { Text("Look") },
            )
        },
    ) { padding ->
        LazyColumn(
            contentPadding = PaddingValues(top = padding.calculateTopPadding(), bottom = padding.calculateBottomPadding() + 96.dp),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            items(gallerySections) { s -> Section(s.title, s.note, s.content) }
        }
    }
    if (sheet) LookSheet(look, onLookChange) { sheet = false }
}

private fun lookSummary(l: Look, dark: Boolean): String {
    val s = l.spec
    return buildString {
        append(s.accent.displayName).append(" · ").append(s.background.displayName).append(" · ").append(if (dark) "Dark" else "Light")
        if (s.contrast == ContrastLevel.High) append(" · High contrast")
        if (s.tint) append(" · Tinted")
    }
}

@Composable
private fun Section(title: String, note: String? = null, content: @Composable () -> Unit) {
    val c = Hideout.colors
    Column(Modifier.padding(top = 16.dp)) {
        Text(title, style = MaterialTheme.typography.titleMedium, color = c.text, modifier = Modifier.padding(horizontal = 16.dp))
        if (note != null) Text(note, style = MaterialTheme.typography.bodySmall, color = c.textMuted, modifier = Modifier.padding(horizontal = 16.dp, vertical = 2.dp))
        Box(Modifier.padding(horizontal = 16.dp, vertical = 10.dp)) { content() }
    }
}

// ---- colour ------------------------------------------------------------------------------------

@Composable
private fun Swatch(name: String, color: Color, modifier: Modifier = Modifier, on: Color? = null, onLabel: String = "Aa", shown: Color = color) {
    val c = Hideout.colors
    Column(modifier) {
        Box(
            Modifier.fillMaxWidth().height(56.dp).clip(RoundedCornerShape(Radius.lg)).background(color)
                .border(1.dp, c.borderSubtle, RoundedCornerShape(Radius.lg)),
            contentAlignment = Alignment.Center,
        ) {
            if (on != null) Text(onLabel, color = on, style = MaterialTheme.typography.titleMedium)
        }
        Text(name, style = MaterialTheme.typography.labelMedium, color = c.text, modifier = Modifier.padding(top = 6.dp))
        Text(hex(shown), style = Hideout.type.code.toTextStyle().copy(fontSize = Hideout.type.caption.size.sp), color = c.textFaint)
    }
}

private val Float.sp get() = androidx.compose.ui.unit.TextUnit(this, androidx.compose.ui.unit.TextUnitType.Sp)

private fun hex(c: Color): String {
    val argb = c.toArgbInt()
    return "#%02x%02x%02x".format((argb shr 16) and 0xff, (argb shr 8) and 0xff, argb and 0xff) + if ((argb ushr 24) != 0xff) "·%02x".format(argb ushr 24) else ""
}

private fun Color.toArgbInt(): Int = toArgb()

@Composable
private fun SwatchRow(content: @Composable (Modifier) -> Unit) = Unit

@Composable
private fun Swatches(vararg items: Triple<String, Color, Color?>) {
    FlowRow(
        maxItemsInEachRow = 3,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
        modifier = Modifier.fillMaxWidth(),
    ) {
        // Foreground tokens are shown as text on the page background; their hex is the text's, not the page's.
        val foreground = setOf("text", "textMuted", "textFaint", "accentText")
        items.forEach { (n, col, on) -> Swatch(n, col, Modifier.weight(1f), on, shown = if (n in foreground && on != null) on else col) }
        repeat((3 - items.size % 3) % 3) { Box(Modifier.weight(1f)) }
    }
}

@Composable
private fun Surfaces() {
    val c = Hideout.colors
    Swatches(
        Triple("bg", c.bg, c.text), Triple("surface", c.surface, c.text), Triple("surfaceRaised", c.surfaceRaised, c.text),
        Triple("surfaceOverlay", c.surfaceOverlay, c.text), Triple("border", c.border, null), Triple("controlBorder", c.controlBorder, null),
    )
}

@Composable
private fun TextAndAccent() {
    val c = Hideout.colors
    Swatches(
        Triple("text", c.bg, c.text), Triple("textMuted", c.bg, c.textMuted), Triple("textFaint", c.bg, c.textFaint),
        Triple("accent", c.accent, c.accentFg), Triple("accentText", c.bg, c.accentText), Triple("accentSubtle", c.accentSubtle, c.accentText),
    )
}

@Composable
private fun StatusColors() {
    val c = Hideout.colors
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        listOf(Triple("success", c.success, c.successBg to c.successBorder), Triple("warning", c.warning, c.warningBg to c.warningBorder),
            Triple("danger", c.danger, c.dangerBg to c.dangerBorder), Triple("info", c.info, c.infoBg to c.infoBorder)).forEach { (n, fg, bgBorder) ->
            Box(
                Modifier.weight(1f).clip(RoundedCornerShape(Radius.lg)).background(bgBorder.first).border(1.dp, bgBorder.second, RoundedCornerShape(Radius.lg)).padding(vertical = 14.dp),
                contentAlignment = Alignment.Center,
            ) { Text(n, color = fg, style = MaterialTheme.typography.labelLarge) }
        }
    }
}

@Composable
private fun ChannelPalette() {
    val c = Hideout.colors
    FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalArrangement = Arrangement.spacedBy(12.dp), maxItemsInEachRow = 6) {
        c.channels.forEach { (key, ch) ->
            Column(Modifier.width(52.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Box(Modifier.size(32.dp).clip(CircleShape).background(ch.ink))
                Text(key, style = MaterialTheme.typography.bodySmall, color = ch.text, maxLines = 1, modifier = Modifier.padding(top = 4.dp))
            }
        }
    }
}

// ---- type --------------------------------------------------------------------------------------

@Composable
private fun TypeRamp() {
    val c = Hideout.colors
    val t = Hideout.type
    HideoutCard(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            listOf(
                Triple("display", t.display, "Warm and crisp"),
                Triple("heading", t.heading, "Pick up where you left off"),
                Triple("title", t.title, "Channel names and dialog titles"),
                Triple("bodyStrong", t.bodyStrong, "Author names read a little heavier"),
                Triple("body", t.body, "Messages are set in the body style, long enough to wrap onto a second line."),
                Triple("control", t.control, "Buttons, tabs and field text"),
                Triple("label", t.label, "SECTION LABELS AND METADATA"),
                Triple("caption", t.caption, "10:42 · edited"),
                Triple("code", t.code, "let delay = (base << attempt).min(30_000);"),
            ).forEach { (name, style, sample) ->
                Column {
                    Text(sample, style = style.toTextStyle(), color = c.text)
                    Text("$name  ${style.size.fmt()}/${style.lineHeight.fmt()}  ·  ${style.weight}", style = MaterialTheme.typography.bodySmall, color = c.textFaint)
                }
            }
        }
    }
}

private fun Float.fmt(): String = if (this % 1f == 0f) toInt().toString() else "%.1f".format(this)

// ---- controls ----------------------------------------------------------------------------------

@Composable
private fun Buttons() {
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(onClick = {}, shapes = ButtonDefaults.shapes()) { SymIcon(Sym.Send, null, size = 18.dp); Box(Modifier.width(8.dp)); Text("Send") }
            FilledTonalButton(onClick = {}, shapes = ButtonDefaults.shapes()) { Text("Tonal") }
            OutlinedButton(onClick = {}, shapes = ButtonDefaults.shapes()) { Text("Outlined") }
            ElevatedButton(onClick = {}, shapes = ButtonDefaults.shapes()) { Text("Elevated") }
            TextButton(onClick = {}, shapes = ButtonDefaults.shapes()) { Text("Text") }
            Button(onClick = {}, enabled = false) { Text("Disabled") }
        }
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(
                onClick = {}, shapes = ButtonDefaults.shapes(),
                modifier = Modifier.height(ButtonDefaults.ExtraSmallContainerHeight), contentPadding = ButtonDefaults.contentPaddingFor(ButtonDefaults.ExtraSmallContainerHeight),
            ) { Text("Small", style = ButtonDefaults.textStyleFor(ButtonDefaults.ExtraSmallContainerHeight)) }
            Button(
                onClick = {}, shapes = ButtonDefaults.shapes(),
                modifier = Modifier.height(ButtonDefaults.MediumContainerHeight), contentPadding = ButtonDefaults.contentPaddingFor(ButtonDefaults.MediumContainerHeight),
            ) { Text("Medium", style = ButtonDefaults.textStyleFor(ButtonDefaults.MediumContainerHeight)) }
            Button(
                onClick = {}, shapes = ButtonDefaults.shapes(),
                modifier = Modifier.height(ButtonDefaults.MinHeight + 8.dp),
            ) { Text("Default") }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            FilledIconButton(onClick = {}, shapes = IconButtonDefaultsShapes()) { SymIcon(Sym.Add, "Add") }
            FilledTonalIconButton(onClick = {}, shapes = IconButtonDefaultsShapes()) { SymIcon(Sym.Mood, "Emoji") }
            OutlinedIconButton(onClick = {}, shapes = IconButtonDefaultsShapes()) { SymIcon(Sym.Reply, "Reply") }
            IconButton(onClick = {}) { SymIcon(Sym.MoreVert, "More") }
            FloatingActionButton(onClick = {}) { SymIcon(Sym.Edit, "Compose") }
        }
    }
}

@Composable
private fun IconButtonDefaultsShapes() = androidx.compose.material3.IconButtonDefaults.shapes()

@Composable
private fun ButtonGroups() {
    var picked by remember { mutableStateOf(1) }
    var mic by remember { mutableStateOf(true) }
    var cam by remember { mutableStateOf(false) }
    var share by remember { mutableStateOf(false) }
    Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween)) {
            listOf("Today", "Week", "Month").forEachIndexed { i, label ->
                ToggleButton(
                    checked = picked == i, onCheckedChange = { picked = i },
                    modifier = Modifier.weight(1f),
                    shapes = when (i) {
                        0 -> ButtonGroupDefaults.connectedLeadingButtonShapes()
                        2 -> ButtonGroupDefaults.connectedTrailingButtonShapes()
                        else -> ButtonGroupDefaults.connectedMiddleButtonShapes()
                    },
                ) { Text(label) }
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            ToggleButton(checked = mic, onCheckedChange = { mic = it }) {
                SymIcon(if (mic) Sym.Mic else Sym.MicOff, null, size = 18.dp); Box(Modifier.width(8.dp)); Text(if (mic) "Mic on" else "Muted")
            }
            ToggleButton(checked = cam, onCheckedChange = { cam = it }) {
                SymIcon(Sym.Videocam, null, size = 18.dp); Box(Modifier.width(8.dp)); Text("Camera")
            }
            ToggleButton(checked = share, onCheckedChange = { share = it }) {
                SymIcon(Sym.ScreenShare, null, size = 18.dp); Box(Modifier.width(8.dp)); Text("Share")
            }
        }
    }
}

@Composable
private fun Fields() {
    val name = rememberTextFieldState("")
    val message = rememberTextFieldState("Ship it")
    val bad = rememberTextFieldState("tim!")
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        OutlinedTextField(
            state = name, label = { Text("Username") }, placeholder = { Text("tim") },
            leadingIcon = { SymIcon(Sym.Person, null) }, supportingText = { Text("Letters, numbers and underscores") },
            lineLimits = TextFieldLineLimits.SingleLine, modifier = Modifier.fillMaxWidth(),
        )
        TextField(
            state = message, label = { Text("Message") },
            trailingIcon = { IconButton(onClick = {}) { SymIcon(Sym.Send, "Send", tint = Hideout.colors.accentText) } },
            modifier = Modifier.fillMaxWidth(),
        )
        OutlinedTextField(
            state = bad, label = { Text("Username") }, isError = true,
            supportingText = { Text("Only letters, numbers and underscores.") },
            lineLimits = TextFieldLineLimits.SingleLine, modifier = Modifier.fillMaxWidth(),
        )
    }
}

@Composable
private fun ChipsAndBadges() {
    var sel by remember { mutableStateOf(setOf("Unread")) }
    val chipShape = RoundedCornerShape(Radius.md)
    Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            listOf("All", "Unread", "Mentions", "Voice").forEach { label ->
                FilterChip(
                    selected = label in sel, onClick = { sel = if (label in sel) sel - label else sel + label }, label = { Text(label) }, shape = chipShape,
                    leadingIcon = if (label in sel) ({ SymIcon(Sym.Check, null, size = 18.dp) }) else null,
                )
            }
        }
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            AssistChip(onClick = {}, label = { Text("Invite people") }, leadingIcon = { SymIcon(Sym.Add, null, size = 18.dp) }, shape = chipShape)
            InputChip(selected = false, onClick = {}, label = { Text("ana") }, shape = chipShape, trailingIcon = { SymIcon(Sym.Close, null, size = 18.dp) })
        }
        Row(horizontalArrangement = Arrangement.spacedBy(20.dp), verticalAlignment = Alignment.CenterVertically) {
            val bc = Hideout.colors
            BadgedBox(badge = { Badge(containerColor = bc.accent, contentColor = bc.accentFg) { Text("3") } }) { SymIcon(Sym.Forum, null) }
            BadgedBox(badge = { Badge(containerColor = bc.accent, contentColor = bc.accentFg) }) { SymIcon(Sym.Notifications, null) }
            UnreadPill(7); UnreadPill(142); MentionBadge(2); MentionBadge(12)
        }
    }
}

// The value-based Slider is deprecated for the state-based one; controlled state is simpler here.
@Suppress("DEPRECATION")
@Composable
private fun Selection() {
    var on by remember { mutableStateOf(true) }
    var checked by remember { mutableStateOf(true) }
    var radio by remember { mutableStateOf(0) }
    var slider by remember { mutableStateOf(0.4f) }
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            Switch(checked = on, onCheckedChange = { on = it })
            Checkbox(checked = checked, onCheckedChange = { checked = it })
            RadioButton(selected = radio == 0, onClick = { radio = 0 })
            RadioButton(selected = radio == 1, onClick = { radio = 1 })
        }
        Slider(value = slider, onValueChange = { slider = it })
    }
}

@Composable
private fun Progress() {
    Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(16.dp), verticalAlignment = Alignment.CenterVertically) {
            LoadingIndicator(progress = { 0.35f })
            ContainedLoadingIndicator(progress = { 0.7f })
            CircularWavyProgressIndicator(progress = { 0.6f })
        }
        LinearWavyProgressIndicator(progress = { 0.6f }, modifier = Modifier.fillMaxWidth())
    }
}

// ---- app components ----------------------------------------------------------------------------

@Composable
private fun Avatars() {
    Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
            GallerySamples.people.forEach { Avatar(it.id, it.username, size = 48.dp) }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
            Avatar(2, "ana", size = 24.dp); Avatar(2, "ana", size = 32.dp); Avatar(2, "ana", size = 40.dp); Avatar(2, "ana", size = 56.dp)
            AvatarStack(GallerySamples.people.map { it.id to it.username }, size = 28.dp)
        }
    }
}

@Composable
private fun Servers() {
    Row(horizontalArrangement = Arrangement.spacedBy(2.dp), verticalAlignment = Alignment.CenterVertically) {
        ServerBadge("The Hideout", selected = true)
        ServerBadge("Garage Band", selected = false, unread = 3)
        ServerBadge("Cooking Club", selected = false, mentions = 2)
        ServerBadge("Night Owls", selected = false)
    }
}

@Composable
private fun Channels() {
    val s = GallerySamples
    HideoutCard(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(8.dp)) {
            SectionHeader("Building", open = true)
            ChannelRow(s.channelGeneral, selected = true, unread = Unread())
            ChannelRow(s.channelDev, selected = false, unread = Unread(4, 0))
            ChannelRow(s.channelDesign, selected = false, unread = Unread(2, 1))
            ChannelRow(s.channelBooks, selected = false, unread = Unread())
            SectionHeader("Voice", open = true)
            ChannelRow(s.channelLounge, selected = false, unread = Unread(), voice = s.loungeStates, names = { id -> id to s.authors.getValue(id).username })
            SectionHeader("Archive", open = false, unread = Unread(3, 0))
        }
    }
}

@Composable
private fun Messages() {
    val s = GallerySamples
    val today = LocalDate.of(2026, 11, 22)
    HideoutCard(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(vertical = 8.dp)) {
            DayDivider(today, today)
            s.messages.forEachIndexed { i, m ->
                val head = i == 0 || s.messages[i - 1].authorId != m.authorId
                if (i == 3) NewDivider()
                MessageRow(MessageUi(m, s.authors[m.authorId], head, me = "tim"))
            }
            MessageRow(MessageUi(s.messages[3].copy(id = 90, authorId = s.tim.id, content = "Sent from here, still on its way", editedAt = null), s.tim, head = true, me = "tim", delivery = app.thehideout.core.store.Delivery.Pending))
            MessageRow(MessageUi(s.messages[3].copy(id = 91, authorId = s.tim.id, content = "This one could not be sent", reactions = emptyList(), editedAt = null), s.tim, head = true, me = "tim", delivery = app.thehideout.core.store.Delivery.Failed))
        }
    }
}
