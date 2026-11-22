package app.thehideout.ui.theme

import android.provider.Settings
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.spring
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialExpressiveTheme
import androidx.compose.material3.MotionScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalInspectionMode
import app.thehideout.design.*

/** The tokens of the current look, for components that need more than the Material roles. */
object Hideout {
    val colors: ThemeColors @Composable get() = LocalHideoutColors.current
    val metrics: Metrics @Composable get() = LocalTheme.current.metrics
    val type: TypeRamp @Composable get() = LocalTheme.current.type
    val isDark: Boolean @Composable get() = LocalTheme.current.isDark
    val spec: ThemeSpec @Composable get() = LocalTheme.current.spec
    val reduceMotion: Boolean @Composable get() = LocalReduceMotion.current
}

internal val LocalHideoutColors = compositionLocalOf { Theme(ThemeSpec(), true).colors }
internal val LocalReduceMotion = staticCompositionLocalOf { false }

/**
 * The warm look on Material 3 Expressive: resolves the persisted look plus the system dark mode to
 * a [Theme], hands the Material roles to [MaterialExpressiveTheme] (no dynamic colour: the look is
 * the product) and publishes the full token palette, metrics and type ramp as composition locals.
 * Colours cross-fade when the look changes, unless reduce motion is on.
 */
@Composable
fun HideoutTheme(
    look: Look = Look(),
    systemDark: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    // The variable fonts are bundled in :design; the token ramp asks for them by family.
    remember { Fonts.install(); true }

    val theme = remember(look.spec, systemDark) { Theme(look.spec, systemDark) }
    val reduce = when (look.reduceMotion) {
        ReduceMotion.On -> true
        ReduceMotion.Off -> false
        ReduceMotion.System -> systemReducesMotion()
    }
    val colors = animatedColors(theme.colors, reduce)
    val scheme: ColorScheme = remember(colors, theme.isDark) { colors.toColorScheme(theme.isDark) }
    val typography = remember(theme.type) { hideoutTypography(theme.type) }
    CompositionLocalProvider(
        LocalTheme provides theme,
        LocalHideoutColors provides colors,
        LocalReduceMotion provides reduce,
    ) {
        MaterialExpressiveTheme(
            colorScheme = scheme,
            motionScheme = MotionScheme.expressive(),
            shapes = hideoutShapes,
            typography = typography,
            content = content,
        )
    }
}

/** Animator scale 0 is how Android says "reduce motion" (the same signal the system's Remove animations sets). */
@Composable
private fun systemReducesMotion(): Boolean {
    if (LocalInspectionMode.current) return false
    val resolver = LocalContext.current.contentResolver
    return remember(resolver) {
        runCatching { Settings.Global.getFloat(resolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f) == 0f }.getOrDefault(false)
    }
}

/** [target], eased from the colours that were on screen when it changed (Motion.reflow's spring). */
@Composable
private fun animatedColors(target: ThemeColors, reduceMotion: Boolean): ThemeColors {
    var from by remember { mutableStateOf0(target) }
    var shown by remember { mutableStateOf0(target) }
    var last by remember { mutableStateOf0(target) }
    var t by remember { mutableFloatStateOf(1f) }
    if (target != last) {
        // The look changed during composition: start from what is on screen now.
        from = shown
        last = target
        if (reduceMotion) { from = target; t = 1f } else t = 0f
    }
    val anim = remember { Animatable(1f) }
    LaunchedEffect(last) {
        if (t == 1f) { anim.snapTo(1f); return@LaunchedEffect }
        anim.snapTo(0f)
        anim.animateTo(1f, spring(dampingRatio = 1f, stiffness = 120f)) { t = value }
        t = 1f
    }
    val current = if (t >= 1f) last else lerp(from, last, t)
    shown = current
    return current
}

private fun <T> mutableStateOf0(v: T) = androidx.compose.runtime.mutableStateOf(v)

internal fun lerp(a: ThemeColors, b: ThemeColors, t: Float): ThemeColors {
    fun l(x: Color, y: Color) = lerp(x, y, t)
    return a.copy(
        bg = l(a.bg, b.bg), surface = l(a.surface, b.surface), surfaceRaised = l(a.surfaceRaised, b.surfaceRaised),
        surfaceOverlay = l(a.surfaceOverlay, b.surfaceOverlay), borderSubtle = l(a.borderSubtle, b.borderSubtle),
        border = l(a.border, b.border), borderStrong = l(a.borderStrong, b.borderStrong), controlBorder = l(a.controlBorder, b.controlBorder),
        text = l(a.text, b.text), textMuted = l(a.textMuted, b.textMuted), textFaint = l(a.textFaint, b.textFaint),
        accent = l(a.accent, b.accent), accentFg = l(a.accentFg, b.accentFg), accentText = l(a.accentText, b.accentText),
        accentSubtle = l(a.accentSubtle, b.accentSubtle), accentWash = l(a.accentWash, b.accentWash),
        success = l(a.success, b.success), successBg = l(a.successBg, b.successBg), successBorder = l(a.successBorder, b.successBorder),
        warning = l(a.warning, b.warning), warningBg = l(a.warningBg, b.warningBg), warningBorder = l(a.warningBorder, b.warningBorder),
        danger = l(a.danger, b.danger), dangerBg = l(a.dangerBg, b.dangerBg), dangerBorder = l(a.dangerBorder, b.dangerBorder),
        dangerSolid = l(a.dangerSolid, b.dangerSolid), onDanger = l(a.onDanger, b.onDanger),
        info = l(a.info, b.info), infoBg = l(a.infoBg, b.infoBg), infoBorder = l(a.infoBorder, b.infoBorder),
        focus = l(a.focus, b.focus), hover = l(a.hover, b.hover), pressed = l(a.pressed, b.pressed),
        accentHover = l(a.accentHover, b.accentHover), accentPressed = l(a.accentPressed, b.accentPressed),
        scrim = l(a.scrim, b.scrim), layer1 = l(a.layer1, b.layer1), layer2 = l(a.layer2, b.layer2), layer3 = l(a.layer3, b.layer3),
        knob = l(a.knob, b.knob), knobOn = l(a.knobOn, b.knobOn),
        channels = if (t < 0.5f) a.channels else b.channels,
    )
}
