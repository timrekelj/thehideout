import Foundation
import HideoutCore
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
public func safeAttachmentFileName(_ name: String) -> String {
    let allowed = name.filter { $0.isLetter || $0.isNumber || ".-_ ".contains($0) }
    return allowed.isEmpty ? "file" : allowed
}

/// Downloads or copies `source` into the app's cache (under `Caches/thehideout-attachments`), named
/// `filename`; a share sheet, Quick Look preview or "save to Photos" all start from this local copy.
public func fetchAttachmentToCache(_ source: MediaSource, filename: String) async throws -> URL {
    let dir = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0].appendingPathComponent("thehideout-attachments", isDirectory: true)
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    let destination = dir.appendingPathComponent(safeAttachmentFileName(filename))
    try await streamAttachment(source, to: destination)
    return destination
}

/// Deletes cached attachment files older than a day, or past `maxBytes` total (oldest first): call once on app start.
public func pruneAttachmentCache(maxAge: TimeInterval = 86_400, maxBytes: Int = 200 * 1024 * 1024) {
    let fm = FileManager.default
    let dir = fm.urls(for: .cachesDirectory, in: .userDomainMask)[0].appendingPathComponent("thehideout-attachments", isDirectory: true)
    let keys: Set<URLResourceKey> = [.fileSizeKey, .contentModificationDateKey]
    guard let files = try? fm.contentsOfDirectory(at: dir, includingPropertiesForKeys: Array(keys)) else { return }
    let now = Date()
    var entries = files.compactMap { url -> (URL, Int, Date)? in
        guard let v = try? url.resourceValues(forKeys: keys) else { return nil }
        return (url, v.fileSize ?? 0, v.contentModificationDate ?? .distantPast)
    }
    entries = entries.filter { url, _, date in
        if now.timeIntervalSince(date) > maxAge { try? fm.removeItem(at: url); return false }
        return true
    }
    entries.sort { $0.2 < $1.2 }
    var total = entries.reduce(0) { $0 + $1.1 }
    for entry in entries {
        if total <= maxBytes { break }
        try? fm.removeItem(at: entry.0)
        total -= entry.1
    }
}

/// A small in-memory + disk cache of decoded attachment thumbnails/pictures, keyed by `MediaSource`.
/// `AsyncImage` alone is not enough here: it has no disk cache of its own, so the same picture would
/// be refetched on every scroll of the transcript. One cache instance is shared process-wide.
public actor MediaImageCache {
    public static let shared = MediaImageCache()

    private let memory = NSCache<NSString, UIImage>()
    private let directory: URL
    private var inFlight: [String: Task<UIImage?, Never>] = [:]

    init() {
        directory = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0].appendingPathComponent("thehideout-media-images", isDirectory: true)
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        memory.countLimit = 200
    }

    private func key(for source: MediaSource) -> String {
        switch source {
        case .remote(let url): return url.absoluteString
        case .fakeAsset(let name): return "fakeasset:\(name)"
        }
    }

    private func diskURL(_ key: String) -> URL {
        // A cache key, not a security boundary: percent-encoding keeps it one path component.
        let name = key.addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? String(key.hashValue)
        return directory.appendingPathComponent(name)
    }

    /// The image for `source`: memory, then disk, then a fetch — coalesced so concurrent requests for
    /// the same picture (several image-grid tiles scrolling into view at once) share one fetch.
    public func image(for source: MediaSource) async -> UIImage? {
        let k = key(for: source)
        if let cached = memory.object(forKey: k as NSString) { return cached }
        if let existing = inFlight[k] { return await existing.value }
        let task = Task<UIImage?, Never> { [weak self] in
            guard let self else { return nil }
            if let data = try? Data(contentsOf: await self.diskURL(k)), let image = UIImage(data: data) {
                await self.store(image, memoryKey: k)
                return image
            }
            guard let image = await self.fetch(source) else { return nil }
            await self.store(image, memoryKey: k)
            if let data = image.pngData() { try? data.write(to: await self.diskURL(k)) }
            return image
        }
        inFlight[k] = task
        let result = await task.value
        inFlight[k] = nil
        return result
    }

    private func store(_ image: UIImage, memoryKey: String) { memory.setObject(image, forKey: memoryKey as NSString) }

    private func fetch(_ source: MediaSource) async -> UIImage? {
        switch source {
        case .fakeAsset(let name):
            guard let url = FakeMedia.url(forAssetNamed: name), let data = try? Data(contentsOf: url) else { return nil }
            return UIImage(data: data)
        case .remote(let url):
            guard let (data, response) = try? await URLSession.shared.data(from: url),
                  let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else { return nil }
            return UIImage(data: data)
        }
    }
}
