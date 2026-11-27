import Foundation
import Observation

/// Parsing of what a person types into the "Server" field.
public enum ServerURL {
    public static let release = "https://thehideout.zabit.si"
    public static let debug = "http://127.0.0.1:8080"

    #if DEBUG
    public static let `default` = debug
    #else
    public static let `default` = release
    #endif

    /// A base URL from typed text: trimmed, `https://` added when no scheme is given, path and query dropped.
    /// Plain `http://` is accepted only for localhost, `.local` names and private addresses (the same set ATS lets through).
    public static func parse(_ raw: String) -> URL? {
        var text = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty, !text.contains(" ") else { return nil }
        if !text.contains("://") { text = (isLocal(hostOf: text) ? "http://" : "https://") + text }
        guard var parts = URLComponents(string: text), let scheme = parts.scheme?.lowercased(), let host = parts.host, !host.isEmpty else { return nil }
        guard scheme == "https" || (scheme == "http" && isLocal(host: host)) else { return nil }
        parts.scheme = scheme
        parts.path = ""; parts.query = nil; parts.fragment = nil; parts.user = nil; parts.password = nil
        return parts.url
    }

    /// "thehideout.zabit.si" for display (the port stays when it is not the default).
    public static func display(_ url: URL) -> String {
        var s = url.absoluteString
        while s.hasSuffix("/") { s.removeLast() }
        return s
    }

    /// The host part of typed text, for the local-vs-domain check before a scheme is added. A bracketed IPv6
    /// literal (`[::1]:8080`) has colons of its own, so it cannot just split on the first `:`.
    private static func hostOf(_ text: String) -> String {
        let h = text.split(separator: "/", maxSplits: 1).first.map(String.init) ?? text
        if h.hasPrefix("["), let end = h.firstIndex(of: "]") { return String(h[h.index(after: h.startIndex)..<end]) }
        return h.split(separator: ":").first.map(String.init) ?? h
    }

    private static func isLocal(hostOf text: String) -> Bool { isLocal(host: hostOf(text)) }

    /// localhost, `*.local`, loopback and the private IPv4 ranges. `host` may be `URLComponents.host` for a
    /// bracketed IPv6 literal, which keeps the brackets (`[::1]`, unlike a browser's unbracketed `::1`).
    public static func isLocal(host: String) -> Bool {
        var h = host.lowercased()
        if h.hasPrefix("["), h.hasSuffix("]") { h = String(h.dropFirst().dropLast()) }
        if h == "localhost" || h.hasSuffix(".local") || h == "::1" { return true }
        let parts = h.split(separator: ".").compactMap { Int($0) }
        guard parts.count == 4, parts.allSatisfy({ (0...255).contains($0) }) else { return false }
        switch (parts[0], parts[1]) {
        case (127, _), (10, _), (192, 168): return true
        case (172, 16...31): return true
        case (169, 254): return true
        default: return false
        }
    }

    /// Whether the host is a domain name (a relying party the system passkey UI can serve), not localhost or an IP.
    public static func isDomain(host: String) -> Bool {
        if isLocal(host: host) { return false }
        let parts = host.split(separator: ".")
        if parts.count == 4, parts.allSatisfy({ Int($0) != nil }) { return false }
        return host.contains(".") && !host.contains(":")
    }
}

/// An `ApiClient` + `AuthApi` that forwards to a replaceable backend, so the server can change at runtime.
public final class SwitchingBackend: ApiClient, AuthApi, @unchecked Sendable {
    public typealias Backend = ApiClient & AuthApi
    private let lock = NSLock()
    private var current: any Backend

    public init(_ initial: any Backend) { current = initial }

    func replace(_ next: any Backend) { lock.withLock { current = next } }
    private var b: any Backend { lock.withLock { current } }

    public func me() async throws -> Me { try await b.me() }
    public func snapshot() async throws -> Snapshot { try await b.snapshot() }
    public func listChannels(guild: ID) async throws -> (channels: [Channel], layout: Layout) { try await b.listChannels(guild: guild) }
    public func listMembers(guild: ID) async throws -> [Member] { try await b.listMembers(guild: guild) }
    public func listRoles(guild: ID) async throws -> (roles: [Role], version: UInt64) { try await b.listRoles(guild: guild) }
    public func listEmojis(guild: ID) async throws -> [Emoji] { try await b.listEmojis(guild: guild) }
    public func listMessages(channel: ID, before: ID?, limit: Int) async throws -> MessagePage { try await b.listMessages(channel: channel, before: before, limit: limit) }
    public func sendMessage(channel: ID, content: String, nonce: String, attachmentIDs: [ID]) async throws -> Message { try await b.sendMessage(channel: channel, content: content, nonce: nonce, attachmentIDs: attachmentIDs) }
    public func editMessage(channel: ID, message: ID, content: String) async throws -> Message { try await b.editMessage(channel: channel, message: message, content: content) }
    public func deleteMessage(channel: ID, message: ID) async throws { try await b.deleteMessage(channel: channel, message: message) }
    public func addReaction(channel: ID, message: ID, emoji: ReactionEmoji) async throws { try await b.addReaction(channel: channel, message: message, emoji: emoji) }
    public func removeReaction(channel: ID, message: ID, emoji: ReactionEmoji) async throws { try await b.removeReaction(channel: channel, message: message, emoji: emoji) }
    public func updateLayout(guild: ID, expectedVersion: UInt64, sectionIDs: [ID], entries: [LayoutEntry]) async throws -> Layout { try await b.updateLayout(guild: guild, expectedVersion: expectedVersion, sectionIDs: sectionIDs, entries: entries) }
    public func startDm(user: ID) async throws -> DMChannel { try await b.startDm(user: user) }
    public func uploadAttachment(channel: ID, filename: String, contentType: String, size: UInt64, fileURL: URL, progress: @escaping @Sendable (UInt64) -> Void) async throws -> PendingAttachment {
        try await b.uploadAttachment(channel: channel, filename: filename, contentType: contentType, size: size, fileURL: fileURL, progress: progress)
    }
    public func subscribe() -> AsyncThrowingStream<GatewayEvent, Error> { b.subscribe() }

    public func login(username: String, password: String, deviceName: String) async throws -> SignInResult { try await b.login(username: username, password: password, deviceName: deviceName) }
    public func requestEmailCode(email: String) async throws { try await b.requestEmailCode(email: email) }
    public func verifyEmailCode(email: String, code: String, deviceName: String) async throws -> SignInResult { try await b.verifyEmailCode(email: email, code: code, deviceName: deviceName) }
    public func completeSecondFactor(challenge: String, totpCode: String?, recoveryCode: String?) async throws -> AuthSession { try await b.completeSecondFactor(challenge: challenge, totpCode: totpCode, recoveryCode: recoveryCode) }
    public func beginPasskeyLogin(username: String, deviceName: String) async throws -> PasskeyCeremony { try await b.beginPasskeyLogin(username: username, deviceName: deviceName) }
    public func finishPasskeyLogin(ceremony: String, credentialJSON: String) async throws -> PasskeyOutcome { try await b.finishPasskeyLogin(ceremony: ceremony, credentialJSON: credentialJSON) }
    public func beginSecondFactorPasskey(challenge: String) async throws -> PasskeyCeremony { try await b.beginSecondFactorPasskey(challenge: challenge) }
    public func finishSecondFactorPasskey(ceremony: String, credentialJSON: String) async throws -> PasskeyOutcome { try await b.finishSecondFactorPasskey(ceremony: ceremony, credentialJSON: credentialJSON) }
    public func requestPasswordReset(email: String) async throws { try await b.requestPasswordReset(email: email) }
    public func resetPassword(email: String, code: String, newPassword: String) async throws { try await b.resetPassword(email: email, code: code, newPassword: newPassword) }
    public func lookupInvite(code: String) async throws -> InvitePreview { try await b.lookupInvite(code: code) }
    public func acceptInvite(code: String, username: String, password: String, deviceName: String, termsVersion: String, ageConfirmed: Bool) async throws -> AuthSession { try await b.acceptInvite(code: code, username: username, password: password, deviceName: deviceName, termsVersion: termsVersion, ageConfirmed: ageConfirmed) }
    public func acceptTerms(version: String) async throws { try await b.acceptTerms(version: version) }
    public func beginPasskeyRegistration(name: String) async throws -> PasskeyCeremony { try await b.beginPasskeyRegistration(name: name) }
    public func finishPasskeyRegistration(ceremony: String, credentialJSON: String) async throws -> Bool { try await b.finishPasskeyRegistration(ceremony: ceremony, credentialJSON: credentialJSON) }
}

/// The server the app talks to: persisted, validated, and switched under the running backend.
@MainActor @Observable
public final class ServerSelection {
    public private(set) var url: URL
    public let backend: SwitchingBackend

    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private let key: String
    @ObservationIgnored private let make: @Sendable (URL) -> any SwitchingBackend.Backend
    @ObservationIgnored private let probe: @Sendable (URL) async -> Bool

    public static let defaultsKey = "server_url"

    /// `make` builds the real client for a URL (it reads the token itself); `probe` says whether a server answers there.
    public init(defaults: UserDefaults = .standard, key: String = ServerSelection.defaultsKey,
                make: @escaping @Sendable (URL) -> any SwitchingBackend.Backend,
                probe: @escaping @Sendable (URL) async -> Bool = ServerSelection.reachable) {
        self.defaults = defaults; self.key = key; self.make = make; self.probe = probe
        let saved = defaults.string(forKey: key).flatMap(ServerURL.parse) ?? ServerURL.parse(ServerURL.default)!
        url = saved
        backend = SwitchingBackend(make(saved))
    }

    /// Fixed server for previews and `--fake`: nothing to switch.
    public init(fixed backend: any SwitchingBackend.Backend, url: URL = URL(string: "http://127.0.0.1:8080")!, defaults: UserDefaults = UserDefaults(suiteName: "fake-server")!) {
        self.defaults = defaults; key = ServerSelection.defaultsKey; self.url = url
        self.make = { _ in backend }
        self.probe = { _ in true }
        self.backend = SwitchingBackend(backend)
    }

    public var host: String { url.host ?? "" }

    public enum ChangeError: Error, Equatable {
        case invalid
        case unreachable
        public var message: String {
            switch self {
            case .invalid: "That isn't a server address. Use https://, or http:// for a server on your own network."
            case .unreachable: "Can't reach a server there. Check the address and your connection."
            }
        }
    }

    /// Validates, probes and switches to `raw`; nothing changes on failure.
    public func change(to raw: String) async throws {
        guard let next = ServerURL.parse(raw) else { throw ChangeError.invalid }
        if next == url { return }
        guard await probe(next) else { throw ChangeError.unreachable }
        apply(next)
    }

    public func reset() { apply(ServerURL.parse(ServerURL.default)!) }

    private func apply(_ next: URL) {
        url = next
        defaults.set(next.absoluteString, forKey: key)
        backend.replace(make(next))
    }

    /// Any HTTP answer from `HEAD /` counts as a server.
    public static let reachable: @Sendable (URL) async -> Bool = { url in
        var req = URLRequest(url: url, timeoutInterval: 6)
        req.httpMethod = "HEAD"
        do { _ = try await URLSession.shared.data(for: req); return true } catch { return false }
    }
}
