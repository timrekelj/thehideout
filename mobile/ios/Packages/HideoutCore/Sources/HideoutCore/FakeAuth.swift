import Foundation

/// The sign-in side of the fake world. Demo accounts:
///
/// | sign in with | result |
/// |---|---|
/// | `tim` or `tim@example.com` + `FakeAccounts.password` | signed in |
/// | `ana` + the password | asks for a second factor: TOTP `FakeAccounts.totp`, recovery `FakeAccounts.recovery`, or a passkey |
/// | `cvetka` + the password | suspended |
/// | an emailed code | `FakeAccounts.emailCode` for `tim@example.com` / `ana@example.com` |
/// | invite code `WXYZ-2345` | a pending invite from ana (`USED-0000`: already used) |
public final class FakeAccounts: @unchecked Sendable {
    public static let password = "correct-horse"
    public static let totp = "123456"
    public static let recovery = "abcde-fghij"
    public static let emailCode = "424242"
    public static let inviteCode = "WXYZ-2345"
    public static let usedInviteCode = "USED-0000"
    public static let challenge = "challenge-1"

    private let lock = NSLock()
    private var _password = FakeAccounts.password
    private var wrong = 0
    private var failNetwork = 0
    private var rateLimited = 0
    /// How many polls of a browser passkey ceremony answer `pending` before it completes.
    private var _pendingPolls = 2
    private var polls = 0
    public var termsRequired: String? {
        get { lock.withLock { _terms } }
        set { lock.withLock { _terms = newValue } }
    }
    private var _terms: String?
    /// The calls made, in order (`"login:tim"`, …), for tests.
    public var log: [String] { lock.withLock { _log } }
    private var _log: [String] = []

    /// "Now" for expiry dates (the fake world's clock).
    public var now = Date()

    public init() {}

    /// `me()` answers "suspended" (a signed-in session whose account was suspended meanwhile).
    public var suspendMe: Bool {
        get { lock.withLock { _suspendMe } }
        set { lock.withLock { _suspendMe = newValue } }
    }
    private var _suspendMe = false

    /// `me()` answers "client too old" (the update-required gate).
    public var tooOld: Bool {
        get { lock.withLock { _tooOld } }
        set { lock.withLock { _tooOld = newValue } }
    }
    private var _tooOld = false

    public var pendingPolls: Int {
        get { lock.withLock { _pendingPolls } }
        set { lock.withLock { _pendingPolls = newValue } }
    }

    /// The next `acceptTerms` call throws this instead of succeeding (for `AppStore.acceptTerms`'s error branches).
    public var nextAcceptTermsError: AppError? {
        get { lock.withLock { _nextAcceptTermsError } }
        set { lock.withLock { _nextAcceptTermsError = newValue } }
    }
    private var _nextAcceptTermsError: AppError?

    func record(_ s: String) { lock.withLock { _log.append(s) } }

    func currentPassword() -> String { lock.withLock { _password } }
    func setPassword(_ p: String) { lock.withLock { _password = p } }

    /// Counts a wrong second-factor answer: the tries left, or a lockout after five.
    func wrongAnswer() -> AppError {
        lock.withLock {
            wrong += 1
            return wrong >= 5 ? .tooManyAttempts : .wrongCode(attemptsRemaining: 5 - wrong)
        }
    }
    func resetWrong() { lock.withLock { wrong = 0 } }

    func nextPoll() -> Bool { lock.withLock { polls += 1; return polls > _pendingPolls } }
    func resetPolls() { lock.withLock { polls = 0 } }

    public func failNext(network n: Int) { lock.withLock { failNetwork = n } }
    public func rateLimitNext(_ n: Int) { lock.withLock { rateLimited = n } }

    func gate() throws {
        try lock.withLock {
            if failNetwork > 0 { failNetwork -= 1; throw AppError.network("offline") }
            if rateLimited > 0 { rateLimited -= 1; throw AppError.rateLimited(retryAfter: 120) }
        }
    }
}

extension FakeBackend: AuthApi {
    private func user(named name: String) -> User? {
        let lower = name.lowercased()
        let map = ["tim": Ids.tim, "tim@example.com": Ids.tim, "ana": Ids.ana, "ana@example.com": Ids.ana, "cvetka": Ids.cvetka, "bruno": Ids.bruno]
        return map[lower].map(userByID)
    }

    private func userByID(_ id: ID) -> User {
        let names = [Ids.tim: "tim", Ids.ana: "ana", Ids.bruno: "bruno", Ids.cvetka: "cvetka"]
        return User(id: id, username: names[id] ?? "user\(id)")
    }

    private func session(_ u: User) -> AuthSession { AuthSession(token: "fake-\(u.username)", user: u) }

    public func login(username: String, password: String, deviceName: String) async throws -> SignInResult {
        try await delay(); try accounts.gate()
        accounts.record("login:\(username)")
        guard let u = user(named: username.trimmingCharacters(in: .whitespaces)), password == accounts.currentPassword() else { throw AppError.unauthenticated }
        if u.id == Ids.cvetka { throw AppError.suspended(reason: "Repeated spam in public channels.", until: accounts.now.addingTimeInterval(7 * 86_400)) }
        if u.id == Ids.ana {
            accounts.resetWrong()
            return .secondFactor(SecondFactorChallenge(id: FakeAccounts.challenge, methods: [.totp, .recoveryCode, .passkey], expiresAt: accounts.now.addingTimeInterval(300)))
        }
        return .session(session(u))
    }

    public func requestEmailCode(email: String) async throws {
        try await delay(); try accounts.gate()
        accounts.record("requestEmailCode:\(email)")
    }

    public func verifyEmailCode(email: String, code: String, deviceName: String) async throws -> SignInResult {
        try await delay(); try accounts.gate()
        accounts.record("verifyEmailCode:\(email)")
        guard code == FakeAccounts.emailCode, let u = user(named: email) else { throw AppError.unauthenticated }
        return .session(session(u))
    }

    public func completeSecondFactor(challenge: String, totpCode: String?, recoveryCode: String?) async throws -> AuthSession {
        try await delay(); try accounts.gate()
        accounts.record("secondFactor:\(totpCode ?? recoveryCode ?? "")")
        guard challenge == FakeAccounts.challenge else { throw AppError.codeExpired }
        if totpCode == FakeAccounts.totp || recoveryCode == FakeAccounts.recovery { return session(userByID(Ids.ana)) }
        throw accounts.wrongAnswer()
    }

    public func beginPasskeyLogin(username: String, deviceName: String) async throws -> PasskeyCeremony {
        try await delay(); try accounts.gate()
        accounts.record("beginPasskeyLogin:\(username)")
        guard user(named: username) != nil else { throw AppError.notFound }
        accounts.resetPolls()
        return PasskeyCeremony(id: "ceremony-login", url: URL(string: "http://127.0.0.1:8080/auth/passkey?ceremony=ceremony-login"), optionsJSON: "{\"publicKey\":{}}")
    }

    public func finishPasskeyLogin(ceremony: String, credentialJSON: String) async throws -> PasskeyOutcome {
        try await delay(); try accounts.gate()
        accounts.record("finishPasskeyLogin:\(credentialJSON.isEmpty ? "poll" : "credential")")
        if credentialJSON.isEmpty && !accounts.nextPoll() { return .pending }
        return .session(session(userByID(Ids.tim)))
    }

    public func beginSecondFactorPasskey(challenge: String) async throws -> PasskeyCeremony {
        try await delay(); try accounts.gate()
        accounts.record("beginSecondFactorPasskey")
        accounts.resetPolls()
        return PasskeyCeremony(id: "ceremony-2fa", url: URL(string: "http://127.0.0.1:8080/auth/passkey?ceremony=ceremony-2fa"), optionsJSON: "{\"publicKey\":{}}")
    }

    public func finishSecondFactorPasskey(ceremony: String, credentialJSON: String) async throws -> PasskeyOutcome {
        try await delay(); try accounts.gate()
        accounts.record("finishSecondFactorPasskey:\(credentialJSON.isEmpty ? "poll" : "credential")")
        if credentialJSON.isEmpty && !accounts.nextPoll() { return .pending }
        return .session(session(userByID(Ids.ana)))
    }

    public func requestPasswordReset(email: String) async throws {
        try await delay(); try accounts.gate()
        accounts.record("requestPasswordReset:\(email)")
    }

    public func resetPassword(email: String, code: String, newPassword: String) async throws {
        try await delay(); try accounts.gate()
        accounts.record("resetPassword:\(email)")
        guard code == FakeAccounts.emailCode else { throw AppError.unauthenticated }
        accounts.setPassword(newPassword)
    }

    public func lookupInvite(code: String) async throws -> InvitePreview {
        try await delay(); try accounts.gate()
        accounts.record("lookupInvite:\(code)")
        let normalized = code.uppercased()
        let expires = accounts.now.addingTimeInterval(5 * 86_400)
        switch normalized {
        case FakeAccounts.inviteCode:
            return InvitePreview(maskedEmail: "j***@example.com", inviter: "ana", guildNames: ["The Hideout"], expiresAt: expires, termsVersion: "2026-01-01", minimumAge: 16)
        case FakeAccounts.usedInviteCode:
            return InvitePreview(maskedEmail: "j***@example.com", inviter: "ana", guildNames: ["The Hideout"], expiresAt: expires, status: .accepted, termsVersion: "2026-01-01")
        default:
            throw AppError.notFound
        }
    }

    public func acceptInvite(code: String, username: String, password: String, deviceName: String, termsVersion: String, ageConfirmed: Bool) async throws -> AuthSession {
        try await delay(); try accounts.gate()
        accounts.record("acceptInvite:\(username)")
        guard code.uppercased() == FakeAccounts.inviteCode else { throw AppError.notFound }
        guard ageConfirmed, !termsVersion.isEmpty else { throw AppError.invalidArgument("you must accept the terms and confirm your age") }
        if user(named: username) != nil { throw AppError.alreadyExists }
        // The fake world is always signed in as tim; a new account lands in it.
        return AuthSession(token: "fake-\(username)", user: userByID(Ids.tim))
    }

    public func acceptTerms(version: String) async throws {
        try await delay(); try accounts.gate()
        if let e = accounts.nextAcceptTermsError { accounts.nextAcceptTermsError = nil; throw e }
        accounts.record("acceptTerms:\(version)")
        accounts.termsRequired = nil
    }

    public func beginPasskeyRegistration(name: String) async throws -> PasskeyCeremony {
        try await delay(); try accounts.gate()
        accounts.record("beginPasskeyRegistration")
        accounts.resetPolls()
        return PasskeyCeremony(id: "ceremony-register", url: URL(string: "http://127.0.0.1:8080/auth/passkey?ceremony=ceremony-register"), optionsJSON: "{\"publicKey\":{}}")
    }

    public func finishPasskeyRegistration(ceremony: String, credentialJSON: String) async throws -> Bool {
        try await delay(); try accounts.gate()
        accounts.record("finishPasskeyRegistration:\(credentialJSON.isEmpty ? "poll" : "credential")")
        return credentialJSON.isEmpty && !accounts.nextPoll()
    }
}
