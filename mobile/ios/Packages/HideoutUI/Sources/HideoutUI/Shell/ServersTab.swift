import HideoutCore
import HideoutDesign
import SwiftUI

public struct ChannelRoute: Hashable, Sendable {
    public let id: ID
    public init(id: ID) { self.id = id }
}

/// A destination in the Servers tab's stack: a server's channel list, then a transcript. The shell owns
/// the path (`ShellNav.serversPath`), so returning to the tab knows which transcript it retains.
public enum ServerRoute: Hashable, Sendable {
    case guild(ID)
    case channel(ID)
}

/// Servers: the list at the root, then a server's channels (sections), then a transcript.
struct ServersTab: View {
    @Environment(AppStore.self) private var store
    @Environment(\.theme) private var theme
    /// The tab's retained navigation path (the shell binds it): the list at the root, at most a server
    /// and its transcript on top, so Back returns toward the list and a returning tab can be synced.
    @Binding var path: [ServerRoute]

    init(path: Binding<[ServerRoute]> = .constant([])) { _path = path }

    var body: some View {
        NavigationStack(path: $path) {
            TokenList {
                ForEach(store.state.guilds) { guild in
                    NavigationLink(value: ServerRoute.guild(guild.id)) {
                        ServerListRow(guild: guild, unread: store.state.guildUnread(guild.id))
                    }
                    .listRowBackground(theme.colors.surface)
                }
            }
            .navigationTitle("Servers")
            .navigationDestination(for: ServerRoute.self) { route in
                switch route {
                case .guild(let id): ChannelListScreen(guildID: id)
                case .channel(let id): TranscriptScreen(channelID: id)
                }
            }
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    Menu {
                        Button("Create a server", systemImage: "plus.circle") {}.disabled(true)
                        Button("Join a server", systemImage: "person.badge.plus") {}.disabled(true)
                    } label: { Image(systemName: "plus") }
                    .accessibilityLabel("Add a server")
                }
            }
        }
    }
}

struct ServerListRow: View {
    @Environment(\.theme) private var theme
    let guild: Guild
    let unread: Unread

    var body: some View {
        HStack(spacing: Space.s4) {
            ServerBadge(guild: guild, size: 48)
            VStack(alignment: .leading, spacing: 2) {
                Text(guild.name).typeStyle(unread.count > 0 ? \.bodyStrong : \.body).lineLimit(1)
                if unread.mentions > 0 {
                    Text("\(unread.mentions) mention\(unread.mentions == 1 ? "" : "s")").typeStyle(\.caption, color: \.accentText)
                } else if unread.count > 0 {
                    Text("\(unread.count) new").typeStyle(\.caption, color: \.textMuted)
                }
            }
            Spacer(minLength: Space.s2)
            if unread.mentions > 0 { UnreadPill(unread.mentions) } else if unread.count > 0 { UnreadDot() }
        }
        .padding(.vertical, Space.s1)
        .accessibilityElement(children: .combine)
    }
}

/// A server's channels grouped by its sections (collapsible), with voice participants under their channel.
struct ChannelListScreen: View {
    @Environment(AppStore.self) private var store
    @Environment(\.theme) private var theme
    let guildID: ID
    @State private var showVoiceComingSoon = false

    var body: some View {
        let state = store.state
        let layout = state.layouts[guildID]
        TokenList {
            let top = (layout?.channelIDs(in: nil) ?? []).compactMap { id in state.channel(id, in: guildID) }
            if !top.isEmpty {
                Section { rows(top) }
            }
            ForEach(layout?.sections ?? []) { section in
                let channels = (layout?.channelIDs(in: section.id) ?? []).compactMap { state.channel($0, in: guildID) }
                let open = state.sectionOpen(guild: guildID, section: section.id)
                Section {
                    if open { rows(channels) }
                } header: {
                    SectionToggle(title: section.name, open: open) { store.toggleSection(guild: guildID, section: section.id) }
                }
            }
        }
        .navigationTitle(state.guild(guildID)?.name ?? "Server")
        .navigationBarTitleDisplayMode(.large)
        .task { store.enterGuild(guildID) }
        .alert("Voice channels", isPresented: $showVoiceComingSoon) {
            Button("OK", role: .cancel) {}
        } message: {
            Text("Voice, video and screen share are coming to thehideout for iOS soon.")
        }
    }

    @ViewBuilder
    private func rows(_ channels: [Channel]) -> some View {
        ForEach(channels) { channel in
            if channel.kind == .text {
                NavigationLink(value: ServerRoute.channel(channel.id)) {
                    ChannelRow(channel, unread: store.state.unread(channel.id))
                }
                .listRowBackground(theme.colors.surface)
                .listRowInsets(EdgeInsets(top: 0, leading: Space.s2, bottom: 0, trailing: Space.s3))
            } else {
                VStack(spacing: 0) {
                    Button { showVoiceComingSoon = true } label: { ChannelRow(channel) }
                        .buttonStyle(.plain)
                    ForEach(store.state.participants(of: channel.id, in: guildID), id: \.userID) { p in
                        if let user = store.state.users[p.userID] { VoiceParticipantRow(user: user, state: p) }
                    }
                }
                .listRowBackground(theme.colors.surface)
                .listRowInsets(EdgeInsets(top: 0, leading: Space.s2, bottom: Space.s1, trailing: Space.s3))
            }
        }
    }
}

struct SectionToggle: View {
    @Environment(\.theme) private var theme
    @Environment(\.motionReduced) private var reduced
    let title: String
    let open: Bool
    let action: () -> Void

    var body: some View {
        Button {
            withAnimation(reduced ? nil : Motion.disclosure, action)
        } label: {
            HStack(spacing: 6) {
                Image(systemName: "chevron.down")
                    .font(.system(size: 11, weight: .bold))
                    .rotationEffect(.degrees(open ? 0 : -90))
                Text(title.uppercased()).typeStyle(\.label, color: \.textMuted).tracking(0.9)
                Spacer()
            }
            .foregroundStyle(theme.colors.textMuted)
            .frame(minHeight: 32)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(title), \(open ? "expanded" : "collapsed")")
        .accessibilityAddTraits(.isHeader)
    }
}
