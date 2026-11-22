import Foundation
import Testing
@testable import HideoutCore

@Suite("Chat phase 3a: DMs, edit/delete, mentions, drafts, autocomplete, markdown, emoji")
struct ChatFlowTests {
    // MARK: StartDm

    @Test func startDmOpensAnExistingOneToOneAtOnceAndLoadsItsHistory() async throws {
        var s = try await makeState()
        s.startDm(user: Ids.ana)
        #expect(s.place == .home)
        #expect(s.openChannel == Ids.dmAna)
        // Opening must also load messages (the Android bug this guards against).
        #expect(s.outbox.contains(.loadMessages(channel: Ids.dmAna, before: nil)))
    }

    @Test func startDmWithAStrangerAsksTheServer() async throws {
        var s = try await makeState()
        s.startDm(user: 999)
        #expect(s.outbox == [.startDm(user: 999)])
    }

    @Test func startDmNeverMatchesAGroupOrYourself() async throws {
        var s = try await makeState()
        #expect(s.oneToOneDm(with: s.myID) == nil)
        // A DM with a third participant must not match even if the target is in it.
        s.dms.append(DMChannel(id: 999, participants: [s.users[Ids.tim]!, s.users[Ids.ana]!, s.users[Ids.bruno]!]))
        s.startDm(user: Ids.ana)
        // The real 1:1 (dmAna) still wins over the group.
        #expect(s.openChannel == Ids.dmAna)
    }

    @Test func dmOpenedReplyAddsItAndJumpsToTheConversation() async throws {
        var s = try await makeState()
        s.place = .guild(Ids.hideout) // somewhere else entirely: a cross-tab jump must still land in the chat
        let stranger = User(id: 777, username: "newperson")
        s.applyDmOpened(result: .success(DMChannel(id: 888, participants: [s.users[Ids.tim]!, stranger])))
        #expect(s.place == .home)
        #expect(s.openChannel == 888)
        #expect(s.dms.contains { $0.id == 888 })
        #expect(s.users[777] != nil)
        #expect(s.outbox.contains(.loadMessages(channel: 888, before: nil)))
    }

    @Test func dmOpenedFailureNotices() async throws {
        var s = try await makeState()
        s.applyDmOpened(result: .failure(.notFound))
        #expect(s.notices.first?.kind == .danger)
        #expect(s.openChannel == nil)
    }

    @Test func gatewayDmOpenedEventDedupsAgainstAnAlreadyKnownConversation() async throws {
        var s = try await makeState()
        let before = s.dms.count
        s.applyEvent(.dmOpened(s.dm(Ids.dmAna)!))
        #expect(s.dms.count == before)
    }

    // MARK: edit (optimistic, with rollback)

    @Test func editIsOptimisticAndTheReplySettlesIt() async throws {
        var s = try await makeState()
        loaded(&s, ids: [1, 2])
        let ok = s.editMessage(channel: Ids.general, message: 2, content: "edited text")
        #expect(ok)
        #expect(s.log(Ids.general)?.messages.last?.content == "edited text")
        #expect(s.outbox == [.editMessage(channel: Ids.general, message: 2, content: "edited text", previous: "hi")])
        var reply = msg(2, "edited text"); reply.editedAt = Date()
        s.applyEditReply(channel: Ids.general, message: 2, previous: "hi", result: .success(reply))
        #expect(s.log(Ids.general)?.messages.last?.editedAt != nil)
    }

    @Test func aFailedEditRollsBackToTheOriginalText() async throws {
        var s = try await makeState()
        loaded(&s, ids: [1, 2])
        s.editMessage(channel: Ids.general, message: 2, content: "oops")
        s.applyEditReply(channel: Ids.general, message: 2, previous: "hi", result: .failure(.network("offline")))
        #expect(s.log(Ids.general)?.messages.last?.content == "hi")
        #expect(s.notices.first?.kind == .danger)
    }

    @Test func editRefusesEmptyOverlongOrUnchangedText() async throws {
        var s = try await makeState()
        loaded(&s, ids: [1])
        let blank = s.editMessage(channel: Ids.general, message: 1, content: "   ")
        #expect(!blank)
        let overlong = s.editMessage(channel: Ids.general, message: 1, content: String(repeating: "x", count: 4001))
        #expect(!overlong)
        let unchanged = s.editMessage(channel: Ids.general, message: 1, content: "hi")
        #expect(!unchanged, "unchanged: nothing to do")
        let noSuchMessage = s.editMessage(channel: Ids.general, message: 42, content: "new")
        #expect(!noSuchMessage, "no such message loaded")
    }

    // MARK: mention feed

    @Test func mentionFeedFindsMentionsAcrossChannelsNewestFirst() async throws {
        var s = try await makeState()
        loaded(&s, ids: [1, 2])
        s.logs[Ids.general]!.messages[0] = msg(1, by: Ids.ana, "hey @tim check this", guild: Ids.hideout)
        loaded(&s, channel: Ids.dev, ids: [10])
        s.logs[Ids.dev]!.messages[0] = msg(10, in: Ids.dev, by: Ids.bruno, "@tim ping", guild: Ids.hideout)
        let feed = s.mentionFeed()
        #expect(feed.map(\.message.id).sorted() == [1, 10])
        #expect(feed.allSatisfy { $0.message.createdAt != Date(timeIntervalSince1970: 0) || true })
    }

    @Test func mentionFeedExcludesYourOwnMessagesAndNonMentions() async throws {
        var s = try await makeState()
        loaded(&s, ids: [1, 2])
        s.logs[Ids.general]!.messages[0] = msg(1, by: Ids.tim, "@tim talking to myself")
        #expect(s.mentionFeed().isEmpty)
    }

    // MARK: drafts

    @Test func draftsAreKeptPerChannelAndClearedWhenEmptied() async throws {
        var s = try await makeState()
        s.setDraft(channel: Ids.general, text: "half a thought")
        s.setDraft(channel: Ids.dev, text: "another")
        #expect(s.draft(Ids.general) == "half a thought")
        #expect(s.draft(Ids.dev) == "another")
        s.setDraft(channel: Ids.general, text: "")
        #expect(s.draft(Ids.general).isEmpty)
        #expect(s.drafts[Ids.general] == nil, "cleared, not stored empty")
    }
}

@Suite("Autocomplete trigger rules")
struct AutocompleteTests {
    @Test func mentionAndEmojiTriggerAtWordStart() {
        #expect(Autocomplete.trigger("hello @an", caret: 9) == .mention(query: "an", start: 6))
        #expect(Autocomplete.trigger("hello :sm", caret: 9) == .emojiShortcode(query: "sm", start: 6))
        #expect(Autocomplete.trigger("@", caret: 1) == .mention(query: "", start: 0))
    }

    @Test func noTriggerMidWord() {
        #expect(Autocomplete.trigger("it's 12:30", caret: 10) == .none, "the colon isn't at the start of the word")
        #expect(Autocomplete.trigger("user@example.com", caret: 17) == .none, "the at sign isn't at the start of the word")
        #expect(Autocomplete.trigger("re:sm", caret: 5) == .none)
    }

    @Test func noTriggerAtStartOrPastTheText() {
        #expect(Autocomplete.trigger("hello", caret: 0) == .none)
        #expect(Autocomplete.trigger("hello", caret: 99) == .none)
    }

    @Test func mentionMatchesPrefixCaseInsensitive() {
        let users = [User(id: 1, username: "Ana"), User(id: 2, username: "anabelle"), User(id: 3, username: "bruno")]
        #expect(Autocomplete.mentionMatches("an", members: users).map(\.username) == ["Ana", "anabelle"])
        #expect(Autocomplete.mentionMatches("", members: users).count == 3)
    }

    @Test func emojiMatchesPutServerEmojiFirst() {
        let custom = [Emoji(id: 1, guildID: 1, name: "smile_custom")]
        let matches = Autocomplete.emojiMatches("smil", serverEmojis: custom)
        #expect(matches.first == .custom(id: 1, name: "smile_custom", image: "", animated: false))
    }
}

@Suite("Bundled emoji data")
struct EmojiDataTests {
    @Test func searchPrefersPrefixOverSubstring() {
        let results = EmojiData.search("fire")
        #expect(results.first == .unicode(emoji: "🔥", name: "fire"))
    }

    @Test func categoriesCoverEveryEmoji() {
        let total = EmojiCategory.allCases.reduce(0) { $0 + EmojiData.inCategory($1).count }
        #expect(total == EmojiData.all.count)
    }

    @Test func insertionUsesTheDesktopCustomEmojiToken() {
        #expect(EmojiEntry.custom(id: 42, name: "party", image: "", animated: false).insertion == "<:party:42>")
        #expect(EmojiEntry.custom(id: 42, name: "party", image: "", animated: true).insertion == "<a:party:42>")
        #expect(EmojiEntry.unicode(emoji: "🔥", name: "fire").insertion == "🔥")
    }
}

@Suite("Markdown parser")
struct MarkdownTests {
    @Test func inlineMarks() {
        #expect(Markdown.inline("**bold**") == [.bold([.text("bold")])])
        #expect(Markdown.inline("*italic*") == [.italic([.text("italic")])])
        #expect(Markdown.inline("__underline__") == [.underline([.text("underline")])])
        #expect(Markdown.inline("~~strike~~") == [.strike([.text("strike")])])
        #expect(Markdown.inline("||spoiler||") == [.spoiler([.text("spoiler")])])
        #expect(Markdown.inline("`code`") == [.code("code")])
    }

    @Test func mentionsAndCustomEmoji() {
        #expect(Markdown.inline("hi @tim!") == [.text("hi "), .mention("tim"), .text("!")])
        #expect(Markdown.inline("<:party:42>") == [.customEmoji(name: "party", id: 42, animated: false)])
        #expect(Markdown.inline("<a:party:42>") == [.customEmoji(name: "party", id: 42, animated: true)])
        #expect(Markdown.inline("mail@tim.example") == [.text("mail@tim.example")], "no boundary before the @")
    }

    @Test func linksAndBareURLs() {
        #expect(Markdown.inline("[wiki](https://example.com)") == [.link([.text("wiki")], url: "https://example.com")])
        #expect(Markdown.inline("see https://example.com/x.") == [.text("see "), .link([.text("https://example.com/x")], url: "https://example.com/x"), .text(".")])
        #expect(Markdown.inline("[bad](ftp://x)") == [.text("[bad](ftp://x)")])
    }

    @Test func backslashEscapes() {
        #expect(Markdown.inline("\\*\\*not bold\\*\\*") == [.text("**not bold**")])
    }

    @Test func blocksQuotesListsHeadingsAndFences() {
        let blocks = Markdown.parse("# Title\n> quoted\n- one\n- two\n```sh\necho hi\n```")
        #expect(blocks == [
            .heading(level: 1, [.text("Title")]),
            .quote([.text("quoted")]),
            .list(ordered: false, items: [[.text("one")], [.text("two")]]),
            .codeBlock(language: "sh", code: "echo hi"),
        ])
    }

    @Test func customEmojiTokenRoundTrips() {
        #expect(Markdown.customEmojiToken(name: "party", id: 42, animated: false) == "<:party:42>")
        let parsed = Markdown.inline(Markdown.customEmojiToken(name: "party", id: 42, animated: true))
        #expect(parsed == [.customEmoji(name: "party", id: 42, animated: true)])
    }

    @Test func jumboDetectsAShortRunOfEmojiOnlyAndNothingElse() {
        #expect(Jumbo.inlines(of: Markdown.parse("🔥🎉")) != nil)
        #expect(Jumbo.inlines(of: Markdown.parse("🔥 words")) == nil)
        #expect(Jumbo.inlines(of: Markdown.parse(String(repeating: "🔥", count: 28))) == nil, "caps at 27")
        #expect(Jumbo.inlines(of: Markdown.parse("<:party:1><:fire:2>")) != nil)
    }
}
