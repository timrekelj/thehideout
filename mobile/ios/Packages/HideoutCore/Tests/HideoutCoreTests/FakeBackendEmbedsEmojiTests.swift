import Testing
@testable import HideoutCore

/// `FakeBackend`'s custom emoji and embed seeds (MSG-09, ATT-06): bundled assets, a jumbo message, one
/// embed per kind/provider. A Swift port of Android's `FakeBackendEmbedsEmojiTest.kt`.
@Suite("FakeBackend: embed and emoji seeds")
struct FakeBackendEmbedsEmojiTests {
    @Test func theHideoutHasThreeCustomEmojiOneAnimatedKeyedToBundledAssets() async throws {
        let fake = FakeBackend()
        let emojis = try await fake.listEmojis(guild: Ids.hideout)
        #expect(emojis.count == 3)
        #expect(emojis.allSatisfy { $0.image.hasPrefix(fakeAssetPrefix) })
        #expect(emojis.filter(\.animated).count == 1)
        #expect(Set(emojis.map(\.name)) == ["wave", "thumbs_up", "party_parrot"])
    }

    @Test func designHasAnInlineCustomEmojiMessageAndAJumboOnlyOne() async throws {
        let fake = FakeBackend()
        let page = try await fake.listMessages(channel: Ids.design, before: nil, limit: 50)
        let inline = try #require(page.messages.first { $0.content.contains("<:thumbs_up:") })
        #expect(Jumbo.inlines(of: Markdown.parse(inline.content)) == nil, "mixed text and emoji is not jumbo")

        let jumboMessage = try #require(page.messages.first { $0.content.contains("<a:party_parrot:") })
        // `Jumbo.inlines` returns the raw parsed inline list (not a whitespace-stripped emoji-only
        // one, unlike Android's `jumboEmojis`): three custom-emoji tokens with plain-space runs between.
        let jumbo = try #require(Jumbo.inlines(of: Markdown.parse(jumboMessage.content)))
        let customEmoji = jumbo.filter { if case .customEmoji = $0 { true } else { false } }
        #expect(customEmoji.count == 3)
        #expect(customEmoji.allSatisfy { if case .customEmoji(_, _, let animated) = $0 { return animated } else { return false } })
    }

    @Test func designHasALinkAnImageAVideoAndOneRichEmbedPerProvider() async throws {
        let fake = FakeBackend()
        let page = try await fake.listMessages(channel: Ids.design, before: nil, limit: 50)
        let embeds = page.messages.flatMap(\.embeds)
        #expect(embeds.count == 7)
        #expect(embeds.filter { $0.kind == .link }.count == 1)
        #expect(embeds.filter { $0.kind == .image }.count == 1)
        #expect(embeds.filter { $0.kind == .video }.count == 1)
        #expect(Set(embeds.filter { $0.kind == .rich }.map(\.provider)) == ["x", "tiktok", "instagram", "reddit"])
    }
}
