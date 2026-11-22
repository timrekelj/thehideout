import Foundation

/// Server ids are 64-bit; one alias keeps signatures readable.
public typealias ID = UInt64

public struct User: Sendable, Hashable, Identifiable {
    public var id: ID
    public var username: String
    public var createdAt: Date
    /// Media key of the avatar, nil for the generated avatar.
    public var avatar: String?
    public var isAdmin: Bool
    public var email: String?
    public var deleted: Bool

    public init(id: ID, username: String, createdAt: Date = Date(timeIntervalSince1970: 0), avatar: String? = nil, isAdmin: Bool = false, email: String? = nil, deleted: Bool = false) {
        self.id = id; self.username = username; self.createdAt = createdAt; self.avatar = avatar
        self.isAdmin = isAdmin; self.email = email; self.deleted = deleted
    }
}

public struct Guild: Sendable, Hashable, Identifiable {
    public var id: ID
    public var name: String
    public var ownerID: ID
    public var inviteCode: String
    public var createdAt: Date
    public var icon: String?

    public init(id: ID, name: String, ownerID: ID, inviteCode: String = "", createdAt: Date = Date(timeIntervalSince1970: 0), icon: String? = nil) {
        self.id = id; self.name = name; self.ownerID = ownerID; self.inviteCode = inviteCode
        self.createdAt = createdAt; self.icon = icon
    }
}

public enum ChannelKind: Sendable, Hashable { case text, voice }

/// A channel's chosen icon: a Phosphor *fill* name, a Unicode emoji, a custom emoji, or the default.
public enum ChannelIconRef: Sendable, Hashable {
    case `default`
    case phosphor(String)
    case unicode(String)
    case customEmoji(ID)
}

public struct Channel: Sendable, Hashable, Identifiable {
    public var id: ID
    public var guildID: ID
    public var name: String
    public var createdAt: Date
    public var sectionID: ID?
    public var position: Int
    public var icon: ChannelIconRef
    /// A palette key (`teal`, `blue`, ...), empty for none.
    public var color: String
    public var topic: String
    public var kind: ChannelKind

    public init(id: ID, guildID: ID, name: String, createdAt: Date = Date(timeIntervalSince1970: 0), sectionID: ID? = nil, position: Int = 0, icon: ChannelIconRef = .default, color: String = "", topic: String = "", kind: ChannelKind = .text) {
        self.id = id; self.guildID = guildID; self.name = name; self.createdAt = createdAt
        self.sectionID = sectionID; self.position = position; self.icon = icon; self.color = color
        self.topic = topic; self.kind = kind
    }
}

public struct Section: Sendable, Hashable, Identifiable {
    public var id: ID
    public var guildID: ID
    public var name: String
    public var position: Int
    public init(id: ID, guildID: ID, name: String, position: Int = 0) {
        self.id = id; self.guildID = guildID; self.name = name; self.position = position
    }
}

public struct LayoutEntry: Sendable, Hashable {
    /// nil is the top level.
    public var sectionID: ID?
    public var channelIDs: [ID]
    public init(sectionID: ID?, channelIDs: [ID]) { self.sectionID = sectionID; self.channelIDs = channelIDs }
}

/// A guild's sections and the order of its channels. `version` guards `UpdateLayout`.
public struct Layout: Sendable, Hashable {
    public var version: UInt64
    public var sections: [Section]
    public var entries: [LayoutEntry]
    public init(version: UInt64 = 0, sections: [Section] = [], entries: [LayoutEntry] = []) {
        self.version = version; self.sections = sections; self.entries = entries
    }

    public func channelIDs(in section: ID?) -> [ID] {
        entries.first { $0.sectionID == section }?.channelIDs ?? []
    }

    /// One entry per container (top level, then each section in order), so edits never meet a missing one.
    public var normalized: Layout {
        var out = self
        out.entries = ([nil] + sections.map { Optional($0.id) }).map { LayoutEntry(sectionID: $0, channelIDs: channelIDs(in: $0)) }
        return out
    }

    /// Every channel id in sidebar order (top level first, then each section).
    public var channelOrder: [ID] {
        channelIDs(in: nil) + sections.flatMap { channelIDs(in: $0.id) }
    }
}

public struct Role: Sendable, Hashable, Identifiable {
    public var id: ID
    public var guildID: ID
    public var name: String
    public var color: String
    public var position: Int
    public var permissions: Permissions
    public var createdAt: Date
    public init(id: ID, guildID: ID, name: String, color: String = "", position: Int = 0, permissions: Permissions = [], createdAt: Date = Date(timeIntervalSince1970: 0)) {
        self.id = id; self.guildID = guildID; self.name = name; self.color = color
        self.position = position; self.permissions = permissions; self.createdAt = createdAt
    }
}

public struct Member: Sendable, Hashable {
    public var user: User
    public var roleIDs: [ID]
    public var joinedAt: Date
    public init(user: User, roleIDs: [ID] = [], joinedAt: Date = Date(timeIntervalSince1970: 0)) {
        self.user = user; self.roleIDs = roleIDs; self.joinedAt = joinedAt
    }
}

public struct Emoji: Sendable, Hashable, Identifiable {
    public var id: ID
    public var guildID: ID
    public var name: String
    public var animated: Bool
    public var image: String
    public var creatorID: ID
    public var createdAt: Date
    public init(id: ID, guildID: ID, name: String, animated: Bool = false, image: String = "", creatorID: ID = 0, createdAt: Date = Date(timeIntervalSince1970: 0)) {
        self.id = id; self.guildID = guildID; self.name = name; self.animated = animated
        self.image = image; self.creatorID = creatorID; self.createdAt = createdAt
    }
}

public enum ReactionEmoji: Sendable, Hashable {
    case unicode(String)
    case custom(ID)
}

public struct Reaction: Sendable, Hashable {
    public var emoji: ReactionEmoji
    public var count: Int
    public var me: Bool
    public init(emoji: ReactionEmoji, count: Int, me: Bool) { self.emoji = emoji; self.count = count; self.me = me }
}

public struct Attachment: Sendable, Hashable, Identifiable {
    public var id: ID
    public var filename: String
    public var contentType: String
    public var size: UInt64
    public var file: String
    public var width: Int?
    public var height: Int?
    public var thumbnail: String?
    public var removed: Bool
    public init(id: ID, filename: String, contentType: String, size: UInt64, file: String, width: Int? = nil, height: Int? = nil, thumbnail: String? = nil, removed: Bool = false) {
        self.id = id; self.filename = filename; self.contentType = contentType; self.size = size; self.file = file
        self.width = width; self.height = height; self.thumbnail = thumbnail; self.removed = removed
    }
}

public enum EmbedKind: Sendable, Hashable { case link, image, video, rich }
public enum EmbedMediaKind: Sendable, Hashable { case image, video, gifv }

public struct EmbedMedia: Sendable, Hashable {
    public var kind: EmbedMediaKind
    public var file: String
    public var poster: String?
    public var width: Int
    public var height: Int
    public var durationMs: Int?
    public var size: UInt64
    public init(kind: EmbedMediaKind, file: String, poster: String? = nil, width: Int, height: Int, durationMs: Int? = nil, size: UInt64 = 0) {
        self.kind = kind; self.file = file; self.poster = poster; self.width = width; self.height = height
        self.durationMs = durationMs; self.size = size
    }
}

public struct Embed: Sendable, Hashable {
    public var url: String
    public var title: String
    public var description: String
    public var siteName: String
    public var thumbnail: String?
    public var thumbnailWidth: Int?
    public var thumbnailHeight: Int?
    public var kind: EmbedKind
    public var provider: String
    public var authorName: String
    public var authorURL: String?
    public var authorIcon: String?
    public var media: [EmbedMedia]
    public var publishedAt: Date?
    public init(url: String, title: String = "", description: String = "", siteName: String = "", thumbnail: String? = nil, thumbnailWidth: Int? = nil, thumbnailHeight: Int? = nil, kind: EmbedKind = .link, provider: String = "", authorName: String = "", authorURL: String? = nil, authorIcon: String? = nil, media: [EmbedMedia] = [], publishedAt: Date? = nil) {
        self.url = url; self.title = title; self.description = description; self.siteName = siteName
        self.thumbnail = thumbnail; self.thumbnailWidth = thumbnailWidth; self.thumbnailHeight = thumbnailHeight
        self.kind = kind; self.provider = provider; self.authorName = authorName; self.authorURL = authorURL
        self.authorIcon = authorIcon; self.media = media; self.publishedAt = publishedAt
    }
}

public struct Message: Sendable, Hashable, Identifiable {
    public var id: ID
    public var channelID: ID
    /// nil in a DM.
    public var guildID: ID?
    public var authorID: ID
    public var content: String
    public var createdAt: Date
    public var editedAt: Date?
    /// The sender's client nonce, echoed by the server so a pending message can be matched.
    public var nonce: String
    public var reactions: [Reaction]
    public var attachments: [Attachment]
    public var embeds: [Embed]

    public init(id: ID, channelID: ID, guildID: ID? = nil, authorID: ID, content: String, createdAt: Date = Date(timeIntervalSince1970: 0), editedAt: Date? = nil, nonce: String = "", reactions: [Reaction] = [], attachments: [Attachment] = [], embeds: [Embed] = []) {
        self.id = id; self.channelID = channelID; self.guildID = guildID; self.authorID = authorID
        self.content = content; self.createdAt = createdAt; self.editedAt = editedAt; self.nonce = nonce
        self.reactions = reactions; self.attachments = attachments; self.embeds = embeds
    }
}

public struct DMChannel: Sendable, Hashable, Identifiable {
    public var id: ID
    public var participants: [User]
    public var createdAt: Date
    public init(id: ID, participants: [User], createdAt: Date = Date(timeIntervalSince1970: 0)) {
        self.id = id; self.participants = participants; self.createdAt = createdAt
    }
}

public struct VoiceState: Sendable, Hashable {
    public var guildID: ID
    public var channelID: ID
    public var userID: ID
    public var selfMute: Bool
    public var selfDeaf: Bool
    public var serverMute: Bool
    public var selfVideo: Bool
    public var selfStream: Bool
    public var joinedAt: Date
    public init(guildID: ID, channelID: ID, userID: ID, selfMute: Bool = false, selfDeaf: Bool = false, serverMute: Bool = false, selfVideo: Bool = false, selfStream: Bool = false, joinedAt: Date = Date(timeIntervalSince1970: 0)) {
        self.guildID = guildID; self.channelID = channelID; self.userID = userID; self.selfMute = selfMute
        self.selfDeaf = selfDeaf; self.serverMute = serverMute; self.selfVideo = selfVideo
        self.selfStream = selfStream; self.joinedAt = joinedAt
    }
}

public enum InviteStatus: Sendable, Hashable { case pending, accepted, expired }

public struct Invite: Sendable, Hashable, Identifiable {
    public var id: ID
    public var email: String
    public var code: String?
    public var status: InviteStatus
    public var inviter: User?
    public var guildIDs: [ID]
    public var createdAt: Date
    public var expiresAt: Date
    public var makeAdmin: Bool
    public init(id: ID, email: String, code: String? = nil, status: InviteStatus = .pending, inviter: User? = nil, guildIDs: [ID] = [], createdAt: Date = Date(timeIntervalSince1970: 0), expiresAt: Date = Date(timeIntervalSince1970: 0), makeAdmin: Bool = false) {
        self.id = id; self.email = email; self.code = code; self.status = status; self.inviter = inviter
        self.guildIDs = guildIDs; self.createdAt = createdAt; self.expiresAt = expiresAt; self.makeAdmin = makeAdmin
    }
}

/// What a server sends about the signed-in user.
public struct Me: Sendable, Hashable {
    public var user: User
    /// Set when the terms must be accepted: the version to accept.
    public var termsUpdateRequired: String?
    public var termsVersion: String
    public init(user: User, termsUpdateRequired: String? = nil, termsVersion: String = "") {
        self.user = user; self.termsUpdateRequired = termsUpdateRequired; self.termsVersion = termsVersion
    }
}

/// A page of messages, newest first (as the API returns it).
public struct MessagePage: Sendable, Hashable {
    public var messages: [Message]
    public var nextBefore: ID?
    public var hasMore: Bool
    public init(messages: [Message], nextBefore: ID? = nil, hasMore: Bool = false) {
        self.messages = messages; self.nextBefore = nextBefore; self.hasMore = hasMore
    }
}

/// The lists the app fetches after sign-in and again after a gateway gap.
public struct Snapshot: Sendable, Hashable {
    public var me: User
    public var guilds: [Guild]
    public var channels: [Channel]
    public var layouts: [ID: Layout]
    public var dms: [DMChannel]
    public init(me: User, guilds: [Guild], channels: [Channel], layouts: [ID: Layout], dms: [DMChannel]) {
        self.me = me; self.guilds = guilds; self.channels = channels; self.layouts = layouts; self.dms = dms
    }
}

// MARK: - Notification settings (NotificationService; the registration UI and APNs come in a later phase)

public enum NotificationLevel: Sendable, Hashable {
    /// Overrides only: use the next less specific setting (channel -> guild -> global default).
    case inherit
    case all
    case mentions
    case nothing
}

public struct NotificationOverride: Sendable, Hashable, Identifiable {
    /// A guild or channel id.
    public var id: ID
    public var level: NotificationLevel
    /// Muted until this time; nil or past = not muted.
    public var mutedUntil: Date?
    public init(id: ID, level: NotificationLevel = .inherit, mutedUntil: Date? = nil) {
        self.id = id; self.level = level; self.mutedUntil = mutedUntil
    }
}

/// The caller's push settings, owned by the server and kept current by `notification_settings_updated`.
public struct NotificationSettings: Sendable, Hashable {
    /// Push every message of DM channels.
    public var dmsEnabled: Bool
    /// Level for guild channels without an override: all, mentions or nothing.
    public var guildDefault: NotificationLevel
    /// Include message text in pushes (off sends "New message").
    public var showPreviews: Bool
    public var guilds: [NotificationOverride]
    public var channels: [NotificationOverride]
    public init(dmsEnabled: Bool = true, guildDefault: NotificationLevel = .mentions, showPreviews: Bool = true,
                guilds: [NotificationOverride] = [], channels: [NotificationOverride] = []) {
        self.dmsEnabled = dmsEnabled; self.guildDefault = guildDefault; self.showPreviews = showPreviews
        self.guilds = guilds; self.channels = channels
    }
}
