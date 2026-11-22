package app.thehideout.ui.components

import androidx.compose.runtime.compositionLocalOf
import androidx.compose.ui.graphics.Color

/** `{server}/media/` (or the fake backend's asset base), used to build attachment URLs. Set once near the root. */
val LocalMediaBase = compositionLocalOf { "" }

/**
 * Ink and fills over pictures and video: the same in both light and dark appearance (desktop's
 * `MEDIA_INK` / `MEDIA_BACK` / `MEDIA_DIM`), since a photo sits on its own dark surface either way.
 */
object MediaColors {
    val ink = Color(0xFFFBF8F5)
    val back = Color(0xFF34302E)
    val dim = Color(0xFF211E1C).copy(alpha = 0.66f)
}
