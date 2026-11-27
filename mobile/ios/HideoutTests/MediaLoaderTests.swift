import HideoutCore
import HideoutUI
import XCTest

/// A stubbed `URLProtocol` so `streamAttachment`'s HTTP-status check can be tested without a real
/// server: registering it affects `URLSession.shared` process-wide, so tests always unregister it.
private final class StubURLProtocol: URLProtocol {
    // Test-only, set before registering and read only while registered (tests never run concurrently
    // with each other within this file), so the actual access pattern is safe despite the global state.
    nonisolated(unsafe) static var statusCode = 200
    nonisolated(unsafe) static var body = Data()

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        let response = HTTPURLResponse(url: request.url!, statusCode: Self.statusCode, httpVersion: "HTTP/1.1", headerFields: nil)!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: Self.body)
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}
}

final class MediaLoaderTests: XCTestCase {
    // MARK: safeAttachmentFileName

    func testSafeAttachmentFileNameKeepsAnOrdinaryName() {
        XCTAssertEqual(safeAttachmentFileName("meeting-notes.txt"), "meeting-notes.txt")
        XCTAssertEqual(safeAttachmentFileName("report final v2.pdf"), "report final v2.pdf")
    }

    /// A name made of nothing but dots would otherwise survive the character filter unchanged and,
    /// used as a lone path component, name the parent directory instead of a file in it.
    func testSafeAttachmentFileNameRejectsDotOnlyNames() {
        XCTAssertEqual(safeAttachmentFileName("."), "file")
        XCTAssertEqual(safeAttachmentFileName(".."), "file")
        XCTAssertEqual(safeAttachmentFileName("...."), "file")
        XCTAssertEqual(safeAttachmentFileName(""), "file")
    }

    func testSafeAttachmentFileNameStripsSlashesOutOfATraversalAttempt() {
        let name = safeAttachmentFileName("../../etc/passwd")
        XCTAssertFalse(name.contains("/"))
        XCTAssertNotEqual(name, ".")
        XCTAssertNotEqual(name, "..")
    }

    // MARK: streamAttachment

    func testStreamAttachmentRejectsANonTwoHundredStatus() async {
        URLProtocol.registerClass(StubURLProtocol.self)
        defer { URLProtocol.unregisterClass(StubURLProtocol.self) }
        StubURLProtocol.statusCode = 404
        StubURLProtocol.body = Data("not found".utf8)
        let destination = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: destination) }

        do {
            try await streamAttachment(.remote(URL(string: "https://example.com/media/missing")!), to: destination)
            XCTFail("a 404 must be refused, not saved as the file")
        } catch let error as AttachmentIOError {
            XCTAssertEqual(error.message, "The server answered with 404.")
        } catch {
            XCTFail("wrong error type: \(error)")
        }
        XCTAssertFalse(FileManager.default.fileExists(atPath: destination.path))
    }

    func testStreamAttachmentSavesA2xxResponse() async throws {
        URLProtocol.registerClass(StubURLProtocol.self)
        defer { URLProtocol.unregisterClass(StubURLProtocol.self) }
        StubURLProtocol.statusCode = 200
        StubURLProtocol.body = Data("hello".utf8)
        let destination = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: destination) }

        try await streamAttachment(.remote(URL(string: "https://example.com/media/ok")!), to: destination)
        XCTAssertEqual(try Data(contentsOf: destination), Data("hello".utf8))
    }

    // MARK: fetchAttachmentToCache

    /// Two attachments sharing a display filename must never collide: each gets its own folder by id.
    func testFetchAttachmentToCacheIsolatesByAttachmentId() async throws {
        let source = MediaSource.fakeAsset(name: "notes-1.txt")
        let urlA = try await fetchAttachmentToCache(source, id: 9_001, filename: "shared-name.txt")
        let urlB = try await fetchAttachmentToCache(source, id: 9_002, filename: "shared-name.txt")
        defer {
            try? FileManager.default.removeItem(at: urlA.deletingLastPathComponent())
            try? FileManager.default.removeItem(at: urlB.deletingLastPathComponent())
        }
        XCTAssertNotEqual(urlA, urlB)
        XCTAssertTrue(urlA.path.contains("/9001/"))
        XCTAssertTrue(urlB.path.contains("/9002/"))
        XCTAssertTrue(FileManager.default.fileExists(atPath: urlA.path))
        XCTAssertTrue(FileManager.default.fileExists(atPath: urlB.path))
    }

    // MARK: pruneAttachmentCache

    func testPruneAttachmentCacheEvictsOlderThanMaxAge() throws {
        let dir = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("thehideout-attachments", isDirectory: true)
            .appendingPathComponent("prune-test-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let oldFile = dir.appendingPathComponent("old.txt")
        let newFile = dir.appendingPathComponent("new.txt")
        try Data("old".utf8).write(to: oldFile)
        try Data("new".utf8).write(to: newFile)
        try FileManager.default.setAttributes([.modificationDate: Date().addingTimeInterval(-1_000)], ofItemAtPath: oldFile.path)
        try FileManager.default.setAttributes([.modificationDate: Date()], ofItemAtPath: newFile.path)

        pruneAttachmentCache(maxAge: 500, maxBytes: .max)

        XCTAssertFalse(FileManager.default.fileExists(atPath: oldFile.path), "older than maxAge is evicted")
        XCTAssertTrue(FileManager.default.fileExists(atPath: newFile.path), "within maxAge survives")
    }

    func testPruneAttachmentCacheEvictsOldestFirstOverTheByteBudget() throws {
        let dir = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("thehideout-attachments", isDirectory: true)
            .appendingPathComponent("prune-test-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let olderFile = dir.appendingPathComponent("older.txt")
        let newerFile = dir.appendingPathComponent("newer.txt")
        try Data(repeating: 0, count: 10).write(to: olderFile)
        try Data(repeating: 0, count: 10).write(to: newerFile)
        try FileManager.default.setAttributes([.modificationDate: Date().addingTimeInterval(-10)], ofItemAtPath: olderFile.path)
        try FileManager.default.setAttributes([.modificationDate: Date()], ofItemAtPath: newerFile.path)

        pruneAttachmentCache(maxAge: 86_400, maxBytes: 12)

        XCTAssertFalse(FileManager.default.fileExists(atPath: olderFile.path), "the oldest file is evicted first over budget")
        XCTAssertTrue(FileManager.default.fileExists(atPath: newerFile.path))
    }

    // MARK: MediaImageCache

    /// Concurrent requests for the same uncached picture must all resolve correctly (coalesced onto
    /// one fetch internally) without racing or corrupting the cache.
    func testMediaImageCacheHandlesConcurrentRequestsForTheSameSource() async {
        let source = MediaSource.fakeAsset(name: "square-1.jpg")
        async let a = MediaImageCache.shared.image(for: source)
        async let b = MediaImageCache.shared.image(for: source)
        async let c = MediaImageCache.shared.image(for: source)
        let results = await [a, b, c]
        for result in results { XCTAssertNotNil(result) }
        XCTAssertNotNil(MediaImageCache.shared.cachedImage(for: source), "a synchronous peek now finds the warmed memory cache")
    }

    func testMediaImageCacheOutcomeIsFetchFailedForAMissingFakeAsset() async {
        let outcome = await MediaImageCache.shared.outcome(for: .fakeAsset(name: "does-not-exist.jpg"))
        guard case .fetchFailed = outcome else { return XCTFail("expected .fetchFailed, got \(outcome)") }
    }
}
