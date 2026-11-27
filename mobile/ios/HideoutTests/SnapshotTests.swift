import HideoutCore
import HideoutUI
import SwiftUI
import XCTest

/// The gallery and the tab shell, dark and light, iPhone and iPad. References live in `__Snapshots__`; a screen is not done
/// until its snapshots exist for both appearances (mobile/README "Testing"). Re-record with `RECORD_SNAPSHOTS=1`.
@MainActor
final class GallerySnapshotTests: SnapshotCase {
    func testGalleryTop() async {
        let (store, _) = await makeFakeStore()
        assertScreen(GalleryView(), store: store)
        assertScreen(GalleryView(), store: store, device: .iPad, styles: [("dark", .dark)])
    }

    func testEverySectionOnIPhone() async {
        let (store, _) = await makeFakeStore()
        for section in GallerySection.allCases {
            // `GalleryContent` already puts its own horizontal insets on the column, matching `GalleryView.page`.
            let content = GalleryContent(sections: [section], showHero: false)
            if [.glass, .buttons, .motion].contains(section) {
                // Glass is composited by the render server: it needs the key window, so it is laid out as a screen.
                assertScreen(ScrollView { content }, store: store, testName: "testSection-\(section.rawValue)")
            } else {
                assertColumn(content, store: store, testName: "testSection-\(section.rawValue)")
            }
        }
    }

    func testHero() async {
        let (store, _) = await makeFakeStore()
        assertColumn(GalleryContent(sections: [], showHero: true), store: store)
    }

    func testLookVariants() async {
        let (store, _) = await makeFakeStore()
        assertColumn(GalleryContent(sections: [.badges, .messages, .channels], showHero: true), store: store, looks: Looks.all, styles: [("dark", .dark)])
        assertColumn(GalleryContent(sections: [.messages, .channels, .badges], showHero: false), store: store, looks: [Looks.coralPaper, Looks.highContrast, Looks.text125], styles: [("light", .light)], testName: "testLookVariantsLight")
    }

    func testLookSheet() async {
        let (store, _) = await makeFakeStore()
        assertScreen(LookSheet().frame(maxWidth: .infinity, maxHeight: .infinity), store: store)
    }
}

@MainActor
final class ShellSnapshotTests: SnapshotCase {
    func testServersTab() async {
        let (store, _) = await makeFakeStore(live: true)
        assertScreen(RootView(), store: store)
        assertScreen(RootView(), store: store, device: .iPad, styles: [("dark", .dark), ("light", .light)])
    }

    func testChannelList() async {
        let (store, _) = await makeFakeStore(live: true)
        store.enterGuild(FakeBackend.Ids.hideout)
        await store.settle()
        assertScreen(NavigationStack { ChannelListScreenForTests(guild: FakeBackend.Ids.hideout) }, store: store)
    }

    func testTranscript() async {
        let (store, _) = await makeFakeStore(open: FakeBackend.Ids.general)
        assertScreen(NavigationStack { TranscriptScreenForTests(channel: FakeBackend.Ids.general) }, store: store)
        assertScreen(NavigationStack { TranscriptScreenForTests(channel: FakeBackend.Ids.general) }, store: store, device: .iPad, styles: [("dark", .dark)])
    }

    func testTranscriptLooks() async {
        let (store, _) = await makeFakeStore(open: FakeBackend.Ids.general)
        assertScreen(NavigationStack { TranscriptScreenForTests(channel: FakeBackend.Ids.general) }, store: store, looks: [Looks.ocean], styles: [("dark", .dark)])
    }

    func testDirectMessages() async {
        let (store, _) = await makeFakeStore(open: FakeBackend.Ids.dmAna)
        assertScreen(NavigationStack { TranscriptScreenForTests(channel: FakeBackend.Ids.dmAna) }, store: store, styles: [("dark", .dark)])
    }

    /// Regression for the vanishing "New" divider: two unread messages from the same author, more than five
    /// minutes apart (so they land as separate groups, same as desktop), arrive while the DM is closed. Opening
    /// it is a double call in the real shell (`ShellNav.select`'s route sync, then the pushed transcript's own
    /// `.task`) — the second must not wipe the marker the first just placed.
    func testTranscriptGroupsWithNewDivider() async {
        let (store, fake) = await makeFakeStore()
        store.setActive(true)
        for _ in 0..<400 where fake.openStreams == 0 { try? await Task.sleep(for: .milliseconds(10)) }
        let t1 = snapshotNow.addingTimeInterval(-3600)
        let t2 = t1.addingTimeInterval(600)
        fake.incoming(channel: FakeBackend.Ids.dmAna, author: FakeBackend.Ids.ana, content: "quick one before you go", at: t1)
        fake.incoming(channel: FakeBackend.Ids.dmAna, author: FakeBackend.Ids.ana, content: "never mind, found it", at: t2)
        for _ in 0..<400 where store.state.unread(FakeBackend.Ids.dmAna).count < 2 { try? await Task.sleep(for: .milliseconds(10)) }
        store.setActive(false)
        store.open(channel: FakeBackend.Ids.dmAna)
        await store.settle()
        // The second call the real shell makes (its own `.task` would too, but asserting it here does not
        // depend on that timing): the marker must survive it.
        store.open(channel: FakeBackend.Ids.dmAna)
        XCTAssertNotEqual(store.state.marker, .none, "the marker must survive the shell's second open")
        assertScreen(NavigationStack { TranscriptScreenForTests(channel: FakeBackend.Ids.dmAna) }, store: store, styles: [("dark", .dark)])
    }

    func testSignedOutAndLoading() async {
        registerFonts()
        let fake = FakeBackend(now: snapshotNow)
        let signedOut = AppStore(api: fake, tokens: MemoryTokenStore(), now: { snapshotNow })
        await signedOut.bootstrap()
        assertScreen(RootView(), store: signedOut)
    }

    func testDMsTab() async {
        let (store, _) = await makeFakeStore()
        assertScreen(DMsTabForTests(), store: store, styles: [("dark", .dark)])
    }

    func testMentionsTab() async {
        // `live: true` seeds a mention of tim in #dev ("heads up @tim, the build is green"); loaded history
        // already has one in #general too (`generalLines`): open both so the feed spans more than one channel.
        let (store, _) = await makeFakeStore(live: true)
        store.open(channel: FakeBackend.Ids.general)
        store.open(channel: FakeBackend.Ids.dev)
        await store.settle()
        store.closeChannel()
        assertScreen(MentionsTabForTests(), store: store, styles: [("dark", .dark)])
    }

    func testEmojiPicker() async {
        let (store, _) = await makeFakeStore()
        assertScreen(EmojiPickerSheet(onPick: { _ in }), store: store, styles: [("dark", .dark)])
    }

    func testMessageActionsPreview() async {
        let (store, _) = await makeFakeStore(open: FakeBackend.Ids.general)
        let m = store.state.log(FakeBackend.Ids.general)!.messages.last!
        assertColumn(
            MessageRow(author: store.state.userName(m.authorID), time: m.createdAt, text: m.content, reactions: m.reactions, me: store.state.me?.username)
                .card(),
            store: store, styles: [("dark", .dark)]
        )
    }
}

/// The shell's navigation state driven directly (no UI): the regression guard for the cross-tab DM jump.
/// From a guild transcript, a profile's Message routes to the DM; when the Servers tab returns to the
/// foreground, its retained transcript must become the open (read) channel again — its unread clears and
/// the DM's inbound messages start counting as unread. Without the sync in `ShellNav.select`, the store
/// keeps the DM as `openChannel` while the guild transcript is the visible one.
@MainActor
final class ShellNavTests: XCTestCase {
    private func liveStore() async throws -> (AppStore, FakeBackend) {
        let fake = FakeBackend()
        let store = AppStore(api: fake, tokens: MemoryTokenStore(token: "fake"))
        await store.bootstrap()
        store.setActive(true)
        for _ in 0..<400 where fake.openStreams == 0 { try await Task.sleep(for: .milliseconds(10)) }
        return (store, fake)
    }

    private func waitFor(_ condition: @escaping () -> Bool) async throws {
        for _ in 0..<400 where !condition() { try await Task.sleep(for: .milliseconds(10)) }
    }

    func testDmRouteThenReturnToServersSyncsTheRetainedTranscriptAsOpen() async throws {
        let (store, fake) = try await liveStore()
        let nav = ShellNav(initialTab: .servers)

        // In the Servers tab, #general pushed and open.
        nav.serversPath = [.guild(FakeBackend.Ids.hideout), .channel(FakeBackend.Ids.general)]
        nav.select(.servers, store: store)
        await store.settle()
        XCTAssertEqual(store.state.openChannel, FakeBackend.Ids.general)
        XCTAssertEqual(nav.foregroundChannel, FakeBackend.Ids.general)

        // A member profile's Message: the explicit route lands in the DM (DMs tab, transcript pushed).
        store.startDm(user: FakeBackend.Ids.ana)
        await store.settle()
        let request = try XCTUnwrap(store.state.routeRequest)
        nav.follow(request, store: store)
        XCTAssertEqual(nav.selectedTab, .dms)
        XCTAssertEqual(nav.dmRoute?.id, FakeBackend.Ids.dmAna)
        XCTAssertEqual(store.state.openChannel, FakeBackend.Ids.dmAna)

        // While the DM tab is foreground, a guild message is unread and a DM message is read.
        fake.incoming(channel: FakeBackend.Ids.general, author: FakeBackend.Ids.bruno, content: "guild ping")
        fake.incoming(channel: FakeBackend.Ids.dmAna, author: FakeBackend.Ids.ana, content: "dm ping")
        try await waitFor { store.state.unread(FakeBackend.Ids.general).count > 0 }
        XCTAssertEqual(store.state.unread(FakeBackend.Ids.general), Unread(count: 1))
        XCTAssertEqual(store.state.unread(FakeBackend.Ids.dmAna), Unread())

        // Back to Servers: the retained #general transcript is visible again and becomes the open
        // channel — its unread clears, and now the DM's inbound messages count as unread instead.
        nav.select(.servers, store: store)
        XCTAssertEqual(nav.foregroundChannel, FakeBackend.Ids.general)
        XCTAssertEqual(store.state.openChannel, FakeBackend.Ids.general)
        XCTAssertEqual(store.state.unread(FakeBackend.Ids.general), Unread(), "the visible transcript is read again")
        fake.incoming(channel: FakeBackend.Ids.dmAna, author: FakeBackend.Ids.ana, content: "dm ping 2")
        try await waitFor { store.state.unread(FakeBackend.Ids.dmAna).count > 0 }
        XCTAssertEqual(store.state.unread(FakeBackend.Ids.dmAna), Unread(count: 1, mentions: 1))
        XCTAssertEqual(store.state.unread(FakeBackend.Ids.general), Unread())
    }

    func testAnInboundDmNeverRoutesOrStealsTheTab() async throws {
        let (store, fake) = try await liveStore()
        let nav = ShellNav(initialTab: .servers)
        nav.select(.servers, store: store)
        XCTAssertNil(store.state.routeRequest)

        // A DM opened elsewhere (or a message in one) arrives on the gateway: no route request, no tab
        // change, no open channel — just the unread badge.
        fake.emit(.dmOpened(DMChannel(id: 4242, participants: [User(id: FakeBackend.Ids.tim, username: "tim"), User(id: FakeBackend.Ids.ana, username: "ana")])))
        fake.incoming(channel: FakeBackend.Ids.dmAna, author: FakeBackend.Ids.ana, content: "hi there")
        try await waitFor { store.state.unread(FakeBackend.Ids.dmAna).count > 0 }
        XCTAssertNil(store.state.routeRequest)
        XCTAssertEqual(nav.selectedTab, .servers)
        XCTAssertNil(store.state.openChannel)
        XCTAssertEqual(store.state.unread(FakeBackend.Ids.dmAna), Unread(count: 1, mentions: 1))
        XCTAssertTrue(store.state.dms.contains { $0.id == 4242 }, "the new conversation is listed, not shown")
    }

    func testAnOrdinaryBackOnlyPopsThePathAndKeepsTheTab() async throws {
        let (store, _) = try await liveStore()
        let nav = ShellNav(initialTab: .dms)
        nav.dmRoute = ChannelRoute(id: FakeBackend.Ids.dmAna)
        nav.select(.dms, store: store)
        XCTAssertEqual(store.state.openChannel, FakeBackend.Ids.dmAna)
        // Back in the DMs tab: the path pops to the list (dmRoute nil) and the transcript's onDisappear
        // closes the channel; the selected tab stays DMs throughout.
        nav.dmRoute = nil
        store.closeChannel()
        XCTAssertEqual(nav.selectedTab, .dms, "Back does not steal the tab")
        XCTAssertNil(nav.foregroundChannel)
        XCTAssertNil(store.state.openChannel)
    }
}
