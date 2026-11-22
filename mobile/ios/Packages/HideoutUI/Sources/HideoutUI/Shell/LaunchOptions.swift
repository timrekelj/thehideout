import Foundation

/// What the launch arguments ask for (`--fake`, `--gallery`, `--ui-testing`).
public struct LaunchOptions: Sendable, Equatable {
    /// Swap the network for the in-memory `FakeBackend` and start signed in.
    public var fake = false
    /// Open the design gallery instead of the app (debug builds only).
    public var gallery = false

    public init(fake: Bool = false, gallery: Bool = false) {
        self.fake = fake
        self.gallery = gallery
    }

    public init(arguments: [String]) {
        fake = arguments.contains("--fake") || arguments.contains("--ui-testing")
        #if DEBUG
        gallery = arguments.contains("--gallery")
        #endif
    }
}
