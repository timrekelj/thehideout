import Foundation

/// Which screen the root view shows.
public enum SessionPhase: Sendable, Equatable {
    /// A saved session is being checked.
    case loading
    case signedOut
    case signedIn
    /// The terms must be accepted (the version to accept) before anything else.
    case termsRequired(version: String)
    /// The server refuses this app version.
    case updateRequired
    case suspended(reason: String, until: Date?)
}

public enum Fetch: Sendable, Equatable { case idle, initial, older }

/// One channel's loaded history, oldest first.
public struct ChannelLog: Sendable, Equatable {
    public var messages: [Message] = []
    public var oldestID: ID?
    public var hasMore = false
    public var loaded = false
    public var fetch: Fetch = .idle
    /// Ids of messages that start a new group whatever the author before them: the oldest message of each
    /// page that older pages were later loaded above (so a row never changes group when a page arrives).
    public var breaks: [ID] = []
    /// Loaded before the gateway dropped and not refetched since: the next visit asks for the newest page again.
    public var stale = false

    public init() {}

    public var newestID: ID? { messages.last?.id }

    /// Adds `m` in id order unless it is already here (the RPC reply and the gateway echo both bring a sent message).
    @discardableResult
    public mutating func insert(_ m: Message) -> Bool {
        let at = messages.partitioningIndex { $0.id >= m.id }
        if at < messages.count, messages[at].id == m.id { return false }
        messages.insert(m, at: at)
        oldestID = messages.first?.id
        return true
    }

    /// Merges a page. A message already here takes the page's reactions: the page is what the server says now,
    /// and events may have been missed.
    public mutating func insertPage(_ items: [Message]) {
        for m in items.sorted(by: { $0.id < $1.id }) {
            if let i = messages.firstIndex(where: { $0.id == m.id }) {
                messages[i].reactions = m.reactions
            } else {
                insert(m)
            }
        }
    }
}

extension Array {
    /// The first index whose element satisfies `pred`, for an array partitioned by it (binary search).
    func partitioningIndex(where pred: (Element) -> Bool) -> Int {
        var lo = 0, hi = count
        while lo < hi {
            let mid = (lo + hi) / 2
            if pred(self[mid]) { hi = mid } else { lo = mid + 1 }
        }
        return lo
    }
}

public enum Delivery: Sendable, Equatable { case pending, failed }

/// A message this client sent that the server hasn't confirmed yet (or refused).
public struct Outgoing: Sendable, Equatable, Identifiable {
    public var nonce: String
    public var channel: ID
    public var content: String
    public var at: Date
    public var state: Delivery
    public var attachmentIDs: [ID]
    /// Local previews of `attachmentIDs`, shown until the server's own message replaces this pending row.
    public var attachments: [PendingPreview]
    public var id: String { nonce }
    public init(nonce: String, channel: ID, content: String, at: Date, state: Delivery = .pending, attachmentIDs: [ID] = [], attachments: [PendingPreview] = []) {
        self.nonce = nonce; self.channel = channel; self.content = content; self.at = at
        self.state = state; self.attachmentIDs = attachmentIDs; self.attachments = attachments
    }
}

public struct Unread: Sendable, Equatable {
    public var count = 0
    public var mentions = 0
    public init(count: Int = 0, mentions: Int = 0) { self.count = count; self.mentions = mentions }
}

public enum Place: Sendable, Equatable { case home, guild(ID) }

/// An explicit route the shell should show now: the conversation `channel` (a DM), with a nonce so a
/// repeat is still a new request. Only `startDm` and the `StartDm` reply set it — the one navigation
/// that isn't a tap on a link. Gateway events never set it, so an incoming DM doesn't move the user.
public struct RouteRequest: Sendable, Equatable {
    public var channel: ID
    public var nonce: Int
    public init(channel: ID, nonce: Int) { self.channel = channel; self.nonce = nonce }
}

/// Where the "New" divider sits in the channel being read.
public enum Marker: Sendable, Equatable {
    case none
    /// Entered with `unread` new messages before the history arrived.
    case pending(channel: ID, unread: Int)
    /// The divider goes after message `id` (nil: before the first).
    case after(channel: ID, id: ID?)
}

public enum ConnectionStatus: Sendable, Equatable {
    case connecting
    case connected
    /// Retrying in this many seconds.
    case reconnecting(Int)
}

/// Whether the event stream has been whole since the last `connected`. The gateway doesn't replay what it
/// missed, and `ConnectionStatus` can't say: `connecting` arrives before every retry and overwrites `reconnecting`.
public enum StreamHealth: Sendable, Equatable { case live, interrupted }

/// One hit in `AppState.mentionFeed()`.
public struct MentionEntry: Sendable, Equatable, Identifiable {
    public var channelID: ID
    public var message: Message
    public var id: String { "\(channelID):\(message.id)" }
    public init(channelID: ID, message: Message) { self.channelID = channelID; self.message = message }
}

public struct Notice: Sendable, Equatable, Identifiable {
    public enum Kind: Sendable, Equatable { case info, success, danger }
    public var id: Int
    public var kind: Kind
    public var text: String
}

/// Work the reducer asks the store to do (desktop's `Command` outbox).
public enum Effect: Sendable, Equatable {
    case loadMessages(channel: ID, before: ID?)
    /// The lists again after a gateway gap.
    case refresh
    case loadChannels(guild: ID)
    case loadMembers(guild: ID)
    /// Roles (and the role assignments, which come with the members) of a guild.
    case loadAccess(guild: ID)
    case loadEmojis(guild: ID)
    case send(channel: ID, content: String, nonce: String, attachmentIDs: [ID])
    case updateLayout(guild: ID, layout: Layout)
    /// Replaces a message's content; `previous` is what to roll back to on a refusal.
    case editMessage(channel: ID, message: ID, content: String, previous: String)
    /// Opens (or asks the server to open) a DM with a user.
    case startDm(user: ID)
    /// Streams `fileURL`'s bytes as a pending attachment (ATT-09); the store gates concurrency and retries
    /// transient failures.
    case uploadAttachment(id: ID, channel: ID, filename: String, contentType: String, size: UInt64, fileURL: URL)
    /// The tray's X on a file still uploading: cancels its running task.
    case cancelUpload(id: ID)
}

/// The whole mirror of the server a signed-in session shows. Plain data and pure transitions (see
/// `Reducer.swift`): intents mutate it and append `Effect`s to `outbox`; replies and gateway events come back
/// through the `apply…` methods. Port of desktop's `AppState` + `apply.rs`.
public struct AppState: Sendable {
    public static let pageSize = 50
    public static let maxMessageChars = 4000
    public static let noticeQueue = 3

    // session
    public var phase: SessionPhase = .loading
    public var me: User?

    // data
    public var guilds: [Guild] = []
    public var channels: [ID: [Channel]] = [:]
    public var dms: [DMChannel] = []
    public var users: [ID: User] = [:]
    /// Member ids per guild, once loaded.
    public var members: [ID: [ID]] = [:]
    /// Guilds whose members were asked for and not yet refused, so one question is ever in flight.
    public var membersAsked: Set<ID> = []
    public var logs: [ID: ChannelLog] = [:]
    public var outgoing: [Outgoing] = []
    public var layouts: [ID: Layout] = [:]
    public var roles: [ID: [Role]] = [:]
    /// Role ids per member per guild.
    public var memberRoles: [ID: [ID: [ID]]] = [:]
    public var accessAsked: Set<ID> = []
    /// The layout as it was before an optimistic edit the server hasn't answered. While one is out, further
    /// structure edits wait (they would be based on a version the server is about to retire).
    public var layoutUndo: [ID: Layout] = [:]
    public var roleVersions: [ID: UInt64] = [:]
    public var emojis: [ID: [Emoji]] = [:]
    public var emojisAsked: Set<ID> = []
    /// Who is in voice, per guild per user (the server keeps one state per user).
    public var voice: [ID: [ID: VoiceState]] = [:]
    /// Guilds we are leaving or deleting ourselves, so their `guildRemoved` isn't announced as news.
    public var exiting: Set<ID> = []
    /// The push settings, once the server has sent them (`notification_settings_updated`; fetching them and editing come with push, phase 8).
    public var notificationSettings: NotificationSettings?

    // ui
    public var place: Place = .home
    /// The channel (or DM) whose transcript is open.
    public var openChannel: ID?
    /// The route the shell should follow now (DMs tab selected, that conversation pushed). See `RouteRequest`.
    public var routeRequest: RouteRequest?
    public var unread: [ID: Unread] = [:]
    public var marker: Marker = .none
    public var connection: ConnectionStatus = .connecting
    public var stream: StreamHealth = .live
    public var notices: [Notice] = []
    /// Where a moderator moved us in voice (the call screen joins it).
    public var voiceMovedTo: ID?
    /// Sections the user folded, `(guild, section)`.
    public var collapsed: Set<SectionKey> = []
    /// The composer's unsent text per channel, kept while switching channels (COMP-03).
    public var drafts: [ID: String] = [:]
    /// Each channel's attached files, from picked to sent or removed (ATT-09).
    public var attachDrafts: [ID: AttachDraft] = [:]
    /// The server's per-upload cap (ATT-09); a file over this is refused locally before it ever uploads.
    public var maxUploadBytes: UInt64 = defaultMaxUploadBytes
    /// Folded embeds (ATT-07), bounded and oldest-first like desktop's `collapsed_embeds`.
    public var collapsedEmbeds: [EmbedRef] = []

    public var outbox: [Effect] = []
    var nonceCounter = 0
    var noticeCounter = 0
    var routeCounter = 0
    var uploadCounter: ID = 0

    public struct SectionKey: Sendable, Hashable {
        public var guild: ID
        public var section: ID
        public init(guild: ID, section: ID) { self.guild = guild; self.section = section }
    }

    public init() {}

    // MARK: queries

    public var myID: ID { me?.id ?? 0 }

    public func guild(_ id: ID) -> Guild? { guilds.first { $0.id == id } }
    public func channels(of guild: ID) -> [Channel] { channels[guild] ?? [] }
    public func channel(_ id: ID, in guild: ID) -> Channel? { channels(of: guild).first { $0.id == id } }
    public func dm(_ id: ID) -> DMChannel? { dms.first { $0.id == id } }

    public func userName(_ id: ID) -> String { users[id]?.username ?? "unknown" }

    /// The other participant of a DM.
    public func dmPeer(_ dm: DMChannel) -> User? {
        dm.participants.first { $0.id != myID } ?? dm.participants.first
    }

    /// A guild's channels in sidebar order: the layout's, then any the layout doesn't list yet.
    public func orderedChannels(_ guild: ID) -> [Channel] {
        let all = channels(of: guild)
        guard let l = layouts[guild] else { return all }
        let listed = l.channelOrder.compactMap { id in all.first { $0.id == id } }
        let ids = Set(listed.map(\.id))
        return listed + all.filter { !ids.contains($0.id) }
    }

    /// What a server opens on (voice channels are joined, not opened).
    public func firstTextChannel(_ guild: ID) -> ID? {
        orderedChannels(guild).first { $0.kind == .text }?.id
    }

    /// What the signed-in user may do in `guild` (nothing until its roles have arrived, unless they own it).
    public func perms(_ guild: ID) -> Permissions {
        guard let g = self.guild(guild) else { return [] }
        return Permissions.effective(guild: g, me: myID, myRoles: memberRoles[guild]?[myID] ?? [], roles: roles[guild] ?? [])
    }

    public func canManageChannels(_ guild: ID) -> Bool { perms(guild).allows(.manageChannels) }

    public func sectionOpen(guild: ID, section: ID) -> Bool { !collapsed.contains(SectionKey(guild: guild, section: section)) }

    public func log(_ channel: ID) -> ChannelLog? { logs[channel] }
    public func unread(_ channel: ID) -> Unread { unread[channel] ?? Unread() }

    public func guildUnread(_ guild: ID) -> Unread {
        channels(of: guild).reduce(Unread()) { a, c in
            let u = unread(c.id)
            return Unread(count: a.count + u.count, mentions: a.mentions + u.mentions)
        }
    }

    public var homeUnread: Unread {
        dms.reduce(Unread()) { a, d in
            let u = unread(d.id)
            return Unread(count: a.count + u.count, mentions: a.mentions + u.mentions)
        }
    }

    /// Everyone the user could start a DM with: known users but themself.
    public var knownUsers: [User] {
        users.values.filter { $0.id != myID }.sorted { $0.username.lowercased() < $1.username.lowercased() }
    }

    public func mentionsMe(_ content: String) -> Bool {
        me.map { Mentions.contains(content, username: $0.username) } ?? false
    }

    public func draft(_ channel: ID) -> String { drafts[channel] ?? "" }

    public mutating func setDraft(channel: ID, text: String) {
        if text.isEmpty { drafts.removeValue(forKey: channel) } else { drafts[channel] = text }
    }

    public func attachDraft(_ channel: ID) -> AttachDraft { attachDrafts[channel] ?? AttachDraft() }

    public func isEmbedCollapsed(_ ref: EmbedRef) -> Bool { collapsedEmbeds.contains(ref) }

    /// Loaded messages that mention the signed-in user, newest first, across every channel whose history is
    /// in memory (a local feed: the server has no mentions index, MOB follow-up).
    public func mentionFeed() -> [MentionEntry] {
        guard let name = me?.username else { return [] }
        var out: [MentionEntry] = []
        for (channel, log) in logs {
            for m in log.messages where m.authorID != myID && Mentions.contains(m.content, username: name) {
                out.append(MentionEntry(channelID: channel, message: m))
            }
        }
        return out.sorted { $0.message.createdAt > $1.message.createdAt }
    }

    /// The 1:1 DM with `user` (never myself, never a group), if one is already known (StartDm matches only
    /// a 1:1 conversation: a group DM with the same person present is not a match).
    public func oneToOneDm(with user: ID) -> DMChannel? {
        guard user != myID else { return nil }
        return dms.first { dm in
            dm.participants.count == 2 && dm.participants.contains { $0.id == user } && dm.participants.contains { $0.id == myID }
        }
    }

    /// The voice state of `user`, wherever they are.
    public func voiceState(of user: ID) -> VoiceState? {
        for users in voice.values { if let s = users[user] { return s } }
        return nil
    }

    /// The voice participants of a channel, longest in the channel first.
    public func participants(of channel: ID, in guild: ID) -> [VoiceState] {
        (voice[guild] ?? [:]).values.filter { $0.channelID == channel }.sorted { ($0.joinedAt, $0.userID) < ($1.joinedAt, $1.userID) }
    }

    /// Outgoing messages of a channel, oldest first.
    public func outgoing(in channel: ID) -> [Outgoing] { outgoing.filter { $0.channel == channel } }

    /// The stable id of a message row: a sent message keeps the id of its pending self (its nonce) so the row
    /// doesn't remount when the server confirms it.
    public func rowID(_ m: Message) -> String {
        m.authorID == myID && !m.nonce.isEmpty ? "n:\(m.nonce)" : "m:\(m.id)"
    }
}

/// `@username` detection (case-insensitive), ignoring code spans and fenced blocks.
public enum Mentions {
    public static func contains(_ content: String, username: String) -> Bool {
        guard !username.isEmpty else { return false }
        let text = stripCode(content).lowercased()
        let needle = "@" + username.lowercased()
        var search = text.startIndex
        while let r = text.range(of: needle, range: search..<text.endIndex) {
            let after = r.upperBound < text.endIndex ? text[r.upperBound] : nil
            let before = r.lowerBound > text.startIndex ? text[text.index(before: r.lowerBound)] : nil
            let boundaryAfter = after.map { !($0.isLetter || $0.isNumber || $0 == "_" || $0 == "-") } ?? true
            let boundaryBefore = before.map { !($0.isLetter || $0.isNumber) } ?? true
            if boundaryAfter && boundaryBefore { return true }
            search = r.upperBound
        }
        return false
    }

    static func stripCode(_ s: String) -> String {
        var out = ""
        var i = s.startIndex
        var fenced = false, inline = false
        while i < s.endIndex {
            if s[i...].hasPrefix("```") { fenced.toggle(); i = s.index(i, offsetBy: 3); continue }
            if !fenced, s[i] == "`" { inline.toggle(); i = s.index(after: i); continue }
            if !fenced && !inline { out.append(s[i]) } else if s[i] == "\n" { inline = false }
            i = s.index(after: i)
        }
        return out
    }
}

/// "wrong code" -> "Wrong code."
public func sentence(_ m: String) -> String {
    let t = m.trimmingCharacters(in: .whitespacesAndNewlines)
    guard let first = t.first else { return "" }
    var s = first.uppercased() + t.dropFirst()
    if let last = s.last, !".!?".contains(last) { s.append(".") }
    return s
}
