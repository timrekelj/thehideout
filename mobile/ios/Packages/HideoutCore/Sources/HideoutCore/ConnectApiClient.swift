import Connect
import Foundation
import HideoutAPI
import SwiftProtobuf

/// Adds `authorization: Bearer <token>` and `x-thehideout-client`, for unary calls and streams.
final class HeaderInterceptor: UnaryInterceptor, StreamInterceptor, @unchecked Sendable {
    private let token: @Sendable () -> String?
    private let client: String

    init(client: String, token: @escaping @Sendable () -> String?) {
        self.client = client
        self.token = token
    }

    private func headers(_ h: Headers) -> Headers {
        var out = h
        out[ClientIdentity.header] = [client]
        if let t = token(), !t.isEmpty { out["authorization"] = ["Bearer \(t)"] }
        return out
    }

    func handleUnaryRequest<Message: ProtobufMessage>(_ request: HTTPRequest<Message>, proceed: @escaping @Sendable (Result<HTTPRequest<Message>, ConnectError>) -> Void) {
        proceed(.success(HTTPRequest(url: request.url, headers: headers(request.headers), message: request.message, method: request.method, trailers: request.trailers, idempotencyLevel: request.idempotencyLevel)))
    }

    func handleStreamStart(_ request: HTTPRequest<Void>, proceed: @escaping @Sendable (Result<HTTPRequest<Void>, ConnectError>) -> Void) {
        proceed(.success(HTTPRequest(url: request.url, headers: headers(request.headers), message: (), method: request.method, trailers: request.trailers, idempotencyLevel: request.idempotencyLevel)))
    }
}

/// The real `ApiClient`: connect-swift (Connect protocol, protobuf codec) over `URLSession`.
public final class ConnectApiClient: ApiClient {
    let auth: Thehideout_V1_AuthServiceClient
    let invites: Thehideout_V1_InviteServiceClient
    private let guilds: Thehideout_V1_GuildServiceClient
    private let messages: Thehideout_V1_MessageServiceClient
    private let dms: Thehideout_V1_DmServiceClient
    private let roles: Thehideout_V1_RoleServiceClient
    private let emojis: Thehideout_V1_EmojiServiceClient
    private let gateway: Thehideout_V1_GatewayServiceClient

    /// `token` is read for every call, so a new session takes effect without rebuilding the client.
    public init(baseURL: URL, clientHeader: String = ClientIdentity.value(), token: @escaping @Sendable () -> String?) {
        let config = ProtocolClientConfig(
            host: baseURL.absoluteString,
            networkProtocol: .connect,
            codec: ProtoCodec(),
            interceptors: [InterceptorFactory { _ in HeaderInterceptor(client: clientHeader, token: token) }]
        )
        let client = ProtocolClient(httpClient: URLSessionHTTPClient(), config: config)
        auth = .init(client: client); guilds = .init(client: client); messages = .init(client: client)
        dms = .init(client: client); roles = .init(client: client); emojis = .init(client: client)
        gateway = .init(client: client); invites = .init(client: client)
    }

    func value<T: ProtobufMessage>(_ r: ResponseMessage<T>) throws -> T {
        switch r.result {
        case .success(let m): return m
        case .failure(let e): throw AppError(e)
        }
    }

    public func me() async throws -> Me {
        Me(try value(await auth.getMe(request: .init(), headers: [:])))
    }

    public func snapshot() async throws -> Snapshot {
        async let me = auth.getMe(request: .init(), headers: [:])
        async let guilds = guilds.listMyGuilds(request: .init(), headers: [:])
        async let dms = dms.listDms(request: .init(), headers: [:])
        let user = User(try value(await me).user)
        let list = try value(await guilds).guilds.map(Guild.init)
        let dmList = try value(await dms).channels.map(DMChannel.init)
        var channels: [Channel] = []
        var layouts: [ID: Layout] = [:]
        try await withThrowingTaskGroup(of: (ID, [Channel], Layout).self) { group in
            for g in list {
                group.addTask { let r = try await self.listChannels(guild: g.id); return (g.id, r.channels, r.layout) }
            }
            for try await (id, c, l) in group { channels += c; layouts[id] = l }
        }
        return Snapshot(me: user, guilds: list, channels: channels, layouts: layouts, dms: dmList)
    }

    public func listChannels(guild: ID) async throws -> (channels: [Channel], layout: Layout) {
        var req = Thehideout_V1_ListChannelsRequest(); req.guildID = guild
        let r = try value(await guilds.listChannels(request: req, headers: [:]))
        return (r.channels.map(Channel.init), Layout(r.layout))
    }

    public func listMembers(guild: ID) async throws -> [Member] {
        var req = Thehideout_V1_ListGuildMembersRequest(); req.guildID = guild
        let r = try value(await guilds.listGuildMembers(request: req, headers: [:]))
        return r.members.map(Member.init)
    }

    public func listRoles(guild: ID) async throws -> (roles: [Role], version: UInt64) {
        var req = Thehideout_V1_ListRolesRequest(); req.guildID = guild
        let r = try value(await roles.listRoles(request: req, headers: [:]))
        return (r.roles.map(Role.init), r.version)
    }

    public func listEmojis(guild: ID) async throws -> [Emoji] {
        var req = Thehideout_V1_ListEmojisRequest(); req.guildID = guild
        return try value(await emojis.listEmojis(request: req, headers: [:])).emojis.map(Emoji.init)
    }

    public func listMessages(channel: ID, before: ID?, limit: Int) async throws -> MessagePage {
        var req = Thehideout_V1_ListMessagesRequest()
        req.channelID = channel; req.limit = UInt32(limit)
        if let before { req.before = before }
        let r = try value(await messages.listMessages(request: req, headers: [:]))
        return MessagePage(messages: r.messages.map(Message.init), nextBefore: r.hasNextBefore ? r.nextBefore : nil, hasMore: r.hasMore_p)
    }

    public func sendMessage(channel: ID, content: String, nonce: String, attachmentIDs: [ID]) async throws -> Message {
        var req = Thehideout_V1_SendMessageRequest()
        req.channelID = channel; req.content = content; req.nonce = nonce; req.attachmentIds = attachmentIDs
        return Message(try value(await messages.sendMessage(request: req, headers: [:])).message)
    }

    public func editMessage(channel: ID, message: ID, content: String) async throws -> Message {
        var req = Thehideout_V1_EditMessageRequest()
        req.channelID = channel; req.messageID = message; req.content = content
        return Message(try value(await messages.editMessage(request: req, headers: [:])).message)
    }

    public func deleteMessage(channel: ID, message: ID) async throws {
        var req = Thehideout_V1_DeleteMessageRequest()
        req.channelID = channel; req.messageID = message
        _ = try value(await messages.deleteMessage(request: req, headers: [:]))
    }

    public func addReaction(channel: ID, message: ID, emoji: ReactionEmoji) async throws {
        var req = Thehideout_V1_AddReactionRequest()
        req.channelID = channel; req.messageID = message; req.emoji = emoji.proto
        _ = try value(await messages.addReaction(request: req, headers: [:]))
    }

    public func removeReaction(channel: ID, message: ID, emoji: ReactionEmoji) async throws {
        var req = Thehideout_V1_RemoveReactionRequest()
        req.channelID = channel; req.messageID = message; req.emoji = emoji.proto
        _ = try value(await messages.removeReaction(request: req, headers: [:]))
    }

    public func updateLayout(guild: ID, expectedVersion: UInt64, sectionIDs: [ID], entries: [LayoutEntry]) async throws -> Layout {
        var req = Thehideout_V1_UpdateLayoutRequest()
        req.guildID = guild; req.expectedVersion = expectedVersion; req.sectionIds = sectionIDs
        req.entries = entries.map { e in
            var x = Thehideout_V1_LayoutEntry(); x.sectionID = e.sectionID ?? 0; x.channelIds = e.channelIDs
            return x
        }
        return Layout(try value(await guilds.updateLayout(request: req, headers: [:])).layout)
    }

    // MARK: gateway

    public func subscribe() -> AsyncThrowingStream<GatewayEvent, Error> {
        let stream = gateway.subscribe(headers: [:])
        return AsyncThrowingStream { continuation in
            let task = Task {
                do {
                    try stream.send(Thehideout_V1_SubscribeRequest())
                    for await result in stream.results() {
                        switch result {
                        // Headers arriving mean the stream is open: say so (the server's heartbeat is up to 30 s away).
                        case .headers: continuation.yield(.heartbeat)
                        case .message(let m):
                            if let e = GatewayEvent(m.event) { continuation.yield(e) }
                        case .complete(let code, let error, _):
                            if let e = error as? ConnectError { continuation.finish(throwing: AppError(e)) }
                            else if code == .ok { continuation.finish() }
                            else { continuation.finish(throwing: AppError.network(error?.localizedDescription ?? "stream ended (\(code))")) }
                            return
                        }
                    }
                    continuation.finish()
                } catch {
                    continuation.finish(throwing: AppError.from(error))
                }
            }
            continuation.onTermination = { _ in task.cancel(); stream.cancel() }
        }
    }
}

extension GatewayEvent {
    /// Maps a wire event; nil for an empty or unknown payload (a newer server's event kind).
    init?(_ e: Thehideout_V1_Event) {
        switch e.payload {
        case .messageCreated(let m): self = .messageCreated(Message(m))
        case .channelCreated(let c): self = .channelCreated(Channel(c))
        case .guildMemberJoined(let j): self = .guildMemberJoined(guild: j.guildID, user: User(j.user))
        case .heartbeat: self = .heartbeat
        case .dmOpened(let d): self = .dmOpened(DMChannel(d))
        case .guildUpdated(let g): self = .guildUpdated(Guild(g))
        case .channelUpdated(let c): self = .channelUpdated(Channel(c))
        case .layoutUpdated(let l): self = .layoutUpdated(guild: l.guildID, layout: Layout(l.layout))
        case .rolesUpdated(let r): self = .rolesUpdated(guild: r.guildID, roles: r.roles.map(Role.init), version: r.version)
        case .memberUpdated(let m): self = .memberUpdated(guild: m.guildID, member: Member(m.member))
        case .userUpdated(let u): self = .userUpdated(User(u))
        case .emojisUpdated(let x): self = .emojisUpdated(guild: x.guildID, emojis: x.emojis.map(Emoji.init))
        case .reactionAdded(let r): self = .reactionAdded(channel: r.channelID, message: r.messageID, emoji: ReactionEmoji(r.emoji), user: r.userID)
        case .reactionRemoved(let r): self = .reactionRemoved(channel: r.channelID, message: r.messageID, emoji: ReactionEmoji(r.emoji), user: r.userID)
        case .messageUpdated(let m): self = .messageUpdated(Message(m.message))
        case .guildAdded(let g): self = .guildAdded(Guild(g))
        case .voiceStateUpdate(let v): self = .voiceStateUpdate(guild: v.guildID, user: v.userID, state: v.hasState ? VoiceState(v.state) : nil)
        case .voiceSnapshot(let v): self = .voiceSnapshot(guilds: v.guildIds, states: v.states.map(VoiceState.init))
        case .voiceMoved(let v): self = .voiceMoved(guild: v.guildID, channel: v.channelID)
        case .messageDeleted(let d): self = .messageDeleted(channel: d.channelID, message: d.messageID)
        case .guildMemberRemoved(let r):
            let reason: MemberRemovalReason
            switch r.reason {
            case .left: reason = .left
            case .kicked: reason = .kicked
            case .banned: reason = .banned
            default: reason = .unspecified
            }
            self = .guildMemberRemoved(guild: r.guildID, user: r.userID, reason: reason)
        case .guildRemoved(let r): self = .guildRemoved(guild: r.guildID)
        case .channelDeleted(let d): self = .channelDeleted(guild: d.guildID, channel: d.channelID)
        case .notificationSettingsUpdated(let s): self = .notificationSettingsUpdated(NotificationSettings(s))
        case nil: return nil
        }
    }
}
