import Connect
import Foundation
import HideoutAPI

/// `AuthApi` over the Connect clients of `ConnectApiClient` (the same base URL, headers and token).
extension ConnectApiClient: AuthApi {
    public func login(username: String, password: String, deviceName: String) async throws -> SignInResult {
        var req = Thehideout_V1_LoginRequest()
        req.username = username; req.password = password; req.deviceName = deviceName
        let r = try value(await auth.login(request: req, headers: [:]))
        return Self.signIn(token: r.token, user: r.user, challenge: r.hasSecondFactorRequired ? r.secondFactorRequired : nil)
    }

    public func requestEmailCode(email: String) async throws {
        var req = Thehideout_V1_RequestEmailCodeRequest(); req.email = email
        _ = try value(await auth.requestEmailCode(request: req, headers: [:]))
    }

    public func verifyEmailCode(email: String, code: String, deviceName: String) async throws -> SignInResult {
        var req = Thehideout_V1_VerifyEmailCodeRequest()
        req.email = email; req.code = code; req.deviceName = deviceName
        let r = try value(await auth.verifyEmailCode(request: req, headers: [:]))
        return Self.signIn(token: r.token, user: r.user, challenge: r.hasSecondFactorRequired ? r.secondFactorRequired : nil)
    }

    public func completeSecondFactor(challenge: String, totpCode: String?, recoveryCode: String?) async throws -> AuthSession {
        var req = Thehideout_V1_CompleteSecondFactorRequest()
        req.challengeID = challenge
        if let totpCode { req.totpCode = totpCode }
        if let recoveryCode { req.recoveryCode = recoveryCode }
        let r = try value(await auth.completeSecondFactor(request: req, headers: [:]))
        return AuthSession(token: r.token, user: User(r.user))
    }

    public func beginPasskeyLogin(username: String, deviceName: String) async throws -> PasskeyCeremony {
        var req = Thehideout_V1_BeginPasskeyLoginRequest()
        req.username = username; req.deviceName = deviceName
        return Self.ceremony(try value(await auth.beginPasskeyLogin(request: req, headers: [:])).ceremony)
    }

    public func finishPasskeyLogin(ceremony: String, credentialJSON: String) async throws -> PasskeyOutcome {
        var req = Thehideout_V1_FinishPasskeyLoginRequest()
        req.ceremonyID = ceremony; req.credentialJson = credentialJSON
        let r = try value(await auth.finishPasskeyLogin(request: req, headers: [:]))
        return r.pending || r.token.isEmpty ? .pending : .session(AuthSession(token: r.token, user: User(r.user)))
    }

    public func beginSecondFactorPasskey(challenge: String) async throws -> PasskeyCeremony {
        var req = Thehideout_V1_BeginSecondFactorPasskeyRequest(); req.challengeID = challenge
        return Self.ceremony(try value(await auth.beginSecondFactorPasskey(request: req, headers: [:])).ceremony)
    }

    public func finishSecondFactorPasskey(ceremony: String, credentialJSON: String) async throws -> PasskeyOutcome {
        var req = Thehideout_V1_FinishSecondFactorPasskeyRequest()
        req.ceremonyID = ceremony; req.credentialJson = credentialJSON
        let r = try value(await auth.finishSecondFactorPasskey(request: req, headers: [:]))
        return r.pending || r.token.isEmpty ? .pending : .session(AuthSession(token: r.token, user: User(r.user)))
    }

    public func requestPasswordReset(email: String) async throws {
        var req = Thehideout_V1_RequestPasswordResetRequest(); req.email = email
        _ = try value(await auth.requestPasswordReset(request: req, headers: [:]))
    }

    public func resetPassword(email: String, code: String, newPassword: String) async throws {
        var req = Thehideout_V1_ResetPasswordRequest()
        req.email = email; req.code = code; req.newPassword = newPassword
        _ = try value(await auth.resetPassword(request: req, headers: [:]))
    }

    public func lookupInvite(code: String) async throws -> InvitePreview {
        var req = Thehideout_V1_LookupInviteRequest(); req.tokenOrCode = code
        let r = try value(await invites.lookupInvite(request: req, headers: [:]))
        let status: InvitePreviewStatus = switch r.status {
        case .pending: .pending
        case .accepted: .accepted
        case .expired: .expired
        case .revoked: .revoked
        default: .unknown
        }
        return InvitePreview(maskedEmail: r.maskedEmail, inviter: r.hasInviter ? r.inviter.username : nil, guildNames: r.guildNames,
                             expiresAt: r.expiresAt.date, status: status, termsVersion: r.termsVersion,
                             minimumAge: r.minimumAge == 0 ? 16 : Int(r.minimumAge))
    }

    public func acceptInvite(code: String, username: String, password: String, deviceName: String, termsVersion: String, ageConfirmed: Bool) async throws -> AuthSession {
        var req = Thehideout_V1_AcceptInviteRequest()
        req.tokenOrCode = code; req.username = username; req.password = password; req.deviceName = deviceName
        req.acceptedTermsVersion = termsVersion; req.ageConfirmed = ageConfirmed
        let r = try value(await invites.acceptInvite(request: req, headers: [:]))
        return AuthSession(token: r.token, user: User(r.user))
    }

    public func acceptTerms(version: String) async throws {
        var req = Thehideout_V1_AcceptTermsRequest(); req.version = version
        _ = try value(await auth.acceptTerms(request: req, headers: [:]))
    }

    public func beginPasskeyRegistration(name: String) async throws -> PasskeyCeremony {
        var req = Thehideout_V1_BeginPasskeyRegistrationRequest(); req.name = name
        return Self.ceremony(try value(await auth.beginPasskeyRegistration(request: req, headers: [:])).ceremony)
    }

    public func finishPasskeyRegistration(ceremony: String, credentialJSON: String) async throws -> Bool {
        var req = Thehideout_V1_FinishPasskeyRegistrationRequest()
        req.ceremonyID = ceremony; req.credentialJson = credentialJSON
        return try value(await auth.finishPasskeyRegistration(request: req, headers: [:])).pending
    }

    private static func ceremony(_ c: Thehideout_V1_PasskeyCeremony) -> PasskeyCeremony {
        PasskeyCeremony(id: c.ceremonyID, url: URL(string: c.ceremonyURL), optionsJSON: c.optionsJson)
    }

    private static func signIn(token: String, user: Thehideout_V1_User, challenge: Thehideout_V1_SecondFactorChallenge?) -> SignInResult {
        guard let challenge else { return .session(AuthSession(token: token, user: User(user))) }
        let methods: [SecondFactorMethod] = challenge.methods.compactMap {
            switch $0 {
            case .totp: .totp
            case .recoveryCode: .recoveryCode
            case .passkey: .passkey
            default: nil
            }
        }
        return .secondFactor(SecondFactorChallenge(id: challenge.challengeID, methods: methods, expiresAt: challenge.hasExpiresAt ? challenge.expiresAt.date : nil))
    }
}
