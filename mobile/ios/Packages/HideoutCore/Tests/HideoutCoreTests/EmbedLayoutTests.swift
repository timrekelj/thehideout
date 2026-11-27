import Testing
@testable import HideoutCore

/// Ports desktop's `embed.rs` tests (and Android's `EmbedLayoutTest.kt` twin): providers from the wire
/// key, the rail palette, the thumbnail side rule, folding (ATT-06/07).
@Suite("Embed layout")
struct EmbedLayoutTests {
    @Test func providersComeFromTheWireKey() {
        #expect(EmbedProvider.from(key: "x") == .x)
        #expect(EmbedProvider.from(key: "tiktok") == .tiktok)
        #expect(EmbedProvider.from(key: "instagram") == .instagram)
        #expect(EmbedProvider.from(key: "reddit") == .reddit)
        #expect(EmbedProvider.from(key: "") == .link)
        #expect(EmbedProvider.from(key: "mastodon") == .link)
    }

    @Test func onlyAPlainLinkHasNoPaletteAndNoPostBody() {
        #expect(EmbedProvider.link.paletteKey == nil)
        #expect(!EmbedProvider.link.isPost)
        for p: EmbedProvider in [.x, .tiktok, .instagram, .reddit] {
            #expect(p.paletteKey != nil, "\(p) has a rail colour")
            #expect(p.isPost, "\(p) is a post")
        }
    }

    @Test func aThumbnailSitsBesideTheTextOnlyWhenItIsNotMarkedlyTall() {
        #expect(embedThumbnailSide(width: 1000, height: 1000)) // square, aspect 1.0
        #expect(embedThumbnailSide(width: 1200, height: 900)) // landscape, aspect 1.33
        #expect(!embedThumbnailSide(width: 1200, height: 800)) // aspect 1.5: markedly wide, so it goes below instead
        #expect(embedThumbnailSide(width: 900, height: 1200)) // portrait is not markedly wide either, so it still sits beside
        #expect(!embedThumbnailSide(width: nil, height: 900))
        #expect(!embedThumbnailSide(width: 900, height: nil))
        #expect(!embedThumbnailSide(width: 900, height: 0))
    }

    @Test func togglingFoldsAndUnfolds() {
        var s = AppState()
        let ref = EmbedRef(messageID: 1, index: 0)
        #expect(!s.isEmbedCollapsed(ref))
        s.toggleEmbedCollapsed(ref)
        #expect(s.isEmbedCollapsed(ref))
        s.toggleEmbedCollapsed(ref)
        #expect(!s.isEmbedCollapsed(ref))
    }

    @Test func theFoldedListIsBoundedOldestDroppedFirst() {
        var s = AppState()
        for i in 0..<(embedCollapseCap + 24) {
            s.toggleEmbedCollapsed(EmbedRef(messageID: ID(i), index: 0))
        }
        #expect(s.collapsedEmbeds.count == embedCollapseCap)
        #expect(!s.isEmbedCollapsed(EmbedRef(messageID: 0, index: 0)))
        #expect(s.isEmbedCollapsed(EmbedRef(messageID: ID(embedCollapseCap + 24 - 1), index: 0)))
    }
}
