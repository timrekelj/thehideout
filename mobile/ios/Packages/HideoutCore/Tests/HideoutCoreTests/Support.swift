import Foundation
@testable import HideoutCore

typealias Ids = FakeBackend.Ids

/// A signed-in state over the fake world, with no effects pending.
func makeState() async throws -> AppState {
    let fake = FakeBackend()
    var s = AppState()
    s.applySnapshot(try await fake.snapshot())
    s.outbox.removeAll()
    return s
}

func msg(_ id: ID, in channel: ID = Ids.general, by author: ID = Ids.ana, _ content: String = "hi", nonce: String = "", guild: ID? = Ids.hideout) -> Message {
    Message(id: id, channelID: channel, guildID: guild, authorID: author, content: content, createdAt: Date(timeIntervalSince1970: Double(id)), nonce: nonce)
}

/// Opens `channel` with `ids` already loaded as its log.
func loaded(_ s: inout AppState, channel: ID = Ids.general, ids: [ID]) {
    s.open(channel: channel)
    s.outbox.removeAll()
    s.applyMessages(channel: channel, before: nil, result: .success(MessagePage(messages: ids.reversed().map { msg($0, in: channel) }, hasMore: true)))
}
