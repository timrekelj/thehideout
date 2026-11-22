import HideoutCore
import HideoutDesign
import SwiftUI

struct DMsTab: View {
    @Environment(AppStore.self) private var store
    @Environment(\.theme) private var theme
    @State private var showNewMessage = false
    /// The conversation pushed on this tab's stack: the shell's explicit route (`MainShell` binds it),
    /// or the row tapped in the list. nil: the list itself.
    @Binding var pushed: ChannelRoute?

    init(pushed: Binding<ChannelRoute?> = .constant(nil)) { _pushed = pushed }

    /// The stack's path as that single route: the list at the root, at most one transcript on top, so
    /// Back returns to the list and the shell's route request pushes (or replaces) the conversation.
    private var path: Binding<[ChannelRoute]> {
        Binding(get: { pushed.map { [$0] } ?? [] }, set: { pushed = $0.last })
    }

    var body: some View {
        NavigationStack(path: path) {
            let dms = store.state.dms.sorted { a, b in
                (store.state.log(a.id)?.messages.last?.createdAt ?? a.createdAt) > (store.state.log(b.id)?.messages.last?.createdAt ?? b.createdAt)
            }
            TokenList {
                if dms.isEmpty {
                    ContentUnavailableView {
                        Label("No conversations", systemImage: "bubble.left.and.bubble.right").typeStyle(\.title)
                    } description: {
                        Text("Start a conversation with someone from your servers.").typeStyle(\.body, color: \.textMuted)
                    }
                    .listRowBackground(Color.clear)
                }
                ForEach(dms) { dm in
                    if let peer = store.state.dmPeer(dm) {
                        let unread = store.state.unread(dm.id)
                        NavigationLink(value: ChannelRoute(id: dm.id)) {
                            HStack(spacing: Space.s4) {
                                Avatar(user: peer, size: 44)
                                Text(peer.username).typeStyle(unread.count > 0 ? \.bodyStrong : \.body)
                                Spacer()
                                if unread.mentions > 0 { UnreadPill(unread.mentions) } else if unread.count > 0 { UnreadDot() }
                            }
                            .padding(.vertical, Space.s1)
                            .accessibilityElement(children: .combine)
                        }
                        .listRowBackground(theme.colors.surface)
                    }
                }
            }
            .navigationTitle("Direct messages")
            .navigationDestination(for: ChannelRoute.self) { TranscriptScreen(channelID: $0.id) }
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    Button("New message", systemImage: "square.and.pencil") { showNewMessage = true }
                }
            }
            .sheet(isPresented: $showNewMessage) { NewMessageSheet() }
        }
    }
}

/// Choose someone from your servers to start (or jump to) a DM with (DM-02).
struct NewMessageSheet: View {
    @Environment(AppStore.self) private var store
    @Environment(\.theme) private var theme
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""

    var body: some View {
        NavigationStack {
            let people = store.state.knownUsers.filter { query.isEmpty || $0.username.localizedCaseInsensitiveContains(query) }
            TokenList {
                ForEach(people) { user in
                    Button {
                        // The reducer records the route request; the shell lands here in the
                        // conversation (existing or freshly opened) once it is set.
                        store.startDm(user: user.id)
                        dismiss()
                    } label: {
                        HStack(spacing: Space.s4) { Avatar(user: user, size: 36); Text(user.username).typeStyle(\.body); Spacer() }
                    }
                    .buttonStyle(.plain)
                    .listRowBackground(theme.colors.surface)
                }
            }
            .searchable(text: $query, prompt: "Search people")
            .navigationTitle("New message")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } } }
        }
        .presentationDetents([.medium, .large])
    }
}

struct MentionsTab: View {
    @Environment(AppStore.self) private var store
    @Environment(\.theme) private var theme
    /// The tab's retained path (the shell binds it): the feed at the root, a transcript on top.
    @Binding var path: [ChannelRoute]

    init(path: Binding<[ChannelRoute]> = .constant([])) { _path = path }

    var body: some View {
        NavigationStack(path: $path) {
            let feed = store.state.mentionFeed()
            Group {
                if feed.isEmpty {
                    ContentUnavailableView {
                        Label("No mentions yet", systemImage: "at").typeStyle(\.title)
                    } description: {
                        Text("Messages that mention you land here, whatever the server.").typeStyle(\.body, color: \.textMuted)
                    }
                } else {
                    TokenList {
                        ForEach(feed) { entry in
                            NavigationLink(value: ChannelRoute(id: entry.channelID)) {
                                MentionRow(entry: entry)
                            }
                            .listRowBackground(theme.colors.surface)
                        }
                    }
                }
            }
            .background(theme.colors.bg)
            .navigationTitle("Mentions")
            .navigationDestination(for: ChannelRoute.self) { TranscriptScreen(channelID: $0.id) }
        }
    }
}

struct MentionRow: View {
    @Environment(AppStore.self) private var store
    let entry: MentionEntry

    var body: some View {
        let channelName: String = {
            if let dm = store.state.dm(entry.channelID) { return store.state.dmPeer(dm)?.username ?? "DM" }
            if let c = store.state.channels.values.joined().first(where: { $0.id == entry.channelID }) { return "#\(c.name)" }
            return "Channel"
        }()
        HStack(alignment: .top, spacing: Space.s4) {
            Avatar(name: store.state.userName(entry.message.authorID), id: String(entry.message.authorID), size: 40)
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(store.state.userName(entry.message.authorID)).typeStyle(\.bodyStrong)
                    Text("in \(channelName)").typeStyle(\.caption, color: \.textMuted)
                }
                Text(entry.message.content).typeStyle(\.body, color: \.textMuted).lineLimit(2)
            }
        }
        .padding(.vertical, Space.s1)
        .accessibilityElement(children: .combine)
    }
}

struct YouTab: View {
    @Environment(AppStore.self) private var store
    @Environment(\.theme) private var theme
    @State private var showLook = false
    @State private var confirmSignOut = false

    var body: some View {
        NavigationStack {
            TokenList {
                if let me = store.state.me {
                    Section {
                        HStack(spacing: Space.s4) {
                            Avatar(user: me, size: 64)
                            VStack(alignment: .leading, spacing: 3) {
                                Text(me.username).typeStyle(\.heading)
                                if let email = me.email { Text(email).typeStyle(\.caption, color: \.textMuted) }
                                ConnectionLabel(status: store.state.connection)
                            }
                        }
                        .padding(.vertical, Space.s2)
                        .accessibilityElement(children: .combine)
                    }
                    .listRowBackground(theme.colors.surface)
                }
                Section {
                    Button { showLook = true } label: { Label("Appearance", systemImage: "paintpalette.fill").typeStyle(\.body) }
                    #if DEBUG
                    NavigationLink { GalleryView() } label: { Label("Design gallery", systemImage: "swatchpalette.fill").typeStyle(\.body) }
                    #endif
                    LabeledContent { Text("0.1.0").typeStyle(\.caption, color: \.textMuted) } label: { Label("About", systemImage: "info.circle.fill").typeStyle(\.body) }
                }
                .listRowBackground(theme.colors.surface)
                Section {
                    Button(role: .destructive) { confirmSignOut = true } label: { Text("Sign out").typeStyle(\.body, color: \.danger) }
                }
                .listRowBackground(theme.colors.surface)
            }
            .navigationTitle("You")
            .sheet(isPresented: $showLook) { LookSheet() }
            .confirmationDialog("Sign out of thehideout?", isPresented: $confirmSignOut, titleVisibility: .visible) {
                Button("Sign out", role: .destructive) { store.signOut() }
            } message: { Text("This also clears the messages cached on this device.") }
        }
    }
}

struct ConnectionLabel: View {
    @Environment(\.theme) private var theme
    let status: ConnectionStatus

    var body: some View {
        HStack(spacing: 6) {
            Circle().fill(color).frame(width: 7, height: 7)
            Text(text).typeStyle(\.caption, color: \.textMuted)
        }
    }

    private var color: Color {
        switch status {
        case .connected: theme.colors.success
        case .connecting: theme.colors.warning
        case .reconnecting: theme.colors.danger
        }
    }

    private var text: String {
        switch status {
        case .connected: "Connected"
        case .connecting: "Connecting"
        case .reconnecting(let s): "Reconnecting in \(s)s"
        }
    }
}
