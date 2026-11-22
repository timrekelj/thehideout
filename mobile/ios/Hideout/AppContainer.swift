import Foundation
import HideoutCore
import HideoutUI
import UIKit

/// Manual dependency injection: the few long-lived objects of the app, built once at launch.
@MainActor
final class AppContainer {
    let options: LaunchOptions
    let theme: ThemeStore
    let server: ServerSelection
    let store: AppStore
    let auth: AuthFlow

    init(arguments: [String] = CommandLine.arguments, defaults: UserDefaults = .standard) {
        let options = LaunchOptions(arguments: arguments)
        self.options = options
        // The gallery and `--fake` runs leave the saved look alone only when asked; the look is the user's.
        theme = ThemeStore(defaults: defaults)
        let server: ServerSelection
        let store: AppStore
        if options.fake || options.gallery {
            let fake = FakeBackend()
            // Debug demo states (`--demo-*`): the gates on their own, for screenshots and manual checks.
            if options.demoTerms { fake.accounts.termsRequired = "2026-06-01" }
            if options.demoUpdate { fake.accounts.tooOld = true }
            if options.demoSuspended { fake.accounts.suspendMe = true }
            server = ServerSelection(fixed: fake)
            store = AppStore(api: server.backend, auth: server.backend, tokens: MemoryTokenStore(token: options.signedOut ? nil : "fake"))
        } else {
            let tokens = KeychainTokenStore()
            server = ServerSelection(defaults: defaults) { url in ConnectApiClient(baseURL: url, token: { tokens.load() }) }
            store = AppStore(api: server.backend, auth: server.backend, tokens: tokens, cache: Cache.standard())
        }
        self.server = server
        self.store = store
        // `UIDevice.current.name` needs an entitlement since iOS 16: the model ("iPhone", "iPad") is what the device list shows.
        auth = AuthFlow(server: server, deviceName: UIDevice.current.model, passkeys: SystemPasskeys(),
                        onSession: { token in await store.signedIn(token: token) },
                        onClientTooOld: { store.requireUpdate() })
    }
}
