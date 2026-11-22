import Foundation

/// A signed-in session: the bearer token and the account it belongs to.
public struct AuthSession: Sendable, Hashable {
    public var token: String
    public var user: User
    public init(token: String, user: User) { self.token = token; self.user = user }
}

public enum SecondFactorMethod: Sendable, Hashable { case totp, recoveryCode, passkey }

/// A password or email sign-in that needs one more step.
public struct SecondFactorChallenge: Sendable, Hashable {
    public var id: String
    public var methods: [SecondFactorMethod]
    public var expiresAt: Date?
    public init(id: String, methods: [SecondFactorMethod], expiresAt: Date? = nil) {
        self.id = id; self.methods = methods; self.expiresAt = expiresAt
    }
}

/// What a first-factor sign-in (password or emailed code) answers.
public enum SignInResult: Sendable, Hashable {
    case session(AuthSession)
    case secondFactor(SecondFactorChallenge)
}

/// A started passkey ceremony. `optionsJSON` is the WebAuthn `PublicKeyCredentialRequestOptions` /
/// `CreationOptions` (webauthn-rs: `{"publicKey": {...}}`); `url` is the browser page for the same ceremony.
public struct PasskeyCeremony: Sendable, Hashable {
    public var id: String
    public var url: URL?
    public var optionsJSON: String
    public init(id: String, url: URL? = nil, optionsJSON: String) { self.id = id; self.url = url; self.optionsJSON = optionsJSON }
}

/// The answer of a passkey `Finish*` call: still waiting for the browser page, or done.
public enum PasskeyOutcome: Sendable, Hashable {
    case pending
    case session(AuthSession)
}

public enum InvitePreviewStatus: Sendable, Hashable { case pending, accepted, expired, revoked, unknown }

/// What `LookupInvite` shows before an account is made.
public struct InvitePreview: Sendable, Hashable {
    public var maskedEmail: String
    public var inviter: String?
    public var guildNames: [String]
    public var expiresAt: Date
    public var status: InvitePreviewStatus
    public var termsVersion: String
    public var minimumAge: Int
    public init(maskedEmail: String, inviter: String? = nil, guildNames: [String] = [], expiresAt: Date, status: InvitePreviewStatus = .pending, termsVersion: String, minimumAge: Int = 16) {
        self.maskedEmail = maskedEmail; self.inviter = inviter; self.guildNames = guildNames; self.expiresAt = expiresAt
        self.status = status; self.termsVersion = termsVersion; self.minimumAge = minimumAge
    }
}

/// The unauthenticated calls of the sign-in flows, plus the few authenticated ones that belong to onboarding
/// (accepting updated terms, adding the first passkey). `ConnectApiClient` is the real one.
public protocol AuthApi: Sendable {
    func login(username: String, password: String, deviceName: String) async throws -> SignInResult
    func requestEmailCode(email: String) async throws
    func verifyEmailCode(email: String, code: String, deviceName: String) async throws -> SignInResult
    func completeSecondFactor(challenge: String, totpCode: String?, recoveryCode: String?) async throws -> AuthSession
    func beginPasskeyLogin(username: String, deviceName: String) async throws -> PasskeyCeremony
    /// An empty `credentialJSON` asks for the result the ceremony page stored (`.pending` until it has one).
    func finishPasskeyLogin(ceremony: String, credentialJSON: String) async throws -> PasskeyOutcome
    func beginSecondFactorPasskey(challenge: String) async throws -> PasskeyCeremony
    func finishSecondFactorPasskey(ceremony: String, credentialJSON: String) async throws -> PasskeyOutcome
    func requestPasswordReset(email: String) async throws
    func resetPassword(email: String, code: String, newPassword: String) async throws
    func lookupInvite(code: String) async throws -> InvitePreview
    func acceptInvite(code: String, username: String, password: String, deviceName: String, termsVersion: String, ageConfirmed: Bool) async throws -> AuthSession
    func acceptTerms(version: String) async throws
    func beginPasskeyRegistration(name: String) async throws -> PasskeyCeremony
    /// Returns whether the ceremony page has not finished yet (poll again).
    func finishPasskeyRegistration(ceremony: String, credentialJSON: String) async throws -> Bool
}

/// The platform half of a passkey ceremony (AuthenticationServices and a browser sheet). Implemented by the UI layer.
@MainActor
public protocol PasskeyPlatform: AnyObject {
    /// Whether the system passkey UI can serve `host` as relying party (it needs `webcredentials:` associated domains,
    /// which a dev server on localhost or an IP cannot have).
    func canUseNative(host: String) -> Bool
    /// Signs `optionsJSON` with a passkey: the `PublicKeyCredential` JSON the server expects.
    func assertion(optionsJSON: String) async throws -> String
    func registration(optionsJSON: String) async throws -> String
    /// The browser fallback: shows the ceremony page until `closeCeremonyPage`; `onDismiss` runs when the person closes it.
    func openCeremonyPage(_ url: URL, onDismiss: @escaping @MainActor () -> Void)
    func closeCeremonyPage()
}

public enum PasskeyFailure: Error, Equatable {
    /// The user dismissed the sheet.
    case cancelled
    /// The system could not do it for this server (no associated domain, no passkeys on the device): use the browser.
    case unavailable(String)
    case failed(String)
}
