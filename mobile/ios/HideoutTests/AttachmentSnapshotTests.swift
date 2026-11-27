import HideoutCore
import HideoutUI
import SwiftUI
import XCTest

/// Attachment rendering (ATT-01..05, phase 3b-1): every grid layout, the video/audio/file cards, the
/// removed placeholder and the full-screen viewer. References live in `__Snapshots__`; compare against
/// desktop's `attachments-images`/`lightbox` scenes (mobile README "Snapshot tests").
///
/// Pictures show their sized loading placeholder, not the bundled sample image: `AttachmentAsyncImage`'s
/// `.task` reliably completes in the real app (confirmed end to end with Maestro, `attachments.yaml`) but
/// not inside `UIHostingController`'s offscreen render pass this harness uses, a SwiftUI/XCTest quirk
/// `assertScreen`'s `wait` does not work around for task-driven async content. The grid math, corner
/// radii, spacing and the "+N" overlay are still exactly what these references check.
@MainActor
final class AttachmentSnapshotTests: SnapshotCase {
    private func image(_ id: ID, _ name: String, _ asset: String, _ contentType: String, _ size: UInt64, _ w: Int, _ h: Int) -> Attachment {
        Attachment(id: id, filename: name, contentType: contentType, size: size, file: "fakeasset:\(asset)", width: w, height: h)
    }

    /// The #design channel, seeded with one message per attachment kind (`FakeBackend.seedAttachments`):
    /// image grids of every size, a video, an audio note, a file and a removed placeholder, in one screen.
    func testDesignChannelTranscript() async {
        let (store, _) = await makeFakeStore(open: FakeBackend.Ids.design)
        assertScreen(NavigationStack { TranscriptScreenForTests(channel: FakeBackend.Ids.design) }, store: store, wait: 3.0)
        assertScreen(NavigationStack { TranscriptScreenForTests(channel: FakeBackend.Ids.design) }, store: store, device: .iPad, styles: [("dark", .dark)], wait: 3.0)
    }

    func testImageGridOne() async {
        let (store, _) = await makeFakeStore()
        let atts = [image(1, "a.jpg", "landscape-1.jpg", "image/jpeg", 840_000, 1200, 800)]
        assertScreen(MessageAttachmentsView(attachments: atts).padding(), store: store, styles: [("dark", .dark)], wait: 3.0)
    }

    func testImageGridTwo() async {
        let (store, _) = await makeFakeStore()
        let atts = [
            image(1, "a.jpg", "landscape-1.jpg", "image/jpeg", 840_000, 1200, 800),
            image(2, "b.jpg", "portrait-1.jpg", "image/jpeg", 910_000, 900, 1200),
        ]
        assertScreen(MessageAttachmentsView(attachments: atts).padding(), store: store, styles: [("dark", .dark)], wait: 3.0)
    }

    func testImageGridThree() async {
        let (store, _) = await makeFakeStore()
        let atts = [
            image(1, "a.jpg", "landscape-1.jpg", "image/jpeg", 840_000, 1200, 800),
            image(2, "b.jpg", "portrait-1.jpg", "image/jpeg", 910_000, 900, 1200),
            image(3, "c.jpg", "landscape-2.jpg", "image/jpeg", 1_200_000, 1200, 800),
        ]
        assertScreen(MessageAttachmentsView(attachments: atts).padding(), store: store, styles: [("dark", .dark)], wait: 3.0)
    }

    /// Four or more lay out as a 2x2, the last tile showing "+N" for a fifth and beyond.
    func testImageGridFourPlus() async {
        let (store, _) = await makeFakeStore()
        let atts = [
            image(1, "a.jpg", "landscape-4.jpg", "image/jpeg", 300_000, 1200, 800),
            image(2, "b.jpg", "landscape-5.jpg", "image/jpeg", 300_000, 1200, 800),
            image(3, "c.jpg", "portrait-1.jpg", "image/jpeg", 300_000, 900, 1200),
            image(4, "d.jpg", "square-1.jpg", "image/jpeg", 300_000, 1000, 1000),
            image(5, "e.jpg", "landscape-1.jpg", "image/jpeg", 300_000, 1200, 800),
        ]
        assertScreen(MessageAttachmentsView(attachments: atts).padding(), store: store, styles: [("dark", .dark)], wait: 3.0)
    }

    func testVideoCard() async {
        let (store, _) = await makeFakeStore()
        var att = image(1, "demo.mp4", "clip-1.mp4", "video/mp4", 590_746, 960, 540)
        att.thumbnail = "fakeasset:clip-1-poster.jpg"
        assertScreen(MessageAttachmentsView(attachments: [att]).padding(), store: store, styles: [("dark", .dark)], wait: 3.0)
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
        let (store, _) = await makeFakeStore()
        let items = [
            image(1, "saturday-1.jpg", "landscape-1.jpg", "image/jpeg", 840_000, 1200, 800),
            image(2, "saturday-2.jpg", "portrait-1.jpg", "image/jpeg", 910_000, 900, 1200),
            image(3, "saturday-3.jpg", "landscape-2.jpg", "image/jpeg", 1_200_000, 1200, 800),
        ].map { LightboxItem(attachment: $0) }
        assertScreen(MediaViewerView(request: LightboxRequest(items: items, startIndex: 1), onDismiss: {}), store: store, styles: [("dark", .dark)], wait: 3.0)
    }

    /// A video page in the viewer: no zoom, native `AVPlayer` controls.
    func testViewerVideo() async {
        let (store, _) = await makeFakeStore()
        var att = image(1, "demo.mp4", "clip-1.mp4", "video/mp4", 590_746, 960, 540)
        att.thumbnail = "fakeasset:clip-1-poster.jpg"
        let items = [LightboxItem(attachment: att)]
        assertScreen(MediaViewerView(request: LightboxRequest(items: items, startIndex: 0), onDismiss: {}), store: store, styles: [("dark", .dark)], wait: 3.0)
    }
}
