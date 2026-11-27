import HideoutCore
import ImageIO
import SwiftUI
import UIKit

/// Decoded frames of an animated picture (GIF; ImageIO also exposes multi-frame WebP, so this is not
/// GIF-specific), with the total loop duration spread evenly across them — a simplification of the
/// format's own per-frame delays, which `UIImage.animatedImage(with:duration:)` cannot carry, but
/// plenty for a short looping emoji.
private func decodeAnimatedFrames(_ data: Data) -> (frames: [UIImage], duration: TimeInterval)? {
    // `UIImage(data:)` decodes an animated GIF into `.images`/`.duration` on its own, through a
    // different code path than the manual per-frame `ImageIO` extraction below — which a filed Apple
    // bug (rdar://143602439) can turn into solid-colour garbage frames for some files on at least one
    // iOS simulator build, without `CGImageSourceCreateImageAtIndex` reporting any failure.
    if let whole = UIImage(data: data), let images = whole.images, images.count > 1 {
        return (images, whole.duration > 0 ? whole.duration : Double(images.count) * 0.1)
    }
    guard let source = CGImageSourceCreateWithData(data as CFData, nil) else { return nil }
    let count = CGImageSourceGetCount(source)
    guard count > 1 else { return nil }
    var frames: [UIImage] = []
    var total: TimeInterval = 0
    for i in 0..<count {
        guard let cg = CGImageSourceCreateImageAtIndex(source, i, nil) else { continue }
        frames.append(UIImage(cgImage: cg))
        let props = CGImageSourceCopyPropertiesAtIndex(source, i, nil) as? [CFString: Any]
        let gif = props?[kCGImagePropertyGIFDictionary] as? [CFString: Any]
        let webp = props?[kCGImagePropertyWebPDictionary] as? [CFString: Any]
        let delay = (gif?[kCGImagePropertyGIFUnclampedDelayTime] as? Double) ?? (gif?[kCGImagePropertyGIFDelayTime] as? Double)
            ?? (webp?[kCGImagePropertyWebPUnclampedDelayTime] as? Double) ?? (webp?[kCGImagePropertyWebPDelayTime] as? Double) ?? 0.1
        total += delay
    }
    guard !frames.isEmpty else { return nil }
    return (frames, total > 0 ? total : Double(frames.count) * 0.1)
}

/// Boxes `(frames:duration:)` as a class so `NSCache` (reference types only) can hold it.
private final class AnimatedFrames { let frames: [UIImage]; let duration: TimeInterval
    init(frames: [UIImage], duration: TimeInterval) { self.frames = frames; self.duration = duration }
}

/// A small in-memory cache of decoded animated-image frames, keyed by the emoji's own id (stable and
/// cheap, unlike hashing the whole `MediaSource`). Emoji images are small (<= 128x128, server-capped),
/// so this never needs `MediaImageCache`'s disk tier or downsampling. `NSCache`, not an actor: a
/// synchronous peek (`cached(for:)`) lets a view already holding the frames skip the one-frame gap
/// `.task` would otherwise show, the same shape `MediaImageCache.cachedImage(for:)` uses.
final class AnimatedEmojiCache: @unchecked Sendable {
    static let shared = AnimatedEmojiCache()
    private let cache = NSCache<NSNumber, AnimatedFrames>()

    func cached(for id: ID) -> (frames: [UIImage], duration: TimeInterval)? {
        cache.object(forKey: NSNumber(value: id)).map { ($0.frames, $0.duration) }
    }

    func frames(for id: ID, data: @escaping @Sendable () async -> Data?) async -> (frames: [UIImage], duration: TimeInterval)? {
        if let cached = cached(for: id) { return cached }
        guard let data = await data(), let decoded = decodeAnimatedFrames(data) else { return nil }
        cache.setObject(AnimatedFrames(frames: decoded.frames, duration: decoded.duration), forKey: NSNumber(value: id))
        return decoded
    }
}

/// A small in-memory cache of a static emoji picture, decoded straight from its fetched bytes via
/// `UIImage(data:)` — not through `MediaImageCache`'s thumbnail-block decode, which a filed Apple bug
/// (rdar://143602439) can turn into a solid-colour garbage `CGImage` for some WebP files rather than
/// failing outright, on at least one iOS simulator build. Keyed by the emoji's own id, like
/// `AnimatedEmojiCache`, for the same synchronous-peek-at-creation reason.
final class StaticEmojiCache: @unchecked Sendable {
    static let shared = StaticEmojiCache()
    private let cache = NSCache<NSNumber, UIImage>()

    func cached(for id: ID) -> UIImage? { cache.object(forKey: NSNumber(value: id)) }

    func image(for id: ID, data: @escaping @Sendable () async -> Data?) async -> UIImage? {
        if let cached = cached(for: id) { return cached }
        guard let data = await data(), let image = UIImage(data: data) else { return nil }
        cache.setObject(image, forKey: NSNumber(value: id))
        return image
    }
}

/// A plain `UIImageView` playing `UIImage.animatedImage(with:duration:)`: SwiftUI's `Image` has no
/// built-in animated-image support, but a `UIImageView` showing one animates on its own once started.
struct AnimatedImageView: UIViewRepresentable {
    let frames: [UIImage]
    let duration: TimeInterval

    func makeUIView(context: Context) -> UIImageView {
        let view = UIImageView(image: .animatedImage(with: frames, duration: duration))
        view.contentMode = .scaleAspectFit
        view.startAnimating()
        return view
    }

    func updateUIView(_ uiView: UIImageView, context: Context) {}

    /// Without this, a `UIImageView`'s intrinsic content size (the GIF's own pixel size) can win over
    /// the SwiftUI `.frame(width:height:)` around it when measured by a custom `Layout` container's
    /// `sizeThatFits(.unspecified)` query (`FlowWrap`, for a jumbo message's emoji) — accepting
    /// whatever size that query proposes keeps this view exactly the size its SwiftUI ancestors ask for.
    func sizeThatFits(_ proposal: ProposedViewSize, uiView: UIImageView, context: Context) -> CGSize? {
        CGSize(width: proposal.width ?? EMOJI_JUMBO_PT, height: proposal.height ?? EMOJI_JUMBO_PT)
    }
}

/// One custom emoji's picture (MSG-09, MD-06/07): animated GIF/WebP plays (never under Reduce Motion,
/// which shows just its first frame instead), a static one decodes once through the shared
/// `MediaImageCache`. Falls back to `:name:` text on a failed fetch or an undecodable image, the same
/// fallback an unknown token already shows.
public struct CustomEmojiImage: View {
    @Environment(\.theme) private var theme
    @Environment(ServerSelection.self) private var server
    @Environment(\.allowsFakeAssets) private var allowsFakeAssets
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let emoji: Emoji
    var size: CGFloat

    @State private var animated: (frames: [UIImage], duration: TimeInterval)?
    @State private var staticImage: UIImage?
    @State private var failed = false

    /// Already-cached animated frames or a cached static picture are known synchronously at creation
    /// (keyed by the emoji's own id, which needs no environment to resolve): a jumbo message
    /// re-rendered, or a cell reused while scrolling, never flashes a blank frame for one this process
    /// already decoded.
    public init(emoji: Emoji, size: CGFloat = EMOJI_INLINE_PT) {
        self.emoji = emoji
        self.size = size
        if emoji.animated, let cached = AnimatedEmojiCache.shared.cached(for: emoji.id) {
            _animated = State(initialValue: cached)
        } else if let cached = StaticEmojiCache.shared.cached(for: emoji.id) {
            _staticImage = State(initialValue: cached)
        }
    }

    private var source: MediaSource? { try? mediaSource(base: server.url, key: emoji.image, allowFakeAssets: allowsFakeAssets) }

    public var body: some View {
        // The base layer is always a plain SwiftUI primitive (never the animated `UIViewRepresentable`
        // directly): a custom `Layout` container (`FlowWrap`, for a jumbo message) measures each child
        // with `sizeThatFits(.unspecified)` to decide wrap points, and a bare `UIViewRepresentable`
        // wrapping a `UIImageView` answers that with the GIF's own pixel size instead of this frame —
        // Text/Color/Image all answer it correctly. The animated view is layered on top via `.overlay`
        // instead, which inherits the base layer's already-resolved size rather than being measured
        // independently.
        Group {
            if let staticImage {
                Image(uiImage: staticImage).resizable().aspectRatio(contentMode: .fit)
            } else if failed {
                Text(":\(emoji.name):").typeStyle(\.caption, color: \.textMuted)
            } else {
                Color.clear
            }
        }
        .frame(width: size, height: size)
        .clipped()
        .overlay {
            if emoji.animated, !reduceMotion, let animated {
                AnimatedImageView(frames: animated.frames, duration: animated.duration)
            }
        }
        .accessibilityLabel(":\(emoji.name):")
        .task(id: emoji.id) { await load() }
    }

    private func load() async {
        guard let source else { failed = true; return }
        if emoji.animated, !reduceMotion {
            if let decoded = await AnimatedEmojiCache.shared.frames(for: emoji.id, data: { await Self.fetchData(source) }) {
                animated = decoded
                return
            }
        }
        if let image = await StaticEmojiCache.shared.image(for: emoji.id, data: { await Self.fetchData(source) }) {
            staticImage = image
        } else {
            failed = true
        }
    }

    private static func fetchData(_ source: MediaSource) async -> Data? {
        switch source {
        case .fakeAsset(let name): return FakeMedia.url(forAssetNamed: name).flatMap { try? Data(contentsOf: $0) }
        case .remote(let url):
            guard let (data, response) = try? await URLSession.shared.data(from: url),
                  let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else { return nil }
            return data
        }
    }
}

/// Decodes `emoji`'s picture into `AnimatedEmojiCache`/`StaticEmojiCache` ahead of time, so a
/// `CustomEmojiImage` for it shows the real picture synchronously at creation instead of through its
/// own `.task` — the same reasoning `MediaImageCache`'s callers warm it for a snapshot test, since a
/// plain `View`-rendering snapshot strategy (not a `UIHostingController` one) never awaits a `.task`.
public func preloadCustomEmojiImage(_ emoji: Emoji, base: URL, allowFakeAssets: Bool = false) async {
    guard let source = try? mediaSource(base: base, key: emoji.image, allowFakeAssets: allowFakeAssets) else { return }
    let data: @Sendable () async -> Data? = {
        switch source {
        case .fakeAsset(let name): return FakeMedia.url(forAssetNamed: name).flatMap { try? Data(contentsOf: $0) }
        case .remote(let url):
            guard let (data, response) = try? await URLSession.shared.data(from: url),
                  let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else { return nil }
            return data
        }
    }
    if emoji.animated {
        _ = await AnimatedEmojiCache.shared.frames(for: emoji.id, data: data)
    } else {
        _ = await StaticEmojiCache.shared.image(for: emoji.id, data: data)
    }
}

/// Side of a custom emoji in a line of text, in points (desktop's `EMOJI_INLINE`).
public let EMOJI_INLINE_PT: CGFloat = 20
/// Side of a custom emoji (and the font size of a Unicode one) in a jumbo message, in points (desktop's `EMOJI_JUMBO`).
public let EMOJI_JUMBO_PT: CGFloat = 48
