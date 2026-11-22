import HideoutCore
import HideoutDesign
import SwiftUI

public enum ShellTab: Hashable, Sendable { case servers, dms, mentions, you }

/// The signed-in shell: the four tabs on the Liquid Glass tab bar (`TabView`), each with its own navigation stack.
/// On iPad and other regular widths `.sidebarAdaptable` turns the bar into a sidebar.
struct MainShell: View {
    @Environment(AppStore.self) private var store
    @Environment(\.initialTab) private var initialTab
    @State private var tab: ShellTab?
    /// The conversation pushed in the DMs tab's stack: an explicit route request (`startDm` from New
    /// message or a member profile) sets it, and so does tapping a row in the list. nil: the list.
    @State private var dmRoute: ChannelRoute?

    var body: some View {
        let selection = Binding(get: { tab ?? initialTab }, set: { tab = $0 })
        let home = store.state.homeUnread
        let serverMentions = store.state.guilds.reduce(0) { $0 + store.state.guildUnread($1.id).mentions }
        TabView(selection: selection) {
            Tab("Servers", systemImage: "square.grid.2x2.fill", value: ShellTab.servers) { ServersTab() }
                .badge(serverMentions)
            Tab("DMs", systemImage: "bubble.left.and.bubble.right.fill", value: ShellTab.dms) { DMsTab(pushed: $dmRoute) }
                .badge(home.mentions)
            // Mentions badges use the accent (never red, which is errors only): the system tab badge is
            // already accent-tinted via `.tint(theme.colors.accent)` at the root (`ThemedRoot`).
            Tab("Mentions", systemImage: "at", value: ShellTab.mentions) { MentionsTab() }
                .badge(store.state.mentionFeed().count)
            Tab("You", systemImage: "person.crop.circle.fill", value: ShellTab.you) { YouTab() }
        }
        .tabViewStyle(.sidebarAdaptable)
        .tabBarMinimizeBehavior(.onScrollDown)
        // The one route the store asks for explicitly: `startDm` — existing or freshly opened, answered
        // synchronously or after the StartDm reply — lands in the conversation (DMs tab selected, its
        // transcript pushed) instead of leaving a guild transcript on screen while a DM is "open".
        // Nothing else navigates: an inbound DM never sets a route, and an ordinary back just pops the
        // stack (leaving the selected tab alone).
        .onChange(of: store.state.routeRequest) { _, request in
            guard let request else { return }
            tab = .dms
            dmRoute = ChannelRoute(id: request.channel)
        }
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
