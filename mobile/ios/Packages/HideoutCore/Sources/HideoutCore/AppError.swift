import Connect
import Foundation
import HideoutAPI

/// A guild that blocks deleting its owner's account.
public struct OwnedGuild: Sendable, Hashable {
    public var id: ID
    public var name: String
    public var memberCount: Int
    public init(id: ID, name: String, memberCount: Int) { self.id = id; self.name = name; self.memberCount = memberCount }
}

/// Why an API call failed, mapped from the Connect error like desktop's `ApiError`.
public enum AppError: Error, Sendable, Equatable, CustomStringConvertible {
    /// Missing, invalid or expired session token, or wrong username/password.
    case unauthenticated
    /// A second-factor answer or a signed-in code check was wrong; that many more tries are accepted.
    case wrongCode(attemptsRemaining: Int)
    /// The challenge, code or setup expired, was used, or never existed: start again.
    case codeExpired
    case tooManyAttempts
    case invalidArgument(String)
    case notFound
    case permissionDenied
    /// The change was based on a stale version (layout or role order): refetch and retry.
    case conflict
    case unimplemented
    /// Too many attempts; try again later.
    case rateLimited(retryAfter: TimeInterval?)
    case alreadyExists
    case inviteRequired
    case precondition(String)
    case tooLarge
    /// The server refuses this app version: the user must install a newer build.
    case clientTooOld
    case suspended(reason: String, until: Date?)
    /// The terms must be accepted first.
    case termsUpdateRequired
    case ownsGuilds([OwnedGuild])
    /// The server could not be reached or the connection broke.
    case network(String)
    case other(String)

    /// Whether retrying later might succeed.
    public var isTransient: Bool {
        switch self {
        case .network, .rateLimited: true
        default: false
        }
    }

    public var description: String {
        switch self {
        case .unauthenticated: "not signed in or invalid credentials"
        case .wrongCode(1): "wrong code, 1 try left"
        case .wrongCode(let n): "wrong code, \(n) tries left"
        case .codeExpired: "expired, start again"
        case .tooManyAttempts: "too many wrong attempts, start again"
        case .invalidArgument(let m): "invalid request: \(m)"
        case .notFound: "not found"
        case .permissionDenied: "permission denied"
        case .conflict: "changed by someone else, refresh and try again"
        case .unimplemented: "not supported by this server"
        case .rateLimited: "too many attempts, try again later"
        case .alreadyExists: "already exists"
        case .inviteRequired: "an invite is required to create an account"
        case .precondition(let m): m.isEmpty ? "not allowed right now" : "not allowed: \(m)"
        case .tooLarge: "file is too large"
        case .clientTooOld: "this version of thehideout is too old, update required"
        case .suspended(let r, _): r.isEmpty ? "your account is suspended" : "your account is suspended: \(r)"
        case .termsUpdateRequired: "the terms must be accepted first"
        case .ownsGuilds(let g): "transfer or delete the servers you own first: " + g.map(\.name).joined(separator: ", ")
        case .network(let m): "network error: \(m)"
        case .other(let m): m
        }
    }

    static let termsPrefix = "you must accept the terms"
    static let clientTooOldPrefix = "client too old"
    static let inviteRequiredMessage = "invite required"

    /// The mapping of a Connect error; the details (`CodeError`, `AccountSuspended`, `OwnedGuilds`) win over the code.
    public init(_ error: ConnectError) {
        if let d = (error.unpackedDetails() as [Thehideout_V1_CodeError]).first {
            switch d.reason {
            case .wrong: self = .wrongCode(attemptsRemaining: Int(d.attemptsRemaining)); return
            case .expired: self = .codeExpired; return
            case .tooManyAttempts: self = .tooManyAttempts; return
            default: break
            }
        }
        if let d = (error.unpackedDetails() as [Thehideout_V1_AccountSuspended]).first {
            self = .suspended(reason: d.reason, until: d.hasUntil ? d.until.date : nil)
            return
        }
        if let d = (error.unpackedDetails() as [Thehideout_V1_OwnedGuilds]).first {
            self = .ownsGuilds(d.guilds.map { OwnedGuild(id: $0.id, name: $0.name, memberCount: Int($0.memberCount)) })
            return
        }
        let msg = error.message ?? ""
        switch error.code {
        case .unauthenticated: self = .unauthenticated
        case .invalidArgument where msg.hasPrefix(Self.termsPrefix): self = .termsUpdateRequired
        case .invalidArgument, .outOfRange: self = .invalidArgument(msg)
        case .notFound: self = .notFound
        case .permissionDenied: self = .permissionDenied
        case .aborted: self = .conflict
        case .unimplemented: self = .unimplemented
        case .resourceExhausted:
            let retry = error.metadata.first { $0.key.lowercased() == "retry-after" }?.value.first.flatMap(TimeInterval.init)
            self = .rateLimited(retryAfter: retry)
        case .alreadyExists: self = .alreadyExists
        case .failedPrecondition where msg == Self.inviteRequiredMessage: self = .inviteRequired
        case .failedPrecondition where msg.lowercased().hasPrefix(Self.clientTooOldPrefix): self = .clientTooOld
        case .failedPrecondition: self = .precondition(msg)
        case .unavailable, .deadlineExceeded: self = .network(msg)
        case .canceled: self = .network("cancelled")
        default:
            if let underlying = error.exception, error.code == .unknown {
                self = .network(underlying.localizedDescription)
            } else {
                self = .other(msg)
            }
        }
    }

    /// The mapping of a plain (non-Connect) HTTP status, e.g. for `POST /attachments`.
    public init(status: Int, body: String) {
        let msg = body.trimmingCharacters(in: .whitespacesAndNewlines)
        switch status {
        case 400: self = .invalidArgument(msg)
        case 401: self = .unauthenticated
        case 403: self = .permissionDenied
        case 404: self = .notFound
        case 413: self = .tooLarge
        case 429: self = .rateLimited(retryAfter: nil)
        case 503, 504: self = .network(msg)
        default: self = .other(msg.isEmpty ? "request failed with status \(status)" : msg)
        }
    }

    /// Any error as an `AppError` (a cancelled or foreign error becomes `.network` / `.other`).
    public static func from(_ error: Error) -> AppError {
        if let e = error as? AppError { return e }
        if let e = error as? ConnectError { return AppError(e) }
        if error is CancellationError { return .network("cancelled") }
        if let e = error as? URLError { return .network(e.localizedDescription) }
        return .other(error.localizedDescription)
    }
}
