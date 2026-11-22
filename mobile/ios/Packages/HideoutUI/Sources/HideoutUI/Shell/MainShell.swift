import HideoutCore
import HideoutDesign
import SwiftUI

public enum ShellTab: Hashable, Sendable { case servers, dms, mentions, you }

/// The signed-in shell: the four tabs on the Liquid Glass tab bar (`TabView`), each with its own navigation stack.
/// On iPad and other regular widths `.sidebarAdaptable` turns the bar into a sidebar.
struct MainShell: View {
    @Environment(AppStore.self) private var store
    @State private var tab: ShellTab = .servers

    var body: some View {
        let home = store.state.homeUnread
        TabView(selection: $tab) {
            Tab("Servers", systemImage: "square.grid.2x2.fill", value: ShellTab.servers) { ServersTab() }
            Tab("DMs", systemImage: "bubble.left.and.bubble.right.fill", value: ShellTab.dms) { DMsTab() }
                .badge(home.mentions)
            Tab("Mentions", systemImage: "at", value: ShellTab.mentions) { MentionsTab() }
            Tab("You", systemImage: "person.crop.circle.fill", value: ShellTab.you) { YouTab() }
        }
        .tabViewStyle(.sidebarAdaptable)
        .tabBarMinimizeBehavior(.onScrollDown)
    }
}

/// A scrolling list on token surfaces: the palette behind, system chrome on top.
struct TokenList<Content: View>: View {
    @Environment(\.theme) private var theme
    @Environment(\.horizontalSizeClass) private var sizeClass
    @ViewBuilder var content: Content

    var body: some View {
        List { content }
            .listStyle(.insetGrouped)
            .scrollContentBackground(.hidden)
            .listRowSeparatorTint(theme.colors.borderSubtle)
            // Readable width on iPad until the split view lands (phase 3); the palette fills the rest.
            .frame(maxWidth: sizeClass == .regular ? 720 : .infinity)
            .frame(maxWidth: .infinity)
            .background(theme.colors.bg)
    }
}
