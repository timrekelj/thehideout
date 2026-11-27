import AVFoundation
import AVKit
import HideoutCore
import HideoutDesign
import QuickLook
import SwiftUI
import UIKit

/// Only one attachment player — an inline video card, an audio row, or the lightbox — may play at a
/// time: starting one pauses whichever was already playing (ATT-02/03/05).
@MainActor
final class ActiveAttachmentPlayer {
    static let shared = ActiveAttachmentPlayer()
    private weak var current: AVPlayer?

    func play(_ player: AVPlayer) {
        if current !== player { current?.pause() }
        current = player
        player.play()
    }

    /// `player` started playing on its own (the viewer's native `VideoPlayer` controls, not our own
    /// play buttons): note it as current without re-issuing `play()`, so a later attachment/viewer
    /// play still pauses it instead of leaving two players running.
    func noteStartedPlaying(_ player: AVPlayer) {
        if current !== player { current?.pause() }
        current = player
    }

    func released(_ player: AVPlayer) { if current === player { current = nil } }
}

extension EnvironmentValues {
    /// Whether `mediaSource` may resolve the fake backend's bundled-asset keys: true only under
    /// `--fake`/`--gallery` (and the snapshot/Maestro harnesses that mimic them). A real server's
    /// response is never trusted to pick this path, so a misbehaving or compromised server can't make
    /// the app load a bundled local file instead of refusing the key outright.
    @Entry public var allowsFakeAssets: Bool = false
}

/// A picture with a sized placeholder (never shifts the row while it loads): tap-to-retry on a failed
/// fetch, and a quiet "can't preview" glyph (no retry — it would just fail the same way again) for
/// bytes that arrived but did not decode as an image.
struct AttachmentAsyncImage: View {
    @Environment(\.theme) private var theme
    let source: MediaSource?
    var contentMode: ContentMode = .fill
    @State private var image: UIImage?
    @State private var outcome: ImageOutcome = .loading
    @State private var retry = 0

    enum ImageOutcome { case loading, fetchFailed, undecodable }

    /// A cached picture (or a source that refuses to resolve) is known synchronously at creation, so
    /// a tile already in `MediaImageCache` never shows even one frame of placeholder.
    init(source: MediaSource?, contentMode: ContentMode = .fill) {
        self.source = source
        self.contentMode = contentMode
        if let source, let cached = MediaImageCache.shared.cachedImage(for: source) {
            _image = State(initialValue: cached)
        } else if source == nil {
            _outcome = State(initialValue: .fetchFailed)
        }
    }

    var body: some View {
        ZStack {
            if let image {
                Image(uiImage: image).resizable().aspectRatio(contentMode: contentMode)
            } else {
                switch outcome {
                case .loading:
                    Rectangle().fill(theme.colors.surfaceRaised)
                case .fetchFailed:
                    Button { retry += 1 } label: {
                        Image(systemName: "arrow.clockwise").foregroundStyle(theme.colors.textMuted)
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(theme.colors.surfaceRaised)
                    .accessibilityLabel("Retry")
                case .undecodable:
                    Image(systemName: "exclamationmark.triangle")
                        .foregroundStyle(theme.colors.textFaint)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                        .background(theme.colors.surfaceRaised)
                        .accessibilityLabel("Can't preview this file")
                }
            }
        }
        .clipped()
        .task(id: Pair(source, retry)) {
            guard let source else { outcome = .fetchFailed; return }
            if image != nil { return }
            outcome = .loading
            switch await MediaImageCache.shared.outcome(for: source) {
            case .loaded(let loaded): image = loaded
            case .fetchFailed: outcome = .fetchFailed
            case .undecodable: outcome = .undecodable
            }
        }
    }
}

/// A `.task(id:)` key of two values, since `MediaSource?` alone can't carry the retry counter.
private struct Pair: Equatable { let source: MediaSource?; let retry: Int
    init(_ source: MediaSource?, _ retry: Int) { self.source = source; self.retry = retry }
}

/// Lays out a message's image tiles per [imageGrid]: one keeps its aspect, two sit side by side, three
/// put the first tall on the left, four or more are a 2x2. Recomputes from the proposed width on every
/// pass, so the grid adapts to the column it is given (never past `mediaMaxW`) the same way Android's
/// custom `Layout` does.
struct AttachmentImageGridLayout: SwiftUI.Layout {
    let count: Int
    let single: (Int, Int)?

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let maxW = min(proposal.width ?? mediaMaxW, mediaMaxW)
        let (size, _) = imageGrid(n: count, single: single, maxW: Double(maxW))
        return CGSize(width: size.0, height: size.1)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let maxW = min(bounds.width, mediaMaxW)
        let (_, cells) = imageGrid(n: count, single: single, maxW: Double(maxW))
        for (i, subview) in subviews.enumerated() where i < cells.count {
            let c = cells[i]
            subview.place(at: CGPoint(x: bounds.minX + c.x, y: bounds.minY + c.y), proposal: ProposedViewSize(width: c.w, height: c.h))
        }
    }
}

/// Sizes a single child to `fitSize` of the proposed width (never past `mediaMaxW`/`mediaMaxH`): the
/// video card's own version of `AttachmentImageGridLayout`, so it adapts to its column instead of
/// always claiming the full 400pt (which overflowed narrower screens).
struct FitSizeLayout: SwiftUI.Layout {
    let natural: (Int, Int)?

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let maxW = min(proposal.width ?? mediaMaxW, mediaMaxW)
        let (w, h) = fitSize(natural: natural, maxW: Double(maxW), maxH: mediaMaxH)
        return CGSize(width: w, height: h)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        for subview in subviews { subview.place(at: bounds.origin, proposal: ProposedViewSize(bounds.size)) }
    }
}

/// The pictures of one message as a grid (ATT-01): tap opens the lightbox at that picture.
struct AttachmentImagesView: View {
    @Environment(ServerSelection.self) private var server
    @Environment(\.allowsFakeAssets) private var allowsFakeAssets
    let images: [Attachment]
    var onOpen: (Int) -> Void

    var body: some View {
        let shown = Array(images.prefix(4))
        let single = images.first.flatMap { a in a.width.flatMap { w in a.height.map { h in (w, h) } } }
        AttachmentImageGridLayout(count: shown.count, single: single) {
            ForEach(Array(shown.enumerated()), id: \.offset) { i, att in
                ZStack {
                    AttachmentAsyncImage(source: try? mediaSource(base: server.url, key: att.thumbnail ?? att.file, allowFakeAssets: allowsFakeAssets))
                    if i == shown.count - 1, images.count > shown.count {
                        Rectangle().fill(Color.black.opacity(0.45))
                        Text("+\(images.count - shown.count + 1)")
                            .font(DesignFont.ui(size: 22, weight: 650))
                            .foregroundStyle(.white)
                    }
                }
                .contentShape(Rectangle())
                .onTapGesture { onOpen(i) }
                .accessibilityElement(children: .ignore)
                .accessibilityLabel("View \(att.filename)")
                .accessibilityAddTraits(.isButton)
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: Radius.lg, style: .continuous))
    }
}

/// Play/pause, position, duration and mute, read off an `AVPlayer`.
@MainActor
@Observable
final class ClipPlayerState {
    var isPlaying = false
    var positionSec: Double = 0
    var durationSec: Double = 0
    var muted = false
    @ObservationIgnored var timeObserver: Any?
    @ObservationIgnored var endObserver: NSObjectProtocol?
    @ObservationIgnored weak var player: AVPlayer?

    func attach(_ player: AVPlayer) {
        self.player = player
        muted = player.isMuted
        // The player itself is captured weakly: it already holds this closure strongly (it's the
        // observer `addPeriodicTimeObserver` keeps), so a strong capture here would be a cycle that
        // only `detach()` breaks — harmless when `detach` always runs, but not guaranteed to.
        timeObserver = player.addPeriodicTimeObserver(forInterval: CMTime(seconds: 0.2, preferredTimescale: 600), queue: .main) { [weak self, weak player] time in
            Task { @MainActor [weak self, weak player] in
                guard let self, let player else { return }
                self.positionSec = time.seconds.isFinite ? time.seconds : 0
                if let d = player.currentItem?.duration.seconds, d.isFinite { self.durationSec = d }
                self.isPlaying = player.rate != 0
            }
        }
        // A periodic time observer stops firing once the player is paused — including the automatic
        // pause at the end of the clip — so without this, `isPlaying` would freeze at `true` forever
        // once a clip plays through to its end.
        endObserver = NotificationCenter.default.addObserver(forName: .AVPlayerItemDidPlayToEndTime, object: player.currentItem, queue: .main) { [weak self] _ in
            self?.isPlaying = false
        }
    }

    func detach() {
        if let timeObserver, let player { player.removeTimeObserver(timeObserver) }
        timeObserver = nil
        if let endObserver { NotificationCenter.default.removeObserver(endObserver) }
        endObserver = nil
    }

    /// Clears played-position state for reuse once a card's player is fully torn down (so reappearing
    /// after scrolling away shows a fresh "tap to play", not a stale paused state from before).
    func reset() {
        isPlaying = false
        positionSec = 0
        durationSec = 0
    }

    func toggleMute() {
        player?.isMuted.toggle()
        muted = player?.isMuted ?? false
    }

    func seek(fraction: Double) {
        guard durationSec > 0, let player else { return }
        player.seek(to: CMTime(seconds: fraction.clamped(to: 0...1) * durationSec, preferredTimescale: 600))
    }

    /// Plays from the current position, restarting from the beginning first if the clip had already
    /// played through to its end: `AVPlayer.play()` on a finished item is otherwise a no-op, which
    /// would make "Play" after a clip ends look like it did nothing.
    func playFromCurrentPositionOrRestart() {
        guard let player else { return }
        if durationSec > 0, positionSec >= durationSec - 0.05 { player.seek(to: .zero) }
        ActiveAttachmentPlayer.shared.play(player)
    }
}

private extension Double {
    func clamped(to range: ClosedRange<Double>) -> Double { min(max(self, range.lowerBound), range.upperBound) }
}

/// A plain `AVPlayerLayer`-backed view: no native controls, since the card and the lightbox draw their own bar.
struct PlayerLayerView: UIViewRepresentable {
    let player: AVPlayer

    func makeUIView(context: Context) -> PlayerContainerView {
        let view = PlayerContainerView()
        view.playerLayer.player = player
        view.playerLayer.videoGravity = .resizeAspect
        return view
    }

    func updateUIView(_ uiView: PlayerContainerView, context: Context) { uiView.playerLayer.player = player }
}

final class PlayerContainerView: UIView {
    override static var layerClass: AnyClass { AVPlayerLayer.self }
    var playerLayer: AVPlayerLayer { layer as! AVPlayerLayer } // swiftlint:disable:this force_cast
}

/// A draggable progress track (tap or drag anywhere to seek): the compact bar under a video or audio row.
struct ScrubBar: View {
    let fraction: Double
    let trackColor: Color
    let fillColor: Color
    var onSeek: (Double) -> Void

    var body: some View {
        GeometryReader { geo in
            let w = max(geo.size.width, 1)
            ZStack(alignment: .leading) {
                Capsule().fill(trackColor).frame(height: 4)
                Capsule().fill(fillColor).frame(width: w * fraction.clamped(to: 0...1), height: 4)
            }
            .frame(maxHeight: .infinity, alignment: .center)
            .contentShape(Rectangle())
            .gesture(DragGesture(minimumDistance: 0).onChanged { value in onSeek((value.location.x / w).clamped(to: 0...1)) })
        }
        .frame(height: 24)
    }
}

/// A video attachment: a poster with a play button, in-place playback with a bar (ATT-02). `onExpand`
/// hands the current position off to the lightbox and pauses this card, so only one player ever plays.
struct AttachmentVideoCard: View {
    @Environment(ServerSelection.self) private var server
    @Environment(\.theme) private var theme
    @Environment(\.allowsFakeAssets) private var allowsFakeAssets
    let attachment: Attachment
    var onExpand: ((Double) -> Void)?
    @State private var player: AVPlayer?
    @State private var started = false
    @State private var ui = ClipPlayerState()
    @State private var errorMessage: String?

    private var natural: (Int, Int)? { attachment.width.flatMap { w in attachment.height.map { h in (w, h) } } }
    private var posterSource: MediaSource? {
        attachment.thumbnail.flatMap { try? mediaSource(base: server.url, key: $0, allowFakeAssets: allowsFakeAssets) }
    }
    private var videoSource: MediaSource? { try? mediaSource(base: server.url, key: attachment.file, allowFakeAssets: allowsFakeAssets) }

    var body: some View {
        FitSizeLayout(natural: natural) {
            ZStack {
                if !started {
                    if posterSource != nil {
                        AttachmentAsyncImage(source: posterSource, contentMode: .fit)
                    } else {
                        Rectangle().fill(theme.colors.surfaceRaised)
                    }
                    Button { ensurePlayer() } label: {
                        Image(systemName: "play.fill")
                            .font(.system(size: 18, weight: .bold))
                            .foregroundStyle(.white)
                            .frame(width: 48, height: 48)
                            .background(Color.black.opacity(0.45), in: Circle())
                    }
                    .accessibilityLabel("Play video")
                } else if let player {
                    PlayerLayerView(player: player)
                        .contentShape(Rectangle())
                        .onTapGesture { ui.isPlaying ? player.pause() : ui.playFromCurrentPositionOrRestart() }
                    VStack {
                        Spacer()
                        controlBar(player)
                    }
                }
            }
            .background(theme.colors.surfaceRaised)
            .clipShape(RoundedRectangle(cornerRadius: Radius.lg, style: .continuous))
        }
        // See `AttachmentAudioRow`'s equivalent: `onScrollVisibilityChange` catches a row scrolled
        // far away within the same `ScrollView`, which `onDisappear` alone does not reliably catch.
        .onScrollVisibilityChange(threshold: 0) { visible in if !visible { teardown() } }
        .onDisappear(perform: teardown)
        .alert("Can't play this video", isPresented: Binding(get: { errorMessage != nil }, set: { if !$0 { errorMessage = nil } }), presenting: errorMessage) { _ in } message: { Text($0) }
    }

    private func controlBar(_ player: AVPlayer) -> some View {
        HStack(spacing: 6) {
            Button { ui.isPlaying ? player.pause() : ui.playFromCurrentPositionOrRestart() } label: {
                Image(systemName: ui.isPlaying ? "pause.fill" : "play.fill").foregroundStyle(.white)
            }
            ScrubBar(fraction: ui.durationSec > 0 ? ui.positionSec / ui.durationSec : 0, trackColor: .white.opacity(0.3), fillColor: .white, onSeek: ui.seek)
            Text("\(clockLabel(ui.positionSec)) / \(clockLabel(ui.durationSec))")
                .font(.system(size: 11, weight: .medium, design: .monospaced)).foregroundStyle(.white)
                .fixedSize()
            Button { ui.toggleMute() } label: {
                Image(systemName: ui.muted ? "speaker.slash.fill" : "speaker.wave.2.fill").foregroundStyle(.white)
            }
            if let onExpand {
                Button {
                    let pos = ui.positionSec
                    player.pause()
                    onExpand(pos)
                } label: { Image(systemName: "arrow.up.left.and.arrow.down.right").foregroundStyle(.white) }
                .accessibilityLabel("Expand")
            }
        }
        .padding(.horizontal, 8)
        .frame(height: 36)
        .background(Color.black.opacity(0.45))
    }

    private func ensurePlayer() {
        guard let videoSource else {
            errorMessage = "This video can't be played."
            return
        }
        let item = AVPlayerItem(asset: AVURLAsset(url: resolvedURL(videoSource)))
        let p = AVPlayer(playerItem: item)
        player = p
        started = true
        ui.attach(p)
        ActiveAttachmentPlayer.shared.play(p)
    }

    /// Pauses and fully releases the player: scrolling this row off-screen and back must not leave a
    /// stuck "started" card whose player was already paused and detached (a stale `isPlaying` would
    /// otherwise make the next tap call `pause()` on an already-paused player instead of resuming).
    private func teardown() {
        if let player {
            ActiveAttachmentPlayer.shared.released(player)
            player.pause()
        }
        ui.detach()
        ui.reset()
        player = nil
        started = false
    }
}

/// A local file URL for `.fakeAsset`, or the server URL for `.remote`: what `AVPlayer`/`AVURLAsset` plays from.
func resolvedURL(_ source: MediaSource) -> URL {
    switch source {
    case .remote(let url): return url
    case .fakeAsset(let name): return FakeMedia.url(forAssetNamed: name) ?? URL(fileURLWithPath: "/dev/null")
    }
}

/// A compact play/pause/scrub row for an audio attachment (ATT-03).
struct AttachmentAudioRow: View {
    @Environment(ServerSelection.self) private var server
    @Environment(\.theme) private var theme
    @Environment(\.allowsFakeAssets) private var allowsFakeAssets
    let attachment: Attachment
    @State private var player: AVPlayer?
    @State private var ui = ClipPlayerState()
    @State private var busy = false
    @State private var shareURL: URL?
    @State private var errorMessage: String?

    private var source: MediaSource? { try? mediaSource(base: server.url, key: attachment.file, allowFakeAssets: allowsFakeAssets) }

    var body: some View {
        HStack(spacing: 10) {
            Button(action: togglePlay) {
                Image(systemName: ui.isPlaying ? "pause.fill" : "play.fill").foregroundStyle(theme.colors.accentText)
            }
            .accessibilityLabel(ui.isPlaying ? "Pause" : "Play")
            VStack(alignment: .leading, spacing: 2) {
                Text(attachment.filename).typeStyle(\.label).lineLimit(1)
                ScrubBar(fraction: ui.durationSec > 0 ? ui.positionSec / ui.durationSec : 0, trackColor: theme.colors.pressed, fillColor: theme.colors.accent, onSeek: ui.seek)
                Text(ui.durationSec > 0 ? "\(clockLabel(ui.positionSec)) / \(clockLabel(ui.durationSec))" : humanSize(attachment.size))
                    .typeStyle(\.caption, color: \.textMuted)
            }
            downloadButton
        }
        .padding(8)
        .frame(maxWidth: CGFloat(cardW))
        .background(theme.colors.hover, in: RoundedRectangle(cornerRadius: Radius.lg, style: .continuous))
        // Scrolling this row off-screen must stop its sound, not just stop tracking it as the active
        // player: the one-player rule has to hold even for a row no longer visible. `onDisappear`
        // alone is not reliable for this in a plain `ScrollView`/`LazyVStack` (a row scrolled far
        // away is not necessarily removed from the view tree the way it would be in a `List`), so
        // `onScrollVisibilityChange` — driven by the actual viewport, not view-tree membership — is
        // the real stop; `onDisappear` still covers the row leaving entirely (closing the channel).
        .onScrollVisibilityChange(threshold: 0) { visible in if !visible { stop() } }
        .onDisappear(perform: stop)
        .sheet(item: Binding(get: { shareURL.map(ShareItem.init) }, set: { shareURL = $0?.url })) { item in ShareSheet(items: [item.url]) }
        .alert("Can't download", isPresented: Binding(get: { errorMessage != nil }, set: { if !$0 { errorMessage = nil } }), presenting: errorMessage) { _ in } message: { Text($0) }
    }

    private var downloadButton: some View {
        Group {
            if busy { ProgressView().controlSize(.mini) } else {
                Button(action: download) { Image(systemName: "square.and.arrow.down").foregroundStyle(theme.colors.textMuted) }
                    .accessibilityLabel("Download \(attachment.filename)")
            }
        }
    }

    private func togglePlay() {
        guard let source else {
            errorMessage = "This audio can't be played."
            return
        }
        if let player {
            ui.isPlaying ? player.pause() : ui.playFromCurrentPositionOrRestart()
            return
        }
        let p = AVPlayer(playerItem: AVPlayerItem(asset: AVURLAsset(url: resolvedURL(source))))
        player = p
        ui.attach(p)
        ActiveAttachmentPlayer.shared.play(p)
    }

    private func stop() {
        if let player {
            player.pause()
            ActiveAttachmentPlayer.shared.released(player)
        }
        ui.isPlaying = false
        ui.detach()
    }

    private func download() {
        guard let source else {
            errorMessage = "This file can't be downloaded."
            return
        }
        busy = true
        Task {
            do { shareURL = try await fetchAttachmentToCache(source, id: attachment.id, filename: attachment.filename) }
            catch let e as AttachmentIOError { errorMessage = e.message }
            catch { errorMessage = "That didn't work. Try again." }
            busy = false
        }
    }
}

struct ShareItem: Identifiable { let url: URL; var id: String { url.path } }

/// The file-kind glyph for a file card (ATT-04).
func fileKindSymbol(_ kind: FileKind) -> String {
    switch kind {
    case .image: return "photo"
    case .video: return "film"
    case .audio: return "waveform"
    case .pdf: return "doc.richtext"
    case .archive: return "doc.zipper"
    case .text, .doc: return "doc.text"
    case .code: return "chevron.left.forwardslash.chevron.right"
    case .other: return "doc"
    }
}

/// A downloadable file: its type icon, name and size, "Open" (Quick Look, for inline kinds) and
/// "Download" (a share sheet, which covers save-to-Files, AirDrop and more) (ATT-04, ATT-12).
struct AttachmentFileCard: View {
    @Environment(ServerSelection.self) private var server
    @Environment(\.theme) private var theme
    @Environment(\.allowsFakeAssets) private var allowsFakeAssets
    let attachment: Attachment
    let kind: FileKind
    @State private var busy = false
    @State private var shareURL: URL?
    @State private var previewURL: URL?
    @State private var errorMessage: String?

    private var source: MediaSource? { try? mediaSource(base: server.url, key: attachment.file, allowFakeAssets: allowsFakeAssets) }

    var body: some View {
        HStack(spacing: 10) {
            ZStack {
                RoundedRectangle(cornerRadius: Radius.md, style: .continuous).fill(theme.colors.accentSubtle)
                Image(systemName: fileKindSymbol(kind)).foregroundStyle(theme.colors.accentText)
            }
            .frame(width: 36, height: 36)
            VStack(alignment: .leading, spacing: 1) {
                Text(attachment.filename).typeStyle(\.label).lineLimit(1)
                Text(humanSize(attachment.size)).typeStyle(\.caption, color: \.textMuted)
            }
            Spacer(minLength: 0)
            if busy {
                ProgressView().controlSize(.mini)
            } else {
                if opensInline(kind) {
                    Button(action: open) { Image(systemName: "eye").foregroundStyle(theme.colors.textMuted) }
                        .accessibilityLabel("Open")
                }
                Button(action: download) { Image(systemName: "square.and.arrow.down").foregroundStyle(theme.colors.textMuted) }
                    .accessibilityLabel("Download \(attachment.filename)")
            }
        }
        .padding(10)
        .frame(maxWidth: CGFloat(cardW))
        .background(theme.colors.hover, in: RoundedRectangle(cornerRadius: Radius.lg, style: .continuous))
        .sheet(item: Binding(get: { shareURL.map(ShareItem.init) }, set: { shareURL = $0?.url })) { item in ShareSheet(items: [item.url]) }
        .quickLookPreview(Binding(get: { previewURL }, set: { previewURL = $0 }))
        .alert("Can't open that", isPresented: Binding(get: { errorMessage != nil }, set: { if !$0 { errorMessage = nil } }), presenting: errorMessage) { _ in } message: { Text($0) }
    }

    private func open() {
        guard let source else {
            errorMessage = "This file can't be opened."
            return
        }
        busy = true
        Task {
            do { previewURL = try await fetchAttachmentToCache(source, id: attachment.id, filename: attachment.filename) }
            catch let e as AttachmentIOError { errorMessage = e.message }
            catch { errorMessage = "That didn't work. Try again." }
            busy = false
        }
    }

    private func download() {
        guard let source else {
            errorMessage = "This file can't be downloaded."
            return
        }
        busy = true
        Task {
            do { shareURL = try await fetchAttachmentToCache(source, id: attachment.id, filename: attachment.filename) }
            catch let e as AttachmentIOError { errorMessage = e.message }
            catch { errorMessage = "That didn't work. Try again." }
            busy = false
        }
    }
}

/// "File removed by an admin", where the file was (ATT-04).
struct RemovedAttachmentPlaceholder: View {
    @Environment(\.theme) private var theme
    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: "trash").foregroundStyle(theme.colors.textFaint).font(.system(size: 14))
            Text("File removed by an admin").typeStyle(\.caption, color: \.textMuted)
        }
        .padding(.horizontal, 10)
        .frame(height: 36)
        .frame(maxWidth: CGFloat(cardW), alignment: .leading)
        .background(theme.colors.hover, in: RoundedRectangle(cornerRadius: Radius.lg, style: .continuous))
        .accessibilityElement(children: .combine)
    }
}

/// What hangs under a message: its image grid, then its video, audio, file and removed attachments in
/// order (desktop's `attachments()`, `ui/attach.rs`). `onOpenLightbox` receives every picture/video of
/// the message and which one was tapped.
public struct MessageAttachmentsView: View {
    let attachments: [Attachment]
    var onOpenLightbox: ([LightboxItem], Int) -> Void

    public init(attachments: [Attachment], onOpenLightbox: @escaping ([LightboxItem], Int) -> Void = { _, _ in }) {
        self.attachments = attachments
        self.onOpenLightbox = onOpenLightbox
    }

    private var images: [Attachment] {
        attachments.filter { !$0.removed && !$0.file.isEmpty && fileKind(contentType: $0.contentType, filename: $0.filename) == .image }
    }

    public var body: some View {
        if !attachments.isEmpty {
            VStack(alignment: .leading, spacing: 6) {
                if !images.isEmpty {
                    AttachmentImagesView(images: images) { i in
                        onOpenLightbox(images.map { LightboxItem(attachment: $0) }, i)
                    }
                }
                ForEach(attachments.filter { a in !images.contains { $0.id == a.id } }) { att in
                    if att.removed || att.file.isEmpty {
                        RemovedAttachmentPlaceholder()
                    } else {
                        let kind = fileKind(contentType: att.contentType, filename: att.filename)
                        switch kind {
                        case .video:
                            AttachmentVideoCard(attachment: att) { pos in
                                onOpenLightbox([LightboxItem(attachment: att, startSeconds: pos)], 0)
                            }
                        case .audio:
                            AttachmentAudioRow(attachment: att)
                        default:
                            AttachmentFileCard(attachment: att, kind: kind)
                        }
                    }
                }
            }
            .padding(.top, 4)
        }
    }
}
