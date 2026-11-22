import Foundation
import Observation

/// One per signed-in session: owns the whole mirror (`AppState`), turns intents into API calls and feeds the
/// replies and gateway events through the reducer. Views read `state` and call intents; no per-screen network code.
///
/// `state` is one observed value, so a view re-evaluates on any change of it; screens derive small view models
/// from it. If that ever shows in profiling, split the hot parts (transcript logs) into their own observables.
@MainActor @Observable
public final class AppStore {
    public private(set) var state = AppState()
    /// Set while `bootstrap` could not reach the server and has nothing cached to show.
    public private(set) var bootstrapError: AppError?
    /// Whether the gateway is meant to run (the app is active).
    public private(set) var active = false
    /// The terms gate: an `acceptTerms` call is in flight / its last error.
    public private(set) var termsBusy = false
    public private(set) var termsError: String?

    @ObservationIgnored private let api: any ApiClient
    @ObservationIgnored private let auth: (any AuthApi)?
    @ObservationIgnored private let tokens: any TokenStore
    @ObservationIgnored private let cache: Cache?
    @ObservationIgnored private let gateway: GatewayStream
    @ObservationIgnored private let now: @Sendable () -> Date
    @ObservationIgnored private var tasks: [UUID: Task<Void, Never>] = [:]

    public init(api: any ApiClient, auth: (any AuthApi)? = nil, tokens: any TokenStore, cache: Cache? = nil,
                gatewayConfig: GatewayConfig = GatewayConfig(), now: @escaping @Sendable () -> Date = { Date() }) {
        self.api = api
        self.auth = auth
        self.tokens = tokens
        self.cache = cache
        self.gateway = GatewayStream(source: api, config: gatewayConfig)
        self.now = now
    }

    // MARK: session

    /// Launch: no token -> signed out; a token -> the cached lists at once, then `GetMe` and the lists from the server.
    public func bootstrap() async {
        bootstrapError = nil
        guard tokens.load() != nil else { state.phase = .signedOut; return }
        if let cache, state.me == nil, let cached = await cache.loadSnapshot() {
            state.applySnapshot(cached)
        }
        await loadSession()
    }

    /// After a sign-in: keeps the token and loads the session.
    public func signedIn(token: String) async {
        try? tokens.save(token)
        state = AppState()
        state.phase = .loading
        await loadSession()
    }

    private func loadSession() async {
        do {
            let me = try await api.me()
            if let version = me.termsUpdateRequired { state.phase = .termsRequired(version: version); return }
            let snapshot = try await api.snapshot()
            state.applySnapshot(snapshot)
            if active { startGateway() }
            drain()
            if let cache { Task { await cache.saveSnapshot(snapshot) } }
        } catch {
            let e = AppError.from(error)
            switch e {
            case .unauthenticated: signOut()
            case .clientTooOld: state.phase = .updateRequired
            case .suspended(let reason, let until): state.phase = .suspended(reason: reason, until: until)
            default:
                // Offline with a cache: keep showing it. Without one: the loading screen offers a retry.
                if state.me != nil { state.phase = .signedIn; if active { startGateway() } } else { bootstrapError = e }
            }
        }
    }

    /// The terms gate's "Accept and continue": accepts `version`, then loads the session.
    public func acceptTerms() async {
        guard case .termsRequired(let version) = state.phase, let auth, !termsBusy else { return }
        termsBusy = true; termsError = nil
        defer { termsBusy = false }
        do {
            try await auth.acceptTerms(version: version)
            await loadSession()
        } catch {
            switch AppError.from(error) {
            case .unauthenticated: signOut()
            case .clientTooOld: state.phase = .updateRequired
            case .precondition:
                // The terms moved on while the prompt was up: look again.
                termsError = "The terms were updated again. Please read the new version."
                await loadSession()
            case .network: termsError = "Can't reach the server. Check your connection and try again."
            case let e: termsError = sentence(e.description)
            }
        }
    }

    /// An auth call found the server refuses this app version.
    public func requireUpdate() { state.phase = .updateRequired }

    /// Clears the token, the cache and every bit of session state.
    public func signOut() {
        tokens.clear()
        let cache = self.cache, gateway = self.gateway
        Task { await cache?.clear() }
        Task { await gateway.stop() }
        for t in tasks.values { t.cancel() }
        tasks.removeAll()
        state = AppState()
        state.phase = .signedOut
        termsError = nil
    }

    // MARK: lifecycle

    /// The app became active (`scenePhase`): the stream runs only then. On foreground it reconnects and refetches.
    public func setActive(_ active: Bool) {
        guard self.active != active else { return }
        self.active = active
        if active {
            if state.phase == .signedIn {
                // Whatever happened while away was not delivered: refetch on the way back.
                if state.connection != .connecting { state.stream = .interrupted }
                startGateway()
            }
        } else {
            let gateway = self.gateway
            Task { await gateway.stop() }
            state.connection = .connecting
        }
    }

    private func startGateway() {
        let gateway = self.gateway
        let me = WeakStore(self)
        Task { await gateway.start { update in await me.store?.receive(update) } }
    }

    func receive(_ update: GatewayUpdate) {
        if case .authExpired = update { signOut(); return }
        state.applyUpdate(update)
        drain()
    }

    // MARK: intents

    public func enterGuild(_ id: ID) { mutate { $0.enterGuild(id) } }
    public func goHome() { mutate { $0.goHome() } }
    public func open(channel: ID) { mutate { $0.open(channel: channel) } }
    public func closeChannel() { mutate { $0.closeChannel() } }
    public func loadOlder(_ channel: ID) { mutate { $0.loadOlder(channel) } }
    public func reloadOpen() { mutate { $0.reloadOpen() } }
    public func toggleSection(guild: ID, section: ID) { mutate { $0.toggleSection(guild: guild, section: section) } }
    public func dismissNotice(_ id: Int) { mutate { $0.dismissNotice(id) } }
    public func retry(nonce: String) { mutate { $0.retry(nonce: nonce) } }
    public func discard(nonce: String) { mutate { $0.discard(nonce: nonce) } }
    public func editLayout(guild: ID, _ layout: Layout) { mutate { _ = $0.editLayout(guild: guild, layout) } }
    public func setDraft(channel: ID, text: String) { mutate { $0.setDraft(channel: channel, text: text) } }
    public func startDm(user: ID) { mutate { $0.startDm(user: user) } }

    @discardableResult
    public func editMessage(channel: ID, message: ID, content: String) -> Bool {
        var ok = false
        mutate { ok = $0.editMessage(channel: channel, message: message, content: content) }
        return ok
    }

    @discardableResult
    public func send(_ text: String, to channel: ID, attachmentIDs: [ID] = []) -> Bool {
        var ok = false
        mutate { ok = $0.send(text, to: channel, at: now(), attachmentIDs: attachmentIDs) }
        return ok
    }

    /// Adds or removes your reaction at once; the server's answer only matters when it fails.
    public func react(channel: ID, message: ID, emoji: ReactionEmoji, on: Bool) {
        guard state.setMyReaction(channel: channel, message: message, emoji: emoji, on: on) else { return }
        run { store in
            do {
                if on { try await store.api.addReaction(channel: channel, message: message, emoji: emoji) }
                else { try await store.api.removeReaction(channel: channel, message: message, emoji: emoji) }
            } catch {
                store.state.setMyReaction(channel: channel, message: message, emoji: emoji, on: !on)
                store.state.notice(.danger, "Couldn't \(on ? "add" : "remove") the reaction. \(sentence(AppError.from(error).description))")
            }
        }
    }

    public func deleteMessage(channel: ID, message: ID) {
        run { store in
            do { try await store.api.deleteMessage(channel: channel, message: message) }
            catch { store.state.notice(.danger, "Couldn't delete the message. \(sentence(AppError.from(error).description))") }
        }
    }

    // MARK: effects

    private func mutate(_ f: (inout AppState) -> Void) {
        f(&state)
        drain()
    }

    private func drain() {
        let effects = state.outbox
        state.outbox.removeAll()
        for e in effects { perform(e) }
    }

    /// Runs `body` as a tracked task (so tests can `settle()`).
    private func run(_ body: @escaping @MainActor (AppStore) async -> Void) {
        let id = UUID()
        tasks[id] = Task { [weak self] in
            guard let self else { return }
            await body(self)
            self.tasks[id] = nil
            self.drain()
        }
    }

    /// Waits for every call in flight (and the calls their answers started). For tests.
    public func settle() async {
        while let (id, t) = tasks.first { await t.value; tasks[id] = nil }
    }

    private func handleFailure(_ error: Error) -> AppError {
        let e = AppError.from(error)
        switch e {
        case .unauthenticated: signOut()
        case .clientTooOld: state.phase = .updateRequired
        default: break
        }
        return e
    }

    private func perform(_ effect: Effect) {
        switch effect {
        case .loadMessages(let channel, let before):
            run { store in
                if before == nil, let cache = store.cache, store.state.logs[channel]?.messages.isEmpty == true,
                   let cached = await cache.loadMessages(channel: channel) {
                    // Shown until the first answer replaces it.
                    store.state.logs[channel]?.messages = cached
                    store.state.logs[channel]?.oldestID = cached.first?.id
                }
                do {
                    let page = try await store.api.listMessages(channel: channel, before: before, limit: AppState.pageSize)
                    store.state.applyMessages(channel: channel, before: before, result: .success(page))
                    if let cache = store.cache, let log = store.state.logs[channel] {
                        let messages = log.messages
                        Task { await cache.saveMessages(messages, channel: channel) }
                    }
                } catch {
                    let e = store.handleFailure(error)
                    store.state.applyMessages(channel: channel, before: before, result: .failure(e))
                }
            }
        case .refresh:
            run { store in
                do {
                    let snapshot = try await store.api.snapshot()
                    store.state.applyRefreshed(snapshot)
                    if let cache = store.cache { Task { await cache.saveSnapshot(snapshot) } }
                } catch { _ = store.handleFailure(error) }
            }
        case .loadChannels(let guild):
            run { store in
                do {
                    let r = try await store.api.listChannels(guild: guild)
                    store.state.applyChannels(guild: guild, channels: r.channels, layout: r.layout)
                } catch { _ = store.handleFailure(error) }
            }
        case .loadMembers(let guild):
            run { store in
                do {
                    let list = try await store.api.listMembers(guild: guild)
                    store.state.applyMembers(guild: guild, members: list)
                } catch {
                    // A refused list may be asked for again on the next visit.
                    store.state.membersAsked.remove(guild)
                    _ = store.handleFailure(error)
                }
            }
        case .loadAccess(let guild):
            run { store in
                do {
                    let r = try await store.api.listRoles(guild: guild)
                    store.state.applyRoles(guild: guild, roles: r.roles, version: r.version)
                    // Role assignments come with the member list.
                    store.state.wantMembers(guild)
                } catch {
                    store.state.accessAsked.remove(guild)
                    _ = store.handleFailure(error)
                }
            }
        case .loadEmojis(let guild):
            run { store in
                do {
                    let list = try await store.api.listEmojis(guild: guild)
                    store.state.applyEmojis(guild: guild, emojis: list)
                } catch {
                    store.state.emojisAsked.remove(guild)
                    _ = store.handleFailure(error)
                }
            }
        case .send(let channel, let content, let nonce, let ids):
            run { store in
                do {
                    let m = try await store.api.sendMessage(channel: channel, content: content, nonce: nonce, attachmentIDs: ids)
                    store.state.applySent(channel: channel, nonce: nonce, result: .success(m))
                } catch {
                    let e = store.handleFailure(error)
                    store.state.applySent(channel: channel, nonce: nonce, result: .failure(e))
                }
            }
        case .updateLayout(let guild, let layout):
            run { store in
                do {
                    let l = try await store.api.updateLayout(guild: guild, expectedVersion: layout.version, sectionIDs: layout.sections.map(\.id), entries: layout.entries)
                    store.state.applyLayoutReply(guild: guild, result: .success(l))
                } catch {
                    let e = store.handleFailure(error)
                    store.state.applyLayoutReply(guild: guild, result: .failure(e))
                }
            }
        case .editMessage(let channel, let message, let content, let previous):
            run { store in
                do {
                    let m = try await store.api.editMessage(channel: channel, message: message, content: content)
                    store.state.applyEditReply(channel: channel, message: message, previous: previous, result: .success(m))
                } catch {
                    let e = store.handleFailure(error)
                    store.state.applyEditReply(channel: channel, message: message, previous: previous, result: .failure(e))
                }
            }
        case .startDm(let user):
            run { store in
                do {
                    let dm = try await store.api.startDm(user: user)
                    store.state.applyDmOpened(result: .success(dm))
                } catch {
                    let e = store.handleFailure(error)
                    store.state.applyDmOpened(result: .failure(e))
                }
            }
        }
    }
}

/// The gateway's handler outlives a single call; it must not keep the store alive.
private final class WeakStore: @unchecked Sendable {
    weak var store: AppStore?
    init(_ store: AppStore) { self.store = store }
}
