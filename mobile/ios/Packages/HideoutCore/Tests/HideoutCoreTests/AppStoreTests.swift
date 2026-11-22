import Foundation
import Testing
@testable import HideoutCore

/// The store against the fake backend: real effects, real gateway, no network.
@MainActor
@Suite("AppStore")
struct AppStoreTests {
    private func store(token: String? = "t", fake: FakeBackend = FakeBackend(), config: GatewayConfig = GatewayConfig()) -> (AppStore, FakeBackend, MemoryTokenStore) {
        let tokens = MemoryTokenStore(token: token)
        return (AppStore(api: fake, tokens: tokens, gatewayConfig: config), fake, tokens)
    }

    private func until(_ timeout: Duration = .seconds(5), _ condition: () -> Bool) async {
        let end = ContinuousClock.now + timeout
        while !condition(), ContinuousClock.now < end { try? await Task.sleep(for: .milliseconds(5)) }
    }

    @Test func noTokenMeansSignedOut() async {
        let (s, _, _) = store(token: nil)
        await s.bootstrap()
        #expect(s.state.phase == .signedOut)
    }

    @Test func aSavedTokenLoadsTheSession() async {
        let (s, _, _) = store()
        await s.bootstrap()
        #expect(s.state.phase == .signedIn && s.state.guilds.count == 3 && s.state.me?.username == "tim")
    }

    @Test func signOutClearsTokenAndState() async {
        let (s, _, tokens) = store()
        await s.bootstrap()
        s.signOut()
        #expect(s.state.phase == .signedOut && tokens.load() == nil && s.state.guilds.isEmpty)
    }

    @Test func openingAChannelLoadsItsHistoryThenOlderPages() async {
        let (s, _, _) = store()
        await s.bootstrap()
        s.open(channel: Ids.general)
        await s.settle()
        var log = s.state.log(Ids.general)!
        #expect(log.loaded && log.messages.count == 50 && log.hasMore)
        s.loadOlder(Ids.general)
        await s.settle()
        log = s.state.log(Ids.general)!
        #expect(log.messages.count == 100 && log.breaks.count == 1)
        #expect(log.messages.map(\.id) == log.messages.map(\.id).sorted())
    }

    @Test func sendingShowsPendingThenReconcilesWithTheGatewayEcho() async {
        let (s, fake, _) = store()
        await s.bootstrap()
        s.setActive(true)
        await until { s.state.connection == .connected }
        s.open(channel: Ids.general)
        await s.settle()
        s.send("hello from the test", to: Ids.general)
        #expect(s.state.outgoing.count == 1 && s.state.outgoing[0].state == .pending)
        await s.settle()
        await until { s.state.log(Ids.general)?.messages.last?.content == "hello from the test" }
        #expect(s.state.outgoing.isEmpty)
        #expect(s.state.log(Ids.general)?.messages.filter { $0.content == "hello from the test" }.count == 1)
        #expect(fake.storedMessages(Ids.general).last?.content == "hello from the test")
    }

    @Test func aFailedSendCanBeRetried() async {
        let (s, fake, _) = store()
        await s.bootstrap()
        s.open(channel: Ids.general); await s.settle()
        fake.failNextSends(1)
        s.send("flaky", to: Ids.general)
        await s.settle()
        #expect(s.state.outgoing.first?.state == .failed)
        s.retry(nonce: s.state.outgoing[0].nonce)
        await s.settle()
        #expect(s.state.outgoing.isEmpty && s.state.log(Ids.general)?.messages.last?.content == "flaky")
    }

    @Test func liveMessagesArriveOverTheGatewayAndCountAsUnreadWhenClosed() async {
        let (s, fake, _) = store()
        await s.bootstrap()
        s.setActive(true)
        await until { s.state.connection == .connected }
        fake.incoming(channel: Ids.dev, author: Ids.ana, content: "ping @tim")
        await until { s.state.unread(Ids.dev).count == 1 }
        #expect(s.state.unread(Ids.dev) == Unread(count: 1, mentions: 1))
        #expect(s.state.guildUnread(Ids.hideout) == Unread(count: 1, mentions: 1))
        // The voice snapshot on connect.
        #expect(s.state.participants(of: Ids.lounge, in: Ids.hideout).map(\.userID) == [Ids.ana, Ids.bruno])
    }

    @Test func aDroppedStreamReconnectsAndRefetchesWhatItMissed() async {
        var config = GatewayConfig(); config.initialBackoff = 0.01; config.jitter = 0
        let (s, fake, _) = store(config: config)
        await s.bootstrap()
        s.setActive(true)
        await until { s.state.connection == .connected }
        s.open(channel: Ids.general); await s.settle()
        fake.dropStreams()
        await until { s.state.stream == .interrupted }
        // A message that arrives while the stream is down is not delivered...
        let missed = fake.incoming(channel: Ids.general, author: Ids.bruno, content: "while you were away")
        // ...and is fetched again once the stream is back.
        await until { s.state.log(Ids.general)?.messages.contains { $0.id == missed.id } == true }
        #expect(s.state.log(Ids.general)?.messages.contains { $0.id == missed.id } == true)
        #expect(s.state.stream == .live && s.state.connection == .connected)
    }

    @Test func reactingIsOptimisticAndRollsBackNothingWhenTheServerAgrees() async {
        let (s, _, _) = store()
        await s.bootstrap()
        s.open(channel: Ids.general); await s.settle()
        let target = s.state.log(Ids.general)!.messages.last!
        s.react(channel: Ids.general, message: target.id, emoji: .unicode("🔥"), on: true)
        #expect(s.state.log(Ids.general)?.messages.last?.reactions.contains { $0.emoji == .unicode("🔥") && $0.me } == true)
        await s.settle()
        #expect(s.state.notices.isEmpty)
    }

    @Test func foregroundingAfterBackgroundingRefetches() async {
        let (s, _, _) = store()
        await s.bootstrap()
        s.setActive(true)
        await until { s.state.connection == .connected }
        s.setActive(false)
        #expect(s.state.connection == .connecting)
        s.setActive(true)
        await until { s.state.connection == .connected }
        await s.settle()
        #expect(s.state.stream == .live)
    }
}
