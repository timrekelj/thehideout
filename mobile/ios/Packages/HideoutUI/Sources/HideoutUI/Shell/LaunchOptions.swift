import Foundation

/// What the launch arguments ask for (`--fake`, `--gallery`, `--ui-testing`).
public struct LaunchOptions: Sendable, Equatable {
    /// Swap the network for the in-memory `FakeBackend` and start signed in.
    public var fake = false
    /// Open the design gallery instead of the app (debug builds only).
    public var gallery = false
    /// With `--fake`: start signed out, so the sign-in flows can be driven against the fake accounts.
    public var signedOut = false
    /// `--tab servers|dms|mentions|you`: the tab to open on (debug builds; for screenshots and demos).
    public var tab: ShellTab = .servers
    /// Debug-only demo states for screenshots: `--demo-terms` (the terms gate), `--demo-update`, `--demo-suspended`.
    public var demoTerms = false
    public var demoUpdate = false
    public var demoSuspended = false

    public init(fake: Bool = false, gallery: Bool = false, signedOut: Bool = false, tab: ShellTab = .servers,
                demoTerms: Bool = false, demoUpdate: Bool = false, demoSuspended: Bool = false) {
        self.fake = fake
        self.signedOut = signedOut
        self.gallery = gallery
        self.tab = tab
        self.demoTerms = demoTerms
        self.demoUpdate = demoUpdate
        self.demoSuspended = demoSuspended
    }

    public init(arguments: [String]) {
        fake = arguments.contains("--fake") || arguments.contains("--ui-testing")
        signedOut = arguments.contains("--signed-out")
        #if DEBUG
        gallery = arguments.contains("--gallery")
        demoTerms = arguments.contains("--demo-terms")
        demoUpdate = arguments.contains("--demo-update")
        demoSuspended = arguments.contains("--demo-suspended")
        if let i = arguments.firstIndex(of: "--tab"), i + 1 < arguments.count {
            switch arguments[i + 1] {
            case "dms": tab = .dms
            case "mentions": tab = .mentions
            case "you": tab = .you
            default: tab = .servers
            }
        }
        #endif
    }
}
