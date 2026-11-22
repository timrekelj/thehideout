import HideoutCore
import HideoutDesign
import SwiftUI

struct DMsTab: View {
    @Environment(AppStore.self) private var store
    @Environment(\.theme) private var theme

    var body: some View {
        NavigationStack {
            TokenList {
                ForEach(store.state.dms) { dm in
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
                    Button("New message", systemImage: "square.and.pencil") {}.disabled(true)
                }
            }
        }
    }
}

struct MentionsTab: View {
    @Environment(\.theme) private var theme

    var body: some View {
        NavigationStack {
            ContentUnavailableView {
                Label("No mentions yet", systemImage: "at").typeStyle(\.title)
            } description: {
                Text("Messages that mention you land here, whatever the server.").typeStyle(\.body, color: \.textMuted)
            }
            .background(theme.colors.bg)
            .navigationTitle("Mentions")
        }
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
                    NavigationLink { GalleryView(embedded: true) } label: { Label("Design gallery", systemImage: "swatchpalette.fill").typeStyle(\.body) }
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
