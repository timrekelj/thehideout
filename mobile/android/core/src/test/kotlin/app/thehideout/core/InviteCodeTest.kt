package app.thehideout.core

import app.thehideout.core.invite.InviteCode
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

/** The Kotlin twin of desktop's `invite_code_input.rs` tests: same rules, same examples. */
class InviteCodeTest {
    @Test fun `typing uppercases and maps lookalikes`() {
        assertEquals("N0X6", InviteCode.normalize("n0x6"))
        assertEquals("110", InviteCode.normalize("ilo"))
    }

    @Test fun `pasted codes lose separators`() {
        assertEquals("N0X6PFNB", InviteCode.normalize("n0x6-pfnb"))
        assertEquals("N0X6PFNB", InviteCode.normalize("N0X6 PFNB"))
        assertEquals("N0X6PFNB", InviteCode.normalize("n0x6_pfnb"))
    }

    @Test fun `a link gives the part after the last invite`() {
        assertEquals("N0X6PFNB", InviteCode.normalize("https://hideout.chat/invite/N0X6-PFNB"))
        assertEquals("N0X6PFNB", InviteCode.normalize("https://x.y/invite/AAAA/invite/N0X6-PFNB"))
    }

    @Test fun `other characters are rejected and the length is capped`() {
        assertEquals("4", InviteCode.normalize("u!@#é4"))
        assertEquals("01234567", InviteCode.normalize("0123456789"))
        assertEquals("", InviteCode.normalize(""))
    }

    @Test fun `u is not in the server's alphabet (Crockford, no I L O U)`() {
        assertEquals("", InviteCode.normalize("u"))
        assertNull2(InviteCode.codeChar('U'))
        assertNull2(InviteCode.codeChar('u'))
    }

    @Test fun `format adds the dash only once a code is complete`() {
        assertEquals("WXYZ", InviteCode.format("WXYZ"))
        assertEquals("WXYZ-2345", InviteCode.format("WXYZ2345"))
    }

    @Test fun `normalizing is idempotent`() {
        val once = InviteCode.format(InviteCode.normalize("https://h.chat/invite/n0x6-pfnbzz"))
        assertEquals("N0X6-PFNB", once)
        assertEquals(once, InviteCode.format(InviteCode.normalize(once)))
    }

    @Test fun `looksLikeLink is true only for a scheme or an invite path`() {
        assertTrue(InviteCode.looksLikeLink("https://thehideout.zabit.si/invite/WXYZ-2345"))
        assertFalse(InviteCode.looksLikeLink("WXYZ-2345"))
    }

    private fun assertNull2(c: Char?) = assertEquals(null, c)
}
