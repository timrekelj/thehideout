import HideoutCore
import HideoutDesign
import Observation
import SwiftUI

public enum ShellTab: Hashable, Sendable { case servers, dms, mentions, you }

/// The shell's navigation state, kept out of the view so tests can drive it directly: the selected tab,
/// each tab's retained navigation path, and the store's explicit route request.
///
/// Selecting a tab syncs the store's open channel with what that tab shows — the transcript on top of
/// its retained path becomes the open (read) channel again, or nothing is open when the tab shows a
/// list. Without that, a DM route leaves a guild transcript mounted (hidden) in the Servers tab while
/// `openChannel` points at the DM, so messages in the visible transcript count as unread. The sync runs
/// on the explicit selection, not on SwiftUI re-running a transcript's `.task` when its tab returns.
@MainActor @Observable
public final class ShellNav {
    public var tab: ShellTab?
    /// The Servers tab's retained path: the server list at the root, then a server, then a transcript.
    public var serversPath: [ServerRoute] = []
    /// The conversation pushed in the DMs tab's stack.
    public var dmRoute: ChannelRoute?
    /// The Mentions tab's retained path: the feed at the root, a transcript on top.
    public var mentionsPath: [ChannelRoute] = []
    public let initialTab: ShellTab

    public init(initialTab: ShellTab, tab: ShellTab? = nil) {
        self.initialTab = initialTab
        self.tab = tab
    }

    public var selectedTab: ShellTab { tab ?? initialTab }

    /// The channel whose transcript the selected tab shows (its retained path's top), if any.
    public var foregroundChannel: ID? {
        switch selectedTab {
        case .servers:
            if case .channel(let id)? = serversPath.last { return id }
            return nil
        case .dms: return dmRoute?.id
        case .mentions: return mentionsPath.last?.id
        case .you: return nil
        }
    }

    /// Selects `tab` (a tap on the tab bar, or a route request) and makes the store agree with what the
    /// tab now shows. An ordinary Back only pops a path; it never changes the selected tab.
    public func select(_ tab: ShellTab, store: AppStore) {
        self.tab = tab
        if let channel = foregroundChannel { store.open(channel: channel) } else { store.closeChannel() }
    }

    /// The store's one explicit route: land in the DMs tab with the conversation pushed. Nothing else
    /// navigates — an inbound DM never asks for a route.
    public func follow(_ request: RouteRequest, store: AppStore) {
        dmRoute = ChannelRoute(id: request.channel)
        select(.dms, store: store)
    }
}

/// The signed-in shell: the four tabs on the Liquid Glass tab bar (`TabView`), each with its own
/// navigation stack whose path the shell retains (`ShellNav`), so a route can land cross-tab and a
/// returning tab can be synced with the store. On iPad and other regular widths `.sidebarAdaptable`
/// turns the bar into a sidebar.
struct MainShell: View {
    @Environment(AppStore.self) private var store
    @Environment(\.initialTab) private var initialTab

    var body: some View {
        ShellBody(initialTab: initialTab)
    }
}

private struct ShellBody: View {
    @Environment(AppStore.self) private var store
    // An owned `@Observable` object: `@State` tracks it under the Observation framework.
    @State private var nav: ShellNav

    init(initialTab: ShellTab) {
        _nav = State(initialValue: ShellNav(initialTab: initialTab))
    }

    var body: some View {
        let selection = Binding(get: { nav.selectedTab }, set: { nav.select($0, store: store) })
        let home = store.state.homeUnread
        let serverMentions = store.state.guilds.reduce(0) { $0 + store.state.guildUnread($1.id).mentions }
        TabView(selection: selection) {
            Tab("Servers", systemImage: "square.grid.2x2.fill", value: ShellTab.servers) { ServersTab(path: $nav.serversPath) }
                .badge(serverMentions)
            Tab("DMs", systemImage: "bubble.left.and.bubble.right.fill", value: ShellTab.dms) { DMsTab(pushed: $nav.dmRoute) }
                .badge(home.mentions)
            // Mentions badges use the accent (never red, which is errors only): the system tab badge is
            // already accent-tinted via `.tint(theme.colors.accent)` at the root (`ThemedRoot`).
            Tab("Mentions", systemImage: "at", value: ShellTab.mentions) { MentionsTab(path: $nav.mentionsPath) }
                .badge(store.state.mentionFeed().count)
            Tab("You", systemImage: "person.crop.circle.fill", value: ShellTab.you) { YouTab() }
        }
        .tabViewStyle(.sidebarAdaptable)
        .tabBarMinimizeBehavior(.onScrollDown)
        // The one route the store asks for explicitly: `startDm` — existing or freshly opened, answered
        // synchronously or after the StartDm reply — lands in the conversation (DMs tab selected, its
        // transcript pushed) instead of leaving a guild transcript on screen while a DM is "open".
        // Selecting a tab afterwards re-syncs the open channel with the transcript that tab retains.
        .onChange(of: store.state.routeRequest) { _, request in
            guard let request else { return }
            nav.follow(request, store: store)
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
