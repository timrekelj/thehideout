import Foundation

/// A real-time event from `GatewayService.Subscribe`, one case per payload of `Event`.
public enum GatewayEvent: Sendable, Equatable {
    case messageCreated(Message)
    case channelCreated(Channel)
    case guildMemberJoined(guild: ID, user: User)
    case heartbeat
    case dmOpened(DMChannel)
    case guildUpdated(Guild)
    case channelUpdated(Channel)
    case layoutUpdated(guild: ID, layout: Layout)
    case rolesUpdated(guild: ID, roles: [Role], version: UInt64)
    case memberUpdated(guild: ID, member: Member)
    case userUpdated(User)
    case emojisUpdated(guild: ID, emojis: [Emoji])
    case reactionAdded(channel: ID, message: ID, emoji: ReactionEmoji, user: ID)
    case reactionRemoved(channel: ID, message: ID, emoji: ReactionEmoji, user: ID)
    case messageUpdated(Message)
    case guildAdded(Guild)
    /// `state == nil`: the user left voice in `guild`.
    case voiceStateUpdate(guild: ID, user: ID, state: VoiceState?)
    case voiceSnapshot(guilds: [ID], states: [VoiceState])
    case voiceMoved(guild: ID, channel: ID)
    case messageDeleted(channel: ID, message: ID)
    case guildMemberRemoved(guild: ID, user: ID, reason: MemberRemovalReason)
    case guildRemoved(guild: ID)
    case channelDeleted(guild: ID, channel: ID)
    /// The caller changed their notification settings on another session: the full new settings.
    case notificationSettingsUpdated(NotificationSettings)
}

public enum MemberRemovalReason: Sendable, Equatable { case left, kicked, banned, unspecified }

/// What the gateway reports to the store.
public enum GatewayUpdate: Sendable, Equatable {
    /// Opening a stream (first attempt or a retry).
    case connecting
    /// The stream is open. Events missed while disconnected are not replayed: refetch what the UI shows.
    case connected
    case event(GatewayEvent)
    /// The connection failed or went silent; the next attempt is in `retryIn`.
    case disconnected(retryIn: TimeInterval, reason: String)
    /// The server rejected the token. The gateway stopped for good.
    case authExpired
    /// The server refuses this app version.
    case updateRequired
}

/// A source of gateway events: one call opens one stream, which ends (throwing) when the connection breaks.
public protocol GatewaySource: Sendable {
    func subscribe() -> AsyncThrowingStream<GatewayEvent, Error>
}

/// The calls the store makes. The real implementation is `ConnectApiClient`; `FakeBackend` is the in-memory one.
public protocol ApiClient: GatewaySource {
    func me() async throws -> Me
    /// The signed-in lists: my guilds with their channels and layouts, and my DMs.
    func snapshot() async throws -> Snapshot
    func listChannels(guild: ID) async throws -> (channels: [Channel], layout: Layout)
    func listMembers(guild: ID) async throws -> [Member]
    func listRoles(guild: ID) async throws -> (roles: [Role], version: UInt64)
    func listEmojis(guild: ID) async throws -> [Emoji]
    func listMessages(channel: ID, before: ID?, limit: Int) async throws -> MessagePage
    func sendMessage(channel: ID, content: String, nonce: String, attachmentIDs: [ID]) async throws -> Message
    func editMessage(channel: ID, message: ID, content: String) async throws -> Message
    func deleteMessage(channel: ID, message: ID) async throws
    func addReaction(channel: ID, message: ID, emoji: ReactionEmoji) async throws
    func removeReaction(channel: ID, message: ID, emoji: ReactionEmoji) async throws
    func updateLayout(guild: ID, expectedVersion: UInt64, sectionIDs: [ID], entries: [LayoutEntry]) async throws -> Layout
    /// Opens a DM with `user`, or returns the existing one (idempotent).
    func startDm(user: ID) async throws -> DMChannel
    /// `POST /attachments`: streams `fileURL`'s bytes (plain HTTP, not Connect) as a pending upload good
    /// for `sendMessage(attachmentIDs:)` within the hour. `progress(sent)` runs as bytes are handed to the
    /// connection.
    func uploadAttachment(channel: ID, filename: String, contentType: String, size: UInt64, fileURL: URL, progress: @escaping @Sendable (UInt64) -> Void) async throws -> PendingAttachment
}

/// The `x-thehideout-client` header value: `ios/<version> (iOS <os version>)`.
public enum ClientIdentity {
    public static let header = "x-thehideout-client"
    public static let appVersion = "0.1.0"

    public static func value(appVersion: String = appVersion, osVersion: OperatingSystemVersion = ProcessInfo.processInfo.operatingSystemVersion) -> String {
        "ios/\(appVersion) (iOS \(osVersion.majorVersion).\(osVersion.minorVersion).\(osVersion.patchVersion))"
    }
}
