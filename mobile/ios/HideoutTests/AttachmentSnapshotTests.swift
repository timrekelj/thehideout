import HideoutCore
import HideoutUI
import SwiftUI
import XCTest

/// Attachment rendering (ATT-01..05, phase 3b-1): every grid layout, the video/audio/file cards, the
/// removed placeholder and the full-screen viewer. References live in `__Snapshots__`; compare against
/// desktop's `attachments-images`/`lightbox` scenes (mobile README "Snapshot tests").
@MainActor
final class AttachmentSnapshotTests: SnapshotCase {
    private func image(_ id: ID, _ name: String, _ asset: String, _ contentType: String, _ size: UInt64, _ w: Int, _ h: Int) -> Attachment {
        Attachment(id: id, filename: name, contentType: contentType, size: size, file: "fakeasset:\(asset)", width: w, height: h)
    }

    /// Warms `MediaImageCache` for the bundled assets a test is about to show, synchronously (awaited
    /// before the view is ever created): `AttachmentAsyncImage` reads an already-cached picture at
    /// creation (see its init), so the snapshot shows the real sample picture rather than its loading
    /// placeholder, with no dependence on whether `.task` gets to run in this offscreen render pass.
    private func preload(_ assetNames: String...) async {
        for name in assetNames { _ = await MediaImageCache.shared.image(for: .fakeAsset(name: name)) }
    }

    /// The #design channel, seeded with one message per attachment kind (`FakeBackend.seedAttachments`):
    /// image grids of every size, a video, an audio note, a file and a removed placeholder, in one screen.
    func testDesignChannelTranscript() async {
        await preload("square-1.jpg", "landscape-3.jpg", "landscape-1.jpg", "portrait-1.jpg", "landscape-2.jpg", "landscape-4.jpg", "landscape-5.jpg", "clip-1-poster.jpg")
        let (store, _) = await makeFakeStore(open: FakeBackend.Ids.design)
        assertScreen(NavigationStack { TranscriptScreenForTests(channel: FakeBackend.Ids.design) }, store: store)
        assertScreen(NavigationStack { TranscriptScreenForTests(channel: FakeBackend.Ids.design) }, store: store, device: .iPad, styles: [("dark", .dark)])
    }

    func testImageGridOne() async {
        await preload("landscape-1.jpg")
        let (store, _) = await makeFakeStore()
        let atts = [image(1, "a.jpg", "landscape-1.jpg", "image/jpeg", 840_000, 1200, 800)]
        assertScreen(MessageAttachmentsView(attachments: atts).padding(), store: store, styles: [("dark", .dark)])
    }

    func testImageGridTwo() async {
        await preload("landscape-1.jpg", "portrait-1.jpg")
        let (store, _) = await makeFakeStore()
        let atts = [
            image(1, "a.jpg", "landscape-1.jpg", "image/jpeg", 840_000, 1200, 800),
            image(2, "b.jpg", "portrait-1.jpg", "image/jpeg", 910_000, 900, 1200),
        ]
        assertScreen(MessageAttachmentsView(attachments: atts).padding(), store: store, styles: [("dark", .dark)])
    }

    func testImageGridThree() async {
        await preload("landscape-1.jpg", "portrait-1.jpg", "landscape-2.jpg")
        let (store, _) = await makeFakeStore()
        let atts = [
            image(1, "a.jpg", "landscape-1.jpg", "image/jpeg", 840_000, 1200, 800),
            image(2, "b.jpg", "portrait-1.jpg", "image/jpeg", 910_000, 900, 1200),
            image(3, "c.jpg", "landscape-2.jpg", "image/jpeg", 1_200_000, 1200, 800),
        ]
        assertScreen(MessageAttachmentsView(attachments: atts).padding(), store: store, styles: [("dark", .dark)])
    }

    /// Four or more lay out as a 2x2, the last tile showing "+N" for a fifth and beyond.
    func testImageGridFourPlus() async {
        await preload("landscape-4.jpg", "landscape-5.jpg", "portrait-1.jpg", "square-1.jpg", "landscape-1.jpg")
        let (store, _) = await makeFakeStore()
        let atts = [
            image(1, "a.jpg", "landscape-4.jpg", "image/jpeg", 300_000, 1200, 800),
            image(2, "b.jpg", "landscape-5.jpg", "image/jpeg", 300_000, 1200, 800),
            image(3, "c.jpg", "portrait-1.jpg", "image/jpeg", 300_000, 900, 1200),
            image(4, "d.jpg", "square-1.jpg", "image/jpeg", 300_000, 1000, 1000),
            image(5, "e.jpg", "landscape-1.jpg", "image/jpeg", 300_000, 1200, 800),
        ]
        assertScreen(MessageAttachmentsView(attachments: atts).padding(), store: store, styles: [("dark", .dark)])
    }

    func testVideoCard() async {
        await preload("clip-1-poster.jpg")
        let (store, _) = await makeFakeStore()
        var att = image(1, "demo.mp4", "clip-1.mp4", "video/mp4", 590_746, 960, 540)
        att.thumbnail = "fakeasset:clip-1-poster.jpg"
        assertScreen(MessageAttachmentsView(attachments: [att]).padding(), store: store, styles: [("dark", .dark)])
    }

    func testAudioRow() async {
        let (store, _) = await makeFakeStore()
        let atts = [image(1, "voice-note.m4a", "tone-1.m4a", "audio/mp4", 98_808, 0, 0)]
        assertScreen(MessageAttachmentsView(attachments: atts).padding(), store: store, styles: [("dark", .dark)])
    }

    func testFileCard() async {
        let (store, _) = await makeFakeStore()
        let atts = [image(1, "meeting-notes.txt", "notes-1.txt", "text/plain", 218, 0, 0)]
        assertScreen(MessageAttachmentsView(attachments: atts).padding(), store: store, styles: [("dark", .dark)])
    }

    func testRemovedPlaceholder() async {
        let (store, _) = await makeFakeStore()
        let atts = [Attachment(id: 1, filename: "invoice.xlsx", contentType: "application/vnd.ms-excel", size: 88_000, file: "", removed: true)]
        assertScreen(MessageAttachmentsView(attachments: atts).padding(), store: store, styles: [("dark", .dark)])
    }

    /// The full-screen viewer (ATT-05): a message's three pictures, opened on the second.
    func testViewer() async {
        await preload("landscape-1.jpg", "portrait-1.jpg", "landscape-2.jpg")
        let (store, _) = await makeFakeStore()
        let items = [
            image(1, "saturday-1.jpg", "landscape-1.jpg", "image/jpeg", 840_000, 1200, 800),
            image(2, "saturday-2.jpg", "portrait-1.jpg", "image/jpeg", 910_000, 900, 1200),
            image(3, "saturday-3.jpg", "landscape-2.jpg", "image/jpeg", 1_200_000, 1200, 800),
        ].map { LightboxItem(attachment: $0) }
        assertScreen(MediaViewerView(request: LightboxRequest(items: items, startIndex: 1), onDismiss: {}), store: store, styles: [("dark", .dark)])
    }

    /// A video page in the viewer: no zoom, native `AVPlayer` controls.
    func testViewerVideo() async {
        let (store, _) = await makeFakeStore()
        var att = image(1, "demo.mp4", "clip-1.mp4", "video/mp4", 590_746, 960, 540)
        att.thumbnail = "fakeasset:clip-1-poster.jpg"
        let items = [LightboxItem(attachment: att)]
        assertScreen(MediaViewerView(request: LightboxRequest(items: items, startIndex: 0), onDismiss: {}), store: store, styles: [("dark", .dark)], wait: 2.0)
    }
}
