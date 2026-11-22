import HideoutCore
import HideoutDesign
import HideoutUI
import SnapshotTesting
import SwiftUI
import UIKit
import XCTest

/// The devices snapshots are laid out at (points). The simulator that renders them only has to be iOS 26+; sizes and
/// safe areas are pinned here so a different device model gives the same pictures.
enum Device: String, CaseIterable {
    case iPhone = "iphone"  // iPhone 17 Pro / 18 Pro: 402 x 874
    case iPad = "ipad"      // iPad Pro 11-inch: 834 x 1210

    // UITraitCollection's mutation closure is main-actor isolated in the iOS 26 SDK.
    @MainActor var config: ViewImageConfig {
        switch self {
        case .iPhone:
            ViewImageConfig(
                safeArea: UIEdgeInsets(top: 62, left: 0, bottom: 34, right: 0),
                size: CGSize(width: 402, height: 874),
                traits: UITraitCollection { $0.displayScale = 2; $0.userInterfaceIdiom = .phone; $0.horizontalSizeClass = .compact; $0.verticalSizeClass = .regular })
        case .iPad:
            ViewImageConfig(
                safeArea: UIEdgeInsets(top: 24, left: 0, bottom: 20, right: 0),
                size: CGSize(width: 834, height: 1210),
                traits: UITraitCollection { $0.displayScale = 2; $0.userInterfaceIdiom = .pad; $0.horizontalSizeClass = .regular; $0.verticalSizeClass = .regular })
        }
    }

    @MainActor var width: CGFloat { config.size?.width ?? 402 }
}

/// The date every snapshot believes it is.
let snapshotNow = ISO8601DateFormatter().date(from: "2026-11-22T12:00:00Z")!

@MainActor
func registerFonts() {
    DesignFonts.register()
    NSTimeZone.default = TimeZone(identifier: "UTC")!
}

/// The app in the fake world: signed in, optionally with the gateway up (voice snapshot, live unread) and a channel open.
@MainActor
func makeFakeStore(live: Bool = false, open channel: ID? = nil) async -> (AppStore, FakeBackend) {
    registerFonts()
    let fake = FakeBackend(now: snapshotNow)
    let store = AppStore(api: fake, tokens: MemoryTokenStore(token: "fake"), now: { snapshotNow })
    await store.bootstrap()
    if live {
        store.setActive(true)
        for _ in 0..<400 where store.state.participants(of: FakeBackend.Ids.lounge, in: FakeBackend.Ids.hideout).isEmpty { try? await Task.sleep(for: .milliseconds(10)) }
        fake.incoming(channel: FakeBackend.Ids.dev, author: FakeBackend.Ids.ana, content: "heads up @tim, the build is green", at: snapshotNow.addingTimeInterval(-120))
        fake.incoming(channel: FakeBackend.Ids.dev, author: FakeBackend.Ids.bruno, content: "merging now", at: snapshotNow.addingTimeInterval(-90))
        fake.incoming(channel: FakeBackend.Ids.random, author: FakeBackend.Ids.cvetka, content: "look at this", at: snapshotNow.addingTimeInterval(-60))
        for _ in 0..<400 where store.state.unread(FakeBackend.Ids.dev).count < 2 { try? await Task.sleep(for: .milliseconds(10)) }
        store.setActive(false)
    }
    if let channel {
        store.open(channel: channel)
        await store.settle()
    }
    return (store, fake)
}

extension View {
    /// The environment every snapshot runs in: the store, the look, a pinned clock, zone and locale.
    @MainActor
    func snapshotEnvironment(store: AppStore, look: Look = Look(), auth: AuthFlow? = nil, server: ServerSelection? = nil) -> some View {
        let themeStore = ThemeStore(defaults: nil, look: look)
        let fake = FakeBackend(now: snapshotNow)
        let server = server ?? ServerSelection(fixed: fake)
        let auth = auth ?? AuthFlow(server: server, deviceName: "iPhone", now: { snapshotNow }, onSession: { _ in })
        return self
            .environment(store)
            .environment(auth)
            .environment(server)
            .environment(themeStore)
            .themed(themeStore)
            .environment(\.currentDate, snapshotNow)
            .environment(\.timeZone, TimeZone(identifier: "UTC")!)
            .environment(\.locale, Locale(identifier: "en_US"))
            .environment(\.calendar, { var c = Calendar(identifier: .gregorian); c.timeZone = TimeZone(identifier: "UTC")!; c.locale = Locale(identifier: "en_US"); return c }())
    }
}

class SnapshotCase: XCTestCase {
    override func invokeTest() {
        // RECORD_SNAPSHOTS=1 re-records every reference; by default only missing ones are written (and fail the run once).
        let record: SnapshotTestingConfiguration.Record = ProcessInfo.processInfo.environment["RECORD_SNAPSHOTS"] != nil ? .all : .missing
        withSnapshotTesting(record: record) { super.invokeTest() }
    }

    static let styles: [(String, UIUserInterfaceStyle)] = [("dark", .dark), ("light", .light)]

    /// A full screen on a device, in dark and light (or just the ones given).
    @MainActor
    func assertScreen<V: View>(_ view: @autoclosure () -> V, store: AppStore, auth: AuthFlow? = nil, server: ServerSelection? = nil, device: Device = .iPhone, looks: [(String, Look)] = [("default", Look())],
                                styles: [(String, UIUserInterfaceStyle)] = styles, wait: TimeInterval = 1.0,
                                file: StaticString = #filePath, testName: String = #function, line: UInt = #line) {
        for (lookName, look) in looks {
            for (styleName, style) in styles {
                let vc = UIHostingController(rootView: view().snapshotEnvironment(store: store, look: look, auth: auth, server: server))
                let name = [device.rawValue, styleName, lookName == "default" ? nil : lookName].compactMap { $0 }.joined(separator: "-")
                assertSnapshot(
                    of: vc,
                    as: .wait(for: wait, on: .image(on: device.config, drawHierarchyInKeyWindow: true, precision: 0.95, perceptualPrecision: 0.9, traits: UITraitCollection(userInterfaceStyle: style))),
                    named: name, file: file, testName: testName, line: line)
            }
        }
    }

    /// A view at its natural height in a column of the device's width (the gallery's sections).
    @MainActor
    func assertColumn<V: View>(_ view: @autoclosure () -> V, store: AppStore, device: Device = .iPhone, looks: [(String, Look)] = [("default", Look())],
                                styles: [(String, UIUserInterfaceStyle)] = styles,
                                file: StaticString = #filePath, testName: String = #function, line: UInt = #line) {
        for (lookName, look) in looks {
            for (styleName, style) in styles {
                let content = view().frame(width: device.width).snapshotEnvironment(store: store, look: look)
                let name = [device.rawValue, styleName, lookName == "default" ? nil : lookName].compactMap { $0 }.joined(separator: "-")
                assertSnapshot(
                    of: content,
                    as: .image(precision: 0.98, perceptualPrecision: 0.97, layout: .sizeThatFits, traits: device.config.traits.modifyingTraits { $0.userInterfaceStyle = style }),
                    named: name, file: file, testName: testName, line: line)
            }
        }
    }
}

/// The looks of PARITY SYS-11 that the snapshots cover besides the default.
enum Looks {
    static let ocean: (String, Look) = ("ocean-cool", { var l = Look(); l.spec.accent = .ocean; l.spec.background = .cool; return l }())
    static let highContrast: (String, Look) = ("high-contrast", { var l = Look(); l.spec.contrast = .high; return l }())
    static let compact: (String, Look) = ("compact", { var l = Look(); l.spec.density = .compact; return l }())
    static let text125: (String, Look) = ("text-125", { var l = Look(); l.spec.textSize = .p125; return l }())
    static let coralPaper: (String, Look) = ("coral-paper", { var l = Look(); l.spec.accent = .coral; l.spec.background = .paper; return l }())
    static let all = [ocean, highContrast, compact, text125, coralPaper]
}
