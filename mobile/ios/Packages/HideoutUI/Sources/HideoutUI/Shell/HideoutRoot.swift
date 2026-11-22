import HideoutCore
import HideoutDesign
import SwiftUI

/// The app's root: injects the store and the theme, runs the session bootstrap and ties the gateway to
/// `scenePhase` (it runs only while the app is active). The `@main` app is a thin shell around this.
public struct HideoutRoot: View {
    let store: AppStore
    let themeStore: ThemeStore
    let options: LaunchOptions
    @Environment(\.scenePhase) private var scenePhase

    public init(store: AppStore, themeStore: ThemeStore, options: LaunchOptions = LaunchOptions()) {
        self.store = store
        self.themeStore = themeStore
        self.options = options
    }

    public var body: some View {
        Group {
            if options.gallery {
                GalleryView()
            } else {
                RootView()
            }
        }
        .environment(store)
        .environment(themeStore)
        .environment(\.initialTab, options.tab)
        .themed(themeStore)
        .task {
            await store.bootstrap()
            store.setActive(scenePhase == .active)
        }
        .onChange(of: scenePhase) { _, phase in store.setActive(phase == .active) }
    }
}

extension EnvironmentValues {
    /// The tab the shell opens on.
    @Entry var initialTab: ShellTab = .servers
}

/// Switches on the session phase.
public struct RootView: View {
    @Environment(AppStore.self) private var store

    public init() {}

    public var body: some View {
        switch store.state.phase {
        case .loading: LoadingScreen()
        case .signedOut: SignedOutScreen()
        case .signedIn: MainShell()
        case .termsRequired(let version):
            GateScreen(symbol: "doc.text.fill", title: "Updated terms", message: "The terms changed (version \(version)). Accepting them arrives with sign-in in the next phase.")
        case .updateRequired:
            GateScreen(symbol: "arrow.down.app.fill", title: "Update required", message: "This version of thehideout is too old for the server. Install the latest build to continue.")
        case .suspended(let reason, _):
            GateScreen(symbol: "pause.circle.fill", title: "Account suspended", message: reason.isEmpty ? "An admin suspended this account." : reason)
        }
    }
}
