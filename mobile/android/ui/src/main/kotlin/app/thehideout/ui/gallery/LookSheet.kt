package app.thehideout.ui.gallery

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.ButtonGroupDefaults
import androidx.compose.material3.ToggleButton
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import app.thehideout.design.*
import app.thehideout.ui.components.GroupLabel
import app.thehideout.ui.icons.Sym
import app.thehideout.ui.icons.SymIcon
import app.thehideout.ui.theme.Hideout
import app.thehideout.ui.theme.Look
import app.thehideout.ui.theme.ReduceMotion

/**
 * Every axis of the look, as native controls: accent swatches, background chips, a segmented
 * appearance control, switches, a text size slider. The change applies as you touch it, so the
 * screen behind re-themes live.
 */
// rememberModalBottomSheetState is deprecated for rememberBottomSheetState; this sheet always opens fully expanded.
@Suppress("DEPRECATION")
@Composable
fun LookSheet(look: Look, onChange: (Look) -> Unit, onDismiss: () -> Unit) {
    val state = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = state, containerColor = Hideout.colors.surfaceOverlay) {
        LookControls(look, onChange)
    }
}

@Composable
fun LookControls(look: Look, onChange: (Look) -> Unit, modifier: Modifier = Modifier, scrollable: Boolean = true) {
    val spec = look.spec
    fun spec(f: ThemeSpec.() -> ThemeSpec) = onChange(look.copy(spec = spec.f()))
    val c = Hideout.colors
    Column(
        modifier.fillMaxWidth().navigationBarsPadding().then(if (scrollable) Modifier.verticalScroll(rememberScrollState()) else Modifier).padding(bottom = 24.dp),
    ) {
        Row(Modifier.fillMaxWidth().padding(start = 20.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Text("Look", style = MaterialTheme.typography.headlineSmall, color = c.text, modifier = Modifier.weight(1f))
            TextButton(onClick = { onChange(Look()) }, enabled = look != Look()) { Text("Reset") }
        }

        GroupLabel("Accent")
        AccentSwatches(spec.accent, Hideout.isDark) { a -> spec { copy(accent = a) } }

        GroupLabel("Background")
        BackgroundChips(spec.background, Hideout.isDark) { b -> spec { copy(background = b) } }

        GroupLabel("Appearance")
        SegmentedChoice(
            AppearanceChoice.entries, spec.appearance, { it.displayName },
            { when (it) { AppearanceChoice.System -> Sym.BrightnessAuto; AppearanceChoice.Light -> Sym.LightMode; AppearanceChoice.Dark -> Sym.DarkMode } },
        ) { a -> spec { copy(appearance = a) } }

        ToggleRow("Tinted surfaces", "A faint accent hue in surfaces and borders", spec.tint) { v -> spec { copy(tint = v) } }
        ToggleRow("High contrast", "Stronger borders and text, 7:1 where it counts", spec.contrast == ContrastLevel.High) { v ->
            spec { copy(contrast = if (v) ContrastLevel.High else ContrastLevel.Standard) }
        }

        GroupLabel("Density")
        SegmentedChoice(Density.entries, spec.density, { it.displayName }, { if (it == Density.Comfortable) Sym.DensityMedium else Sym.Menu }) { d ->
            spec { copy(density = d) }
        }

        GroupLabel("Text size")
        TextSizeSlider(spec.textSize) { t -> spec { copy(textSize = t) } }

        GroupLabel("Reduce motion")
        SegmentedChoice(ReduceMotion.entries, look.reduceMotion, { it.displayName }, null) { m -> onChange(look.copy(reduceMotion = m)) }
    }
}

@Composable
private fun AccentSwatches(selected: AccentPreset, dark: Boolean, onPick: (AccentPreset) -> Unit) {
    val c = Hideout.colors
    FlowRow(
        Modifier.fillMaxWidth().padding(horizontal = 20.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        AccentPreset.entries.forEach { preset ->
            val colors = remember(preset, dark) { Theme(ThemeSpec(accent = preset), dark).colors }
            val isSelected = preset == selected
            Box(
                Modifier.size(56.dp)
                    .clip(CircleShape)
                    .then(if (isSelected) Modifier.border(2.dp, c.text, CircleShape) else Modifier)
                    .semantics { this.selected = isSelected; this.role = Role.RadioButton; contentDescription = preset.displayName }
                    .clickable { onPick(preset) },
                contentAlignment = Alignment.Center,
            ) {
                Box(Modifier.size(44.dp).clip(CircleShape).background(colors.accent), contentAlignment = Alignment.Center) {
                    if (isSelected) SymIcon(Sym.Check, null, tint = colors.accentFg)
                }
            }
        }
    }
}

@Composable
private fun BackgroundChips(selected: BackgroundPreset, dark: Boolean, onPick: (BackgroundPreset) -> Unit) {
    val c = Hideout.colors
    FlowRow(
        Modifier.fillMaxWidth().padding(horizontal = 20.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        BackgroundPreset.entries.forEach { preset ->
            val colors = remember(preset, dark) { Theme(ThemeSpec(background = preset), dark).colors }
            val isSelected = preset == selected
            Row(
                Modifier
                    .clip(RoundedCornerShape(Radius.lg))
                    .background(if (isSelected) c.accentSubtle else c.surfaceRaised)
                    .semantics { this.selected = isSelected; this.role = Role.RadioButton }
                    .clickable { onPick(preset) }
                    .padding(start = 8.dp, end = 14.dp, top = 8.dp, bottom = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                Box(Modifier.size(28.dp).clip(RoundedCornerShape(Radius.md)).background(colors.bg).border(1.dp, colors.border, RoundedCornerShape(Radius.md))) { Box(Modifier.align(Alignment.BottomEnd).size(14.dp).background(colors.surfaceRaised, RoundedCornerShape(topStart = Radius.sm))) }
                Text(
                    preset.displayName, style = MaterialTheme.typography.labelLarge,
                    color = if (isSelected) c.accentText else c.text,
                    fontWeight = if (isSelected) FontWeight(650) else null,
                )
            }
        }
    }
}

/**
 * One choice of a few as a connected group of toggle buttons (the Expressive answer to segmented
 * buttons: the selected one rounds into a pill). Each option may carry an icon.
 */
@Composable
private fun <T> SegmentedChoice(options: List<T>, selected: T, label: (T) -> String, icon: ((T) -> Sym)?, onPick: (T) -> Unit) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 20.dp),
        horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween),
    ) {
        options.forEachIndexed { i, option ->
            val shapes = when (i) {
                0 -> ButtonGroupDefaults.connectedLeadingButtonShapes()
                options.lastIndex -> ButtonGroupDefaults.connectedTrailingButtonShapes()
                else -> ButtonGroupDefaults.connectedMiddleButtonShapes()
            }
            ToggleButton(
                checked = option == selected,
                onCheckedChange = { onPick(option) },
                modifier = Modifier.weight(1f).semantics { role = Role.RadioButton },
                shapes = shapes,
            ) {
                if (icon != null) {
                    SymIcon(icon(option), null, size = 18.dp)
                    Box(Modifier.width(6.dp))
                }
                Text(label(option), maxLines = 1)
            }
        }
    }
}

@Composable
private fun ToggleRow(title: String, subtitle: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    val c = Hideout.colors
    Row(
        Modifier.fillMaxWidth().clickable { onChange(!checked) }.padding(horizontal = 20.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge, color = c.text)
            Text(subtitle, style = MaterialTheme.typography.bodySmall, color = c.textMuted)
        }
        Box(Modifier.width(16.dp))
        Switch(checked = checked, onCheckedChange = onChange)
    }
}

@Suppress("DEPRECATION")
@Composable
private fun TextSizeSlider(selected: TextSize, onPick: (TextSize) -> Unit) {
    val c = Hideout.colors
    val sizes = TextSize.entries
    Column(Modifier.padding(horizontal = 20.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text("A", style = MaterialTheme.typography.bodySmall, color = c.textMuted)
            Slider(
                value = sizes.indexOf(selected).toFloat(),
                onValueChange = { onPick(sizes[it.toInt().coerceIn(0, sizes.lastIndex)]) },
                valueRange = 0f..sizes.lastIndex.toFloat(),
                steps = sizes.size - 2,
                modifier = Modifier.weight(1f).padding(horizontal = 12.dp),
            )
            Text("A", style = MaterialTheme.typography.titleLarge, color = c.textMuted)
        }
        Text(
            "${selected.displayName}  ·  The quick brown fox jumps over the lazy dog.",
            style = MaterialTheme.typography.bodyLarge, color = c.text,
        )
    }
}
