package app.thehideout.core.invite

/**
 * An invite code (`XXXX-XXXX`), the Kotlin twin of desktop's `InviteCodeInput`
 * (`client/design/src/components/invite_code_input.rs`): invites now carry only the 8-character
 * code, never a token, and a pasted link (`.../invite/XXXX-XXXX`) yields just its code.
 */
object InviteCode {
    /** Characters in a code. */
    const val LEN = 8
    /** Characters before the dash. */
    const val GROUP = 4
    /** The characters a code may hold (Crockford base32: no I, L, O or U). */
    const val ALPHABET = "0123456789ABCDEFGHJKMNPQRSTVWXYZ"

    /** One typed or pasted character as a code character: uppercased, I/L read as 1, O as 0, `null` when the alphabet has no such character (including `-`, spaces and `_`). */
    fun codeChar(c: Char): Char? {
        val up = when (val u = c.uppercaseChar()) {
            'I', 'L' -> '1'
            'O' -> '0'
            else -> u
        }
        return up.takeIf { ALPHABET.contains(it) }
    }

    /** The code characters in [s], in order, at most [LEN]. */
    private fun filtered(s: String): List<Char> = s.mapNotNull(::codeChar).take(LEN)

    /**
     * The field's whole text as it should be stored: at most [LEN] characters from the server's
     * alphabet, no dash. A pasted link anywhere in [raw] counts only from after its last
     * `/invite/`.
     */
    fun normalize(raw: String): String {
        val afterLink = raw.substringAfterLast("/invite/", raw)
        return filtered(afterLink).joinToString("")
    }

    /** `XXXX-XXXX` once [normalized] is complete; otherwise unchanged (no trailing dash while typing). */
    fun format(normalized: String): String = if (normalized.length == LEN) "${normalized.take(GROUP)}-${normalized.drop(GROUP)}" else normalized

    /** Whether [raw] looks like it holds a link rather than a bare code (shown while nothing has been typed yet). */
    fun looksLikeLink(raw: String): Boolean = raw.contains("://") || raw.contains("/invite/")

    /** Whether [normalized] (no dash) is a full code. */
    fun isComplete(normalized: String): Boolean = normalized.length == LEN
}
