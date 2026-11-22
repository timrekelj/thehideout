import Foundation
import HideoutAPI
import SwiftProtobuf

// Models <-> generated protos. The protos stay inside HideoutCore (and the cache); views see the models.

private typealias Timestamp = Google_Protobuf_Timestamp

private extension Timestamp {
    var asDate: Date { date }
    static func of(_ d: Date) -> Timestamp { Timestamp(date: d) }
}

extension User {
    public init(_ p: Thehideout_V1_User) {
        self.init(id: p.id, username: p.username, createdAt: p.createdAt.date, avatar: p.avatar.isEmpty ? nil : p.avatar,
                  isAdmin: p.isAdmin, email: p.hasEmail ? p.email : nil, deleted: p.deleted)
    }
    public var proto: Thehideout_V1_User {
        var p = Thehideout_V1_User()
        p.id = id; p.username = username; p.createdAt = .of(createdAt); p.avatar = avatar ?? ""
        p.isAdmin = isAdmin; p.deleted = deleted
        if let email { p.email = email }
        return p
    }
}

extension Guild {
    public init(_ p: Thehideout_V1_Guild) {
        self.init(id: p.id, name: p.name, ownerID: p.ownerID, inviteCode: p.inviteCode, createdAt: p.createdAt.date,
                  icon: p.icon.isEmpty ? nil : p.icon)
    }
    public var proto: Thehideout_V1_Guild {
        var p = Thehideout_V1_Guild()
        p.id = id; p.name = name; p.ownerID = ownerID; p.inviteCode = inviteCode
        p.createdAt = .of(createdAt); p.icon = icon ?? ""
        return p
    }
}

extension ChannelIconRef {
    init(_ p: Thehideout_V1_ChannelIcon) {
        switch p.kind {
        case .phosphor(let s): self = .phosphor(s)
        case .unicode(let s): self = .unicode(s)
        case .customEmojiID(let id): self = .customEmoji(id)
        case nil: self = .default
        }
    }
    var proto: Thehideout_V1_ChannelIcon {
        var p = Thehideout_V1_ChannelIcon()
        switch self {
        case .default: break
        case .phosphor(let s): p.kind = .phosphor(s)
        case .unicode(let s): p.kind = .unicode(s)
        case .customEmoji(let id): p.kind = .customEmojiID(id)
        }
        return p
    }
}

extension Channel {
    public init(_ p: Thehideout_V1_Channel) {
        // `icon_ref` wins; a server that only sends the legacy `icon` name still gets a Phosphor icon.
        let icon: ChannelIconRef = p.hasIconRef ? ChannelIconRef(p.iconRef) : (p.icon.isEmpty ? .default : .phosphor(p.icon))
        self.init(id: p.id, guildID: p.guildID, name: p.name, createdAt: p.createdAt.date,
                  sectionID: p.hasSectionID ? p.sectionID : nil, position: Int(p.position), icon: icon,
                  color: p.color, topic: p.topic, kind: p.kind == .voice ? .voice : .text)
    }
    public var proto: Thehideout_V1_Channel {
        var p = Thehideout_V1_Channel()
        p.id = id; p.guildID = guildID; p.name = name; p.createdAt = .of(createdAt)
        if let sectionID { p.sectionID = sectionID }
        p.position = Int32(position); p.color = color; p.topic = topic
        p.kind = kind == .voice ? .voice : .text
        if case .phosphor(let s) = icon { p.icon = s }
        if icon != .default { p.iconRef = icon.proto }
        return p
    }
}

extension Section {
    public init(_ p: Thehideout_V1_Section) {
        self.init(id: p.id, guildID: p.guildID, name: p.name, position: Int(p.position))
    }
    public var proto: Thehideout_V1_Section {
        var p = Thehideout_V1_Section()
        p.id = id; p.guildID = guildID; p.name = name; p.position = Int32(position)
        return p
    }
}

extension Layout {
    public init(_ p: Thehideout_V1_GuildLayout) {
        self.init(version: p.version, sections: p.sections.map(Section.init),
                  entries: p.entries.map { LayoutEntry(sectionID: $0.sectionID == 0 ? nil : $0.sectionID, channelIDs: $0.channelIds) })
    }
    public var proto: Thehideout_V1_GuildLayout {
        var p = Thehideout_V1_GuildLayout()
        p.version = version; p.sections = sections.map(\.proto)
        p.entries = entries.map { e in
            var x = Thehideout_V1_LayoutEntry()
            x.sectionID = e.sectionID ?? 0; x.channelIds = e.channelIDs
            return x
        }
        return p
    }
}

extension Role {
    public init(_ p: Thehideout_V1_Role) {
        self.init(id: p.id, guildID: p.guildID, name: p.name, color: p.color, position: Int(p.position),
                  permissions: Permissions(rawValue: p.permissions), createdAt: p.createdAt.date)
    }
    public var proto: Thehideout_V1_Role {
        var p = Thehideout_V1_Role()
        p.id = id; p.guildID = guildID; p.name = name; p.color = color; p.position = Int32(position)
        p.permissions = permissions.rawValue; p.createdAt = .of(createdAt)
        return p
    }
}

extension Member {
    public init(_ p: Thehideout_V1_Member) {
        self.init(user: User(p.user), roleIDs: p.roleIds, joinedAt: p.joinedAt.date)
    }
    public var proto: Thehideout_V1_Member {
        var p = Thehideout_V1_Member()
        p.user = user.proto; p.roleIds = roleIDs; p.joinedAt = .of(joinedAt)
        return p
    }
}

extension Emoji {
    public init(_ p: Thehideout_V1_Emoji) {
        self.init(id: p.id, guildID: p.guildID, name: p.name, animated: p.animated, image: p.image,
                  creatorID: p.creatorID, createdAt: p.createdAt.date)
    }
    public var proto: Thehideout_V1_Emoji {
        var p = Thehideout_V1_Emoji()
        p.id = id; p.guildID = guildID; p.name = name; p.animated = animated; p.image = image
        p.creatorID = creatorID; p.createdAt = .of(createdAt)
        return p
    }
}

extension ReactionEmoji {
    init(_ p: Thehideout_V1_ReactionEmoji) {
        switch p.kind {
        case .customEmojiID(let id): self = .custom(id)
        case .unicode(let s): self = .unicode(s)
        case nil: self = .unicode("")
        }
    }
    var proto: Thehideout_V1_ReactionEmoji {
        var p = Thehideout_V1_ReactionEmoji()
        switch self {
        case .unicode(let s): p.kind = .unicode(s)
        case .custom(let id): p.kind = .customEmojiID(id)
        }
        return p
    }
}

extension Attachment {
    init(_ p: Thehideout_V1_Attachment) {
        self.init(id: p.id, filename: p.filename, contentType: p.contentType, size: p.size, file: p.file,
                  width: p.hasWidth ? Int(p.width) : nil, height: p.hasHeight ? Int(p.height) : nil,
                  thumbnail: p.hasThumbnail ? p.thumbnail : nil, removed: p.removed)
    }
    var proto: Thehideout_V1_Attachment {
        var p = Thehideout_V1_Attachment()
        p.id = id; p.filename = filename; p.contentType = contentType; p.size = size; p.file = file; p.removed = removed
        if let width { p.width = UInt32(width) }
        if let height { p.height = UInt32(height) }
        if let thumbnail { p.thumbnail = thumbnail }
        return p
    }
}

extension EmbedMedia {
    init(_ p: Thehideout_V1_EmbedMedia) {
        let kind: EmbedMediaKind = p.kind == .video ? .video : (p.kind == .gifv ? .gifv : .image)
        self.init(kind: kind, file: p.file, poster: p.hasPoster ? p.poster : nil, width: Int(p.width),
                  height: Int(p.height), durationMs: p.hasDurationMs ? Int(p.durationMs) : nil, size: p.size)
    }
    var proto: Thehideout_V1_EmbedMedia {
        var p = Thehideout_V1_EmbedMedia()
        p.kind = kind == .video ? .video : (kind == .gifv ? .gifv : .image)
        p.file = file; p.width = UInt32(width); p.height = UInt32(height); p.size = size
        if let poster { p.poster = poster }
        if let durationMs { p.durationMs = UInt32(durationMs) }
        return p
    }
}

extension Embed {
    init(_ p: Thehideout_V1_Embed) {
        let kind: EmbedKind
        switch p.kind {
        case .image: kind = .image
        case .video: kind = .video
        case .rich: kind = .rich
        default: kind = .link
        }
        self.init(url: p.url, title: p.title, description: p.description_p, siteName: p.siteName,
                  thumbnail: p.hasThumbnail ? p.thumbnail : nil,
                  thumbnailWidth: p.hasThumbnailWidth ? Int(p.thumbnailWidth) : nil,
                  thumbnailHeight: p.hasThumbnailHeight ? Int(p.thumbnailHeight) : nil,
                  kind: kind, provider: p.provider, authorName: p.authorName,
                  authorURL: p.hasAuthorURL ? p.authorURL : nil, authorIcon: p.hasAuthorIcon ? p.authorIcon : nil,
                  media: p.media.map(EmbedMedia.init), publishedAt: p.hasPublishedAt ? p.publishedAt.date : nil)
    }
    var proto: Thehideout_V1_Embed {
        var p = Thehideout_V1_Embed()
        p.url = url; p.title = title; p.description_p = description; p.siteName = siteName
        switch kind {
        case .link: p.kind = .link
        case .image: p.kind = .image
        case .video: p.kind = .video
        case .rich: p.kind = .rich
        }
        p.provider = provider; p.authorName = authorName; p.media = media.map(\.proto)
        if let thumbnail { p.thumbnail = thumbnail }
        if let thumbnailWidth { p.thumbnailWidth = UInt32(thumbnailWidth) }
        if let thumbnailHeight { p.thumbnailHeight = UInt32(thumbnailHeight) }
        if let authorURL { p.authorURL = authorURL }
        if let authorIcon { p.authorIcon = authorIcon }
        if let publishedAt { p.publishedAt = .of(publishedAt) }
        return p
    }
}

extension Message {
    public init(_ p: Thehideout_V1_Message) {
        self.init(id: p.id, channelID: p.channelID, guildID: p.guildID == 0 ? nil : p.guildID, authorID: p.authorID,
                  content: p.content, createdAt: p.createdAt.date, editedAt: p.hasEditedAt ? p.editedAt.date : nil,
                  nonce: p.nonce,
                  reactions: p.reactions.map { Reaction(emoji: ReactionEmoji($0.emoji), count: Int($0.count), me: $0.me) },
                  attachments: p.attachments.map(Attachment.init), embeds: p.embeds.map(Embed.init))
    }
    public var proto: Thehideout_V1_Message {
        var p = Thehideout_V1_Message()
        p.id = id; p.channelID = channelID; p.guildID = guildID ?? 0; p.authorID = authorID
        p.content = content; p.createdAt = .of(createdAt); p.nonce = nonce
        if let editedAt { p.editedAt = .of(editedAt) }
        p.reactions = reactions.map { r in
            var x = Thehideout_V1_Reaction()
            x.emoji = r.emoji.proto; x.count = UInt32(r.count); x.me = r.me
            return x
        }
        p.attachments = attachments.map(\.proto); p.embeds = embeds.map(\.proto)
        return p
    }
}

extension DMChannel {
    public init(_ p: Thehideout_V1_DmChannel) {
        self.init(id: p.id, participants: p.participants.map(User.init), createdAt: p.createdAt.date)
    }
    public var proto: Thehideout_V1_DmChannel {
        var p = Thehideout_V1_DmChannel()
        p.id = id; p.participants = participants.map(\.proto); p.createdAt = .of(createdAt)
        return p
    }
}

extension VoiceState {
    public init(_ p: Thehideout_V1_VoiceState) {
        self.init(guildID: p.guildID, channelID: p.channelID, userID: p.userID, selfMute: p.selfMute, selfDeaf: p.selfDeaf,
                  serverMute: p.serverMute, selfVideo: p.selfVideo, selfStream: p.selfStream, joinedAt: p.joinedAt.date)
    }
    public var proto: Thehideout_V1_VoiceState {
        var p = Thehideout_V1_VoiceState()
        p.guildID = guildID; p.channelID = channelID; p.userID = userID; p.selfMute = selfMute; p.selfDeaf = selfDeaf
        p.serverMute = serverMute; p.selfVideo = selfVideo; p.selfStream = selfStream; p.joinedAt = .of(joinedAt)
        return p
    }
}

extension Invite {
    public init(_ p: Thehideout_V1_Invite) {
        let status: InviteStatus = p.status == .accepted ? .accepted : (p.status == .expired ? .expired : .pending)
        self.init(id: p.id, email: p.email, code: p.hasCode ? p.code : nil, status: status,
                  inviter: p.hasInviter ? User(p.inviter) : nil, guildIDs: p.guildIds,
                  createdAt: p.createdAt.date, expiresAt: p.expiresAt.date, makeAdmin: p.makeAdmin)
    }
}

extension Me {
    init(_ p: Thehideout_V1_GetMeResponse) {
        self.init(user: User(p.user), termsUpdateRequired: p.hasTermsUpdateRequired ? p.termsUpdateRequired : nil,
                  termsVersion: p.termsVersion)
    }
}
