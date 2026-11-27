import Foundation

/// An in-memory `ApiClient` + gateway seeded like desktop's `fake.rs`: users ana/bruno/cvetka/tim, three servers
/// (The Hideout with sections and channels, Garage Band, Cooking Club), DMs and voice channels with people in them.
/// Used by previews, snapshot tests, the gallery, unit tests and the `--fake` launch argument.
public final class FakeBackend: ApiClient, @unchecked Sendable {
    // Ids shared with desktop's fake world.
    public enum Ids {
        public static let hideout: ID = 1, garageBand: ID = 2, cookingClub: ID = 3
        public static let general: ID = 10, dev: ID = 11, random: ID = 12, plans: ID = 13, announcements: ID = 14
        public static let design: ID = 15, music: ID = 16, gaming: ID = 17, memes: ID = 18, books: ID = 19
        public static let lounge: ID = 22, gameNight: ID = 23
        public static let building: ID = 101, hangout: ID = 102, archive: ID = 103, voiceSection: ID = 104
        public static let tim: ID = 1, ana: ID = 2, bruno: ID = 3, cvetka: ID = 4
        public static let dmAna: ID = 50, dmBruno: ID = 51
        public static let hideoutAdmins: ID = 1002
        /// The Hideout's custom emoji (MSG-09): two static, one animated.
        public static let emojiWave: ID = 9001, emojiThumbsUp: ID = 9002, emojiPartyParrot: ID = 9003
    }

    private let lock = NSLock()
    /// The sign-in side of the fake world (see `FakeAuth.swift`).
    public let accounts = FakeAccounts()
    private var world: World
    private var subscribers: [UUID: AsyncThrowingStream<GatewayEvent, Error>.Continuation] = [:]
    private var subscribeCount = 0

    /// Seconds each call waits before it answers.
    public var latency: TimeInterval {
        get { lock.withLock { world.latency } }
        set { lock.withLock { world.latency = newValue } }
    }

    /// Fails the next `n` sends with a network error (for the Failed + Retry flow).
    public func failNextSends(_ n: Int) { lock.withLock { world.failSends = n } }

    /// The next `n` uploads fail with a network error, after their simulated progress (for the failed
    /// chip + Try again flow).
    public func failNextUploads(_ n: Int) { lock.withLock { world.failUploads = n } }

    /// The fake's stand-in for the server's per-upload cap (ATT-09); a file over this throws `.tooLarge` at once.
    public var maxUploadBytes: UInt64 {
        get { lock.withLock { world.maxUploadBytes } }
        set { lock.withLock { world.maxUploadBytes = newValue } }
    }

    /// How many `progress` calls a simulated upload makes on its way to 100%.
    public var uploadTicks: Int {
        get { lock.withLock { world.uploadTicks } }
        set { lock.withLock { world.uploadTicks = newValue } }
    }

    /// The next `updateLayout` answers `conflict`.
    public func conflictNextLayout() { lock.withLock { world.conflictLayout = true } }

    /// The next `listMessages` fails with a network error.
    public func failNextListMessages(_ n: Int = 1) { lock.withLock { world.failList = n } }

    /// Signed in as this user (default tim). cvetka is the site admin.
    public init(me: ID = Ids.tim, now: Date = Date()) {
        world = World(me: me, now: now)
        accounts.now = now
    }

    // MARK: world access for tests and scenes

    /// A snapshot of a channel's stored messages, oldest first.
    public func storedMessages(_ channel: ID) -> [Message] { lock.withLock { world.messages[channel] ?? [] } }

    /// Someone else posts a message: stored, then published to every open stream.
    @discardableResult
    public func incoming(channel: ID, author: ID, content: String, at: Date? = nil) -> Message {
        let m = lock.withLock { world.push(channel: channel, author: author, at: at ?? Date(), content: content) }
        emit(.messageCreated(m))
        return m
    }

    /// Publishes any event to every open stream.
    public func emit(_ event: GatewayEvent) {
        let subs = lock.withLock { Array(subscribers.values) }
        for s in subs { s.yield(event) }
    }

    /// Breaks every open stream with `error` (a network drop); a client reconnects with a fresh `subscribe`.
    public func dropStreams(_ error: AppError = .network("connection lost")) {
        let subs = lock.withLock { () -> [AsyncThrowingStream<GatewayEvent, Error>.Continuation] in
            let s = Array(subscribers.values); subscribers.removeAll(); return s
        }
        for s in subs { s.finish(throwing: error) }
    }

    public var openStreams: Int { lock.withLock { subscribers.count } }

    /// How many times `subscribe()` has opened a stream, ever (a recording fake: tests check this against
    /// `openStreams` to tell "reconnected" from "started a redundant second stream").
    public var totalSubscribes: Int { lock.withLock { subscribeCount } }

    func delay() async throws {
        let l = latency
        if l > 0 { try await Task.sleep(for: .seconds(l)) }
    }

    // MARK: ApiClient

    public func me() async throws -> Me {
        try await delay()
        if accounts.suspendMe { throw AppError.suspended(reason: "Repeated spam in public channels.", until: accounts.now.addingTimeInterval(7 * 86_400)) }
        if accounts.tooOld { throw AppError.clientTooOld }
        let terms = accounts.termsRequired
        return lock.withLock { Me(user: world.user(world.meID), termsUpdateRequired: terms, termsVersion: terms ?? "2026-01-01") }
    }

    public func snapshot() async throws -> Snapshot {
        try await delay()
        return lock.withLock { world.snapshot() }
    }

    public func listChannels(guild: ID) async throws -> (channels: [Channel], layout: Layout) {
        try await delay()
        return try lock.withLock {
            guard world.guilds.contains(where: { $0.id == guild }) else { throw AppError.notFound }
            return (world.channels.filter { $0.guildID == guild }, world.layouts[guild] ?? Layout())
        }
    }

    public func listMembers(guild: ID) async throws -> [Member] {
        try await delay()
        return lock.withLock {
            (world.members[guild] ?? []).map { Member(user: world.user($0), roleIDs: world.memberRoles[MemberKey(guild: guild, user: $0)] ?? [], joinedAt: world.created) }
        }
    }

    public func listRoles(guild: ID) async throws -> (roles: [Role], version: UInt64) {
        try await delay()
        return lock.withLock { (world.roles[guild] ?? [], 1) }
    }

    public func listEmojis(guild: ID) async throws -> [Emoji] {
        try await delay()
        return lock.withLock { world.emojis[guild] ?? [] }
    }

    public func listMessages(channel: ID, before: ID?, limit: Int) async throws -> MessagePage {
        try await delay()
        return try lock.withLock {
            if world.failList > 0 { world.failList -= 1; throw AppError.network("offline") }
            let all = world.messages[channel] ?? []
            let older = before.map { b in all.filter { $0.id < b } } ?? all
            let page = Array(older.suffix(limit))
            let more = older.count > page.count
            return MessagePage(messages: page.reversed(), nextBefore: more ? page.first?.id : nil, hasMore: more)
        }
    }

    public func sendMessage(channel: ID, content: String, nonce: String, attachmentIDs: [ID]) async throws -> Message {
        try await delay()
        let m: Message = try lock.withLock {
            if world.failSends > 0 { world.failSends -= 1; throw AppError.network("connection lost") }
            guard content.count <= AppState.maxMessageChars else { throw AppError.invalidArgument("message too long") }
            let attachments = attachmentIDs.compactMap { id in world.pendingUploads.removeValue(forKey: id).map(World.fakeAttachment) }
            var m = world.push(channel: channel, author: world.meID, at: Date(), content: content, attachments: attachments)
            world.setNonce(channel: channel, id: m.id, nonce: nonce)
            m.nonce = nonce
            return m
        }
        // The gateway echo follows the reply, like the server's.
        emit(.messageCreated(m))
        return m
    }

    /// Simulates a streamed upload: reads `fileURL` whole (the fake has no real network to stream
    /// over), reports `uploadTicks` progress steps paced by `latency`, then either fails
    /// (`failUploads`) or answers a `PendingAttachment` good for `sendMessage` within the hour.
    public func uploadAttachment(channel: ID, filename: String, contentType: String, size: UInt64, fileURL: URL, progress: @escaping @Sendable (UInt64) -> Void) async throws -> PendingAttachment {
        if size > maxUploadBytes { throw AppError.tooLarge }
        let bytes = (try? Data(contentsOf: fileURL))?.count ?? Int(size)
        let ticks = max(1, uploadTicks)
        let l = latency
        for t in 1...ticks {
            if l > 0 { try await Task.sleep(for: .seconds(l / Double(ticks))) }
            progress(UInt64(bytes) * UInt64(t) / UInt64(ticks))
        }
        return try lock.withLock {
            if world.failUploads > 0 { world.failUploads -= 1; throw AppError.network("connection reset") }
            world.nextPendingID += 1
            let pending = PendingAttachment(id: world.nextPendingID, filename: filename, contentType: contentType.isEmpty ? "application/octet-stream" : contentType, size: UInt64(bytes), expiresAt: Date().addingTimeInterval(3600))
            world.pendingUploads[pending.id] = pending
            return pending
        }
    }

    public func editMessage(channel: ID, message: ID, content: String) async throws -> Message {
        try await delay()
        let m: Message = try lock.withLock {
            guard let i = world.messages[channel]?.firstIndex(where: { $0.id == message }) else { throw AppError.notFound }
            world.messages[channel]![i].content = content
            world.messages[channel]![i].editedAt = Date()
            return world.messages[channel]![i]
        }
        emit(.messageUpdated(m))
        return m
    }

    public func deleteMessage(channel: ID, message: ID) async throws {
        try await delay()
        try lock.withLock {
            guard world.messages[channel]?.contains(where: { $0.id == message }) == true else { throw AppError.notFound }
            world.messages[channel]!.removeAll { $0.id == message }
        }
        emit(.messageDeleted(channel: channel, message: message))
    }

    public func addReaction(channel: ID, message: ID, emoji: ReactionEmoji) async throws {
        try await delay()
        let me = lock.withLock { world.meID }
        lock.withLock { world.react(channel: channel, message: message, emoji: emoji, user: me, add: true) }
        emit(.reactionAdded(channel: channel, message: message, emoji: emoji, user: me))
    }

    public func removeReaction(channel: ID, message: ID, emoji: ReactionEmoji) async throws {
        try await delay()
        let me = lock.withLock { world.meID }
        lock.withLock { world.react(channel: channel, message: message, emoji: emoji, user: me, add: false) }
        emit(.reactionRemoved(channel: channel, message: message, emoji: emoji, user: me))
    }

    public func startDm(user: ID) async throws -> DMChannel {
        try await delay()
        let dm: DMChannel = try lock.withLock {
            guard user != world.meID, world.users.contains(where: { $0.id == user }) else { throw AppError.invalidArgument("no such user") }
            if let existing = world.dms.first(where: { $0.participants.count == 2 && $0.participants.contains { $0.id == user } }) {
                return existing
            }
            world.nextID += 1
            let dm = DMChannel(id: world.nextID, participants: [world.user(world.meID), world.user(user)], createdAt: Date())
            world.dms.append(dm)
            return dm
        }
        emit(.dmOpened(dm))
        return dm
    }

    public func updateLayout(guild: ID, expectedVersion: UInt64, sectionIDs: [ID], entries: [LayoutEntry]) async throws -> Layout {
        try await delay()
        let l: Layout = try lock.withLock {
            if world.conflictLayout { world.conflictLayout = false; throw AppError.conflict }
            guard var l = world.layouts[guild] else { throw AppError.notFound }
            guard l.version == expectedVersion else { throw AppError.conflict }
            l.sections = sectionIDs.compactMap { id in l.sections.first { $0.id == id } }
            l.entries = entries
            l.version += 1
            world.layouts[guild] = l
            return l
        }
        emit(.layoutUpdated(guild: guild, layout: l))
        return l
    }

    // MARK: GatewaySource

    public func subscribe() -> AsyncThrowingStream<GatewayEvent, Error> {
        AsyncThrowingStream { continuation in
            let id = UUID()
            let snapshot = lock.withLock { () -> GatewayEvent in
                subscribers[id] = continuation
                subscribeCount += 1
                return .voiceSnapshot(guilds: world.guilds.map(\.id), states: world.voiceStates)
            }
            // Headers + first events, as the server opens a stream.
            continuation.yield(.heartbeat)
            continuation.yield(snapshot)
            continuation.onTermination = { [weak self] _ in
                guard let self else { return }
                _ = self.lock.withLock { self.subscribers.removeValue(forKey: id) }
            }
        }
    }
}

// MARK: - the world

private struct MemberKey: Hashable { var guild: ID; var user: ID }

private struct World {
    typealias Ids = FakeBackend.Ids

    var meID: ID
    var users: [User]
    var guilds: [Guild]
    var channels: [Channel]
    var dms: [DMChannel]
    var members: [ID: [ID]]
    var messages: [ID: [Message]] = [:]
    var layouts: [ID: Layout] = [:]
    var roles: [ID: [Role]] = [:]
    var memberRoles: [MemberKey: [ID]] = [:]
    var voiceStates: [VoiceState] = []
    var emojis: [ID: [Emoji]] = [:]
    var nextID: ID = 10_000
    var latency: TimeInterval = 0
    var failSends = 0
    var failList = 0
    var failUploads = 0
    var maxUploadBytes: UInt64 = defaultMaxUploadBytes
    var uploadTicks = 4
    var pendingUploads: [ID: PendingAttachment] = [:]
    var nextPendingID: ID = 90_000
    var conflictLayout = false
    let created: Date

    /// A generated `Attachment` for a message that linked `pending`: a bundled asset standing in for the
    /// real upload, by its kind (the fake has no real media pipeline to re-encode it through).
    static func fakeAttachment(_ pending: PendingAttachment) -> Attachment {
        let kind = fileKind(contentType: pending.contentType, filename: pending.filename)
        let (asset, width, height, thumbnail): (String, Int?, Int?, String?)
        switch kind {
        case .image: (asset, width, height, thumbnail) = ("square-1.jpg", 1000, 1000, nil)
        case .video: (asset, width, height, thumbnail) = ("clip-1.mp4", 960, 540, "clip-1-poster.jpg")
        case .audio: (asset, width, height, thumbnail) = ("tone-1.m4a", nil, nil, nil)
        default: (asset, width, height, thumbnail) = ("notes-1.txt", nil, nil, nil)
        }
        return Attachment(
            id: pending.id, filename: pending.filename, contentType: pending.contentType, size: pending.size,
            file: fakeAssetPrefix + asset, width: width, height: height, thumbnail: thumbnail.map { fakeAssetPrefix + $0 }
        )
    }

    func user(_ id: ID) -> User { users.first { $0.id == id } ?? User(id: id, username: "user\(id)") }

    func snapshot() -> Snapshot {
        Snapshot(me: user(meID), guilds: guilds, channels: channels, layouts: layouts, dms: dms)
    }

    mutating func push(channel: ID, author: ID, at: Date, content: String, attachments: [Attachment] = [], embeds: [Embed] = []) -> Message {
        nextID += 1
        let guild = channels.first { $0.id == channel }?.guildID
        let m = Message(id: nextID, channelID: channel, guildID: guild, authorID: author, content: content, createdAt: at, attachments: attachments, embeds: embeds)
        messages[channel, default: []].append(m)
        return m
    }

    mutating func setNonce(channel: ID, id: ID, nonce: String) {
        if let i = messages[channel]?.firstIndex(where: { $0.id == id }) { messages[channel]![i].nonce = nonce }
    }

    mutating func react(channel: ID, message: ID, emoji: ReactionEmoji, user: ID, add: Bool) {
        guard let i = messages[channel]?.firstIndex(where: { $0.id == message }) else { return }
        if user == meID { _ = AppState.setMine(&messages[channel]![i], emoji, add) }
        else { AppState.setOther(&messages[channel]![i], emoji, add) }
    }

    init(me: ID, now: Date) {
        meID = me
        let day: TimeInterval = 86_400
        created = now.addingTimeInterval(-300 * day)
        func user(_ id: ID, _ name: String) -> User {
            User(id: id, username: name, createdAt: now.addingTimeInterval(-300 * day), isAdmin: id == Ids.cvetka, email: id == Ids.bruno ? nil : "\(name)@example.com")
        }
        users = [user(Ids.tim, "tim"), user(Ids.ana, "ana"), user(Ids.bruno, "bruno"), user(Ids.cvetka, "cvetka")]
        func guild(_ id: ID, _ name: String, owner: ID, _ code: String) -> Guild {
            Guild(id: id, name: name, ownerID: owner, inviteCode: code, createdAt: now.addingTimeInterval(-300 * day))
        }
        guilds = [guild(1, "The Hideout", owner: Ids.tim, "HIDEOUT1"), guild(2, "Garage Band", owner: Ids.ana, "GARAGE22"), guild(3, "Cooking Club", owner: Ids.bruno, "COOK333")]
        func ch(_ id: ID, _ guild: ID, _ pos: Int, _ name: String, icon: String? = nil, color: String = "", topic: String = "", kind: ChannelKind = .text, section: ID? = nil) -> Channel {
            Channel(id: id, guildID: guild, name: name, createdAt: now.addingTimeInterval(-290 * day), sectionID: section, position: pos,
                    icon: icon.map { .phosphor($0) } ?? .default, color: color, topic: topic, kind: kind)
        }
        channels = [
            ch(Ids.general, 1, 0, "general", icon: "chat-circle-dots", color: "teal", topic: "Say hi, share links, plan the weekend"),
            ch(Ids.dev, 1, 1, "dev", icon: "code", color: "blue", topic: "Builds, reviews and the odd rant about lifetimes"),
            ch(Ids.random, 1, 2, "random", icon: "dice-five", color: "amber", section: Ids.hangout),
            ch(Ids.plans, 1, 3, "plans", icon: "calendar-check", color: "green", section: Ids.building),
            ch(Ids.announcements, 1, 4, "announcements", icon: "megaphone", color: "orange", topic: "Read-only news from the mods"),
            ch(Ids.design, 1, 5, "design", icon: "paint-brush", color: "pink", section: Ids.building),
            ch(Ids.music, 1, 6, "music", icon: "music-notes", color: "indigo", section: Ids.hangout),
            ch(Ids.gaming, 1, 7, "gaming", icon: "game-controller", color: "red", section: Ids.hangout),
            ch(Ids.memes, 1, 8, "memes", icon: "smiley", color: "yellow", section: Ids.hangout),
            ch(Ids.books, 1, 9, "books", icon: "books", color: "slate", section: Ids.archive),
            ch(Ids.lounge, 1, 0, "lounge", icon: "speaker-high", kind: .voice, section: Ids.voiceSection),
            ch(Ids.gameNight, 1, 1, "game-night", icon: "game-controller", kind: .voice, section: Ids.voiceSection),
            ch(20, 2, 0, "general"), ch(21, 2, 1, "gigs"),
            ch(30, 3, 0, "recipes"), ch(31, 3, 1, "shopping"),
        ]
        let dmDate = now.addingTimeInterval(-60 * day)
        dms = [
            DMChannel(id: Ids.dmAna, participants: [users[0], users[1]], createdAt: dmDate),
            DMChannel(id: Ids.dmBruno, participants: [users[0], users[2]], createdAt: dmDate),
        ]
        members = [1: [Ids.tim, Ids.ana, Ids.bruno, Ids.cvetka], 2: [Ids.tim, Ids.ana, Ids.cvetka], 3: [Ids.tim, Ids.bruno]]
        func section(_ id: ID, _ name: String, _ pos: Int) -> Section { Section(id: id, guildID: 1, name: name, position: pos) }
        layouts[1] = Layout(version: 3, sections: [section(Ids.building, "Building", 0), section(Ids.hangout, "Hangout", 1), section(Ids.archive, "Archive", 2), section(Ids.voiceSection, "Voice", 3)], entries: [
            LayoutEntry(sectionID: nil, channelIDs: [Ids.general, Ids.dev, Ids.announcements]),
            LayoutEntry(sectionID: Ids.building, channelIDs: [Ids.design, Ids.plans]),
            LayoutEntry(sectionID: Ids.hangout, channelIDs: [Ids.random, Ids.music, Ids.gaming, Ids.memes]),
            LayoutEntry(sectionID: Ids.archive, channelIDs: [Ids.books]),
            LayoutEntry(sectionID: Ids.voiceSection, channelIDs: [Ids.lounge, Ids.gameNight]),
        ])
        layouts[2] = Layout(version: 1, entries: [LayoutEntry(sectionID: nil, channelIDs: [20, 21])])
        layouts[3] = Layout(version: 1, entries: [LayoutEntry(sectionID: nil, channelIDs: [30, 31])])
        func role(_ id: ID, _ guild: ID, _ name: String, _ color: String, _ pos: Int, _ perms: Permissions) -> Role {
            Role(id: id, guildID: guild, name: name, color: color, position: pos, permissions: perms, createdAt: created)
        }
        roles[1] = [role(Ids.hideoutAdmins, 1, "Admin", "red", 3, .administrator), role(1001, 1, "Moderator", "teal", 2, .manageChannels),
                    role(1003, 1, "Designer", "pink", 1, []), role(1004, 1, "Regular", "", 0, [])]
        roles[2] = [role(2003, 2, "Drummer", "orange", 3, []), role(2001, 2, "Roadie", "blue", 2, [.manageRoles, .manageEmojis]),
                    role(2002, 2, "Guest", "slate", 1, []), role(2004, 2, "Fan", "", 0, [])]
        roles[3] = [role(3001, 3, "Moderator", "teal", 0, .manageChannels)]
        memberRoles = [MemberKey(guild: 1, user: Ids.bruno): [1001], MemberKey(guild: 1, user: Ids.ana): [1003], MemberKey(guild: 1, user: Ids.cvetka): [Ids.hideoutAdmins],
                       MemberKey(guild: 2, user: Ids.tim): [2001], MemberKey(guild: 2, user: Ids.cvetka): [2003], MemberKey(guild: 3, user: Ids.tim): [3001]]
        voiceStates = [
            VoiceState(guildID: 1, channelID: Ids.lounge, userID: Ids.ana, joinedAt: now.addingTimeInterval(-1200)),
            VoiceState(guildID: 1, channelID: Ids.lounge, userID: Ids.bruno, selfMute: true, joinedAt: now.addingTimeInterval(-600)),
        ]
        emojis[1] = [
            Emoji(id: Ids.emojiWave, guildID: 1, name: "wave", animated: false, image: fakeAssetPrefix + "wave.webp", creatorID: Ids.tim, createdAt: now.addingTimeInterval(-60 * day)),
            Emoji(id: Ids.emojiThumbsUp, guildID: 1, name: "thumbs_up", animated: false, image: fakeAssetPrefix + "thumbs_up.webp", creatorID: Ids.ana, createdAt: now.addingTimeInterval(-55 * day)),
            Emoji(id: Ids.emojiPartyParrot, guildID: 1, name: "party_parrot", animated: true, image: fakeAssetPrefix + "party_parrot.gif", creatorID: Ids.cvetka, createdAt: now.addingTimeInterval(-40 * day)),
        ]
        seedMessages(now: now)
    }

    private mutating func seedMessages(now: Date) {
        // #general: 150 messages over four days, authors in runs so groups form, the newest few with markup.
        let who: [ID] = [Ids.ana, Ids.ana, Ids.bruno, Ids.tim, Ids.tim, Ids.tim, Ids.cvetka, Ids.ana, Ids.bruno, Ids.bruno, Ids.tim, Ids.cvetka]
        var rng = SeededRandom(seed: 7)
        let total = 150
        let authors = (0..<total).map { who[$0 % who.count] }
        let gaps: [TimeInterval] = (0..<total).map { i in
            if i % 17 == 0 { return 5400 }
            if i > 0 && authors[i] == authors[i - 1] { return TimeInterval(20 + rng.next(130)) }
            return TimeInterval(70 + rng.next(800))
        }
        // Lay the newest day out backwards so it ends just before "now"; the older days sit at fixed offsets.
        func day(_ i: Int) -> Int { i < 40 ? -3 : i < 82 ? -2 : i < 120 ? -1 : 0 }
        var t = now
        var times = [Date](repeating: now, count: total)
        for i in stride(from: total - 1, through: 0, by: -1) {
            if i == total - 1 { t = now.addingTimeInterval(-18 * 60) }
            else { t = t.addingTimeInterval(-gaps[i + 1]) }
            if i + 1 < total, day(i) != day(i + 1) {
                t = now.addingTimeInterval(TimeInterval(day(i)) * 86_400 - 9 * 3600 + TimeInterval(i % 7) * 180)
            }
            times[i] = t
        }
        for i in 0..<total {
            _ = push(channel: Ids.general, author: authors[i], at: times[i], content: Self.generalLine(i, total: total))
        }
        // A few reactions on the newest messages.
        if let last = messages[Ids.general]?.indices.suffix(4) {
            let idx = Array(last)
            messages[Ids.general]![idx[0]].reactions = [Reaction(emoji: .unicode("👍"), count: 3, me: true), Reaction(emoji: .unicode("🎉"), count: 1, me: false)]
            messages[Ids.general]![idx[1]].reactions = [Reaction(emoji: .unicode("😂"), count: 2, me: false)]
        }
        for (i, line) in Self.devLines.enumerated() {
            _ = push(channel: Ids.dev, author: [Ids.bruno, Ids.tim, Ids.ana][i % 3], at: now.addingTimeInterval(-4 * 86_400 + TimeInterval(i) * 400), content: line)
        }
        _ = push(channel: Ids.random, author: Ids.cvetka, at: now.addingTimeInterval(-5 * 86_400), content: "found this at the flea market, 5 euros")
        for (i, (who, line)) in Self.dmAnaLines.enumerated() {
            _ = push(channel: Ids.dmAna, author: who, at: now.addingTimeInterval(-3 * 3600 + TimeInterval(i) * 90), content: line)
        }
        for (i, (who, line)) in Self.dmBrunoLines.enumerated() {
            _ = push(channel: Ids.dmBruno, author: who, at: now.addingTimeInterval(-26 * 3600 + TimeInterval(i) * 120), content: line)
        }
        _ = push(channel: 20, author: Ids.ana, at: now.addingTimeInterval(-6 * 86_400), content: "rehearsal moved to Thursday")
        _ = push(channel: 30, author: Ids.bruno, at: now.addingTimeInterval(-7 * 86_400), content: "sourdough starter is alive")
        // Oldest seeds first: the transcript orders by message id, and the attachment UI tests scroll
        // to the attachment rows, so those must stay the newest messages in #design.
        seedEmbeds(now: now)
        seedEmoji(now: now)
        seedAttachments(now: now)
    }

    /// Custom emoji inline in a sentence, and a jumbo-only message (MD-06/07), into #design. Seeded
    /// older than the attachment messages so they stay before them (see `seedAttachments`'s comment).
    private mutating func seedEmoji(now: Date) {
        var t = now.addingTimeInterval(-4 * 86_400 - 2 * 3600)
        _ = push(channel: Ids.design, author: Ids.bruno, at: t, content: "nice work team <:thumbs_up:\(Ids.emojiThumbsUp)> see you at standup <:wave:\(Ids.emojiWave)>")
        t = t.addingTimeInterval(300)
        _ = push(channel: Ids.design, author: Ids.ana, at: t, content: "<a:party_parrot:\(Ids.emojiPartyParrot)> <a:party_parrot:\(Ids.emojiPartyParrot)> <a:party_parrot:\(Ids.emojiPartyParrot)>")
    }

    /// A link, image and video embed, then one rich post per provider (X, TikTok, Instagram, Reddit) —
    /// desktop's `fake_files.rs::seed_embeds` (ATT-06), into #design. `message_updated` is simulated by
    /// attaching the embed directly rather than delaying it (the gateway path is exercised by the
    /// reducer and UI tests, not the fake's timing). Seeded older than the attachment messages so those
    /// stay the newest in #design (see `seedEmoji`).
    private mutating func seedEmbeds(now: Date) {
        var t = now.addingTimeInterval(-4 * 86_400 - 6 * 3600)
        _ = push(channel: Ids.design, author: Ids.tim, at: t, content: "the release notes are up", embeds: [
            Embed(url: "https://example.com/wiki/release", title: "Release notes: v0.0.3",
                  description: "Adds a live gateway, drops polling in favor of push updates, and tightens the reconnect backoff so flaky networks recover faster without hammering the server.",
                  siteName: "example wiki", thumbnail: fakeAssetPrefix + "landscape-3.jpg", thumbnailWidth: 1200, thumbnailHeight: 800, kind: .link),
        ])
        t = t.addingTimeInterval(240)
        _ = push(channel: Ids.design, author: Ids.ana, at: t, content: "found the old mockup", embeds: [
            Embed(url: "https://example.com/gallery/mockup", title: "Early dashboard mockup", description: "Before the warm palette landed.",
                  siteName: "example gallery", thumbnail: fakeAssetPrefix + "square-1.jpg", thumbnailWidth: 1000, thumbnailHeight: 1000, kind: .image),
        ])
        t = t.addingTimeInterval(240)
        _ = push(channel: Ids.design, author: Ids.bruno, at: t, content: "clip from the demo", embeds: [
            Embed(url: "https://example.com/clips/demo", title: "Screen recording of the bug", siteName: "example clips", kind: .video,
                  media: [EmbedMedia(kind: .video, file: fakeAssetPrefix + "clip-1.mp4", poster: fakeAssetPrefix + "clip-1-poster.jpg", width: 960, height: 540, durationMs: 6_000, size: 590_746)]),
        ])
        t = t.addingTimeInterval(240)
        _ = push(channel: Ids.design, author: Ids.cvetka, at: t, content: "", embeds: [
            Embed(url: "https://x.example.com/cvetka/status/1",
                  description: "Shipped the new icon set today. Been a long couple of weeks but it's finally in the wiki.",
                  kind: .rich, provider: "x", authorName: "cvetka", authorIcon: fakeAssetPrefix + "square-1.jpg",
                  media: [EmbedMedia(kind: .image, file: fakeAssetPrefix + "landscape-4.jpg", width: 1200, height: 800)], publishedAt: t),
        ])
        t = t.addingTimeInterval(240)
        _ = push(channel: Ids.design, author: Ids.ana, at: t, content: "", embeds: [
            Embed(url: "https://tiktok.example.com/@ana/video/1", description: "weekend hike recap #trail", kind: .rich, provider: "tiktok", authorName: "ana",
                  media: [EmbedMedia(kind: .video, file: fakeAssetPrefix + "clip-1.mp4", poster: fakeAssetPrefix + "clip-1-poster.jpg", width: 960, height: 540, durationMs: 6_000, size: 590_746)], publishedAt: t),
        ])
        t = t.addingTimeInterval(240)
        _ = push(channel: Ids.design, author: Ids.bruno, at: t, content: "", embeds: [
            Embed(url: "https://instagram.example.com/p/1", description: "Saturday's moodboard.", kind: .rich, provider: "instagram", authorName: "bruno.cooks", authorIcon: fakeAssetPrefix + "square-1.jpg",
                  media: [
                    EmbedMedia(kind: .image, file: fakeAssetPrefix + "landscape-1.jpg", width: 1200, height: 800),
                    EmbedMedia(kind: .image, file: fakeAssetPrefix + "portrait-1.jpg", width: 900, height: 1200),
                  ], publishedAt: t),
        ])
        t = t.addingTimeInterval(240)
        _ = push(channel: Ids.design, author: Ids.tim, at: t, content: "", embeds: [
            Embed(url: "https://reddit.example.com/r/thehideout/comments/1", title: "Found this at the flea market, 5 euros", description: "Still works. Mechanical, loud, perfect.",
                  kind: .rich, provider: "reddit", authorName: "u/tim",
                  media: [EmbedMedia(kind: .image, file: fakeAssetPrefix + "landscape-5.jpg", width: 1200, height: 800)], publishedAt: t),
        ])
    }

    private var nextAttachmentID: ID = 1

    /// A generated attachment: `displayName` is what the message shows (unique per attachment, like a real
    /// upload's name); `asset` is which bundled sample file it points to (`HideoutUI/Resources/FakeMedia`,
    /// several attachments can share one, as several real uploads could share a generated thumbnail).
    private mutating func asset(_ displayName: String, _ asset: String, _ contentType: String, _ size: UInt64,
                                 width: Int? = nil, height: Int? = nil, thumbnail: String? = nil, removed: Bool = false) -> Attachment {
        nextAttachmentID += 1
        return Attachment(id: nextAttachmentID, filename: displayName, contentType: contentType, size: size,
                           file: removed ? "" : fakeAssetPrefix + asset, width: width, height: height,
                           thumbnail: thumbnail.map { fakeAssetPrefix + $0 }, removed: removed)
    }

    /// Pictures (1, 2, 3 and 4+ up, for every grid layout), a video, an audio note, a file and one an admin
    /// removed — desktop's `fake_files.rs::seed_attachments`, into #design. Seeded newest, after `seedEmbeds`
    /// and `seedEmoji`, so the attachment UI tests' scroll-to-the-attachment-rows keeps working unchanged.
    private mutating func seedAttachments(now: Date) {
        var t = now.addingTimeInterval(-3 * 86_400)
        _ = push(channel: Ids.design, author: Ids.bruno, at: t, content: "the new dashboard, before the polish pass",
                  attachments: [asset("dashboard.jpg", "square-1.jpg", "image/jpeg", 420_000, width: 1000, height: 1000)])
        t = t.addingTimeInterval(360)
        _ = push(channel: Ids.design, author: Ids.tim, at: t, content: "shots from the hike", attachments: [
            asset("hike-1.jpg", "landscape-3.jpg", "image/jpeg", 480_000, width: 1200, height: 800),
            asset("hike-2.jpg", "square-1.jpg", "image/jpeg", 410_000, width: 1000, height: 1000),
        ])
        t = t.addingTimeInterval(360)
        _ = push(channel: Ids.design, author: Ids.ana, at: t, content: "Photos from Saturday", attachments: [
            asset("saturday-1.jpg", "landscape-1.jpg", "image/jpeg", 840_000, width: 1200, height: 800),
            asset("saturday-2.jpg", "portrait-1.jpg", "image/jpeg", 910_000, width: 900, height: 1200),
            asset("saturday-3.jpg", "landscape-2.jpg", "image/jpeg", 1_200_000, width: 1200, height: 800),
        ])
        t = t.addingTimeInterval(360)
        _ = push(channel: Ids.design, author: Ids.cvetka, at: t, content: "moodboard", attachments: [
            asset("mood-1.jpg", "landscape-4.jpg", "image/jpeg", 300_000, width: 1200, height: 800),
            asset("mood-2.jpg", "landscape-5.jpg", "image/jpeg", 300_000, width: 1200, height: 800),
            asset("mood-3.jpg", "portrait-1.jpg", "image/jpeg", 300_000, width: 900, height: 1200),
            asset("mood-4.jpg", "square-1.jpg", "image/jpeg", 300_000, width: 1000, height: 1000),
        ])
        t = t.addingTimeInterval(360)
        _ = push(channel: Ids.design, author: Ids.ana, at: t, content: "screen recording of the bug", attachments: [
            asset("demo.mp4", "clip-1.mp4", "video/mp4", 590_746, width: 960, height: 540, thumbnail: "clip-1-poster.jpg"),
        ])
        t = t.addingTimeInterval(360)
        _ = push(channel: Ids.design, author: Ids.ana, at: t, content: "", attachments: [
            asset("voice-note.m4a", "tone-1.m4a", "audio/mp4", 98_808),
        ])
        t = t.addingTimeInterval(360)
        _ = push(channel: Ids.design, author: Ids.bruno, at: t, content: "notes from the planning session", attachments: [
            asset("meeting-notes.txt", "notes-1.txt", "text/plain", 218),
        ])
        t = t.addingTimeInterval(360)
        _ = push(channel: Ids.design, author: Ids.cvetka, at: t, content: "this was the wrong sheet", attachments: [
            asset("invoice.xlsx", "notes-1.txt", "application/vnd.ms-excel", 88_000, removed: true),
        ])
    }

    static func generalLine(_ i: Int, total: Int) -> String {
        if let k = Optional(i - (total - markdownLines.count)), k >= 0 { return markdownLines[k] }
        return generalLines[(i * 17 + i / 60) % generalLines.count]
    }

    static let markdownLines = [
        "Release notes are up: **v0.0.3** adds a live gateway, ~~polling~~ and *much* better reconnects.",
        "Draft is in [the wiki](https://example.com/wiki/release), the changelog is at https://example.com/changelog.",
        "> staging disk is at 80%\nThat is the first thing to fix. ||it was the old snapshots||",
        "Plan for Friday:\n- cut the release\n- update the `README`\n- tell @cvetka",
        "# Release checklist\n1. tag it\n2. ship it",
        "```sh\ncargo build --release\n```",
        "__underlined__, `inline code` and _italic_ words, all in one line.",
    ]

    static let generalLines = [
        "Morning all.", "Staging is back up after the migration.", "Pushed the fix for the reconnect loop, review welcome.",
        "On it. Is `backoff_ms` capped?", "Yep, capped at 30s. @tim can you check the gateway side?", "Looks right to me.",
        "Lunch? The new ramen place on Elm opens at noon.", "@ana you coming on Friday?", "Count me in, I'll bring the speaker.",
        "Did anyone see the build fail on main?", "That was me, sorry. Fixed in `a41c9e2`.", "No worries.",
        "Reminder: standup moved to 10:15 tomorrow.", "Thanks @bruno, noted.", "Anyone tried the new release of the router crate?",
        "All green here.", "Nice. Merging.", "I'm out for an hour, dentist.", "Good luck!",
        "Movie night Saturday? I can host.", "Yes please.", "Which one though? Last time we argued for an hour.",
        "Quick one: does the invite code expire?", "No, but the owner can rotate it.", "Makes sense.",
        "The deploy script now prints the commit it ships. Small thing but it saved me twice.", "Love that.",
        "Coffee machine is broken again, don't trust it.", "Does anyone have the router password for the studio?",
        "It's on the fridge, the yellow note.", "Of course it is.", "I'm bringing snacks Saturday. Dietary restrictions, speak now.",
    ]

    static let devLines = [
        "rebased onto main, CI is running", "the flaky test is the reconnect one again", "I can reproduce it locally with a 10 ms delay",
        "so the watchdog fires before the first heartbeat?", "yes, the interval starts at connect, not at first event", "that explains it, patch incoming",
        "```rust\nlet silence = interval.mul_f64(2.5);\n```", "lgtm, ship it",
    ]

    static let dmAnaLines: [(ID, String)] = [
        (Ids.ana, "hey, got a minute?"), (Ids.tim, "sure, what's up"),
        (Ids.ana, "I'm stuck on the invite flow, can you look at it tonight?"),
        (Ids.tim, "yep. I'll ping you when it's pushed"), (Ids.ana, "perfect, thank you!"),
    ]

    static let dmBrunoLines: [(ID, String)] = [
        (Ids.bruno, "are we still on for Saturday?"), (Ids.tim, "yes, 7 at yours"), (Ids.bruno, "great, I'll make the sourdough pizza"),
    ]
}

/// Small deterministic generator so the seeded world is the same on every run.
private struct SeededRandom {
    var state: UInt64
    init(seed: UInt64) { state = seed }
    mutating func next(_ below: UInt64) -> UInt64 {
        state = state &* 6_364_136_223_846_793_005 &+ 1_442_695_040_888_963_407
        return (state >> 33) % below
    }
}
