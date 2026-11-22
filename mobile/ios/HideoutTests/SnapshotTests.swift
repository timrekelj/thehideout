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
            // The insets `GalleryView.page` puts on the scroll view, so the references show the real layout.
            let content = GalleryContent(sections: [section], showHero: false).padding(.horizontal, Space.s4)
            if [.glass, .buttons, .motion].contains(section) {
                // Glass is composited by the render server: it needs the key window, so it is laid out as a screen.
                assertScreen(ScrollView { content }.padding(.horizontal, Space.s4), store: store, testName: "testSection-\(section.rawValue)")
            } else {
                assertColumn(content, store: store, testName: "testSection-\(section.rawValue)")
            }
        }
    }

    func testHero() async {
        let (store, _) = await makeFakeStore()
        assertColumn(GalleryContent(sections: [], showHero: true).padding(.horizontal, Space.s4), store: store)
    }

    func testLookVariants() async {
        let (store, _) = await makeFakeStore()
        assertColumn(GalleryContent(sections: [.badges, .messages, .channels], showHero: true).padding(.horizontal, Space.s4), store: store, looks: Looks.all, styles: [("dark", .dark)])
        assertColumn(GalleryContent(sections: [.messages, .channels, .badges], showHero: false).padding(.horizontal, Space.s4), store: store, looks: [Looks.coralPaper, Looks.highContrast, Looks.text125], styles: [("light", .light)], testName: "testLookVariantsLight")
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

    func testSignedOutAndLoading() async {
        registerFonts()
        let fake = FakeBackend(now: snapshotNow)
        let signedOut = AppStore(api: fake, tokens: MemoryTokenStore(), now: { snapshotNow })
        await signedOut.bootstrap()
        assertScreen(RootView(), store: signedOut)
    }
}
