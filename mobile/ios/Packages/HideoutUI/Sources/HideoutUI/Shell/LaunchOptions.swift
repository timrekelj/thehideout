import Foundation

/// What the launch arguments ask for (`--fake`, `--gallery`, `--ui-testing`).
public struct LaunchOptions: Sendable, Equatable {
    /// Swap the network for the in-memory `FakeBackend` and start signed in.
    public var fake = false
    /// Open the design gallery instead of the app (debug builds only).
    public var gallery = false
    /// `--tab servers|dms|mentions|you`: the tab to open on (debug builds; for screenshots and demos).
    public var tab: ShellTab = .servers

    public init(fake: Bool = false, gallery: Bool = false, tab: ShellTab = .servers) {
        self.fake = fake
        self.gallery = gallery
        self.tab = tab
    }

    public init(arguments: [String]) {
        fake = arguments.contains("--fake") || arguments.contains("--ui-testing")
        #if DEBUG
        gallery = arguments.contains("--gallery")
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
