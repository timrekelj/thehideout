import HideoutCore
import HideoutDesign
import SwiftUI

/// Members · N, a phone sheet from the channel toolbar (MEM-01), grouped by the member's highest role (MEM-02).
/// Tapping a member opens their profile with a "Message" button that starts a DM (PROF-01).
struct MembersSheet: View {
    @Environment(AppStore.self) private var store
    @Environment(\.theme) private var theme
    @Environment(\.dismiss) private var dismiss
    let guildID: ID
    @State private var profile: User?

    struct Group { var title: String; var users: [User]; var colorKey: String }

    private func grouped(_ state: AppState) -> [Group] {
        let memberIDs = state.members[guildID] ?? []
        let roles = state.roles[guildID] ?? []
        let users = memberIDs.compactMap { state.users[$0] }.sorted { $0.username.lowercased() < $1.username.lowercased() }
        guard !roles.isEmpty else { return [Group(title: "Members", users: users, colorKey: "")] }
        var byRole: [ID: [User]] = [:]
        var noRole: [User] = []
        let assignments = state.memberRoles[guildID] ?? [:]
        for u in users {
            let assigned = assignments[u.id] ?? []
            let highest = roles.filter { assigned.contains($0.id) }.max { $0.position < $1.position }
            if let highest { byRole[highest.id, default: []].append(u) } else { noRole.append(u) }
        }
        var groups: [Group] = roles.sorted { $0.position > $1.position }.compactMap { role in
            guard let members = byRole[role.id], !members.isEmpty else { return nil }
            return Group(title: role.name, users: members, colorKey: role.color)
        }
        if !noRole.isEmpty { groups.append(Group(title: "Members", users: noRole, colorKey: "")) }
        return groups
    }

    var body: some View {
        NavigationStack {
            let state = store.state
            let groups = grouped(state)
            let total = (state.members[guildID] ?? []).count
            TokenList {
                ForEach(groups, id: \.title) { group in
                    Section {
                        ForEach(group.users) { user in
                            Button { profile = user } label: {
                                HStack(spacing: Space.s4) {
                                    Avatar(user: user, size: 36)
                                    Text(user.username).typeStyle(\.body, tint: group.colorKey.isEmpty ? theme.colors.text : (theme.colors.channel(group.colorKey)?.text ?? theme.colors.text))
                                    Spacer()
                                }
                                .padding(.vertical, 2)
                            }
                            .buttonStyle(.plain)
                            .listRowBackground(theme.colors.surface)
                        }
                    } header: {
                        Text("\(group.title.uppercased()) — \(group.users.count)")
                            .typeStyle(\.label, color: \.textMuted)
                    }
                }
            }
            .navigationTitle("Members · \(total)")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Close") { dismiss() } } }
            .sheet(item: $profile) { user in ProfileSheet(user: user) }
        }
        .presentationDetents([.medium, .large])
    }
}

/// A member's profile: avatar, username, and "Message" (opens or starts a DM, PROF-01).
struct ProfileSheet: View {
    @Environment(AppStore.self) private var store
    @Environment(\.theme) private var theme
    @Environment(\.dismiss) private var dismiss
    let user: User

    var body: some View {
        VStack(spacing: Space.s4) {
            Avatar(user: user, size: 84)
            Text(user.username).typeStyle(\.heading)
            if user.id != store.state.myID {
                Button {
                    store.startDm(user: user.id)
                    dismiss()
                } label: {
                    Label("Message", systemImage: "bubble.left.fill").frame(maxWidth: .infinity)
                }
                .buttonStyle(.glassProminent)
                .controlSize(.large)
                .padding(.horizontal, Space.s6)
            }
            Spacer()
        }
        .padding(.top, Space.s8)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(theme.colors.bg)
        .presentationDetents([.medium])
    }
}
