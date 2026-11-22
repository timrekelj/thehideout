import Foundation
import SwiftProtobuf
import HideoutAPI

/// A small file cache, not a database: the data is a mirror the server owns and the gateway keeps current, so
/// schema migrations and a query layer would cost more than they give. It writes the guild, channel, layout and
/// DM lists and the last 100 messages of each recently opened channel as protobuf binary blobs (one file per
/// channel, atomic writes), loads them before the first network answer, caps the message files at `maxBytes`
/// (desktop's 64 MB media budget) and is wiped on sign-out.
public actor Cache {
    public static let messagesPerChannel = 100

    private let directory: URL
    private let maxBytes: Int
    private let fm = FileManager.default

    public init(directory: URL, maxBytes: Int = 64 * 1024 * 1024) {
        self.directory = directory
        self.maxBytes = maxBytes
        try? fm.createDirectory(at: directory, withIntermediateDirectories: true)
    }

    /// `Caches/thehideout` for the app.
    public static func standard() -> Cache {
        let base = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
        return Cache(directory: base.appendingPathComponent("thehideout", isDirectory: true))
    }

    private func url(_ name: String) -> URL { directory.appendingPathComponent(name) }

    private func write<M: SwiftProtobuf.Message>(_ message: M, to name: String) {
        guard let data = try? message.serializedData() else { return }
        try? data.write(to: url(name), options: .atomic)
    }

    private func read<M: SwiftProtobuf.Message>(_ type: M.Type, _ name: String) -> M? {
        guard let data = try? Data(contentsOf: url(name)) else { return nil }
        return try? M(serializedBytes: data)
    }

    // MARK: lists

    public func saveSnapshot(_ s: Snapshot) {
        write(s.me.proto, to: "me.pb")
        var guilds = Thehideout_V1_ListMyGuildsResponse(); guilds.guilds = s.guilds.map(\.proto)
        write(guilds, to: "guilds.pb")
        var dms = Thehideout_V1_ListDmsResponse(); dms.channels = s.dms.map(\.proto)
        write(dms, to: "dms.pb")
        for g in s.guilds {
            var r = Thehideout_V1_ListChannelsResponse()
            r.channels = s.channels.filter { $0.guildID == g.id }.map(\.proto)
            r.layout = (s.layouts[g.id] ?? Layout()).proto
            write(r, to: "guild-\(g.id).pb")
        }
    }

    public func loadSnapshot() -> Snapshot? {
        guard let me = read(Thehideout_V1_User.self, "me.pb"),
              let guilds = read(Thehideout_V1_ListMyGuildsResponse.self, "guilds.pb") else { return nil }
        let dms = read(Thehideout_V1_ListDmsResponse.self, "dms.pb")?.channels.map(DMChannel.init) ?? []
        var channels: [Channel] = []
        var layouts: [ID: Layout] = [:]
        for g in guilds.guilds {
            if let r = read(Thehideout_V1_ListChannelsResponse.self, "guild-\(g.id).pb") {
                channels += r.channels.map(Channel.init)
                layouts[g.id] = Layout(r.layout)
            }
        }
        return Snapshot(me: User(me), guilds: guilds.guilds.map(Guild.init), channels: channels, layouts: layouts, dms: dms)
    }

    // MARK: messages

    /// Keeps the newest 100 of `messages` (oldest first).
    public func saveMessages(_ messages: [Message], channel: ID) {
        var r = Thehideout_V1_ListMessagesResponse()
        r.messages = messages.suffix(Self.messagesPerChannel).map(\.proto)
        write(r, to: "messages-\(channel).pb")
        trim()
    }

    /// Oldest first.
    public func loadMessages(channel: ID) -> [Message]? {
        read(Thehideout_V1_ListMessagesResponse.self, "messages-\(channel).pb")?.messages.map(Message.init)
    }

    // MARK: housekeeping

    /// Total size of the message files.
    public func messageBytes() -> Int {
        messageFiles().reduce(0) { $0 + $1.size }
    }

    private func messageFiles() -> [(url: URL, size: Int, date: Date)] {
        let keys: Set<URLResourceKey> = [.fileSizeKey, .contentModificationDateKey]
        let files = (try? fm.contentsOfDirectory(at: directory, includingPropertiesForKeys: Array(keys))) ?? []
        return files.filter { $0.lastPathComponent.hasPrefix("messages-") }.compactMap { u in
            guard let v = try? u.resourceValues(forKeys: keys) else { return nil }
            return (u, v.fileSize ?? 0, v.contentModificationDate ?? .distantPast)
        }
    }

    /// Evicts the least recently written message files until the budget fits.
    public func trim() {
        var files = messageFiles().sorted { $0.date < $1.date }
        var total = files.reduce(0) { $0 + $1.size }
        while total > maxBytes, let oldest = files.first {
            try? fm.removeItem(at: oldest.url)
            total -= oldest.size
            files.removeFirst()
        }
    }

    /// Drops everything (sign-out).
    public func clear() {
        let files = (try? fm.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil)) ?? []
        for f in files { try? fm.removeItem(at: f) }
    }
}
