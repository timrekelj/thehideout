import Connect
import Foundation
import HideoutAPI
import SwiftProtobuf
import Testing
@testable import HideoutCore

@Suite("Error mapping")
struct ErrorMappingTests {
    private func detail<M: SwiftProtobuf.Message>(_ m: M) -> ConnectError.Detail {
        ConnectError.Detail(type: M.protoMessageName, payload: try? m.serializedData())
    }

    @Test func connectCodes() {
        func map(_ c: Code, _ msg: String = "boom") -> AppError { AppError(ConnectError(code: c, message: msg)) }
        #expect(map(.unauthenticated) == .unauthenticated)
        #expect(map(.invalidArgument) == .invalidArgument("boom"))
        #expect(map(.notFound) == .notFound)
        #expect(map(.permissionDenied) == .permissionDenied)
        #expect(map(.aborted) == .conflict)
        #expect(map(.unimplemented) == .unimplemented)
        #expect(map(.alreadyExists) == .alreadyExists)
        #expect(map(.resourceExhausted) == .rateLimited(retryAfter: nil))
        #expect(map(.failedPrecondition) == .precondition("boom"))
        #expect(map(.failedPrecondition, "invite required") == .inviteRequired)
        #expect(map(.failedPrecondition, "Client too old: need 0.2.0") == .clientTooOld)
        #expect(map(.notFound, "client too old") == .notFound, "only FAILED_PRECONDITION counts")
        #expect(map(.invalidArgument, "you must accept the terms (2026-01-01)") == .termsUpdateRequired)
        #expect(map(.unavailable) == .network("boom"))
        #expect(map(.deadlineExceeded) == .network("boom"))
        #expect(map(.internalError) == .other("boom"))
    }

    @Test func rateLimitCarriesRetryAfter() {
        let e = ConnectError(code: .resourceExhausted, message: "slow down", metadata: ["Retry-After": ["7"]])
        #expect(AppError(e) == .rateLimited(retryAfter: 7))
        #expect(AppError(e).isTransient)
    }

    @Test func codeErrorDetailsWinAndCarryAttemptsLeft() {
        var d = Thehideout_V1_CodeError()
        d.reason = .wrong; d.attemptsRemaining = 3
        #expect(AppError(ConnectError(code: .invalidArgument, message: "x", details: [detail(d)])) == .wrongCode(attemptsRemaining: 3))
        #expect(AppError.wrongCode(attemptsRemaining: 1).description == "wrong code, 1 try left")
        d.reason = .expired
        #expect(AppError(ConnectError(code: .failedPrecondition, message: "x", details: [detail(d)])) == .codeExpired)
        d.reason = .tooManyAttempts
        #expect(AppError(ConnectError(code: .resourceExhausted, message: "x", details: [detail(d)])) == .tooManyAttempts)
    }

    @Test func suspendedAndOwnedGuildsDetails() {
        var s = Thehideout_V1_AccountSuspended()
        s.reason = "spam"
        s.until = .init(date: Date(timeIntervalSince1970: 1_800_000_000))
        #expect(AppError(ConnectError(code: .permissionDenied, message: "x", details: [detail(s)])) == .suspended(reason: "spam", until: Date(timeIntervalSince1970: 1_800_000_000)))
        var o = Thehideout_V1_OwnedGuilds()
        var g = Thehideout_V1_OwnedGuild(); g.id = 4; g.name = "Club"; g.memberCount = 3
        o.guilds = [g]
        #expect(AppError(ConnectError(code: .failedPrecondition, message: "x", details: [detail(o)])) == .ownsGuilds([OwnedGuild(id: 4, name: "Club", memberCount: 3)]))
    }

    @Test func httpStatuses() {
        #expect(AppError(status: 401, body: "") == .unauthenticated)
        #expect(AppError(status: 413, body: "") == .tooLarge)
        #expect(AppError(status: 429, body: "") == .rateLimited(retryAfter: nil))
        #expect(AppError(status: 503, body: "down") == .network("down"))
        #expect(AppError(status: 500, body: "") == .other("request failed with status 500"))
    }

    @Test func clientHeaderFormat() {
        let v = ClientIdentity.value(appVersion: "0.1.0", osVersion: OperatingSystemVersion(majorVersion: 26, minorVersion: 1, patchVersion: 0))
        #expect(v == "ios/0.1.0 (iOS 26.1.0)")
    }
}

@Suite("Proto mapping")
struct MappingTests {
    @Test func everyModelSurvivesAProtoRoundTrip() async throws {
        let snapshot = try await FakeBackend().snapshot()
        #expect(snapshot.guilds.map { Guild($0.proto) } == snapshot.guilds)
        #expect(snapshot.channels.map { Channel($0.proto) } == snapshot.channels)
        #expect(snapshot.layouts.mapValues { Layout($0.proto) } == snapshot.layouts)
        #expect(snapshot.dms.map { DMChannel($0.proto) } == snapshot.dms)
        let fake = FakeBackend()
        let page = try await fake.listMessages(channel: Ids.general, before: nil, limit: 50)
        #expect(page.messages.map { Message($0.proto) } == page.messages)
        let roles = try await fake.listRoles(guild: 1).roles
        #expect(roles.map { Role($0.proto) } == roles)
        let rich = Message(id: 5, channelID: 1, guildID: nil, authorID: 2, content: "x", createdAt: Date(timeIntervalSince1970: 100),
                           editedAt: Date(timeIntervalSince1970: 200), nonce: "n", reactions: [Reaction(emoji: .custom(9), count: 2, me: true)],
                           attachments: [Attachment(id: 1, filename: "a.png", contentType: "image/png", size: 10, file: "f", width: 3, height: 4, thumbnail: "t")],
                           embeds: [Embed(url: "u", title: "t", kind: .video, media: [EmbedMedia(kind: .gifv, file: "f", poster: "p", width: 1, height: 2, durationMs: 3, size: 4)], publishedAt: Date(timeIntervalSince1970: 50))])
        #expect(Message(rich.proto) == rich)
        let v = VoiceState(guildID: 1, channelID: 22, userID: 3, selfMute: true, selfDeaf: true, serverMute: true, selfVideo: true, selfStream: true, joinedAt: Date(timeIntervalSince1970: 9))
        #expect(VoiceState(v.proto) == v)
    }

    @Test func gatewayEventsMapEveryPayload() throws {
        func event(_ f: (inout Thehideout_V1_Event) -> Void) -> GatewayEvent? { var e = Thehideout_V1_Event(); f(&e); return GatewayEvent(e) }
        var m = Thehideout_V1_Message(); m.id = 7; m.channelID = 1
        #expect(event { $0.payload = .messageCreated(m) } == .messageCreated(Message(m)))
        #expect(event { $0.payload = .messageUpdated({ var u = Thehideout_V1_MessageUpdated(); u.message = m; return u }()) } == .messageUpdated(Message(m)))
        #expect(event { $0.payload = .heartbeat(Thehideout_V1_Heartbeat()) } == .heartbeat)
        #expect(event { $0.payload = .guildRemoved({ var r = Thehideout_V1_GuildRemoved(); r.guildID = 4; return r }()) } == .guildRemoved(guild: 4))
        #expect(event { $0.payload = .channelDeleted({ var r = Thehideout_V1_ChannelDeleted(); r.guildID = 4; r.channelID = 5; return r }()) } == .channelDeleted(guild: 4, channel: 5))
        #expect(event { $0.payload = .messageDeleted({ var r = Thehideout_V1_MessageDeleted(); r.channelID = 4; r.messageID = 5; return r }()) } == .messageDeleted(channel: 4, message: 5))
        #expect(event { $0.payload = .guildMemberRemoved({ var r = Thehideout_V1_GuildMemberRemoved(); r.guildID = 1; r.userID = 2; r.reason = .banned; return r }()) } == .guildMemberRemoved(guild: 1, user: 2, reason: .banned))
        #expect(event { $0.payload = .voiceMoved({ var r = Thehideout_V1_VoiceMoved(); r.guildID = 1; r.channelID = 2; return r }()) } == .voiceMoved(guild: 1, channel: 2))
        #expect(event { $0.payload = .voiceStateUpdate({ var r = Thehideout_V1_VoiceStateUpdate(); r.guildID = 1; r.userID = 2; return r }()) } == .voiceStateUpdate(guild: 1, user: 2, state: nil))
        #expect(event { _ in } == nil, "an empty payload is a newer server's event")
    }
}

@Suite("Token store")
struct TokenStoreTests {
    @Test func keychainRoundTrip() throws {
        let store = KeychainTokenStore(service: "si.zabit.thehideout.tests.\(UUID().uuidString)")
        defer { store.clear() }
        #expect(store.load() == nil)
        try store.save("t1")
        #expect(store.load() == "t1")
        try store.save("t2")
        #expect(store.load() == "t2", "saving again replaces")
        store.clear()
        #expect(store.load() == nil)
    }

    @Test func memoryStore() throws {
        let store = MemoryTokenStore()
        try store.save("x"); #expect(store.load() == "x")
        store.clear(); #expect(store.load() == nil)
    }
}

@Suite("Cache")
struct CacheTests {
    private func tempCache(maxBytes: Int = 64 * 1024 * 1024) -> (Cache, URL) {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("cache-\(UUID().uuidString)")
        return (Cache(directory: dir, maxBytes: maxBytes), dir)
    }

    @Test func snapshotRoundTrip() async throws {
        let (cache, dir) = tempCache(); defer { try? FileManager.default.removeItem(at: dir) }
        #expect(await cache.loadSnapshot() == nil)
        let s = try await FakeBackend().snapshot()
        await cache.saveSnapshot(s)
        #expect(await cache.loadSnapshot() == s)
    }

    @Test func keepsTheNewest100MessagesPerChannel() async throws {
        let (cache, dir) = tempCache(); defer { try? FileManager.default.removeItem(at: dir) }
        await cache.saveMessages((1...150).map { msg(ID($0)) }, channel: 10)
        let loaded = try #require(await cache.loadMessages(channel: 10))
        #expect(loaded.count == 100 && loaded.first?.id == 51 && loaded.last?.id == 150)
        #expect(await cache.loadMessages(channel: 11) == nil)
    }

    @Test func evictsTheOldestMessageFilesOverBudget() async throws {
        let (cache, dir) = tempCache(maxBytes: 4000); defer { try? FileManager.default.removeItem(at: dir) }
        for c in 1...6 {
            await cache.saveMessages((1...20).map { msg(ID($0), in: ID(c), String(repeating: "x", count: 60)) }, channel: ID(c))
            try await Task.sleep(for: .milliseconds(20))
        }
        #expect(await cache.messageBytes() <= 4000)
        #expect(await cache.loadMessages(channel: 6) != nil, "the newest survives")
        #expect(await cache.loadMessages(channel: 1) == nil, "the oldest went first")
    }

    @Test func clearDropsEverything() async throws {
        let (cache, dir) = tempCache(); defer { try? FileManager.default.removeItem(at: dir) }
        await cache.saveSnapshot(try await FakeBackend().snapshot())
        await cache.saveMessages([msg(1)], channel: 1)
        await cache.clear()
        let snap = await cache.loadSnapshot(), msgs = await cache.loadMessages(channel: 1)
        #expect(snap == nil && msgs == nil)
    }
}

@Suite("GatewayStream")
struct GatewayStreamTests {
    /// Hands out one scripted stream per `subscribe`.
    final class Script: GatewaySource, @unchecked Sendable {
        private let lock = NSLock()
        private var attempts: [(AsyncThrowingStream<GatewayEvent, Error>.Continuation) -> Void]
        private(set) var calls = 0
        init(_ attempts: [(AsyncThrowingStream<GatewayEvent, Error>.Continuation) -> Void]) { self.attempts = attempts }
        func subscribe() -> AsyncThrowingStream<GatewayEvent, Error> {
            AsyncThrowingStream { c in
                let next: ((AsyncThrowingStream<GatewayEvent, Error>.Continuation) -> Void)? = lock.withLock { calls += 1; return attempts.isEmpty ? nil : attempts.removeFirst() }
                (next ?? { _ in })(c)
            }
        }
    }

    final class Recorder: @unchecked Sendable {
        private let lock = NSLock()
        private var items: [GatewayUpdate] = []
        func add(_ u: GatewayUpdate) { lock.withLock { items.append(u) } }
        var updates: [GatewayUpdate] { lock.withLock { items } }
        func wait(for count: Int, timeout: Duration = .seconds(5)) async {
            let end = ContinuousClock.now + timeout
            while updates.count < count, ContinuousClock.now < end { try? await Task.sleep(for: .milliseconds(5)) }
        }
    }

    @Test func connectsForwardsEventsAndReconnectsWithBackoff() async throws {
        let sleeps = Recorder2()
        let source = Script([
            { c in c.yield(.heartbeat); c.yield(.guildRemoved(guild: 1)); c.finish(throwing: AppError.network("drop")) },
            { c in c.yield(.heartbeat); c.finish(throwing: AppError.network("drop")) },
            { c in c.finish(throwing: AppError.network("refused")) },
            { c in c.yield(.heartbeat) },
        ])
        var config = GatewayConfig(); config.jitter = 0
        let gateway = GatewayStream(source: source, config: config, sleep: { await sleeps.add($0) })
        let rec = Recorder()
        await gateway.start { rec.add($0) }
        await rec.wait(for: 14)
        await gateway.stop()
        let u = rec.updates
        #expect(Array(u.prefix(5)) == [.connecting, .connected, .event(.heartbeat), .event(.guildRemoved(guild: 1)), .disconnected(retryIn: 0.5, reason: "network error: drop")])
        // A stream that delivered something resets the backoff; one that failed at once doubles it.
        let delays = u.compactMap { if case .disconnected(let d, _) = $0 { d } else { nil } }
        #expect(delays == [0.5, 0.5, 1.0], "delivered->0.5, delivered->0.5, failed at once->1.0")
        #expect(u.last == .event(.heartbeat) && u.contains(.connected))
        #expect(await sleeps.values == [0.5, 0.5, 1.0])
    }

    @Test func backoffDoublesUpToTheCap() {
        var c = GatewayConfig(); c.initialBackoff = 0.5; c.maxBackoff = 30
        #expect((0..<9).map { c.backoff(attempt: $0) } == [0.5, 1, 2, 4, 8, 16, 30, 30, 30])
        #expect(c.silenceTimeout == 75)
    }

    @Test func unauthenticatedStopsForGoodWithAuthExpired() async throws {
        let source = Script([{ c in c.finish(throwing: AppError.unauthenticated) }, { c in c.yield(.heartbeat) }])
        let gateway = GatewayStream(source: source, sleep: { _ in })
        let rec = Recorder()
        await gateway.start { rec.add($0) }
        await rec.wait(for: 2)
        try await Task.sleep(for: .milliseconds(100))
        #expect(rec.updates == [.connecting, .authExpired] && source.calls == 1)
    }

    @Test func clientTooOldStopsWithUpdateRequired() async throws {
        let source = Script([{ c in c.finish(throwing: AppError.clientTooOld) }])
        let gateway = GatewayStream(source: source, sleep: { _ in })
        let rec = Recorder()
        await gateway.start { rec.add($0) }
        await rec.wait(for: 2)
        #expect(rec.updates == [.connecting, .updateRequired])
    }

    @Test func aSilentStreamIsDeclaredDeadByTheWatchdog() async throws {
        let source = Script([{ c in c.yield(.heartbeat) /* then nothing */ }, { c in c.yield(.heartbeat) }])
        var config = GatewayConfig(); config.heartbeatInterval = 0.04; config.silenceFactor = 2.5; config.initialBackoff = 0.01; config.jitter = 0
        let gateway = GatewayStream(source: source, config: config)
        let rec = Recorder()
        await gateway.start { rec.add($0) }
        await rec.wait(for: 8)
        await gateway.stop()
        #expect(rec.updates.contains(.disconnected(retryIn: 0.01, reason: "no heartbeat")))
        #expect(source.calls >= 2)
    }

    @Test func aServerThatClosesCleanlyIsReopened() async throws {
        let source = Script([{ c in c.yield(.heartbeat); c.finish() }, { c in c.yield(.heartbeat) }])
        var config = GatewayConfig(); config.jitter = 0
        let gateway = GatewayStream(source: source, config: config, sleep: { _ in })
        let rec = Recorder()
        await gateway.start { rec.add($0) }
        await rec.wait(for: 8)
        await gateway.stop()
        #expect(rec.updates.contains(.disconnected(retryIn: 0.5, reason: "stream ended")) && source.calls >= 2)
    }

    actor Recorder2 {
        private(set) var values: [TimeInterval] = []
        func add(_ v: TimeInterval) { values.append(v) }
    }
}
