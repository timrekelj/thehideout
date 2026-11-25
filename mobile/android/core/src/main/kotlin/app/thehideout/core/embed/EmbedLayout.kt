package app.thehideout.core.embed

/**
 * Link-embed layout (ATT-06/07), a Kotlin port of desktop's `components::chat::embed`: which
 * provider a card is, the colour of its left rail, whether a plain link's thumbnail sits beside
 * the text or below it, and which embeds of the transcript are folded.
 */
enum class EmbedProvider(val label: String, val paletteKey: String?) {
    Link("Link", null),
    X("X", "slate"),
    TikTok("TikTok", "cyan"),
    Instagram("Instagram", "pink"),
    Reddit("Reddit", "orange");

    /** Whether this provider draws the rich post body (author, text, media, "Open original") rather than a plain link card. */
    val isPost: Boolean get() = this != Link

    companion object {
        /** From the wire's provider key ("x", "tiktok", "instagram", "reddit"); anything else is a plain link. */
        fun fromKey(key: String): EmbedProvider = when (key) {
            "x" -> X
            "tiktok" -> TikTok
            "instagram" -> Instagram
            "reddit" -> Reddit
            else -> Link
        }
    }
}

/** Widest an embed card grows, in dp. */
const val EMBED_MAX_WIDTH_DP = 440f
/** Lines of description a link card shows before it truncates. */
const val EMBED_DESCRIPTION_LINES = 3
/** Side of a plain link's thumbnail when it sits beside the text, in dp. */
const val EMBED_SIDE_THUMB_DP = 96f
/** Widest/tallest a plain link's thumbnail grows when it sits below the text, in dp. */
const val EMBED_BELOW_THUMB_MAX_W = 400f
const val EMBED_BELOW_THUMB_MAX_H = 225f

/**
 * Whether a plain link's thumbnail sits beside its text (a small square) instead of below it (a
 * wide picture): true for a roughly landscape-or-wider image (aspect < 1.4), same rule as
 * desktop's `embed.rs`. Unknown or missing dimensions default to below.
 */
fun embedThumbnailSide(width: Int?, height: Int?): Boolean =
    width != null && height != null && height > 0 && width.toFloat() / height.toFloat() < 1.4f

/** One embed of one message, identified the way the folded set remembers it. */
data class EmbedRef(val messageId: Long, val index: Int)

/** How many folded embeds the app remembers at once (desktop's `EMBED_PREFS_CAP`). */
const val EMBED_COLLAPSE_CAP = 400

/** Whether [ref] is folded in [collapsed]. */
fun isEmbedCollapsed(collapsed: List<EmbedRef>, ref: EmbedRef): Boolean = ref in collapsed

/**
 * Flips [ref]'s folded state: unfolds it if it was folded, else folds it and remembers it last
 * (oldest dropped once there are more than [EMBED_COLLAPSE_CAP]), matching desktop's bounded,
 * oldest-first `collapsed_embeds` list.
 */
fun toggleEmbedCollapsed(collapsed: List<EmbedRef>, ref: EmbedRef, cap: Int = EMBED_COLLAPSE_CAP): List<EmbedRef> =
    if (ref in collapsed) {
        collapsed - ref
    } else {
        (collapsed + ref).let { if (it.size > cap) it.subList(it.size - cap, it.size) else it }
    }
