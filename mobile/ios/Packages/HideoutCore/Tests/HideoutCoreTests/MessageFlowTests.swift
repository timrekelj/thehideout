import Foundation
import Testing
@testable import HideoutCore

@Suite("Messages: sending, pagination, layout edits")
struct MessageFlowTests {
    // MARK: optimistic sends

    @Test func sendQueuesAPendingMessageWithANonce() async throws {
        var s = try await makeState()
        let ok10 = s.send("  hello  ", to: Ids.general, at: Date(timeIntervalSince1970: 5))
        #expect(ok10)
        let o = s.outgoing.first!
        #expect(o.content == "hello" && o.state == .pending && o.nonce == "1-1" && o.channel == Ids.general)
        #expect(s.outbox == [.send(channel: Ids.general, content: "hello", nonce: "1-1", attachmentIDs: [])])
        let ok15 = s.send("again", to: Ids.general, at: .now)
        #expect(ok15 && s.outgoing.last?.nonce == "1-2")
    }

    @Test func emptyAndOverlongMessagesAreRefused() async throws {
        var s = try await makeState()
        let ok21 = s.send("   \n ", to: Ids.general, at: .now)
        #expect(!ok21)
        let ok23 = s.send(String(repeating: "x", count: 4001), to: Ids.general, at: .now)
        #expect(!ok23)
        let ok25 = s.send(String(repeating: "x", count: 4000), to: Ids.general, at: .now)
        #expect(ok25)
        let okFile = s.send("", to: Ids.general, at: .now, attachmentIDs: [7])
        #expect(okFile, "a file alone is a message")
    }

    @Test func theReplyAndTheGatewayEchoReconcileToOneMessage() async throws {
        var s = try await makeState()
        loaded(&s, ids: [1])
        s.send("hi", to: Ids.general, at: .now)
        let nonce = s.outgoing[0].nonce
        // The echo can come first...
        s.applyEvent(.messageCreated(msg(2, by: Ids.tim, "hi", nonce: nonce)))
        #expect(s.outgoing.isEmpty)
        // ...and the reply after it adds nothing.
        s.applySent(channel: Ids.general, nonce: nonce, result: .success(msg(2, by: Ids.tim, "hi", nonce: nonce)))
        #expect(s.log(Ids.general)?.messages.map(\.id) == [1, 2])
        #expect(s.rowID(s.log(Ids.general)!.messages[1]) == "n:\(nonce)", "the row keeps the identity of its pending self")
    }

    @Test func theReplyCanComeFirst() async throws {
        var s = try await makeState()
        loaded(&s, ids: [1])
        s.send("hi", to: Ids.general, at: .now)
        let nonce = s.outgoing[0].nonce
        s.applySent(channel: Ids.general, nonce: nonce, result: .success(msg(2, by: Ids.tim, "hi", nonce: nonce)))
        s.applyEvent(.messageCreated(msg(2, by: Ids.tim, "hi", nonce: nonce)))
        #expect(s.outgoing.isEmpty && s.log(Ids.general)?.messages.count == 2)
        #expect(s.unread(Ids.general) == Unread(), "your own message is never unread")
    }

    @Test func aFailedSendStaysAsFailedUntilRetriedOrDiscarded() async throws {
        var s = try await makeState()
        s.send("hi", to: Ids.general, at: .now); s.outbox.removeAll()
        let nonce = s.outgoing[0].nonce
        s.applySent(channel: Ids.general, nonce: nonce, result: .failure(.network("offline")))
        #expect(s.outgoing[0].state == .failed && s.notices.isEmpty, "a transient failure only shows the Failed line")
        s.retry(nonce: nonce)
        #expect(s.outgoing[0].state == .pending && s.outbox == [.send(channel: Ids.general, content: "hi", nonce: nonce, attachmentIDs: [])])
        s.applySent(channel: Ids.general, nonce: nonce, result: .failure(.invalidArgument("too long")))
        #expect(s.outgoing[0].state == .failed && s.notices.map(\.text) == ["Message not sent. Invalid request: too long."])
        s.discard(nonce: nonce)
        #expect(s.outgoing.isEmpty)
    }

    @Test func retryOnlyTouchesFailedMessages() async throws {
        var s = try await makeState()
        s.send("hi", to: Ids.general, at: .now); s.outbox.removeAll()
        s.retry(nonce: s.outgoing[0].nonce)
        #expect(s.outbox.isEmpty)
    }

    // MARK: pagination

    @Test func theFirstPageLoadsTheLogAndMarksItLoaded() async throws {
        var s = try await makeState()
        s.open(channel: Ids.general)
        #expect(s.logs[Ids.general]?.fetch == .initial)
        s.applyMessages(channel: Ids.general, before: nil, result: .success(MessagePage(messages: [msg(3), msg(2)], nextBefore: 2, hasMore: true)))
        let log = s.log(Ids.general)!
        #expect(log.loaded && log.fetch == .idle && log.hasMore && log.messages.map(\.id) == [2, 3] && log.oldestID == 2)
    }

    /// A failed first load no longer gets an implicit retry from a second `open()` for the same (already
    /// open) channel — that call is now a no-op, so the marker it placed cannot be wiped by a redundant
    /// call. `reloadOpen()` is the dedicated retry for this.
    @Test func aFailedFirstLoadRecoversThroughReloadOpenNotASecondOpen() async throws {
        var s = try await makeState()
        s.open(channel: Ids.general)
        s.outbox.removeAll()
        s.applyMessages(channel: Ids.general, before: nil, result: .failure(.network("offline")))
        #expect(!(s.log(Ids.general)?.loaded ?? false) && s.notices.count == 1)
        s.open(channel: Ids.general)
        #expect(s.outbox.isEmpty, "a second open for the already-open channel does not retry the failed load")
        s.reloadOpen()
        #expect(s.outbox == [.loadMessages(channel: Ids.general, before: nil)], "reloadOpen is the dedicated retry")
    }

    @Test func olderPagesAreAskedOnceAndPrependedWithAGroupBreak() async throws {
        var s = try await makeState()
        loaded(&s, ids: [10, 11])
        s.loadOlder(Ids.general); s.loadOlder(Ids.general)
        #expect(s.outbox == [.loadMessages(channel: Ids.general, before: 10)], "one page at a time")
        #expect(s.logs[Ids.general]?.fetch == .older)
        s.applyMessages(channel: Ids.general, before: 10, result: .success(MessagePage(messages: [msg(9), msg(8)], hasMore: false)))
        let log = s.log(Ids.general)!
        #expect(log.messages.map(\.id) == [8, 9, 10, 11] && log.breaks == [10] && !log.hasMore && log.fetch == .idle)
        s.outbox.removeAll()
        s.loadOlder(Ids.general)
        #expect(s.outbox.isEmpty, "nothing older to ask for")
    }

    @Test func aFailedOlderPageReleasesTheFetchAndSaysSo() async throws {
        var s = try await makeState()
        loaded(&s, ids: [10])
        s.loadOlder(Ids.general); s.outbox.removeAll()
        s.applyMessages(channel: Ids.general, before: 10, result: .failure(.network("offline")))
        #expect(s.log(Ids.general)?.fetch == .idle && s.notices.count == 1)
        s.loadOlder(Ids.general)
        #expect(s.outbox.count == 1, "can be asked again")
    }

    @Test func aRefreshOfTheNewestPageMergesAndKeepsYourReactions() async throws {
        var s = try await makeState()
        loaded(&s, ids: [1, 2])
        s.logs[Ids.general]!.stale = true
        var fresh = msg(2); fresh.reactions = [Reaction(emoji: .unicode("🔥"), count: 4, me: false)]
        s.applyMessages(channel: Ids.general, before: nil, result: .success(MessagePage(messages: [msg(3), fresh], hasMore: true)))
        let log = s.log(Ids.general)!
        #expect(log.messages.map(\.id) == [1, 2, 3] && log.messages[1].reactions.first?.count == 4 && !log.stale)
    }

    @Test func aGapBetweenTheLoadedLogAndTheNewestPageStartsOver() async throws {
        var s = try await makeState()
        loaded(&s, ids: [1, 2])
        s.applyMessages(channel: Ids.general, before: nil, result: .success(MessagePage(messages: [msg(500), msg(499)], hasMore: true)))
        #expect(s.log(Ids.general)?.messages.map(\.id) == [499, 500], "the old page would leave a hole")
    }

    @Test func aFailedRefreshMarksTheLogStaleForTheNextVisit() async throws {
        var s = try await makeState()
        loaded(&s, ids: [1])
        s.applyMessages(channel: Ids.general, before: nil, result: .failure(.network("x")))
        #expect(s.logs[Ids.general]?.stale == true)
    }

    @Test func theNewDividerResolvesWhenTheHistoryArrives() async throws {
        var s = try await makeState()
        s.unread[Ids.general] = Unread(count: 2, mentions: 0)
        s.open(channel: Ids.general)
        s.applyMessages(channel: Ids.general, before: nil, result: .success(MessagePage(messages: [msg(5), msg(4), msg(3), msg(2), msg(1)], hasMore: false)))
        #expect(s.marker == .after(channel: Ids.general, id: 3), "two unread: the divider goes after the third newest... after message 3")
    }

    @Test func theNewDividerIsPlacedAtOnceWhenTheLogIsLoaded() async throws {
        var s = try await makeState()
        loaded(&s, ids: [1, 2, 3, 4])
        s.closeChannel()
        s.unread[Ids.general] = Unread(count: 1, mentions: 0)
        s.open(channel: Ids.general)
        #expect(s.marker == .after(channel: Ids.general, id: 3))
        #expect(s.outbox.isEmpty)
        // A second open for the same (already open) channel is a no-op: it does not re-read `unread` (already
        // consumed) and wipe the marker it just placed.
        s.open(channel: Ids.general)
        #expect(s.marker == .after(channel: Ids.general, id: 3), "a second open does not wipe the marker")
    }

    // MARK: layout edits

    @Test func aLayoutEditIsOptimisticAndTheReplySettlesIt() async throws {
        var s = try await makeState()
        var edit = s.layouts[1]!; edit.entries[0].channelIDs = [Ids.dev, Ids.general, Ids.announcements]
        let ok158 = s.editLayout(guild: 1, edit)
        #expect(ok158)
        #expect(s.layouts[1]?.channelIDs(in: nil).first == Ids.dev)
        let second = s.editLayout(guild: 1, edit)
        #expect(!second, "further edits wait for the answer")
        var answer = edit; answer.version += 1
        s.applyLayoutReply(guild: 1, result: .success(answer))
        #expect(s.layouts[1]?.version == answer.version && s.layoutUndo[1] == nil)
    }

    @Test func aRefusedLayoutEditRollsBack() async throws {
        var s = try await makeState()
        let before = s.layouts[1]!
        var edit = before; edit.entries[0].channelIDs.reverse()
        s.editLayout(guild: 1, edit); s.outbox.removeAll()
        s.applyLayoutReply(guild: 1, result: .failure(.permissionDenied))
        #expect(s.layouts[1] == before && s.notices.first?.kind == .danger)
    }

    @Test func aStaleLayoutVersionRefetches() async throws {
        var s = try await makeState()
        var edit = s.layouts[1]!; edit.entries[0].channelIDs.reverse()
        s.editLayout(guild: 1, edit); s.outbox.removeAll()
        s.applyLayoutReply(guild: 1, result: .failure(.conflict))
        #expect(s.outbox == [.loadChannels(guild: 1)] && s.notices.first?.kind == .info)
    }

    @Test func onlyThoseWhoManageChannelsMayEdit() async throws {
        var s = try await makeState()
        var edit = s.layouts[2]!; edit.entries[0].channelIDs.reverse()
        let allowed = s.editLayout(guild: 2, edit)
        #expect(!allowed, "tim is a plain member of Garage Band")
    }

    @Test func noticesKeepTheNewestThree() async throws {
        var s = try await makeState()
        for i in 1...5 { s.notice(.info, "n\(i)") }
        #expect(s.notices.map(\.text) == ["n3", "n4", "n5"])
        s.dismissNotice(s.notices[0].id)
        #expect(s.notices.count == 2)
    }

    @Test func mentionDetection() {
        #expect(Mentions.contains("hi @Tim!", username: "tim"))
        #expect(Mentions.contains("@tim", username: "tim"))
        #expect(!Mentions.contains("@timothy", username: "tim"))
        #expect(!Mentions.contains("mail@tim.example", username: "tim"))
        #expect(!Mentions.contains("```\n@tim\n```", username: "tim"))
        #expect(Mentions.contains("```\nx\n``` then @tim", username: "tim"))
    }

    @Test func sentences() {
        #expect(sentence("wrong code") == "Wrong code.")
        #expect(sentence(" done! ") == "Done!")
        #expect(sentence("") == "")
    }
}
