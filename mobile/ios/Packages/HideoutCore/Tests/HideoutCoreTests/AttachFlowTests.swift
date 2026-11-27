import Foundation
import Testing
@testable import HideoutCore

/// A small temp file with `bytes` of content, for `PickedFile.fileURL` (ATT-09's uploads stream from a
/// file, never from memory).
func tempFile(bytes: Int = 16) -> URL {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try! Data(repeating: 0x61, count: bytes).write(to: url)
    return url
}

func picked(_ name: String = "photo.jpg", contentType: String = "image/jpeg", size: UInt64 = 1000, preview: String? = nil) -> PickedFile {
    PickedFile(name: name, contentType: contentType, size: size, preview: preview, fileURL: tempFile(bytes: Int(size)))
}

@Suite("Attachments: the reducer's upload state machine (ATT-09)")
struct AttachReducerTests {
    @Test func pickingAFileQueuesItAndAsksTheStoreToUploadIt() async throws {
        var s = try await makeState()
        s.attachFiles(channel: Ids.general, files: [picked("a.jpg")])
        let draft = s.attachDraft(Ids.general)
        #expect(draft.files.count == 1 && draft.files[0].state.phase == .queued)
        #expect(draft.uploading)
        #expect(s.outbox.count == 1)
        guard case .uploadAttachment(let id, let channel, let filename, _, _, _) = s.outbox[0] else { Issue.record("wrong effect"); return }
        #expect(channel == Ids.general && filename == "a.jpg" && id == draft.files[0].id)
    }

    @Test func progressThenReadyCarriesThePendingIdForSendMessage() async throws {
        var s = try await makeState()
        s.attachFiles(channel: Ids.general, files: [picked()])
        let id = s.attachDraft(Ids.general).files[0].id
        s.attachStarted(id)
        #expect(s.attachDraft(Ids.general).files[0].state.phase == .uploading)
        s.attachProgress(id, sent: 500, total: 1000)
        #expect(s.attachDraft(Ids.general).files[0].state.sent == 500)
        s.attachUploaded(id, result: .success(PendingAttachment(id: 77, filename: "a.jpg", contentType: "image/jpeg", size: 1000, expiresAt: Date())))
        let f = s.attachDraft(Ids.general).files[0]
        #expect(f.state.phase == .ready && f.state.pendingID == 77)
        #expect(!s.attachDraft(Ids.general).uploading)
        #expect(s.attachDraft(Ids.general).readyCount == 1)
    }

    @Test func aFailedUploadShowsAReasonAndCanBeRetried() async throws {
        var s = try await makeState()
        s.attachFiles(channel: Ids.general, files: [picked()])
        let id = s.attachDraft(Ids.general).files[0].id
        s.attachUploaded(id, result: .failure(.tooLarge))
        #expect(s.attachDraft(Ids.general).files[0].state.phase == .failed)
        #expect(s.attachDraft(Ids.general).files[0].state.reason == "Too large")
        s.outbox.removeAll()
        s.retryAttachment(id)
        #expect(s.attachDraft(Ids.general).files[0].state.phase == .queued)
        #expect(s.outbox.count == 1)
    }

    @Test func removingAQueuedFileAsksToCancelItsUpload() async throws {
        var s = try await makeState()
        s.attachFiles(channel: Ids.general, files: [picked()])
        let id = s.attachDraft(Ids.general).files[0].id
        s.outbox.removeAll()
        let wasRunning: Bool = s.removeAttachment(id)
        #expect(wasRunning)
        #expect(s.attachDraft(Ids.general).files.isEmpty)
        #expect(s.outbox == [.cancelUpload(id: id)])
    }

    @Test func removingAReadyFileAsksForNoCancellation() async throws {
        var s = try await makeState()
        s.attachFiles(channel: Ids.general, files: [picked()])
        let id = s.attachDraft(Ids.general).files[0].id
        s.attachUploaded(id, result: .success(PendingAttachment(id: 1, filename: "a.jpg", contentType: "image/jpeg", size: 1, expiresAt: Date())))
        s.outbox.removeAll()
        let wasRunning = s.removeAttachment(id)
        #expect(!wasRunning)
        #expect(s.outbox.isEmpty)
    }

    @Test func moreThanTenFilesAreRefusedWithANote() async throws {
        var s = try await makeState()
        s.attachFiles(channel: Ids.general, files: (0..<maxAttachments).map { picked("f\($0).jpg") })
        #expect(s.attachDraft(Ids.general).note == nil)
        s.attachFiles(channel: Ids.general, files: [picked("one-too-many.jpg")])
        #expect(s.attachDraft(Ids.general).files.count == maxAttachments)
        #expect(s.attachDraft(Ids.general).note?.contains("up to \(maxAttachments)") == true)
    }

    @Test func aFileOverTheSizeCapIsRefusedLocally() async throws {
        var s = try await makeState()
        s.maxUploadBytes = 100
        s.attachFiles(channel: Ids.general, files: [picked("big.jpg", size: 200)])
        #expect(s.attachDraft(Ids.general).files.isEmpty)
        #expect(s.attachDraft(Ids.general).note?.contains("over the") == true)
        #expect(s.outbox.isEmpty)
    }

    // MARK: send()

    @Test func sendBlocksWhileAFileIsStillQueuedOrUploading() async throws {
        var s = try await makeState()
        s.attachFiles(channel: Ids.general, files: [picked()])
        var ok = s.send("hi", to: Ids.general, at: .now)
        #expect(!ok)
        let id = s.attachDraft(Ids.general).files[0].id
        s.attachStarted(id)
        ok = s.send("hi", to: Ids.general, at: .now)
        #expect(!ok)
    }

    @Test func aFileAloneWithNoTextIsAMessage() async throws {
        var s = try await makeState()
        s.attachFiles(channel: Ids.general, files: [picked("a.jpg")])
        let id = s.attachDraft(Ids.general).files[0].id
        s.attachUploaded(id, result: .success(PendingAttachment(id: 42, filename: "a.jpg", contentType: "image/jpeg", size: 1000, expiresAt: Date())))
        let ok = s.send("", to: Ids.general, at: .now)
        #expect(ok)
        #expect(s.outgoing.first?.attachmentIDs == [42])
        #expect(s.outgoing.first?.attachments.first?.name == "a.jpg")
        // Sent: it leaves the tray, so a second send of text alone no longer carries it.
        #expect(s.attachDraft(Ids.general).files.isEmpty)
    }

    @Test func sendingKeepsAFailedFileInTheTrayButClearsOnlyTheReadyOnes() async throws {
        var s = try await makeState()
        s.attachFiles(channel: Ids.general, files: [picked("ready.jpg"), picked("failed.jpg")])
        let ids = s.attachDraft(Ids.general).files.map(\.id)
        s.attachUploaded(ids[0], result: .success(PendingAttachment(id: 1, filename: "ready.jpg", contentType: "image/jpeg", size: 1, expiresAt: Date())))
        s.attachUploaded(ids[1], result: .failure(.network("reset")))
        let ok = s.send("note", to: Ids.general, at: .now)
        #expect(ok)
        #expect(s.attachDraft(Ids.general).files.count == 1 && s.attachDraft(Ids.general).files[0].name == "failed.jpg")
    }

    @Test func emptyTextWithNoReadyFileIsRefused() async throws {
        var s = try await makeState()
        let ok1 = s.send("", to: Ids.general, at: .now)
        #expect(!ok1)
        let ok2 = s.send("   ", to: Ids.general, at: .now)
        #expect(!ok2)
    }
}

/// The store against the fake backend: concurrency, retry, and the Android-review lesson (a failure
/// after the response starts coming back must resolve, never hang).
@MainActor
@Suite("Attachments: the store's upload runner")
struct AttachStoreTests {
    private func store(fake: FakeBackend = FakeBackend()) -> AppStore {
        AppStore(api: fake, tokens: MemoryTokenStore(token: "t"))
    }

    private func until(_ timeout: Duration = .seconds(5), _ condition: () -> Bool) async {
        let end = ContinuousClock.now + timeout
        while !condition(), ContinuousClock.now < end { try? await Task.sleep(for: .milliseconds(5)) }
    }

    @Test func aPickedFileUploadsAndBecomesReady() async {
        let fake = FakeBackend()
        fake.uploadTicks = 2
        let s = store(fake: fake)
        await s.bootstrap()
        s.attachFiles(channel: Ids.general, files: [picked("a.jpg")])
        await s.settle()
        let f = s.state.attachDraft(Ids.general).files[0]
        #expect(f.state.phase == .ready && f.state.pendingID != nil)
    }

    /// The exact bug class the Android review found (reading the upload's response body can itself
    /// throw): simulated here by a failure that happens after progress already reported bytes sent, on
    /// every attempt (more than `uploadWithRetry`'s 3 extra tries), so the file settles as `.failed`
    /// rather than hanging. `ConnectApiClientUploadTests` covers the real client's equivalent over a
    /// socket; Swift's `async`/`await` has no separate callback step to forget to resume, so this is a
    /// regression guard (bounded `settle()`, a terminal state) rather than a reproduction of a bug iOS
    /// could have in the same shape as Android's OkHttp callback.
    @Test func aFailureAfterProgressResolvesToFailedRatherThanHanging() async {
        let fake = FakeBackend()
        fake.failNextUploads(10)
        let s = store(fake: fake)
        await s.bootstrap()
        s.attachFiles(channel: Ids.general, files: [picked("a.jpg")])
        await s.settle()
        let f = s.state.attachDraft(Ids.general).files[0]
        #expect(f.state.phase == .failed)
    }

    @Test func atMostThreeUploadsRunAtOnce() async {
        let fake = FakeBackend()
        fake.latency = 0.2
        fake.uploadTicks = 1
        let s = store(fake: fake)
        await s.bootstrap()
        s.attachFiles(channel: Ids.general, files: (0..<5).map { picked("f\($0).jpg") })
        await until { s.state.attachDraft(Ids.general).files.filter { $0.state.phase == .uploading }.count == maxConcurrentUploads }
        #expect(s.state.attachDraft(Ids.general).files.filter { $0.state.phase == .uploading }.count == maxConcurrentUploads)
        #expect(s.state.attachDraft(Ids.general).files.filter { $0.state.phase == .queued }.count == 2)
        await s.settle()
        #expect(s.state.attachDraft(Ids.general).files.allSatisfy { $0.state.phase == .ready })
    }

    @Test func removingARunningUploadCancelsItWithoutFailingTheOthers() async {
        let fake = FakeBackend()
        fake.latency = 1
        fake.uploadTicks = 20
        let s = store(fake: fake)
        await s.bootstrap()
        s.attachFiles(channel: Ids.general, files: [picked("slow.jpg")])
        await until { s.state.attachDraft(Ids.general).files.first?.state.phase == .uploading }
        let id = s.state.attachDraft(Ids.general).files[0].id
        s.removeAttachment(id)
        await s.settle()
        #expect(s.state.attachDraft(Ids.general).files.isEmpty)
    }
}
