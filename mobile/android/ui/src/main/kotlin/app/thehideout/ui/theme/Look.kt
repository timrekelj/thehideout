package app.thehideout.ui.theme

import app.thehideout.design.*
import org.json.JSONException
import org.json.JSONObject

/** Whether to follow the system's reduce-motion setting or force it. */
enum class ReduceMotion(val wire: String, val displayName: String) {
    System("system", "System"),
    On("on", "On"),
    Off("off", "Off"),
}

/**
 * Everything on the Appearance page: the design crate's [ThemeSpec] plus reduce motion. Persisted
 * as the same JSON shape desktop writes under `appearance` (unknown or missing fields fall back to
 * the defaults, so a file from another version still loads):
 * `{"accent":"sage-teal","background":"warm","appearance":"system","tint":false,"contrast":"standard","density":"comfortable","text_size":"100","reduce_motion":"system"}`
 */
data class Look(val spec: ThemeSpec = ThemeSpec(), val reduceMotion: ReduceMotion = ReduceMotion.System) {
    fun toJson(): String = JSONObject()
        .put("accent", spec.accent.wire)
        .put("background", spec.background.wire)
        .put("appearance", spec.appearance.wire)
        .put("tint", spec.tint)
        .put("contrast", spec.contrast.wire)
        .put("density", spec.density.wire)
        .put("text_size", spec.textSize.wire)
        .put("reduce_motion", reduceMotion.wire)
        .toString()

    companion object {
        /** The saved look, or the default when there is none (or it does not parse). */
        fun fromJson(json: String?): Look {
            if (json.isNullOrBlank()) return Look()
            val o = try { JSONObject(json) } catch (_: JSONException) { return Look() }
            val d = ThemeSpec()
            return Look(
                spec = ThemeSpec(
                    accent = AccentPreset.entries.firstOrNull { it.wire == o.optString("accent") } ?: d.accent,
                    background = BackgroundPreset.entries.firstOrNull { it.wire == o.optString("background") } ?: d.background,
                    appearance = AppearanceChoice.entries.firstOrNull { it.wire == o.optString("appearance") } ?: d.appearance,
                    tint = o.optBoolean("tint", d.tint),
                    contrast = ContrastLevel.entries.firstOrNull { it.wire == o.optString("contrast") } ?: d.contrast,
                    density = Density.entries.firstOrNull { it.wire == o.optString("density") } ?: d.density,
                    textSize = TextSize.entries.firstOrNull { it.wire == o.optString("text_size") } ?: d.textSize,
                ),
                reduceMotion = ReduceMotion.entries.firstOrNull { it.wire == o.optString("reduce_motion") } ?: ReduceMotion.System,
            )
        }
    }
}
