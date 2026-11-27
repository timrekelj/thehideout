import Foundation
import HideoutCore
import ImageIO
import UIKit

/// A fetch of attachment bytes that failed: `message` is safe to show the user directly.
public struct AttachmentIOError: Error, Equatable {
    public let message: String
    public init(_ message: String) { self.message = message }
}

/// Where the fake backend's bundled sample media lives (`Resources/FakeMedia`, copied from Android's
/// `ui/src/main/assets/fake_media` so both apps' snapshots show the same pictures and clips).
enum FakeMedia {
    static func url(forAssetNamed name: String) -> URL? {
        // Defensive even though every caller already gates this behind `allowFakeAssets` (only ever
        // true under `--fake`/`--gallery`, never from a real server's response): a name can't escape
        // the `FakeMedia` subdirectory it's looked up in.
        guard !name.contains("/"), !name.contains("..") else { return nil }
        let base = (name as NSString).deletingPathExtension
        let ext = (name as NSString).pathExtension
        return Bundle.module.url(forResource: base, withExtension: ext, subdirectory: "FakeMedia")
    }
}

/// Streams `source`'s bytes to `destination` (overwritten if present): a bundled fake asset copied
/// straight from the bundle, or an HTTP(S) download that goes to disk as it arrives rather than
/// buffering the whole file in memory. Checks the HTTP status before treating the response as the
/// file: a server's error page is never saved as if it were the attachment.
public func streamAttachment(_ source: MediaSource, to destination: URL) async throws {
    switch source {
    case .fakeAsset(let name):
        guard let url = FakeMedia.url(forAssetNamed: name) else { throw AttachmentIOError("That sample file is missing from the app.") }
        try? FileManager.default.removeItem(at: destination)
        do { try FileManager.default.copyItem(at: url, to: destination) } catch { throw AttachmentIOError("Couldn't read the file.") }
    case .remote(let url):
        let (tempURL, response): (URL, URLResponse)
        do { (tempURL, response) = try await URLSession.shared.download(from: url) }
        catch { throw AttachmentIOError("Couldn't reach the file.") }
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            let code = (response as? HTTPURLResponse)?.statusCode ?? 0
            try? FileManager.default.removeItem(at: tempURL)
            throw AttachmentIOError("The server answered with \(code).")
        }
        try? FileManager.default.removeItem(at: destination)
        do { try FileManager.default.moveItem(at: tempURL, to: destination) }
        catch { throw AttachmentIOError("Couldn't save the file.") }
    }
}

/// `name` (an attachment's display filename), stripped to characters safe for a local file name.
/// Keeping "." lets a real extension through, but a name made up of nothing but dots (`"."`, `".."`,
/// `"..."`) would otherwise survive unchanged and, used as a lone path component, name the parent
/// directory instead of a file in it — so that case falls back to "file" too.
public func safeAttachmentFileName(_ name: String) -> String {
    let allowed = name.filter { $0.isLetter || $0.isNumber || ".-_ ".contains($0) }
    let meaningful = allowed.trimmingCharacters(in: CharacterSet(charactersIn: "."))
    return meaningful.isEmpty ? "file" : allowed
}

/// Coalesces concurrent fetches of the same attachment (opening Quick Look while a download of the
/// same file is also in flight, say) onto one `streamAttachment` call, and keeps each attachment's
/// cached copy in its own folder named by id: two attachments that happen to share a display filename
/// can never collide or overwrite each other.
actor AttachmentDownloadCoordinator {
    static let shared = AttachmentDownloadCoordinator()
    private var inFlight: [ID: Task<URL, Error>] = [:]

    func fetch(_ source: MediaSource, id: ID, filename: String) async throws -> URL {
        if let existing = inFlight[id] { return try await existing.value }
        let task = Task<URL, Error> { try await Self.perform(source, id: id, filename: filename) }
        inFlight[id] = task
        defer { inFlight[id] = nil }
        return try await task.value
    }

    private static func perform(_ source: MediaSource, id: ID, filename: String) async throws -> URL {
        let base = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0].appendingPathComponent("thehideout-attachments", isDirectory: true)
        let dir = base.appendingPathComponent(String(id), isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let destination = dir.appendingPathComponent(safeAttachmentFileName(filename))
        try await streamAttachment(source, to: destination)
        return destination
    }
}

/// Downloads or copies `source` into the app's cache (under `Caches/thehideout-attachments/<id>`),
/// named `filename`; a share sheet, Quick Look preview or "save to Photos" all start from this local
/// copy. `id` is the attachment's own id, so two attachments never share a destination file even if
/// their display filenames are identical, and concurrent calls for the same attachment share one fetch.
public func fetchAttachmentToCache(_ source: MediaSource, id: ID, filename: String) async throws -> URL {
    try await AttachmentDownloadCoordinator.shared.fetch(source, id: id, filename: filename)
}

/// Deletes a cache directory's files older than `maxAge`, or past `maxBytes` total (oldest first),
/// walking subdirectories (an attachment's own `<id>` folder, say) too.
private func pruneCacheDirectory(named name: String, maxAge: TimeInterval, maxBytes: Int) {
    let fm = FileManager.default
    let dir = fm.urls(for: .cachesDirectory, in: .userDomainMask)[0].appendingPathComponent(name, isDirectory: true)
    let keys: [URLResourceKey] = [.fileSizeKey, .contentModificationDateKey, .isDirectoryKey]
    guard let enumerator = fm.enumerator(at: dir, includingPropertiesForKeys: keys) else { return }
    let now = Date()
    var entries: [(url: URL, size: Int, date: Date)] = []
    for case let url as URL in enumerator {
        guard let v = try? url.resourceValues(forKeys: Set(keys)), v.isDirectory != true else { continue }
        let date = v.contentModificationDate ?? .distantPast
        if now.timeIntervalSince(date) > maxAge {
            try? fm.removeItem(at: url)
            continue
        }
        entries.append((url, v.fileSize ?? 0, date))
    }
    entries.sort { $0.date < $1.date }
    var total = entries.reduce(0) { $0 + $1.size }
    for entry in entries {
        if total <= maxBytes { break }
        try? fm.removeItem(at: entry.url)
        total -= entry.size
    }
}

/// Call once on app start (ATT-04/ATT-12 hygiene): evicts cached downloaded attachments older than a
/// day, or past `maxBytes` total.
public func pruneAttachmentCache(maxAge: TimeInterval = 86_400, maxBytes: Int = 200 * 1024 * 1024) {
    pruneCacheDirectory(named: "thehideout-attachments", maxAge: maxAge, maxBytes: maxBytes)
}

/// Call once on app start (ATT-01/02 hygiene): evicts `MediaImageCache`'s disk-cached pictures older
/// than a week, or past `maxBytes` total.
public func pruneMediaImageCache(maxAge: TimeInterval = 7 * 86_400, maxBytes: Int = 150 * 1024 * 1024) {
    pruneCacheDirectory(named: "thehideout-media-images", maxAge: maxAge, maxBytes: maxBytes)
}

/// Whether a load of attachment picture bytes succeeded, failed to even fetch (worth a retry), or
/// fetched fine but could not be decoded as an image (an SVG or other unsupported format: retrying
/// would just fail the same way again).
public enum MediaLoadOutcome: Sendable {
    case loaded(UIImage)
    case fetchFailed
    case undecodable
}

/// A small in-memory + disk cache of decoded attachment thumbnails/posters, keyed by `MediaSource`.
/// `AsyncImage` alone is not enough here: it has no disk cache of its own, so the same picture would
/// be refetched on every scroll of the transcript. One cache instance is shared process-wide.
public actor MediaImageCache {
    public static let shared = MediaImageCache()

    // `NSCache` is thread-safe on its own: marking these `nonisolated(unsafe)` lets `cachedImage(for:)`
    // (and the key/path helpers it needs) peek synchronously from any thread, without an actor hop, so
    // a view can show an already-cached picture at creation with no one-frame gap waiting for a `.task`.
    private nonisolated(unsafe) let memory = NSCache<NSString, UIImage>()
    private let directory: URL
    private var inFlight: [String: Task<MediaLoadOutcome, Never>] = [:]

    /// The longest side a decoded picture is kept at: a thumbnail or a poster frame never needs more,
    /// and a full-resolution upload decoded at its native size would otherwise bloat memory for no
    /// visible benefit.
    private static let maxDecodedPixelSize: CGFloat = 800

    init() {
        directory = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0].appendingPathComponent("thehideout-media-images", isDirectory: true)
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        // Capped by estimated decoded bytes, not by picture count: a handful of full-bleed photos
        // should not starve the memory cache for the rest of the transcript's thumbnails.
        memory.totalCostLimit = 64 * 1024 * 1024
    }

    private nonisolated func key(for source: MediaSource) -> String {
        switch source {
        case .remote(let url): return url.absoluteString
        case .fakeAsset(let name): return "fakeasset:\(name)"
        }
    }

    private nonisolated func diskURL(_ key: String) -> URL {
        // A cache key, not a security boundary: percent-encoding keeps it one path component.
        let name = key.addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? String(key.hashValue)
        return directory.appendingPathComponent(name)
    }

    /// A synchronous, thread-safe peek at the in-memory cache only: nil doesn't mean the picture will
    /// fail to load, only that `outcome(for:)`/`image(for:)` have not been awaited yet.
    public nonisolated func cachedImage(for source: MediaSource) -> UIImage? {
        memory.object(forKey: key(for: source) as NSString)
    }

    /// The outcome for `source`: memory, then disk, then a fetch — coalesced so concurrent requests for
    /// the same picture (several grid tiles scrolling into view at once) share one fetch. The fetch and
    /// decode run detached from this actor's executor, so one slow picture never queues up the rest.
    public func outcome(for source: MediaSource) async -> MediaLoadOutcome {
        let k = key(for: source)
        if let cached = memory.object(forKey: k as NSString) { return .loaded(cached) }
        if let existing = inFlight[k] { return await existing.value }
        let diskPath = diskURL(k)
        let task = Task<MediaLoadOutcome, Never>.detached(priority: .userInitiated) {
            if let data = try? Data(contentsOf: diskPath) {
                if let image = Self.decodeDownsampled(data) { return .loaded(image) }
                return .undecodable
            }
            guard let data = await Self.fetchData(source) else { return .fetchFailed }
            guard let image = Self.decodeDownsampled(data) else { return .undecodable }
            try? data.write(to: diskPath)
            return .loaded(image)
        }
        inFlight[k] = task
        let result = await task.value
        inFlight[k] = nil
        if case .loaded(let image) = result { memory.setObject(image, forKey: k as NSString, cost: Self.cost(of: image)) }
        return result
    }

    /// Convenience for callers that only need the picture, not why it is missing (the viewer).
    public func image(for source: MediaSource) async -> UIImage? {
        if case .loaded(let image) = await outcome(for: source) { return image }
        return nil
    }

    private static func cost(of image: UIImage) -> Int {
        guard let cg = image.cgImage else { return 0 }
        return cg.bytesPerRow * cg.height
    }

    private static func fetchData(_ source: MediaSource) async -> Data? {
        switch source {
        case .fakeAsset(let name):
            guard let url = FakeMedia.url(forAssetNamed: name) else { return nil }
            return try? Data(contentsOf: url)
        case .remote(let url):
            guard let (data, response) = try? await URLSession.shared.data(from: url),
                  let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else { return nil }
            return data
        }
    }

    /// Decodes `data` downsampled to at most `maxDecodedPixelSize` on the long side, with EXIF
    /// orientation applied (`kCGImageSourceCreateThumbnailWithTransform`) — the raw bytes stay on disk
    /// as fetched (no re-encoding, which would otherwise bloat JPEGs and could drop metadata).
    private static func decodeDownsampled(_ data: Data) -> UIImage? {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil) else { return nil }
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: maxDecodedPixelSize,
        ]
        guard let cgImage = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else { return nil }
        return UIImage(cgImage: cgImage)
    }
}
