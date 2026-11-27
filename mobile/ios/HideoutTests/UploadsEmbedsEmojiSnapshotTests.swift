import HideoutCore
import HideoutUI
import SwiftUI
import XCTest

/// Composer attachment uploads, link embeds and custom emoji images (phase 3b-2): the tray's states,
/// an open/folded/rich embed, inline/jumbo custom emoji and the emoji picker's "This server" section.
/// References live in `__Snapshots__`; compare against desktop's `attachments-composer`/`embeds`/
/// `embeds-gallery`/`jumbo-emoji` scenes (mobile README "Snapshot tests").
@MainActor
final class UploadsEmbedsEmojiSnapshotTests: SnapshotCase {
    private func preload(_ assetNames: String...) async {
        for name in assetNames { _ = await MediaImageCache.shared.image(for: .fakeAsset(name: name)) }
    }

    /// Warms `CustomEmojiImage`'s own caches (`preloadCustomEmojiImage`): `assertColumn`'s plain
    /// `View`-rendering snapshot strategy never awaits a `.task`, so a cell shows its real picture only
    /// if this ran first.
    private func preloadEmoji(_ emojis: Emoji...) async {
        for emoji in emojis { await preloadCustomEmojiImage(emoji, base: URL(string: "https://fake.invalid")!, allowFakeAssets: true) }
    }

    private func draftFile(_ id: ID, _ name: String, _ size: UInt64, _ state: AttachUploadState, preview: String? = nil) -> DraftFile {
        DraftFile(id: id, name: name, contentType: "image/jpeg", size: size, preview: preview, fileURL: URL(fileURLWithPath: "/dev/null"), state: state)
    }

    /// Every tray state at once (ATT-09): queued, uploading, ready and failed, plus the too-many-files note.
    func testAttachTrayStates() async {
        let previewURL = FakeMedia.url(forAssetNamed: "square-1.jpg")!.absoluteString
        let draft = AttachDraft(files: [
            draftFile(1, "queued.jpg", 500_000, AttachUploadState(phase: .queued), preview: previewURL),
            draftFile(2, "uploading.jpg", 1_000_000, AttachUploadState(phase: .uploading, sent: 400_000, total: 1_000_000), preview: previewURL),
            draftFile(3, "ready.jpg", 420_000, AttachUploadState(phase: .ready, sent: 420_000, total: 420_000), preview: previewURL),
            draftFile(4, "failed.pdf", 2_000_000, AttachUploadState(phase: .failed, reason: "Too large")),
        ], note: "You can attach up to 10 files to a message")
        let (store, _) = await makeFakeStore()
        assertColumn(AttachTray(draft: draft, onRemove: { _ in }, onRetry: { _ in }).padding(), store: store, styles: [("dark", .dark), ("light", .light)])
    }

    /// A plain link embed, open (ATT-06/07).
    func testEmbedLinkOpen() async {
        await preload("landscape-3.jpg")
        let embed = Embed(url: "https://example.com/wiki/release", title: "Release notes: v0.0.3",
                           description: "Adds a live gateway, drops polling in favor of push updates, and tightens the reconnect backoff.",
                           siteName: "example wiki", thumbnail: "fakeasset:landscape-3.jpg", thumbnailWidth: 1200, thumbnailHeight: 800, kind: .link)
        let (store, _) = await makeFakeStore()
        assertColumn(EmbedCard(embed: embed, collapsed: false, onToggleCollapsed: {}).padding(), store: store, styles: [("dark", .dark), ("light", .light)])
    }

    /// The same embed, folded: just the rail, site and title in the header.
    func testEmbedFolded() async {
        let embed = Embed(url: "https://example.com/wiki/release", title: "Release notes: v0.0.3", siteName: "example wiki", kind: .link)
        let (store, _) = await makeFakeStore()
        assertColumn(EmbedCard(embed: embed, collapsed: true, onToggleCollapsed: {}).padding(), store: store, styles: [("dark", .dark)])
    }

    /// A rich post embed (X): author, text, media, published time, "Open original".
    func testEmbedRichPost() async {
        await preload("landscape-4.jpg", "square-1.jpg")
        let embed = Embed(url: "https://x.example.com/cvetka/status/1",
                           description: "Shipped the new icon set today.", kind: .rich, provider: "x", authorName: "cvetka", authorIcon: "fakeasset:square-1.jpg",
                           media: [EmbedMedia(kind: .image, file: "fakeasset:landscape-4.jpg", width: 1200, height: 800)], publishedAt: snapshotNow)
        let (store, _) = await makeFakeStore()
        assertColumn(EmbedCard(embed: embed, collapsed: false, onToggleCollapsed: {}).padding(), store: store, styles: [("dark", .dark), ("light", .light)])
    }

    // A snapshot of the whole #design transcript (all 19 seeded messages: attachments, embeds and
    // custom emoji together) is not here: this machine's offscreen `drawHierarchyInKeyWindow` capture
    // of that specific screen renders blank (confirmed reproducible at wait up to 5 s, and independent
    // of the animated emoji's `UIViewRepresentable`), the same known fragility the README already
    // flags for Liquid Glass screens, just triggered here by this channel's accumulated complexity
    // rather than glass. The real, live app renders it correctly (confirmed with Maestro, screenshots
    // reviewed by hand) and every piece has its own focused, reliable snapshot above/below; #design's
    // seed data and layout maths are also unit-tested directly (`FakeBackendEmbedsEmojiTests`,
    // `EmbedLayoutTests`).

    /// Inline custom emoji mixed with text (MD-06): a static and an animated one, same line as text.
    func testInlineCustomEmoji() async {
        let thumbsUp = Emoji(id: FakeBackend.Ids.emojiThumbsUp, guildID: FakeBackend.Ids.hideout, name: "thumbs_up", image: "fakeasset:thumbs_up.webp")
        let wave = Emoji(id: FakeBackend.Ids.emojiWave, guildID: FakeBackend.Ids.hideout, name: "wave", image: "fakeasset:wave.webp")
        // Inline (unlike jumbo/the picker) resolves its image through `MediaImageCache` directly
        // (`MessageBody.cachedEmojiImage`, never `CustomEmojiImage`): it needs its own warm-up.
        await preload("thumbs_up.webp", "wave.webp")
        let (store, _) = await makeFakeStore()
        let emojis: [ID: Emoji] = [FakeBackend.Ids.emojiThumbsUp: thumbsUp, FakeBackend.Ids.emojiWave: wave]
        let text = "nice work team <:thumbs_up:\(FakeBackend.Ids.emojiThumbsUp)> see you at standup <:wave:\(FakeBackend.Ids.emojiWave)>"
        assertColumn(MessageBody(text, customEmojis: emojis).padding(), store: store, styles: [("dark", .dark), ("light", .light)])
    }

    /// A jumbo message of only custom emoji (MD-07): drawn large, as real animatable views.
    func testJumboCustomEmoji() async {
        let emoji = Emoji(id: FakeBackend.Ids.emojiPartyParrot, guildID: FakeBackend.Ids.hideout, name: "party_parrot", animated: true, image: "fakeasset:party_parrot.gif")
        await preloadEmoji(emoji)
        let (store, _) = await makeFakeStore()
        let text = "<a:party_parrot:\(FakeBackend.Ids.emojiPartyParrot)> <a:party_parrot:\(FakeBackend.Ids.emojiPartyParrot)> <a:party_parrot:\(FakeBackend.Ids.emojiPartyParrot)>"
        assertColumn(MessageBody(text, customEmojis: [FakeBackend.Ids.emojiPartyParrot: emoji]).padding(), store: store, styles: [("dark", .dark)])
    }

    /// The emoji picker's "This server" section (MSG-09): real pictures, not `:name:` placeholders.
    func testEmojiPickerServerSection() async {
        let emojis = [
            Emoji(id: FakeBackend.Ids.emojiWave, guildID: FakeBackend.Ids.hideout, name: "wave", image: "fakeasset:wave.webp"),
            Emoji(id: FakeBackend.Ids.emojiThumbsUp, guildID: FakeBackend.Ids.hideout, name: "thumbs_up", image: "fakeasset:thumbs_up.webp"),
            Emoji(id: FakeBackend.Ids.emojiPartyParrot, guildID: FakeBackend.Ids.hideout, name: "party_parrot", animated: true, image: "fakeasset:party_parrot.gif"),
        ]
        for emoji in emojis { await preloadCustomEmojiImage(emoji, base: URL(string: "https://fake.invalid")!, allowFakeAssets: true) }
        let (store, _) = await makeFakeStore()
        assertScreen(EmojiPickerSheet(serverEmojis: emojis, onPick: { _ in }), store: store, styles: [("dark", .dark), ("light", .light)])
    }
}
