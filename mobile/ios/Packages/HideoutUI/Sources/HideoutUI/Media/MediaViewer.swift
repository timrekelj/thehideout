import AVKit
import HideoutCore
import HideoutDesign
import SwiftUI

/// What the lightbox shows: one picture or video of a message, by its attachment (ATT-05).
public struct LightboxItem: Identifiable, Equatable, Sendable {
    public var id: ID
    public var filename: String
    public var contentType: String
    public var file: String
    public var thumbnail: String?
    public var width: Int?
    public var height: Int?
    public var isVideo: Bool
    /// Where to start a video: handed off from the inline card that was playing when "Expand" was tapped.
    public var startSeconds: Double

    public init(attachment: Attachment, startSeconds: Double = 0) {
        id = attachment.id
        filename = attachment.filename
        contentType = attachment.contentType
        file = attachment.file
        thumbnail = attachment.thumbnail
        width = attachment.width
        height = attachment.height
        isVideo = fileKind(contentType: attachment.contentType, filename: attachment.filename) == .video
        self.startSeconds = startSeconds
    }
}

/// What the lightbox is showing right now: the message's items and which one is open.
public struct LightboxRequest: Identifiable, Equatable {
    public var items: [LightboxItem]
    public var startIndex: Int
    public var id: ID { items.first?.id ?? 0 }
    public init(items: [LightboxItem], startIndex: Int) { self.items = items; self.startIndex = startIndex }
}

/// The full-screen media viewer (ATT-05): swipes between a message's pictures and videos, pinch and
/// double-tap zoom with pan on pictures, swipe down to dismiss, share and save to Photos, and native
/// `AVPlayer` controls on video.
public struct MediaViewerView: View {
    @Environment(ServerSelection.self) private var server
    @Environment(\.allowsFakeAssets) private var allowsFakeAssets
    let request: LightboxRequest
    let onDismiss: () -> Void
    @State private var index: Int
    @State private var dragY: CGFloat = 0
    @State private var scale: CGFloat = 1
    @State private var busy = false
    @State private var shareURL: URL?
    @State private var errorMessage: String?

    public init(request: LightboxRequest, onDismiss: @escaping () -> Void) {
        self.request = request
        self.onDismiss = onDismiss
        _index = State(initialValue: request.startIndex.clamped(to: 0...(max(request.items.count - 1, 0))))
    }

    private var current: LightboxItem { request.items[min(index, request.items.count - 1)] }

    public var body: some View {
        let dismissFraction = min(abs(dragY) / 300, 1)
        ZStack {
            Color.black.opacity(1 - dismissFraction * 0.85).ignoresSafeArea()
            TabView(selection: $index) {
                ForEach(Array(request.items.enumerated()), id: \.offset) { i, item in
                    page(item, at: i).tag(i)
                }
            }
            .tabViewStyle(.page(indexDisplayMode: .never))
            .offset(y: dragY)
            .onChange(of: index) { _, _ in scale = 1 }
            // Only at zoom scale 1 does a vertical pan dismiss; otherwise it pans the zoomed picture.
            .simultaneousGesture(
                scale <= 1.01
                    ? DragGesture()
                        .onChanged { value in if abs(value.translation.height) > abs(value.translation.width) { dragY = value.translation.height } }
                        .onEnded { value in
                            if abs(dragY) > 140 { onDismiss() } else { withAnimation(Motion.control) { dragY = 0 } }
                        }
                    : nil
            )
            VStack {
                topBar
                Spacer()
            }
        }
        .statusBarHidden()
        .alert("Can't do that", isPresented: Binding(get: { errorMessage != nil }, set: { if !$0 { errorMessage = nil } }), presenting: errorMessage) { _ in } message: { Text($0) }
        .sheet(item: Binding(get: { shareURL.map(ShareItem.init) }, set: { shareURL = $0?.url })) { item in ShareSheet(items: [item.url]) }
    }

    @ViewBuilder
    private func page(_ item: LightboxItem, at i: Int) -> some View {
        let source = try? mediaSource(base: server.url, key: item.file, allowFakeAssets: allowsFakeAssets)
        if item.isVideo {
            LightboxVideoPage(item: item, source: source, isActive: index == i)
        } else {
            // Each page owns its zoom state; only the current page's reports up, so a picture left
            // zoomed in on a page swiped away from never blocks (or wrongly allows) this page's
            // swipe-down-to-dismiss.
            ZoomablePage(item: item, source: source, isActive: index == i) { scale = $0 }
        }
    }

    private var topBar: some View {
        HStack(spacing: 4) {
            Text(request.items.count > 1 ? "\(current.filename) · \(index + 1) of \(request.items.count)" : current.filename)
                .typeStyle(\.label, tint: .white)
                .lineLimit(1)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.leading, 12)
            if busy {
                ProgressView().tint(.white)
            } else {
                Button(action: share) { Image(systemName: "square.and.arrow.up").foregroundStyle(.white) }
                    .accessibilityLabel("Share")
                Button(action: save) { Image(systemName: "arrow.down.to.line").foregroundStyle(.white) }
                    .accessibilityLabel("Save")
            }
            Button(action: onDismiss) { Image(systemName: "xmark").foregroundStyle(.white) }
                .accessibilityLabel("Close")
        }
        .font(.system(size: 17, weight: .semibold))
        .padding(.horizontal, 12)
        .padding(.top, 8)
        .frame(height: 44)
    }

    private func share() {
        guard let source = try? mediaSource(base: server.url, key: current.file, allowFakeAssets: allowsFakeAssets) else {
            errorMessage = "This can't be shared."
            return
        }
        busy = true
        let id = current.id
        Task {
            do { shareURL = try await fetchAttachmentToCache(source, id: id, filename: current.filename) }
            catch let e as AttachmentIOError { errorMessage = e.message }
            catch { errorMessage = "That didn't work. Try again." }
            busy = false
        }
    }

    private func save() {
        guard let source = try? mediaSource(base: server.url, key: current.file, allowFakeAssets: allowsFakeAssets) else {
            errorMessage = "This can't be saved."
            return
        }
        busy = true
        let id = current.id
        let isVideo = current.isVideo
        Task {
            do {
                let url = try await fetchAttachmentToCache(source, id: id, filename: current.filename)
                try await saveToPhotoLibrary(fileURL: url, isVideo: isVideo)
            } catch let e as AttachmentIOError { errorMessage = e.message }
            catch let e as PhotoSaveError { errorMessage = e.message }
            catch { errorMessage = "That didn't work. Try again." }
            busy = false
        }
    }
}

private extension Int {
    func clamped(to range: ClosedRange<Int>) -> Int { Swift.min(Swift.max(self, range.lowerBound), range.upperBound) }
}

/// A picture page: loads the full-size picture (falling back to the thumbnail while it arrives),
/// zoomable; only the active page reports its zoom scale up (see `MediaViewerView.page(_:at:)`).
/// `source` is resolved by the parent (which has the environment access this needs) and passed in, so
/// an already-cached picture can be read synchronously at creation, the same way `AttachmentAsyncImage`
/// does, instead of only ever loading through `.task`.
private struct ZoomablePage: View {
    let item: LightboxItem
    let source: MediaSource?
    let isActive: Bool
    var onScaleChange: (CGFloat) -> Void
    @State private var image: UIImage?
    @State private var scale: CGFloat = 1

    init(item: LightboxItem, source: MediaSource?, isActive: Bool, onScaleChange: @escaping (CGFloat) -> Void) {
        self.item = item
        self.source = source
        self.isActive = isActive
        self.onScaleChange = onScaleChange
        _image = State(initialValue: source.flatMap { MediaImageCache.shared.cachedImage(for: $0) })
    }

    var body: some View {
        ZoomableImage(image: image, scale: $scale)
            .task(id: item.id) {
                guard image == nil, let source else { return }
                image = await MediaImageCache.shared.image(for: source)
            }
            .onChange(of: scale) { _, new in if isActive { onScaleChange(new) } }
            // A page returned to while still zoomed from before must report that *now*: `scale`
            // itself did not change, only which page is active.
            .onChange(of: isActive) { _, active in if active { onScaleChange(scale) } }
    }
}

/// A video page: a plain `AVPlayer` with native controls, starting at `item.startSeconds`; plays only
/// while `isActive` is true (the pager's current page), so swiping away pauses it and only one player
/// plays at once. No pinch/double-tap zoom on video — scrubbing and seeking are the controls' job.
private struct LightboxVideoPage: View {
    let item: LightboxItem
    let source: MediaSource?
    let isActive: Bool
    @State private var player: AVPlayer?

    var body: some View {
        Group {
            if let player {
                VideoPlayer(player: player)
                    // The native controls can start playback themselves, bypassing `play(_:)`:
                    // observing `timeControlStatus` keeps `ActiveAttachmentPlayer` honest either way,
                    // so a later attachment/viewer play still pauses this one.
                    .onReceive(player.publisher(for: \.timeControlStatus)) { status in
                        if status == .playing { ActiveAttachmentPlayer.shared.noteStartedPlaying(player) }
                    }
            } else {
                Color.clear
            }
        }
        .task(id: item.id) {
            guard let source else { return }
            let p = AVPlayer(playerItem: AVPlayerItem(asset: AVURLAsset(url: resolvedURL(source))))
            if item.startSeconds > 0 { await p.seek(to: CMTime(seconds: item.startSeconds, preferredTimescale: 600)) }
            player = p
            // The poster's "Expand" hands its position here and pauses itself; this picks playback
            // back up immediately when the viewer opens straight onto this page.
            if isActive { ActiveAttachmentPlayer.shared.play(p) }
        }
        .onChange(of: isActive) { _, active in
            guard let player else { return }
            if active { ActiveAttachmentPlayer.shared.play(player) } else { player.pause() }
        }
        .onDisappear {
            guard let player else { return }
            ActiveAttachmentPlayer.shared.released(player)
            player.pause()
        }
    }
}
