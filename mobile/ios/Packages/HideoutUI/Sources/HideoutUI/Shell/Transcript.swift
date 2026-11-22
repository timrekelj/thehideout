import HideoutCore
import HideoutDesign
import SwiftUI

/// A channel's messages, newest at the bottom, with the composer. Phase 1 shows the plumbing end to end (history,
/// older pages, optimistic send, failed + retry); the full transcript (jump to unread, embeds, menus) is phase 3.
struct TranscriptScreen: View {
    @Environment(AppStore.self) private var store
    @Environment(\.theme) private var theme
    @Environment(\.calendar) private var calendar
    let channelID: ID
    @State private var draft = ""

    var body: some View {
        let state = store.state
        let log = state.log(channelID)
        let messages = log?.messages ?? []
        let outgoing = state.outgoing(in: channelID)
        let guildChannel = state.channels.values.joined().first { $0.id == channelID }
        ScrollView {
            LazyVStack(spacing: 0) {
                if log?.hasMore == true {
                    ProgressView().padding().onAppear { store.loadOlder(channelID) }
                }
                if log?.loaded == true, messages.isEmpty, outgoing.isEmpty {
                    ContentUnavailableView("Nothing here yet", systemImage: "bubble.left", description: Text("Say hi."))
                        .padding(.top, Space.s16)
                }
                ForEach(Array(messages.enumerated()), id: \.element.id) { index, m in
                    let previous = index > 0 ? messages[index - 1] : nil
                    if previous == nil || !calendar.isDate(previous!.createdAt, inSameDayAs: m.createdAt) {
                        DayDivider(date: m.createdAt)
                    }
                    if case .after(let c, let id) = state.marker, c == channelID, id == previous?.id, previous != nil || id == nil {
                        NewDivider()
                    }
                    MessageRow(
                        author: state.userName(m.authorID), authorID: String(m.authorID), time: m.createdAt, text: m.content,
                        reactions: m.reactions, me: state.me?.username, grouped: isFollowUp(m, previous), edited: m.editedAt != nil
                    )
                    .id(state.rowID(m))
                }
                ForEach(outgoing) { o in
                    MessageRow(author: state.me?.username ?? "you", authorID: String(state.myID), time: o.at, text: o.content, me: state.me?.username,
                               grouped: messages.last?.authorID == state.myID && o.id != outgoing.first?.id ? true : false, delivery: o.state,
                               onRetry: { store.retry(nonce: o.nonce) })
                        .id("n:\(o.nonce)")
                }
                Color.clear.frame(height: Space.s2).id("bottom")
            }
        }
        .defaultScrollAnchor(.bottom)
        .scrollDismissesKeyboard(.interactively)
        .background(theme.colors.bg)
        .navigationTitle(guildChannel.map { "#\($0.name)" } ?? dmTitle(state))
        .navigationSubtitle(guildChannel?.topic ?? "")
        .navigationBarTitleDisplayMode(.inline)
        .safeAreaInset(edge: .bottom) { composer }
        .task { store.open(channel: channelID) }
        .onDisappear { if store.state.openChannel == channelID { store.closeChannel() } }
    }

    private func dmTitle(_ s: AppState) -> String {
        s.dm(channelID).flatMap { s.dmPeer($0)?.username } ?? "Channel"
    }

    private func isFollowUp(_ m: Message, _ previous: Message?) -> Bool {
        guard let p = previous, p.authorID == m.authorID else { return false }
        return m.createdAt.timeIntervalSince(p.createdAt) < 300 && calendar.isDate(p.createdAt, inSameDayAs: m.createdAt)
    }

    private var composer: some View {
        GlassEffectContainer(spacing: Space.s2) {
            HStack(alignment: .bottom, spacing: Space.s2) {
                TextField("Message", text: $draft, axis: .vertical)
                    .font(DesignFont.ui(size: 16, weight: 450))
                    .lineLimit(1...5)
                    .padding(.horizontal, Space.s4)
                    .padding(.vertical, 11)
                    .glassEffect(.regular, in: .rect(cornerRadius: 22))
                Button {
                    if store.send(draft, to: channelID) { draft = "" }
                } label: {
                    Image(systemName: "arrow.up").font(.system(size: 17, weight: .bold)).frame(width: 22, height: 22)
                }
                .buttonStyle(.glassProminent)
                .buttonBorderShape(.circle)
                .controlSize(.large)
                .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                .accessibilityLabel("Send")
            }
            .padding(.horizontal, Space.s3)
            .padding(.bottom, Space.s1)
        }
    }
}

struct DayDivider: View {
    @Environment(\.theme) private var theme
    @Environment(\.calendar) private var calendar
    @Environment(\.locale) private var locale
    @Environment(\.currentDate) private var now
    let date: Date

    var body: some View {
        HStack(spacing: Space.s3) {
            Rectangle().fill(theme.colors.borderSubtle).frame(height: 1)
            Text(label).typeStyle(\.caption, color: \.textMuted).fixedSize()
            Rectangle().fill(theme.colors.borderSubtle).frame(height: 1)
        }
        .padding(.horizontal, Space.s4)
        .padding(.vertical, Space.s3)
        .accessibilityElement(children: .combine)
    }

    private var label: String {
        if calendar.isDate(date, inSameDayAs: now) { return "Today" }
        if let y = calendar.date(byAdding: .day, value: -1, to: now), calendar.isDate(date, inSameDayAs: y) { return "Yesterday" }
        return date.formatted(Date.FormatStyle(locale: locale, calendar: calendar, timeZone: calendar.timeZone).weekday(.wide).month(.abbreviated).day())
    }
}

struct NewDivider: View {
    @Environment(\.theme) private var theme

    var body: some View {
        HStack(spacing: 0) {
            Rectangle().fill(theme.colors.accent).frame(height: 1)
            Text("New")
                .font(DesignFont.ui(size: 12, weight: 650))
                .foregroundStyle(theme.colors.accentFg)
                .padding(.horizontal, 8).padding(.vertical, 2)
                .background(theme.colors.accent, in: RoundedRectangle(cornerRadius: Radius.sm, style: .continuous))
        }
        .padding(.horizontal, Space.s4)
        .padding(.vertical, Space.s2)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("New messages")
    }
}

// Entry points for snapshot tests, which cannot see the shell's internal screens.
public struct ChannelListScreenForTests: View {
    let guild: ID
    public init(guild: ID) { self.guild = guild }
    public var body: some View { ChannelListScreen(guildID: guild) }
}

public struct TranscriptScreenForTests: View {
    let channel: ID
    public init(channel: ID) { self.channel = channel }
    public var body: some View { TranscriptScreen(channelID: channel) }
}
