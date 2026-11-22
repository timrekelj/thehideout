import HideoutCore
import HideoutDesign
import HideoutUI
import SwiftUI

@main
struct HideoutApp: App {
    @State private var container = AppContainer()

    init() {
        DesignFonts.register()
    }

    var body: some Scene {
        WindowGroup {
            HideoutRoot(store: container.store, themeStore: container.theme, auth: container.auth, server: container.server, options: container.options)
        }
    }
}
