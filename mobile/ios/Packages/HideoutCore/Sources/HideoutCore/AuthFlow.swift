import Foundation
import Observation

/// Where an auth error is shown.
public enum AuthField: Sendable, Hashable { case form, login, password, email, code, invite, username }

public struct AuthIssue: Sendable, Hashable {
    public var field: AuthField
    public var message: String
    public init(_ field: AuthField, _ message: String) { self.field = field; self.message = message }
}

public struct SecondFactorState: Sendable, Hashable {
    public var challenge: String
    public var methods: [SecondFactorMethod]
    /// Showing the recovery-code field instead of the authenticator code.
    public var recovery = false
    /// Tries the server says are left after a wrong answer.
    public var remaining: Int?
    /// The challenge is used up or expired: only "Start again" is left.
    public var locked = false
    /// Waiting for a passkey.
    public var passkey = false
    public var offersPasskey: Bool { methods.contains(.passkey) }
    public var offersRecovery: Bool { methods.contains(.recoveryCode) }
}

public enum AuthStep: Sendable, Hashable {
    case password
    case emailCode(sent: Bool)
    case secondFactor(SecondFactorState)
    case passkey(waiting: Bool)
    case reset(sent: Bool)
    case inviteCode
    case inviteAccept(preview: InvitePreview, code: String)
}

public struct AuthSuspension: Sendable, Hashable {
    public var reason: String
    public var until: Date?
}

/// The "Add a passkey?" sheet shown once after an account is created.
public enum WelcomeState: Sendable, Hashable {
    case hidden
    case prompt
    case busy
    case failed(String)
}

/// The state machine of every signed-out screen (sign in, email code, second factor, passkey, reset, invite), a port of
/// desktop's `AuthStep` flow in `account.rs`. Views bind to the fields and call the intents; the flow validates, calls
/// the `AuthApi`, maps errors the way desktop does (never saying whether an account exists) and hands a new session
/// to `onSession`.
@MainActor @Observable
public final class AuthFlow {
    public static let minPassword = 8
    public static let resendSeconds = 30

    public private(set) var step: AuthStep = .password
    public private(set) var busy = false
    public private(set) var issue: AuthIssue?
    public private(set) var notice: String?
    public private(set) var suspension: AuthSuspension?
    public private(set) var resendAt: Date?
    public private(set) var welcome: WelcomeState = .hidden
    /// Counts finished sign-ins (a trigger for the success haptic).
    public private(set) var signInCount = 0

    // fields
    public var login = ""
    public var password = ""
    public var email = ""
    public private(set) var code = ""
    public var recovery = ""
    public var invite = ""
    public var username = ""
    public var acceptTerms = false
    public var confirmAge = false
    public var addPasskey = false

    @ObservationIgnored private let server: ServerSelection
    @ObservationIgnored private let deviceName: String
    @ObservationIgnored private let now: @Sendable () -> Date
    @ObservationIgnored private let pollInterval: Duration
    @ObservationIgnored public var passkeys: (any PasskeyPlatform)?
    @ObservationIgnored private let onSession: @MainActor (String) async -> Void
    @ObservationIgnored private let onClientTooOld: @MainActor () -> Void
    @ObservationIgnored private var task: Task<Void, Never>?
    /// Bumped by every action so a slow answer to an abandoned step is dropped.
    @ObservationIgnored private var generation = 0

    public init(server: ServerSelection, deviceName: String, passkeys: (any PasskeyPlatform)? = nil,
                pollInterval: Duration = .milliseconds(1500), now: @escaping @Sendable () -> Date = { Date() },
                onSession: @escaping @MainActor (String) async -> Void,
                onClientTooOld: @escaping @MainActor () -> Void = {}) {
        self.server = server; self.deviceName = deviceName; self.passkeys = passkeys; self.pollInterval = pollInterval
        self.now = now; self.onSession = onSession; self.onClientTooOld = onClientTooOld
    }

    private var api: any AuthApi { server.backend }

    // MARK: derived

    public var inviteMode: Bool {
        switch step {
        case .inviteCode, .inviteAccept: true
        default: false
        }
    }

    /// Top-level steps end with the quiet line to the other one ("Have an invite?" / "Already have an account?").
    public var topLevel: Bool {
        suspension == nil && (step == .password || step == .inviteCode)
    }

    /// Whether passkeys use the system sheet (a real domain with associated domains) rather than the browser page.
    public var passkeysAreNative: Bool { passkeys?.canUseNative(host: server.host) ?? false }

    public func issue(for field: AuthField) -> String? { issue?.field == field ? issue?.message : nil }

    /// Seconds until "Resend code" works again at `date` (0: now).
    public func resendWait(at date: Date) -> Int {
        guard let resendAt, resendAt > date else { return 0 }
        return Int(resendAt.timeIntervalSince(date).rounded(.up))
    }

    /// Waits for the call in flight. For tests.
    public func settle() async { await task?.value }

    // MARK: navigation

    public func startEmailCode() { go(.emailCode(sent: false)); prefillEmail() }
    public func startReset() { go(.reset(sent: false)); prefillEmail() }
    public func startPasskey() { go(.passkey(waiting: false)) }
    public func startInvite() { go(.inviteCode) }
    public func showSignIn() { go(.password) }
    public func dismissSuspension() { suspension = nil; go(.password) }

    private func prefillEmail() { if email.isEmpty, ServerURLCheck.looksLikeEmail(login) { email = login.trimmingCharacters(in: .whitespaces) } }

    /// "Back" on a second step: to the step it came from.
    public func back() {
        switch step {
        case .inviteAccept: go(.inviteCode)
        default: go(.password)
        }
    }

    public func toggleRecovery() {
        guard case .secondFactor(var s) = step, !s.locked, !s.passkey else { return }
        s.recovery.toggle()
        issue = nil; code = ""; recovery = ""
        step = .secondFactor(s)
    }

    private func go(_ next: AuthStep) {
        cancelPasskey()
        generation += 1
        step = next
        issue = nil; notice = nil; suspension = nil; busy = false
        code = ""; recovery = ""; password = ""
        resendAt = nil
        if next == .password || next == .inviteCode { acceptTerms = false; confirmAge = false }
    }

    // MARK: fields

    /// The code field: digits only, six at most; a full code is sent at once where nothing else is needed.
    public func setCode(_ raw: String) {
        let clean = String(raw.filter(\.isASCII).filter(\.isNumber).prefix(6))
        code = clean
        if issue?.field == .code { issue = nil }
        guard clean.count == 6 else { return }
        switch step {
        case .emailCode(sent: true): submit()
        case .secondFactor(let s) where !s.recovery && !s.locked && !s.passkey: submit()
        default: break
        }
    }

    // MARK: submit

    public func submit() {
        guard !busy else { return }
        switch step {
        case .password:
            let name = login.trimmingCharacters(in: .whitespaces)
            if name.isEmpty { return invalid(.login, "Enter your username or email.") }
            if password.isEmpty { return invalid(.password, "Enter your password.") }
            start { me in
                let r = try await me.api.login(username: name, password: me.password, deviceName: me.deviceName)
                try await me.signedIn(r)
            }
        case .emailCode(sent: false), .reset(sent: false):
            let address = email.trimmingCharacters(in: .whitespaces)
            guard ServerURLCheck.looksLikeEmail(address) else { return invalid(.email, "Enter the email address of your account.") }
            let reset = if case .reset = step { true } else { false }
            start { me in
                if reset { try await me.api.requestPasswordReset(email: address) } else { try await me.api.requestEmailCode(email: address) }
                me.codeSent(to: address, reset: reset)
            }
        case .emailCode(sent: true):
            guard code.count == 6 else { return invalid(.code, "Enter the 6-digit code from the email.") }
            let address = email.trimmingCharacters(in: .whitespaces), typed = code
            start { me in
                let r = try await me.api.verifyEmailCode(email: address, code: typed, deviceName: me.deviceName)
                try await me.signedIn(r)
            }
        case .reset(sent: true):
            guard code.count == 6 else { return invalid(.code, "Enter the 6-digit code from the email.") }
            guard password.count >= Self.minPassword else { return invalid(.password, "Use at least \(Self.minPassword) characters.") }
            let address = email.trimmingCharacters(in: .whitespaces), typed = code, new = password
            start { me in
                try await me.api.resetPassword(email: address, code: typed, newPassword: new)
                me.passwordWasReset(address)
            }
        case .passkey(waiting: true):
            return
        case .passkey(waiting: false):
            let name = login.trimmingCharacters(in: .whitespaces)
            if name.isEmpty { return invalid(.login, "Enter your username.") }
            runPasskey(register: false, begin: { me in try await me.api.beginPasskeyLogin(username: name, deviceName: me.deviceName) },
                       finish: { me, c, cred in try await me.api.finishPasskeyLogin(ceremony: c, credentialJSON: cred) })
        case .secondFactor(let s):
            guard !s.locked, !s.passkey else { return }
            let typed: String
            if s.recovery {
                typed = recovery.trimmingCharacters(in: .whitespaces)
                if typed.isEmpty { return invalid(.code, "Enter one of your recovery codes.") }
            } else {
                typed = code
                if typed.count != 6 { return invalid(.code, "Enter the 6-digit code from your authenticator app.") }
            }
            start { me in
                let session = try await me.api.completeSecondFactor(challenge: s.challenge, totpCode: s.recovery ? nil : typed, recoveryCode: s.recovery ? typed : nil)
                await me.finish(session)
            }
        case .inviteCode:
            let typed = InviteCode.normalize(invite)
            if typed.isEmpty { return invalid(.invite, "Enter the code from your invite email.") }
            if InviteCode.looksLikeLink(typed) { return invalid(.invite, "Paste just the code from your invite email, not the link.") }
            lookUp(typed)
        case .inviteAccept(let preview, let code):
            let name = username.trimmingCharacters(in: .whitespaces)
            if !(3...32).contains(name.count) { return invalid(.username, "Choose a username of 3 to 32 characters.") }
            if password.count < Self.minPassword { return invalid(.password, "Use at least \(Self.minPassword) characters.") }
            if !acceptTerms { return invalid(.form, "Accept the terms and privacy notice to continue.") }
            if !confirmAge { return invalid(.form, "Confirm that you are at least \(preview.minimumAge) years old.") }
            let new = password, add = addPasskey
            start { me in
                let session = try await me.api.acceptInvite(code: code, username: name, password: new, deviceName: me.deviceName, termsVersion: preview.termsVersion, ageConfirmed: true)
                if add { me.welcome = .prompt }
                await me.finish(session)
            }
        }
    }

    /// Whether "Create account" may be pressed: both boxes ticked and nothing in flight.
    public var canCreateAccount: Bool { !busy && acceptTerms && confirmAge }

    public func resendCode() {
        guard !busy, resendWait(at: now()) == 0 else { return }
        let address = email.trimmingCharacters(in: .whitespaces)
        let reset: Bool
        switch step {
        case .emailCode(sent: true): reset = false
        case .reset(sent: true): reset = true
        default: return
        }
        code = ""
        start { me in
            if reset { try await me.api.requestPasswordReset(email: address) } else { try await me.api.requestEmailCode(email: address) }
            me.codeSent(to: address, reset: reset)
        }
    }

    // MARK: invites

    /// `thehideout://invite/<token>` or `{server}/invite/<token>`: opens the invite step and looks it up.
    /// Returns whether the URL was an invite link.
    @discardableResult
    public func handle(url: URL) -> Bool {
        guard let token = InviteCode.token(fromLink: url.absoluteString), !token.isEmpty else { return false }
        go(.inviteCode)
        invite = ""
        lookUp(token)
        return true
    }

    private func lookUp(_ token: String) {
        start { me in
            let preview = try await me.api.lookupInvite(code: token)
            if let problem = Self.problem(with: preview.status) { me.fail(.invite, problem); return }
            me.step = .inviteAccept(preview: preview, code: token)
            me.issue = nil; me.notice = nil; me.busy = false
        }
    }

    static func problem(with status: InvitePreviewStatus) -> String? {
        switch status {
        case .pending: nil
        case .accepted: "This invite was already used. Sign in instead."
        case .expired: "This invite has expired. Ask for a new one."
        case .revoked: "This invite was withdrawn. Ask for a new one."
        case .unknown: "This invite can't be used."
        }
    }

    // MARK: passkeys

    /// "Use a passkey" on the second factor.
    public func useSecondFactorPasskey() {
        guard !busy, case .secondFactor(var s) = step, s.offersPasskey, !s.locked, !s.passkey else { return }
        s.passkey = true
        step = .secondFactor(s)
        issue = nil
        runPasskey(register: false, begin: { me in try await me.api.beginSecondFactorPasskey(challenge: s.challenge) },
                   finish: { me, c, cred in try await me.api.finishSecondFactorPasskey(ceremony: c, credentialJSON: cred) })
    }

    /// Stops waiting for a passkey (Cancel, or the browser sheet was closed).
    public func cancelPasskey() {
        guard case .passkey(waiting: true) = step else {
            if case .secondFactor(var s) = step, s.passkey { s.passkey = false; step = .secondFactor(s); stopTask() }
            return
        }
        step = .passkey(waiting: false)
        stopTask()
    }

    private func stopTask() {
        generation += 1
        task?.cancel()
        passkeys?.closeCeremonyPage()
        busy = false
    }

    /// The prompt after sign-up: "Add a passkey".
    public func addPasskeyNow() {
        switch welcome {
        case .prompt, .failed: break
        default: return
        }
        welcome = .busy
        generation += 1
        let g = generation
        task = Task { [weak self] in
            guard let self else { return }
            do {
                let ceremony = try await self.api.beginPasskeyRegistration(name: self.deviceName)
                var pending = true
                if let platform = self.passkeys, platform.canUseNative(host: self.server.host) {
                    do {
                        let cred = try await platform.registration(optionsJSON: ceremony.optionsJSON)
                        pending = try await self.api.finishPasskeyRegistration(ceremony: ceremony.id, credentialJSON: cred)
                        pending = false
                    } catch PasskeyFailure.cancelled { self.welcome = .prompt; return }
                    catch PasskeyFailure.unavailable { pending = true }
                }
                if pending {
                    let api = self.api
                    let _: Bool = try await self.pollPage(ceremony) { c, cred in try await api.finishPasskeyRegistration(ceremony: c, credentialJSON: cred) ? nil : true }
                }
                if g == self.generation { self.welcome = .hidden }
            } catch is CancellationError {
                if g == self.generation { self.welcome = .prompt }
            } catch {
                if g == self.generation { self.welcome = .failed("Couldn't add a passkey. You can add one later in Settings.") }
            }
        }
    }

    public func skipWelcome() { welcome = .hidden }

    private func runPasskey(register: Bool, begin: @escaping @MainActor (AuthFlow) async throws -> PasskeyCeremony,
                            finish: @escaping @MainActor (AuthFlow, String, String) async throws -> PasskeyOutcome) {
        if case .passkey = step { step = .passkey(waiting: true) }
        issue = nil
        start { me in
            let ceremony = try await begin(me)
            var outcome: PasskeyOutcome?
            if let platform = me.passkeys, platform.canUseNative(host: me.server.host) {
                do {
                    let cred = try await platform.assertion(optionsJSON: ceremony.optionsJSON)
                    outcome = try await finish(me, ceremony.id, cred)
                } catch PasskeyFailure.cancelled {
                    me.endPasskey(); return
                } catch PasskeyFailure.unavailable {
                    outcome = nil // the system cannot serve this server: the browser page can
                }
            }
            if outcome == nil {
                let s = try await me.pollPage(ceremony) { c, cred -> AuthSession? in
                    if case .session(let s) = try await finish(me, c, cred) { s } else { nil }
                }
                outcome = .session(s)
            }
            if case .session(let s)? = outcome { await me.finish(s) } else { me.endPasskey() }
        }
    }

    private func endPasskey() {
        busy = false
        switch step {
        case .passkey: step = .passkey(waiting: false)
        case .secondFactor(var s): s.passkey = false; step = .secondFactor(s)
        default: break
        }
    }

    /// The browser fallback: open the ceremony page and ask `Finish*` with an empty credential until it is done.
    private func pollPage<T>(_ ceremony: PasskeyCeremony, finish: (String, String) async throws -> T?) async throws -> T {
        guard let platform = passkeys, let url = ceremony.url else { throw PasskeyFailure.failed("no passkey support") }
        platform.openCeremonyPage(url, onDismiss: { [weak self] in self?.cancelPasskey() })
        defer { platform.closeCeremonyPage() }
        // The server keeps a ceremony for five minutes.
        let deadline = now().addingTimeInterval(300)
        while now() < deadline {
            try Task.checkCancellation()
            try await Task.sleep(for: pollInterval)
            try Task.checkCancellation()
            if let r = try await finish(ceremony.id, "") { return r }
        }
        throw AppError.codeExpired
    }

    // MARK: effects

    private func start(_ body: @escaping @MainActor (AuthFlow) async throws -> Void) {
        busy = true; issue = nil; notice = nil
        generation += 1
        let g = generation
        task = Task { [weak self] in
            guard let self else { return }
            do { try await body(self) }
            catch is CancellationError { if g == self.generation { self.busy = false } }
            catch {
                guard g == self.generation else { return }
                self.refused(error)
            }
        }
    }

    private func invalid(_ field: AuthField, _ message: String) {
        issue = AuthIssue(field, message)
    }

    private func fail(_ field: AuthField, _ message: String) {
        busy = false
        issue = AuthIssue(field, message)
    }

    private func codeSent(to address: String, reset: Bool) {
        busy = false
        step = reset ? .reset(sent: true) : .emailCode(sent: true)
        notice = "If an account uses \(address), a code is on its way. It works for 10 minutes."
        issue = nil
        resendAt = now().addingTimeInterval(TimeInterval(Self.resendSeconds))
    }

    private func passwordWasReset(_ address: String) {
        go(.password)
        login = address
        notice = "Your password was changed and every device was signed out. Sign in with the new one."
    }

    private func signedIn(_ result: SignInResult) async throws {
        switch result {
        case .session(let s): await finish(s)
        case .secondFactor(let c):
            busy = false
            password = ""; code = ""; recovery = ""
            step = .secondFactor(SecondFactorState(challenge: c.id, methods: c.methods.isEmpty ? [.totp, .recoveryCode] : c.methods))
        }
    }

    /// A session was made: hand the token over and forget what was typed.
    private func finish(_ session: AuthSession) async {
        await onSession(session.token)
        signInCount += 1
        busy = false
        step = .password
        login = ""; password = ""; email = ""; code = ""; recovery = ""; invite = ""; username = ""
        acceptTerms = false; confirmAge = false; addPasskey = false
        issue = nil; notice = nil; resendAt = nil; suspension = nil
    }

    /// A call was refused: the error goes where the person is looking and what they typed stays (only the secret goes).
    private func refused(_ error: Error) {
        let e = AppError.from(error)
        busy = false
        password = ""
        if case .clientTooOld = e { onClientTooOld(); return }
        if case .suspended(let reason, let until) = e { suspension = AuthSuspension(reason: reason, until: until); issue = nil; return }
        if let f = error as? PasskeyFailure {
            endPasskey()
            if case .failed = f { issue = AuthIssue(.form, "That passkey wasn't accepted. Try again.") }
            return
        }
        switch step {
        case .secondFactor(var s):
            let wasPasskey = s.passkey
            s.passkey = false
            code = ""; recovery = ""
            switch e {
            case .wrongCode(let left):
                s.remaining = left
                let (field, what) = wasPasskey ? (AuthField.form, "That passkey wasn't accepted.") : s.recovery ? (.code, "That recovery code didn't work.") : (.code, "That code didn't work.")
                issue = AuthIssue(field, "\(what) \(Self.triesLeft(left))")
            case .tooManyAttempts:
                s.locked = true; issue = AuthIssue(.form, "Too many wrong attempts. Start signing in again.")
            case .codeExpired:
                s.locked = true; issue = AuthIssue(.form, "This sign-in expired. Start again.")
            case .precondition where wasPasskey:
                issue = AuthIssue(.form, "A passkey can't be used for this sign-in. Use a code instead.")
            default:
                issue = Self.issue(for: e)
            }
            step = .secondFactor(s)
        case .emailCode(sent: true) where e == .unauthenticated || e == .codeExpired || e == .tooManyAttempts:
            code = ""
            issue = AuthIssue(.code, "That code didn't work. Check it and try again.")
        case .emailCode(sent: true):
            if case .wrongCode(let left) = e { code = ""; issue = AuthIssue(.code, "That code didn't work. \(Self.triesLeft(left))") } else { issue = Self.issue(for: e) }
        case .reset(sent: true) where e == .unauthenticated || e == .codeExpired || e == .tooManyAttempts:
            code = ""
            issue = AuthIssue(.code, "That code didn't work. Check it, or ask for a new one.")
        case .passkey:
            step = .passkey(waiting: false)
            switch e {
            case .precondition, .notFound: issue = AuthIssue(.form, "Passkey sign-in isn't available for that account. Use your password or an emailed code.")
            case .unauthenticated: issue = AuthIssue(.form, "The passkey wasn't accepted. Try again.")
            default: issue = Self.issue(for: e)
            }
        case .inviteCode:
            switch e {
            case .notFound: issue = AuthIssue(.invite, "We couldn't find that invite. Check the code and try again.")
            default: issue = Self.issue(for: e, field: .invite)
            }
        case .inviteAccept:
            switch e {
            case .alreadyExists: issue = AuthIssue(.username, "That username is taken. Try another.")
            case .precondition(let m): issue = AuthIssue(.form, "This invite can't be used any more (\(m)). Ask for a new one.")
            case .notFound: issue = AuthIssue(.form, "This invite can't be used any more. Ask for a new one.")
            case .invalidArgument(let m): issue = AuthIssue(.form, sentence(m))
            default: issue = Self.issue(for: e)
            }
        case .password where e == .unauthenticated:
            issue = AuthIssue(.form, "Wrong username, email or password.")
        default:
            issue = Self.issue(for: e)
        }
    }

    static func triesLeft(_ n: Int) -> String {
        switch n {
        case 0: "No tries left."
        case 1: "1 try left."
        default: "\(n) tries left."
        }
    }

    /// What an error means on an auth form. Wrong credentials never say which part was wrong, so they reveal nothing
    /// about whether an account exists.
    public static func issue(for e: AppError, field: AuthField = .form) -> AuthIssue {
        switch e {
        case .unauthenticated: AuthIssue(.form, "Those details didn't work. Check them and try again.")
        case .alreadyExists: AuthIssue(.username, "That username is taken.")
        case .inviteRequired: AuthIssue(.form, "You need an invite to make an account.")
        case .invalidArgument(let m): AuthIssue(field, sentence(m))
        case .rateLimited(let after): AuthIssue(.form, rateLimitText(after))
        case .termsUpdateRequired: AuthIssue(.form, "Accept the terms and privacy notice to continue.")
        case .network: AuthIssue(.form, "Can't reach the server. Check your connection and try again.")
        case .codeExpired: AuthIssue(.form, "That code expired. Ask for a new one.")
        case .tooManyAttempts: AuthIssue(.form, "Too many wrong attempts. Ask for a new code.")
        default: AuthIssue(.form, sentence(e.description))
        }
    }

    static func rateLimitText(_ after: TimeInterval?) -> String {
        guard let after, after >= 1 else { return "Too many attempts. Try again in a moment." }
        if after < 90 { return "Too many attempts. Try again in \(Int(after.rounded(.up))) seconds." }
        return "Too many attempts. Try again in \(Int((after / 60).rounded(.up))) minutes."
    }
}

/// Typed invite codes and invite links.
public enum InviteCode {
    /// What goes to the server: trimmed and uppercased, the dash put in (`wxyz2345` -> `WXYZ-2345`). A longer value
    /// (an emailed link token) is left as typed.
    public static func normalize(_ raw: String) -> String {
        let t = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !looksLikeLink(t) else { return t }
        let squeezed = t.filter { $0 != "-" && $0 != " " }
        if squeezed.count == 8, squeezed.allSatisfy({ $0.isLetter || $0.isNumber }) {
            let u = squeezed.uppercased()
            return "\(u.prefix(4))-\(u.suffix(4))"
        }
        return t.uppercased()
    }

    /// The field holds a link rather than a code.
    public static func looksLikeLink(_ raw: String) -> Bool { raw.contains("://") || raw.contains("/invite/") }

    /// The token at the end of an invite link (`thehideout://invite/<token>`, `https://host/invite/<token>`).
    public static func token(fromLink link: String) -> String? {
        let url = link.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let r = url.range(of: "/invite/") else { return nil }
        let rest = url[r.upperBound...]
        return rest.split(whereSeparator: { "?#/".contains($0) }).first.map(String.init)
    }
}

enum ServerURLCheck {
    /// A loose email check: something@something.something, no spaces.
    static func looksLikeEmail(_ s: String) -> Bool {
        let t = s.trimmingCharacters(in: .whitespaces)
        guard let at = t.firstIndex(of: "@"), !t.contains(where: \.isWhitespace) else { return false }
        let (a, b) = (t[..<at], t[t.index(after: at)...])
        return !a.isEmpty && b.contains(".") && !b.hasPrefix(".") && !b.hasSuffix(".")
    }
}
