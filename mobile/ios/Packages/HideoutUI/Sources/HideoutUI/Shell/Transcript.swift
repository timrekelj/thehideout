import HideoutCore
import HideoutDesign
import SwiftUI

/// A channel's messages, newest at the bottom, with the composer: grouping, day dividers, the "New" marker,
/// full markdown, reactions, pending/failed, load-older-keeps-anchor, a jump-to-newest glass button, message
/// actions (context menu + swipe-to-reply), autocomplete, per-channel drafts and the reconnecting banner.
struct TranscriptScreen: View {
    @Environment(AppStore.self) private var store
    @Environment(\.theme) private var theme
    @Environment(\.calendar) private var calendar
    let channelID: ID
    @State private var draft = ""
    @State private var editing: Message?
    @State private var draftBeforeEdit = ""
    @State private var showEmojiPicker = false
    @State private var showMembers = false
    @State private var scrolledID: String?
    @State private var leftBottomAt: Int?
    @State private var lightboxRequest: LightboxRequest?
    @FocusState private var composerFocused: Bool

    var body: some View {
        let state = store.state
        let log = state.log(channelID)
        let messages = log?.messages ?? []
        let outgoing = state.outgoing(in: channelID)
        let guildChannel = state.channels.values.joined().first { $0.id == channelID }
        let totalCount = messages.count + outgoing.count
        let newBelow = (leftBottomAt.map { totalCount - $0 }) ?? 0

        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: 0) {
                    if log?.hasMore == true {
                        ProgressView().padding().onAppear { store.loadOlder(channelID) }
                    }
                    if log?.loaded == true, messages.isEmpty, outgoing.isEmpty {
                        ContentUnavailableView(emptyTitle(state), systemImage: "bubble.left", description: Text(emptyDescription(state)))
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
                            reactions: m.reactions, attachments: m.attachments, embeds: m.embeds, messageID: m.id,
                            customEmojis: customEmojis(guild: guildChannel?.guildID), me: state.me?.username, grouped: isFollowUp(m, previous), edited: m.editedAt != nil,
                            mentionsMe: m.authorID != state.myID && state.mentionsMe(m.content),
                            isOwn: m.authorID == state.myID, canModerate: guildChannel != nil && state.perms(guildChannel!.guildID).allows(.manageMessages),
                            collapsedEmbeds: Set(state.collapsedEmbeds),
                            onReact: { emoji, on in store.react(channel: channelID, message: m.id, emoji: emoji, on: on) },
                            onReply: { reply(to: state.userName(m.authorID)) },
                            onEdit: { beginEdit(m) },
                            onDelete: { store.deleteMessage(channel: channelID, message: m.id) },
                            onMoreReactions: { showEmojiPicker = true },
                            onOpenLightbox: { items, i in lightboxRequest = LightboxRequest(items: items, startIndex: i) },
                            onToggleEmbedCollapsed: { ref in store.toggleEmbedCollapsed(ref) }
                        )
                        .id(state.rowID(m))
                    }
                    ForEach(outgoing) { o in
                        MessageRow(author: state.me?.username ?? "you", authorID: String(state.myID), time: o.at, text: o.content,
                                   attachmentPreviews: o.attachments, customEmojis: customEmojis(guild: guildChannel?.guildID), me: state.me?.username,
                                   grouped: messages.last?.authorID == state.myID && o.id != outgoing.first?.id ? true : false, delivery: o.state,
                                   isOwn: true, onRetry: { store.retry(nonce: o.nonce) })
                            .id("n:\(o.nonce)")
                    }
                    Color.clear.frame(height: Space.s2).id("bottom")
                }
            }
            .defaultScrollAnchor(.bottom)
            .scrollPosition(id: $scrolledID, anchor: .bottom)
            .onChange(of: scrolledID) { _, id in
                if id == "bottom" {
                    leftBottomAt = nil
                } else if leftBottomAt == nil {
                    leftBottomAt = totalCount
                }
            }
            .overlay(alignment: .bottom) {
                if newBelow > 0 {
                    Button {
                        withAnimation(Motion.control) { proxy.scrollTo("bottom", anchor: .bottom) }
                        leftBottomAt = nil
                    } label: {
                        HStack(spacing: 6) {
                            Image(systemName: "arrow.down")
                            Text("\(newBelow) new")
                        }
                        .font(DesignFont.ui(size: 13, weight: 650))
                        .foregroundStyle(theme.colors.accentFg)
                        .padding(.horizontal, 14)
                        .frame(minHeight: 36)
                        .glassEffect(.regular.tint(theme.colors.accent), in: Capsule())
                    }
                    .padding(.bottom, Space.s3)
                    .accessibilityLabel("\(newBelow) new messages, jump to the newest")
                }
            }
        }
        .scrollDismissesKeyboard(.interactively)
        .background(theme.colors.bg)
        .navigationTitle(guildChannel.map { "#\($0.name)" } ?? dmTitle(state))
        .navigationSubtitle(guildChannel?.topic ?? "")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            if guildChannel != nil {
                ToolbarItem(placement: .primaryAction) {
                    Button("Members", systemImage: "person.2") { showMembers = true }
                }
            }
        }
        .safeAreaInset(edge: .top) { if state.connection != .connected { ReconnectingBanner(status: state.connection) } }
        .safeAreaInset(edge: .bottom) { composer(state: state, guild: guildChannel?.guildID) }
        .sheet(isPresented: $showMembers) { if let g = guildChannel?.guildID { MembersSheet(guildID: g) } }
        .sheet(isPresented: $showEmojiPicker) {
            EmojiPickerSheet(serverEmojis: guildChannel.map { state.emojis[$0.guildID] ?? [] } ?? []) { entry in insert(entry.insertion) }
        }
        .fullScreenCover(item: $lightboxRequest) { request in
            MediaViewerView(request: request) { lightboxRequest = nil }
        }
        .task(id: channelID) {
            store.open(channel: channelID)
            draft = store.state.draft(channelID)
        }
        .onChange(of: draft) { _, new in if editing == nil { store.setDraft(channel: channelID, text: new) } }
        // Only a transcript that is still the open channel closes it on the way out: one going to the
        // background (its tab switched away, a sheet over it) must not clear the channel that just
        // became visible — the shell re-syncs the open channel on every tab selection instead.
        .onDisappear { if store.state.openChannel == channelID { store.closeChannel() } }
    }

    private func emptyTitle(_ s: AppState) -> String { s.dm(channelID) != nil ? "Start the conversation" : "Nothing here yet" }
    private func emptyDescription(_ s: AppState) -> String {
        if let dm = s.dm(channelID), let peer = s.dmPeer(dm) { return "This is the start of your chat with \(peer.username)." }
        return "Say hi."
    }

    private func dmTitle(_ s: AppState) -> String {
        s.dm(channelID).flatMap { s.dmPeer($0)?.username } ?? "Channel"
    }

    /// This channel's custom emoji keyed by id (MD-06/07, MSG-06/09); empty in a DM, same Unicode-only
    /// scope as the emoji picker and autocomplete (DM-06).
    private func customEmojis(guild: ID?) -> [ID: Emoji] {
        guard let guild else { return [:] }
        return Dictionary(uniqueKeysWithValues: (store.state.emojis[guild] ?? []).map { ($0.id, $0) })
    }

    private func isFollowUp(_ m: Message, _ previous: Message?) -> Bool {
        guard let p = previous, p.authorID == m.authorID else { return false }
        return m.createdAt.timeIntervalSince(p.createdAt) < 300 && calendar.isDate(p.createdAt, inSameDayAs: m.createdAt)
    }

    // MARK: composing

    /// Reply PREPENDS `@name ` to the draft and focuses the composer (MSG-03; the protocol has no reference).
    private func reply(to author: String) {
        draft = "@\(author) " + draft
        composerFocused = true
    }

    private func beginEdit(_ m: Message) {
        draftBeforeEdit = draft
        editing = m
        draft = m.content
        composerFocused = true
    }

    /// Edit mode restores the draft that was there before editing started, on Cancel or Save.
    private func cancelEdit() {
        editing = nil
        draft = draftBeforeEdit
    }

    private func commit() {
        if let m = editing {
            if store.editMessage(channel: channelID, message: m.id, content: draft) { cancelEdit() }
        } else {
            if store.send(draft, to: channelID) { draft = "" }
        }
    }

    private func insert(_ text: String) {
        draft += (draft.isEmpty || draft.hasSuffix(" ") ? "" : " ") + text + " "
        composerFocused = true
    }

    /// The autocomplete trigger from the end of the draft (the common case: typing at the caret's natural place).
    private var trigger: Trigger { Autocomplete.trigger(draft, caret: draft.utf16.count) }

    private func members(guild: ID?, state: AppState) -> [User] {
        if let guild { return (state.members[guild] ?? []).compactMap { state.users[$0] } }
        return state.dm(channelID)?.participants.filter { $0.id != state.myID } ?? []
    }

    private func replaceTrigger(_ start: Int, with completion: String) {
        let units = Array(draft.utf16)
        let prefixUnits = Array(units[0..<min(start, units.count)])
        let prefix = String(utf16CodeUnits: prefixUnits, count: prefixUnits.count)
        draft = prefix + completion + " "
    }

    @ViewBuilder
    private func autocompleteStrip(guild: ID?, state: AppState) -> some View {
        switch trigger {
        case .mention(let query, let start):
            let matches = Autocomplete.mentionMatches(query, members: members(guild: guild, state: state))
            if !matches.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: Space.s2) {
                        ForEach(matches) { u in
                            Button { replaceTrigger(start, with: "@\(u.username)") } label: {
                                HStack(spacing: 6) { Avatar(user: u, size: 20); Text(u.username) }
                                    .typeStyle(\.caption)
                                    .padding(.horizontal, 10).frame(minHeight: 32)
                                    .background(theme.colors.surfaceRaised, in: Capsule())
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(.horizontal, Space.s4)
                }
            }
        case .emojiShortcode(let query, let start):
            let matches = Autocomplete.emojiMatches(query, serverEmojis: guild.map { state.emojis[$0] ?? [] } ?? [])
            if !matches.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: Space.s2) {
                        ForEach(matches) { entry in
                            Button { replaceTrigger(start, with: entry.insertion) } label: {
                                HStack(spacing: 6) {
                                    switch entry {
                                    case .unicode(let e, _): Text(e)
                                    case .custom(let id, let name, let image, let animated):
                                        CustomEmojiImage(emoji: Emoji(id: id, guildID: guild ?? 0, name: name, animated: animated, image: image), size: 20)
                                    }
                                    Text(entry.name).typeStyle(\.caption, color: \.textMuted)
                                }
                                .padding(.horizontal, 10).frame(minHeight: 32)
                                .background(theme.colors.surfaceRaised, in: Capsule())
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(.horizontal, Space.s4)
                }
            }
        case .none: EmptyView()
        }
    }

    private func composer(state: AppState, guild: ID?) -> some View {
        let count = draft.count
        let attachDraft = state.attachDraft(channelID)
        let canSend = editing != nil
            ? !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && count <= AppState.maxMessageChars
            : (!draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || attachDraft.readyCount > 0)
                && count <= AppState.maxMessageChars && !attachDraft.uploading
        return VStack(spacing: Space.s2) {
            autocompleteStrip(guild: guild, state: state)
            if editing == nil {
                AttachTray(draft: attachDraft, onRemove: { store.removeAttachment($0) }, onRetry: { store.retryAttachment($0) })
            }
            if editing != nil {
                HStack {
                    Text("Editing message").typeStyle(\.caption, color: \.accentText)
                    Spacer()
                    Button("Cancel", action: cancelEdit).font(DesignFont.ui(size: 13, weight: 530))
                }
                .padding(.horizontal, Space.s4)
            }
            if count > 3800 {
                Text("\(count) / \(AppState.maxMessageChars)")
                    .typeStyle(\.caption, color: count > AppState.maxMessageChars ? \.danger : \.textMuted)
                    .frame(maxWidth: .infinity, alignment: .trailing)
                    .padding(.horizontal, Space.s4)
            }
            GlassEffectContainer(spacing: Space.s2) {
                HStack(alignment: .bottom, spacing: Space.s2) {
                    if editing == nil {
                        AttachMenuButton(onPicked: { files in store.attachFiles(channel: channelID, files: files) })
                            .buttonStyle(.glass)
                            .buttonBorderShape(.circle)
                    }
                    Button { showEmojiPicker = true } label: { Image(systemName: "face.smiling") }
                        .buttonStyle(.glass)
                        .buttonBorderShape(.circle)
                        .accessibilityLabel("Emoji")
                    TextField(placeholder(state: state, guild: guild), text: $draft, axis: .vertical)
                        .font(DesignFont.ui(size: 16, weight: 450))
                        .lineLimit(1...5)
                        .focused($composerFocused)
                        .padding(.horizontal, Space.s4)
                        .padding(.vertical, 11)
                        .glassEffect(.regular, in: .rect(cornerRadius: 22))
                        .onSubmit(commit)
                        // A multi-line `TextField` is backed by a text view: XCUITest's automatic type
                        // detection flips between `TextField` and `TextView` as its content changes, so tests
                        // key off this fixed identifier rather than the placeholder-as-label convention.
                        .accessibilityIdentifier("composer")
                    Button {
                        commit()
                    } label: {
                        Image(systemName: editing != nil ? "checkmark" : "arrow.up").font(.system(size: 17, weight: .bold)).frame(width: 22, height: 22)
                    }
                    .buttonStyle(.glassProminent)
                    .buttonBorderShape(.circle)
                    .controlSize(.large)
                    .disabled(!canSend)
                    .accessibilityLabel(editing != nil ? "Save" : "Send")
                }
                .padding(.horizontal, Space.s3)
                .padding(.bottom, Space.s1)
            }
        }
    }

    private func placeholder(state: AppState, guild: ID?) -> String {
        if let dm = state.dm(channelID), let peer = state.dmPeer(dm) { return "Message @\(peer.username)" }
        if let guildChannel = state.channels.values.joined().first(where: { $0.id == channelID }) { return "Message #\(guildChannel.name)" }
        return "Message"
    }
}

struct ReconnectingBanner: View {
    @Environment(\.theme) private var theme
    let status: ConnectionStatus

    var body: some View {
        HStack(spacing: 6) {
            ProgressView().controlSize(.mini)
            Text(text).typeStyle(\.caption, color: \.textMuted)
        }
        .padding(.horizontal, Space.s4)
        .frame(minHeight: 30)
        .frame(maxWidth: .infinity)
        .background(theme.colors.surfaceRaised)
        .accessibilityElement(children: .combine)
    }

    private var text: String {
        switch status {
        case .connecting: "Connecting…"
        case .reconnecting(let s): "Reconnecting in \(s)s…"
        case .connected: ""
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

public struct DMsTabForTests: View {
    public init() {}
    public var body: some View { DMsTab() }
}

public struct MentionsTabForTests: View {
    public init() {}
    public var body: some View { MentionsTab() }
}
