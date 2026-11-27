import HideoutCore
import HideoutDesign
import SwiftUI

/// The app's root: injects the store and the theme, runs the session bootstrap and ties the gateway to
/// `scenePhase` (it runs only while the app is active). The `@main` app is a thin shell around this.
public struct HideoutRoot: View {
    let store: AppStore
    let themeStore: ThemeStore
    let options: LaunchOptions
    let auth: AuthFlow
    let server: ServerSelection
    @Environment(\.scenePhase) private var scenePhase

    public init(store: AppStore, themeStore: ThemeStore, auth: AuthFlow, server: ServerSelection, options: LaunchOptions = LaunchOptions()) {
        self.store = store
        self.themeStore = themeStore
        self.auth = auth
        self.server = server
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
        .environment(auth)
        .environment(server)
        .environment(themeStore)
        .environment(\.initialTab, options.tab)
        .themed(themeStore)
        .task {
            await store.bootstrap()
            store.setActive(scenePhase == .active)
        }
        .onChange(of: scenePhase) { _, phase in store.setActive(phase == .active) }
        // Invite links: `thehideout://invite/<code>` and `https://<server>/invite/<code>` (universal link).
        .onOpenURL { url in if store.state.phase == .signedOut || store.state.phase == .loading { auth.handle(url: url) } }
        .onContinueUserActivity(NSUserActivityTypeBrowsingWeb) { activity in
            if let url = activity.webpageURL, store.state.phase == .signedOut || store.state.phase == .loading { auth.handle(url: url) }
        }
        // A sign-out starts the next sign-in from a clean first step.
        .onChange(of: store.state.phase) { old, new in if new == .signedOut && old != .loading { auth.showSignIn() } }
        .sensoryFeedback(.success, trigger: auth.signInCount)
        .confirmationDialog("Switch to \(auth.pendingInviteLink?.host ?? "")?",
                            isPresented: Binding(get: { auth.pendingInviteLink != nil }, set: { if !$0 { auth.dismissPendingInviteLink() } }),
                            titleVisibility: .visible) {
            Button("Switch and open the invite") { auth.switchServerForPendingInviteLink() }
            Button("Cancel", role: .cancel) { auth.dismissPendingInviteLink() }
        } message: {
            Text("This invite is for a different server than the one you're using.")
        }
    }
}

extension EnvironmentValues {
    /// The tab the shell opens on.
    @Entry var initialTab: ShellTab = .servers
}

/// Switches on the session phase.
public struct RootView: View {
    @Environment(AppStore.self) private var store
    @Environment(AuthFlow.self) private var auth

    public init() {}

    public var body: some View {
        Group {
            switch store.state.phase {
            case .loading: SplashScreen()
            case .signedOut: AuthScreen()
            case .signedIn:
                MainShell()
                    .sheet(isPresented: Binding(get: { auth.welcome != .hidden }, set: { if !$0 { auth.skipWelcome() } })) {
                        WelcomeSheet().presentationDetents([.medium])
                    }
            case .termsRequired: TermsGate()
            case .updateRequired: UpdateGate()
            case .suspended(let reason, let until): SuspendedGate(reason: reason, until: until)
            }
        }
        .animation(Motion.overlay, value: store.state.phase)
    }
}
