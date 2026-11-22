import Foundation
import HideoutCore
import HideoutUI

/// Manual dependency injection: the few long-lived objects of the app, built once at launch.
@MainActor
final class AppContainer {
    static let serverURLKey = "server_url"
    static let defaultServer = "http://127.0.0.1:8080"

    let options: LaunchOptions
    let theme: ThemeStore
    let store: AppStore

    init(arguments: [String] = CommandLine.arguments, defaults: UserDefaults = .standard) {
        let options = LaunchOptions(arguments: arguments)
        self.options = options
        // The gallery and `--fake` runs leave the saved look alone only when asked; the look is the user's.
        theme = ThemeStore(defaults: defaults)
        if options.fake || options.gallery {
            let tokens = MemoryTokenStore(token: "fake")
            store = AppStore(api: FakeBackend(), tokens: tokens)
        } else {
            let tokens = KeychainTokenStore()
            let url = URL(string: defaults.string(forKey: Self.serverURLKey) ?? Self.defaultServer) ?? URL(string: Self.defaultServer)!
            let api = ConnectApiClient(baseURL: url, token: { tokens.load() })
            store = AppStore(api: api, tokens: tokens, cache: Cache.standard())
        }
    }
}
