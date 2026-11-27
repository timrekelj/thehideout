import Foundation

// Composer attachment uploads (ATT-09/ATT-11, phase 3b-2), a Swift port of Android's
// `store/AttachDraft.kt` + `ConnectApiClient.uploadAttachment`/`AttachmentJson.kt`. Unlike Android's
// `UploadSource` (an `InputStream` opened fresh per attempt), a picked file is copied into the app's
// own tmp directory once (`AttachPicker`, HideoutUI) and every upload attempt streams from that same
// `fileURL`: `URLSession`'s `upload(for:fromFile:)` already streams a file without reading it whole
// into memory, so there is no need for a chunked-reader abstraction here.

/// The server's per-upload cap unless it says otherwise (`server/src/media/attachments.rs`).
public let defaultMaxUploadBytes: UInt64 = 50 * 1024 * 1024
/// How many uploads run at once; the rest wait their turn.
public let maxConcurrentUploads = 3

/// Where one attached file is in its upload (the Swift twin of Android's `AttachUploadPhase`).
public enum AttachUploadPhase: Sendable, Equatable {
    case queued
    case uploading
    case ready
    case failed
}

/// `.uploading`'s `sent`/`total`; `.ready`'s `pendingID` links `SendMessage`; `.failed`'s `reason` is
/// shown in the chip.
public struct AttachUploadState: Sendable, Equatable {
    public var phase: AttachUploadPhase
    public var sent: UInt64
    public var total: UInt64
    public var pendingID: ID?
    public var reason: String?
    public init(phase: AttachUploadPhase, sent: UInt64 = 0, total: UInt64 = 0, pendingID: ID? = nil, reason: String? = nil) {
        self.phase = phase; self.sent = sent; self.total = total; self.pendingID = pendingID; self.reason = reason
    }
}

/// One file the platform picker, camera or document picker handed the composer, already copied into
/// the app's own tmp directory (so it can be re-streamed on retry even if the original picker item's
/// security scope has since closed).
public struct PickedFile: Sendable, Equatable {
    public var name: String
    public var contentType: String
    public var size: UInt64
    /// Shown in the tray and, once sent, in the pending message: a local file URL's string for an
    /// image, nil otherwise.
    public var preview: String?
    public var fileURL: URL
    public init(name: String, contentType: String, size: UInt64, preview: String?, fileURL: URL) {
        self.name = name; self.contentType = contentType; self.size = size; self.preview = preview; self.fileURL = fileURL
    }
}

/// One file in a channel's composer tray, from the moment it is picked to the moment it is sent or
/// removed (desktop's `DraftFile`).
public struct DraftFile: Sendable, Equatable, Identifiable {
    public var id: ID
    public var name: String
    public var contentType: String
    public var size: UInt64
    public var preview: String?
    public var fileURL: URL
    public var state: AttachUploadState
    public init(id: ID, name: String, contentType: String, size: UInt64, preview: String?, fileURL: URL, state: AttachUploadState) {
        self.id = id; self.name = name; self.contentType = contentType; self.size = size
        self.preview = preview; self.fileURL = fileURL; self.state = state
    }
}

/// One channel's files waiting to be sent (desktop's `Draft`).
public struct AttachDraft: Sendable, Equatable {
    public var files: [DraftFile] = []
    public var note: String?
    public init(files: [DraftFile] = [], note: String? = nil) { self.files = files; self.note = note }

    /// Blocks `send`: at least one file has not reached `.ready` or `.failed` yet.
    public var uploading: Bool { files.contains { $0.state.phase == .queued || $0.state.phase == .uploading } }
    public var readyCount: Int { files.count { $0.state.phase == .ready } }
}

/// A sent file's local preview, kept on `Outgoing` until the server's own message replaces it
/// (desktop's `OutFile`).
public struct PendingPreview: Sendable, Equatable, Identifiable {
    public var name: String
    public var contentType: String
    public var size: UInt64
    public var preview: String?
    public var id: String { name + preview.map { ":\($0)" }.orEmpty }
    public init(name: String, contentType: String, size: UInt64, preview: String?) {
        self.name = name; self.contentType = contentType; self.size = size; self.preview = preview
    }
}

private extension Optional where Wrapped == String {
    var orEmpty: String { self ?? "" }
}

/// A pending upload: `POST /attachments` answers one; `SendMessage.attachmentIDs` links it to a message.
public struct PendingAttachment: Sendable, Equatable {
    public var id: ID
    public var filename: String
    public var contentType: String
    public var size: UInt64
    public var expiresAt: Date
    public init(id: ID, filename: String, contentType: String, size: UInt64, expiresAt: Date) {
        self.id = id; self.filename = filename; self.contentType = contentType; self.size = size; self.expiresAt = expiresAt
    }
}

public enum PendingAttachmentParseError: Error, Equatable { case malformed(String) }

/// The protobuf-JSON mapping of `PendingAttachment` (`attachment.proto`): camelCase, ids and sizes as
/// strings (64-bit integers are strings in that mapping) or numbers, `expiresAt` an RFC3339 string.
public func parsePendingAttachment(_ data: Data) throws -> PendingAttachment {
    guard let obj = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else {
        throw PendingAttachmentParseError.malformed("bad upload response")
    }
    func asUInt64(_ key: String) -> UInt64? {
        if let s = obj[key] as? String { return UInt64(s) }
        if let n = obj[key] as? NSNumber { return n.uint64Value }
        return nil
    }
    guard let id = asUInt64("id") else { throw PendingAttachmentParseError.malformed("bad upload response: missing id") }
    let filename = obj["filename"] as? String ?? ""
    let contentType = obj["contentType"] as? String ?? ""
    let size = asUInt64("size") ?? 0
    let expiresAt = (obj["expiresAt"] as? String).flatMap(parseTimestamp) ?? Date(timeIntervalSince1970: 0)
    return PendingAttachment(id: id, filename: filename, contentType: contentType, size: size, expiresAt: expiresAt)
}

/// Parses with and without fractional seconds, like the server's `Timestamp` JSON mapping sends. A
/// fresh formatter each call (not a cached static): `ISO8601DateFormatter` isn't `Sendable`, and this
/// runs rarely enough (once per upload reply) that reuse isn't worth a global-actor-isolated cache.
private func parseTimestamp(_ s: String) -> Date? {
    let fractional = ISO8601DateFormatter()
    fractional.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    if let d = fractional.date(from: s) { return d }
    return ISO8601DateFormatter().date(from: s)
}

/// What a refused upload's chip says (desktop's `attach::upload_error`).
public func uploadErrorMessage(_ e: AppError) -> String {
    switch e {
    case .tooLarge: return "Too large"
    case .network: return "Upload failed"
    case .permissionDenied: return "You can't attach files here"
    case .rateLimited: return "Slow down, try again"
    default:
        var s = sentence(e.description)
        if s.hasSuffix(".") { s.removeLast() }
        return s
    }
}
