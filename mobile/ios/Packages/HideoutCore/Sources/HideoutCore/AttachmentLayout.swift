import Foundation

/// What a file is, by its content type then its extension (desktop's `file_kind`, `attachments.rs`).
public enum FileKind: Sendable, Equatable, Hashable {
    case image, video, audio, pdf, archive, text, code, doc, other
}

/// Files one message may carry (ATT-09, desktop's `attach::MAX_FILES`).
public let maxAttachments = 10

/// Widest and tallest an inline picture or video grows, in points.
public let mediaMaxW: Double = 400
public let mediaMaxH: Double = 300
/// The gap between grid tiles, in points.
public let gridGap: Double = 4
/// Height of a two-or-more-column grid, in points.
public let gridH: Double = 240
/// The widest a file card or audio row grows, in points.
public let cardW: Double = 340

/// The kind of a file by its content type, then its filename's extension.
public func fileKind(contentType: String, filename: String) -> FileKind {
    let ct = contentType.lowercased()
    let ext = (filename as NSString).pathExtension.lowercased()
    switch true {
    case ct.hasPrefix("image/"): return .image
    case ct.hasPrefix("video/") || ["mp4", "mov", "webm", "m4v"].contains(ext): return .video
    case ct.hasPrefix("audio/") || ["mp3", "m4a", "wav", "flac", "ogg", "aac"].contains(ext): return .audio
    case ct == "application/pdf" || ext == "pdf": return .pdf
    case ct.contains("zip") || ct.contains("tar") || ct.contains("compressed") || ct.contains("7z")
        || ["zip", "tar", "gz", "tgz", "7z", "rar", "bz2", "xz"].contains(ext): return .archive
    case ct.contains("json") || ct.contains("xml") || ct.contains("javascript")
        || ["rs", "js", "ts", "py", "json", "toml", "yaml", "yml", "html", "css", "sh", "c", "cpp", "go"].contains(ext): return .code
    case ct.hasPrefix("text/") || ["txt", "md", "csv", "log"].contains(ext): return .text
    case ct.contains("word") || ct.contains("officedocument") || ct.contains("opendocument")
        || ["doc", "docx", "odt", "xls", "xlsx", "ppt", "pptx"].contains(ext): return .doc
    default: return .other
    }
}

/// Whether the server serves it inline, so "Open" shows it rather than downloading it.
public func opensInline(_ kind: FileKind) -> Bool { kind == .image || kind == .video || kind == .audio || kind == .pdf }

/// "12 B", "3.4 KB", "5.0 MB", "1.2 GB" (powers of 1024, one decimal from KB up).
public func humanSize(_ bytes: UInt64) -> String {
    if bytes < 1024 { return "\(bytes) B" }
    let units = ["KB", "MB", "GB", "TB"]
    var v = Double(bytes) / 1024.0
    var unit = 0
    while v >= 1024.0, unit + 1 < units.count {
        v /= 1024.0
        unit += 1
    }
    return String(format: "%.1f %@", v, units[unit])
}

/// "0:07", "12:03", "1:02:03".
public func clockLabel(_ seconds: Double) -> String {
    let s = seconds.isFinite ? max(Int64(seconds), 0) : 0
    let h = s / 3600, m = (s / 60) % 60, sec = s % 60
    if h > 0 { return String(format: "%d:%02d:%02d", h, m, sec) }
    return String(format: "%d:%02d", m, sec)
}

/// `(w, h)` scaled down (never up past natural) to fit `maxW` x `maxH`, keeping its aspect.
public func fitSize(natural: (Int, Int)?, maxW: Double, maxH: Double) -> (Double, Double) {
    let nw: Double, nh: Double
    if let natural, natural.0 > 0, natural.1 > 0 {
        nw = Double(natural.0); nh = Double(natural.1)
    } else {
        nw = maxW; nh = maxW * 9 / 16
    }
    var scale = min(min(maxW / nw, maxH / nh), 1)
    // A thumbnail is never a speck: a tiny picture still gets a legible box.
    if nw * scale < 96, nh * scale < 96 {
        scale = min(min(96 / nw, 96 / nh), min(maxW / nw, maxH / nh))
    }
    return ((nw * scale).rounded(), (nh * scale).rounded())
}

/// A tile's place in a grid, in points from the grid's top-left corner.
public struct Cell: Sendable, Equatable {
    public var x: Double
    public var y: Double
    public var w: Double
    public var h: Double
    public init(x: Double, y: Double, w: Double, h: Double) { self.x = x; self.y = y; self.w = w; self.h = h }
}

/// Where `n` pictures sit: one alone keeps its aspect (within `mediaMaxW` x `mediaMaxH`); two sit side
/// by side; three put the first tall on the left; four or more are a 2x2 (a fifth and beyond are reached
/// through the lightbox, and the fourth tile says how many more there are). Returns the grid's size and its cells.
public func imageGrid(n: Int, single: (Int, Int)? = nil, maxW: Double = mediaMaxW) -> ((Double, Double), [Cell]) {
    if n == 0 { return ((0, 0), []) }
    if n == 1 {
        let (w, h) = fitSize(natural: single, maxW: maxW, maxH: mediaMaxH)
        return ((w, h), [Cell(x: 0, y: 0, w: w, h: h)])
    }
    let w = maxW
    let half = (w - gridGap) / 2
    let cells: [Cell]
    switch n {
    case 2:
        cells = [Cell(x: 0, y: 0, w: half, h: gridH), Cell(x: half + gridGap, y: 0, w: half, h: gridH)]
    case 3:
        let small = (gridH - gridGap) / 2
        cells = [
            Cell(x: 0, y: 0, w: half, h: gridH),
            Cell(x: half + gridGap, y: 0, w: half, h: small),
            Cell(x: half + gridGap, y: small + gridGap, w: half, h: small),
        ]
    default:
        let small = (gridH - gridGap) / 2
        let right = half + gridGap
        let low = small + gridGap
        cells = [
            Cell(x: 0, y: 0, w: half, h: small),
            Cell(x: right, y: 0, w: half, h: small),
            Cell(x: 0, y: low, w: half, h: small),
            Cell(x: right, y: low, w: half, h: small),
        ]
    }
    return ((w, gridH), cells)
}

/// Where an attachment's bytes come from: the server's `/media/<key>`, or one of the fake backend's
/// bundled sample files.
public enum MediaSource: Sendable, Equatable {
    case remote(URL)
    case fakeAsset(name: String)
}

/// `mediaSource` refused a key.
public enum MediaKeyError: Error, Equatable {
    /// The key names a scheme of its own (`file://`, a foreign host): never passed through.
    case foreignScheme(String)
    /// The key names one of the fake backend's bundled assets, but the caller did not say this is the
    /// fake backend (`allowFakeAssets: false`): a real server's response is never trusted to pick this
    /// path, so a misbehaving or compromised server can't make the app load a bundled local file.
    case fakeAssetNotAllowed(String)
}

/// The fake backend's bundled-asset keys pass through `mediaSource` as `.fakeAsset`, when `allowFakeAssets`
/// says so; nothing else may.
public let fakeAssetPrefix = "fakeasset:"

/// `{base}/media/{key}`, unless `key` names one of the fake backend's bundled assets (`fakeAssetPrefix`)
/// and `allowFakeAssets` is true (only ever passed as true by UI code running under `--fake`/`--gallery`).
/// Any other scheme (`file://`, a foreign host) is rejected rather than passed through, so a malicious
/// media key can't act as a tracking pixel or read a local file in production; `key` is percent-encoded
/// into one opaque path segment, so it can't contain a `/` or `..` that would change which path the
/// request actually reaches.
public func mediaSource(base: URL, key: String, allowFakeAssets: Bool = false) throws -> MediaSource {
    if key.hasPrefix(fakeAssetPrefix) {
        guard allowFakeAssets else { throw MediaKeyError.fakeAssetNotAllowed(key) }
        return .fakeAsset(name: String(key.dropFirst(fakeAssetPrefix.count)))
    }
    if key.contains("://") { throw MediaKeyError.foreignScheme(key) }
    guard var components = URLComponents(url: base, resolvingAgainstBaseURL: false) else { return .remote(base) }
    let encodedKey = key.addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? key
    // `percentEncodedPath`, not `path`: the latter re-escapes its input, which would turn the `%2F`
    // etc. already in `encodedKey` into `%252F` instead of keeping the key one opaque segment.
    components.percentEncodedPath += "/media/" + encodedKey
    return .remote(components.url ?? base)
}
