package app.thehideout.core

import app.thehideout.core.embed.EMBED_COLLAPSE_CAP
import app.thehideout.core.embed.EmbedProvider
import app.thehideout.core.embed.EmbedRef
import app.thehideout.core.embed.embedThumbnailSide
import app.thehideout.core.embed.isEmbedCollapsed
import app.thehideout.core.embed.toggleEmbedCollapsed
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

/** Ports desktop's `embed.rs` tests: providers from the wire key, the rail palette, the thumbnail side rule, folding. */
class EmbedLayoutTest {
    @Test fun `providers come from the wire key`() {
        assertEquals(EmbedProvider.X, EmbedProvider.fromKey("x"))
        assertEquals(EmbedProvider.TikTok, EmbedProvider.fromKey("tiktok"))
        assertEquals(EmbedProvider.Instagram, EmbedProvider.fromKey("instagram"))
        assertEquals(EmbedProvider.Reddit, EmbedProvider.fromKey("reddit"))
        assertEquals(EmbedProvider.Link, EmbedProvider.fromKey(""))
        assertEquals(EmbedProvider.Link, EmbedProvider.fromKey("mastodon"))
    }

    @Test fun `only a plain link has no palette and no post body`() {
        assertEquals(null, EmbedProvider.Link.paletteKey)
        assertFalse(EmbedProvider.Link.isPost)
        for (p in listOf(EmbedProvider.X, EmbedProvider.TikTok, EmbedProvider.Instagram, EmbedProvider.Reddit)) {
            assertTrue(p.paletteKey != null, "$p has a rail colour")
            assertTrue(p.isPost, "$p is a post")
        }
    }

    @Test fun `a thumbnail sits beside the text only when it is not markedly tall`() {
        assertTrue(embedThumbnailSide(1000, 1000)) // square, aspect 1.0
        assertTrue(embedThumbnailSide(1200, 900)) // landscape, aspect 1.33
        assertFalse(embedThumbnailSide(1200, 800)) // aspect 1.5: markedly wide, so it goes below instead
        assertTrue(embedThumbnailSide(900, 1200)) // portrait is not markedly wide either, so it still sits beside
        assertFalse(embedThumbnailSide(null, 900))
        assertFalse(embedThumbnailSide(900, null))
        assertFalse(embedThumbnailSide(900, 0))
    }

    @Test fun `toggling folds and unfolds`() {
        var collapsed = emptyList<EmbedRef>()
        val ref = EmbedRef(messageId = 1, index = 0)
        assertFalse(isEmbedCollapsed(collapsed, ref))
        collapsed = toggleEmbedCollapsed(collapsed, ref)
        assertTrue(isEmbedCollapsed(collapsed, ref))
        collapsed = toggleEmbedCollapsed(collapsed, ref)
        assertFalse(isEmbedCollapsed(collapsed, ref))
    }

    @Test fun `the folded list is bounded, oldest dropped first`() {
        var collapsed = emptyList<EmbedRef>()
        for (i in 0 until EMBED_COLLAPSE_CAP + 24) {
            collapsed = toggleEmbedCollapsed(collapsed, EmbedRef(i.toLong(), 0))
        }
        assertEquals(EMBED_COLLAPSE_CAP, collapsed.size)
        assertFalse(isEmbedCollapsed(collapsed, EmbedRef(0, 0)))
        assertTrue(isEmbedCollapsed(collapsed, EmbedRef((EMBED_COLLAPSE_CAP + 24 - 1).toLong(), 0)))
    }
}
