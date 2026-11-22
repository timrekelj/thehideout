package app.thehideout.ui.theme

import androidx.compose.material3.Shapes
import androidx.compose.material3.Typography
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontVariation
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import app.thehideout.design.DesignFonts
import app.thehideout.design.Radius
import app.thehideout.design.TypeRamp
import androidx.compose.foundation.shape.RoundedCornerShape

/** Sets [DesignFonts] to the bundled variable fonts: Instrument Sans at the exact weights the type scale asks for. */
internal object Fonts {
    private val weights = listOf(400, 440, 450, 500, 530, 600, 650, 700)

    fun install() {
        DesignFonts.ui = FontFamily(
            weights.map { w ->
                Font(
                    resId = app.thehideout.design.R.font.instrument_sans_vf,
                    weight = FontWeight(w),
                    variationSettings = FontVariation.Settings(FontVariation.weight(w)),
                )
            },
        )
        DesignFonts.mono = FontFamily(
            Font(app.thehideout.design.R.font.jetbrains_mono_regular, FontWeight.Normal),
            Font(app.thehideout.design.R.font.jetbrains_mono_bold, FontWeight.Bold),
        )
    }
}

/** Crisp corners are the desktop language: radii 2 to 10 dp; sheets and dialogs get the larger ones. */
internal val hideoutShapes = Shapes(
    extraSmall = RoundedCornerShape(Radius.xs),
    small = RoundedCornerShape(Radius.sm),
    medium = RoundedCornerShape(Radius.md),
    large = RoundedCornerShape(Radius.lg),
    largeIncreased = RoundedCornerShape(Radius.xl),
    extraLarge = RoundedCornerShape(Radius.xl),
    extraLargeIncreased = RoundedCornerShape(14.dp),
    extraExtraLarge = RoundedCornerShape(20.dp),
)

private fun TextStyle.scaled(f: Float): TextStyle = copy(
    fontSize = TextUnit(fontSize.value * f, fontSize.type),
    lineHeight = TextUnit(lineHeight.value * f, lineHeight.type),
)

/**
 * The type ramp as a Material [Typography]: `bodyLarge` = body, `labelLarge` = control (the
 * medium-weight one, so buttons read well), `titleMedium` = title, `headlineSmall` = heading,
 * `displaySmall` = display. The in-between sizes are the neighbouring token scaled.
 */
internal fun hideoutTypography(r: TypeRamp): Typography {
    val body = r.body.toTextStyle()
    val bodyStrong = r.bodyStrong.toTextStyle()
    val title = r.title.toTextStyle()
    val heading = r.heading.toTextStyle()
    val display = r.display.toTextStyle()
    val control = r.controlStrong.toTextStyle()
    val label = r.label.toTextStyle()
    val caption = r.caption.toTextStyle()
    return Typography(
        displayLarge = display.scaled(1.75f), displayMedium = display.scaled(1.4f), displaySmall = display,
        headlineLarge = heading.scaled(1.4f), headlineMedium = heading.scaled(1.2f), headlineSmall = heading,
        titleLarge = heading.scaled(0.95f), titleMedium = title, titleSmall = bodyStrong,
        bodyLarge = body, bodyMedium = body.scaled(0.95f), bodySmall = caption,
        labelLarge = control, labelMedium = label, labelSmall = caption,
        displayLargeEmphasized = display.scaled(1.75f), displayMediumEmphasized = display.scaled(1.4f), displaySmallEmphasized = display,
        headlineLargeEmphasized = heading.scaled(1.4f), headlineMediumEmphasized = heading.scaled(1.2f), headlineSmallEmphasized = heading,
        titleLargeEmphasized = heading.scaled(0.95f), titleMediumEmphasized = title.copy(fontWeight = FontWeight(650)), titleSmallEmphasized = bodyStrong,
        bodyLargeEmphasized = bodyStrong, bodyMediumEmphasized = bodyStrong.scaled(0.95f), bodySmallEmphasized = caption.copy(fontWeight = FontWeight(600)),
        labelLargeEmphasized = control.copy(fontWeight = FontWeight(650)), labelMediumEmphasized = label.copy(fontWeight = FontWeight(600)), labelSmallEmphasized = caption.copy(fontWeight = FontWeight(600)),
    )
}
