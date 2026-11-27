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
    /// When the challenge stops accepting answers (`SecondFactorChallenge.expiresAt`); checked locally so a stale
    /// code is refused without a round trip.
    public var expiresAt: Date? = nil
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

/// A universal invite link whose host is not the selected server: `AuthFlow.pendingInviteLink`.
public struct PendingInviteLink: Sendable, Hashable {
    public var url: URL
    public var host: String
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
    public private(set) var invite = ""
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
        task?.cancel()
        generation += 1
        step = next
        issue = nil; notice = nil; suspension = nil; busy = false
        code = ""; recovery = ""; password = ""
        resendAt = nil
        if next == .password || next == .inviteCode { acceptTerms = false; confirmAge = false }
    }

    // MARK: fields

    /// The code field: digits only, six at most; a full code is sent at once where nothing else is needed. A
    /// no-op when the cleaned result is unchanged (a 7th digit, a pasted dash): `code` capped at 6 already holds
    /// it, so there is nothing new to clear or resubmit — the field itself resyncs to that same value.
    public func setCode(_ raw: String) {
        let clean = String(raw.filter(\.isASCII).filter(\.isNumber).prefix(6))
        guard clean != code else { return }
        code = clean
        if issue?.field == .code { issue = nil }
        guard clean.count == 6 else { return }
        switch step {
        case .emailCode(sent: true): submit()
        case .secondFactor(let s) where !s.recovery && !s.locked && !s.passkey: submit()
        default: break
        }
    }

    /// The invite code field: filtered through `InviteCode.normalize` on every change (so a pasted link gives
    /// just its code), eight characters at most. The 8th character looks it up at once, like `setCode` does for
    /// a six-digit one; editing a code that already failed tries again. A no-op when the cleaned result is
    /// unchanged (a 9th character, a pasted dash): nothing new to clear or resubmit.
    public func setInvite(_ raw: String) {
        let clean = InviteCode.normalize(raw)
        guard clean != invite else { return }
        invite = clean
        if issue?.field == .invite { issue = nil }
        guard clean.count == InviteCode.length, step == .inviteCode else { return }
        submit()
    }

    // MARK: submit

    public func submit() {
        guard !busy else { return }
        switch step {
        case .password:
            let name = login.trimmingCharacters(in: .whitespaces)
            if name.isEmpty { return invalid(.login, "Enter your username or email.") }
            if password.isEmpty { return invalid(.password, "Enter your password.") }
            // Captured now, not read from `me.password` inside the task: `go()` (a "Back"/navigation tap while
            // this call is in flight) clears `password` right away, before the task body gets a turn to run.
            let pass = password
            start { me, g in
                let r = try await me.api.login(username: name, password: pass, deviceName: me.deviceName)
                guard g == me.generation else { return }
                try await me.signedIn(r)
            }
        case .emailCode(sent: false), .reset(sent: false):
            let address = email.trimmingCharacters(in: .whitespaces)
            guard ServerURLCheck.looksLikeEmail(address) else { return invalid(.email, "Enter the email address of your account.") }
            let reset = if case .reset = step { true } else { false }
            start { me, g in
                if reset { try await me.api.requestPasswordReset(email: address) } else { try await me.api.requestEmailCode(email: address) }
                guard g == me.generation else { return }
                me.codeSent(to: address, reset: reset)
            }
        case .emailCode(sent: true):
            guard code.count == 6 else { return invalid(.code, "Enter the 6-digit code from the email.") }
            let address = email.trimmingCharacters(in: .whitespaces), typed = code
            start { me, g in
                let r = try await me.api.verifyEmailCode(email: address, code: typed, deviceName: me.deviceName)
                guard g == me.generation else { return }
                try await me.signedIn(r)
            }
        case .reset(sent: true):
            guard code.count == 6 else { return invalid(.code, "Enter the 6-digit code from the email.") }
            guard password.count >= Self.minPassword else { return invalid(.password, "Use at least \(Self.minPassword) characters.") }
            let address = email.trimmingCharacters(in: .whitespaces), typed = code, new = password
            start { me, g in
                try await me.api.resetPassword(email: address, code: typed, newPassword: new)
                guard g == me.generation else { return }
                me.passwordWasReset(address)
            }
        case .passkey(waiting: true):
            return
        case .passkey(waiting: false):
            let name = login.trimmingCharacters(in: .whitespaces)
            if name.isEmpty { return invalid(.login, "Enter your username.") }
            runPasskey(register: false, begin: { me in try await me.api.beginPasskeyLogin(username: name, deviceName: me.deviceName) },
                       finish: { me, c, cred in try await me.api.finishPasskeyLogin(ceremony: c, credentialJSON: cred) })
        case .secondFactor(var s):
            guard !s.locked, !s.passkey else { return }
            if let expiresAt = s.expiresAt, now() > expiresAt {
                s.locked = true
                step = .secondFactor(s)
                issue = AuthIssue(.form, "This sign-in expired. Start again.")
                return
            }
            let typed: String
            if s.recovery {
                typed = recovery.trimmingCharacters(in: .whitespaces)
                if typed.isEmpty { return invalid(.code, "Enter one of your recovery codes.") }
            } else {
                typed = code
                if typed.count != 6 { return invalid(.code, "Enter the 6-digit code from your authenticator app.") }
            }
            start { me, g in
                let session = try await me.api.completeSecondFactor(challenge: s.challenge, totpCode: s.recovery ? nil : typed, recoveryCode: s.recovery ? typed : nil)
                guard g == me.generation else { return }
                await me.finish(session)
            }
        case .inviteCode:
            if invite.isEmpty { return invalid(.invite, "Enter the code from your invite email.") }
            if invite.count != InviteCode.length { return invalid(.invite, "Invite codes have 8 characters.") }
            lookUp(InviteCode.dashed(invite))
        case .inviteAccept(let preview, let code):
            let name = username.trimmingCharacters(in: .whitespaces)
            if !(3...32).contains(name.count) { return invalid(.username, "Choose a username of 3 to 32 characters.") }
            if password.count < Self.minPassword { return invalid(.password, "Use at least \(Self.minPassword) characters.") }
            if !acceptTerms { return invalid(.form, "Accept the terms and privacy notice to continue.") }
            if !confirmAge { return invalid(.form, "Confirm that you are at least \(preview.minimumAge) years old.") }
            let new = password, add = addPasskey
            start { me, g in
                let session = try await me.api.acceptInvite(code: code, username: name, password: new, deviceName: me.deviceName, termsVersion: preview.termsVersion, ageConfirmed: true)
                guard g == me.generation else { return }
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
        start { me, g in
            if reset { try await me.api.requestPasswordReset(email: address) } else { try await me.api.requestEmailCode(email: address) }
            guard g == me.generation else { return }
            me.codeSent(to: address, reset: reset)
        }
    }

    // MARK: invites

    /// `thehideout://invite/<code>` or `{server}/invite/<code>`: opens the invite step with the code prefilled
    /// and looks it up at once (a short or malformed code fails locally, the same "8 characters" message a typed
    /// one would). Returns whether the URL was an invite link at all.
    @discardableResult
    public func handle(url: URL) -> Bool {
        guard InviteCode.isLink(url.absoluteString) else { return false }
        // A universal link's host is the server that minted the invite; the custom scheme (`thehideout://invite/…`)
        // has no real host to compare (its authority component is "invite"), only the universal link does.
        if url.scheme == "https", let linkHost = url.host, !linkHost.isEmpty, linkHost.lowercased() != server.host.lowercased() {
            pendingInviteLink = PendingInviteLink(url: url, host: linkHost)
            return true
        }
        go(.inviteCode)
        openInviteLink(url.absoluteString)
        return true
    }

    /// Prefills the code field from a link and either looks it up (whole) or shows the length error (not).
    private func openInviteLink(_ link: String) {
        invite = InviteCode.normalize(link)
        if invite.count == InviteCode.length {
            lookUp(InviteCode.dashed(invite))
        } else {
            invalid(.invite, "Invite codes have 8 characters.")
        }
    }

    /// Set by `handle(url:)` when a universal invite link's host is not the selected server: looking the invite up
    /// there would just fail with "couldn't find that invite", so ask first instead.
    public private(set) var pendingInviteLink: PendingInviteLink?

    public func dismissPendingInviteLink() { pendingInviteLink = nil }

    /// Switches to the invite link's server, then opens and looks up the invite there. Does not go through `go()`:
    /// that would cancel this very task (it cancels whatever `task` currently is) before the lookup below runs.
    public func switchServerForPendingInviteLink() {
        guard let pending = pendingInviteLink else { return }
        pendingInviteLink = nil
        start { me, g in
            do { try await me.server.change(to: pending.host) }
            catch {
                guard g == me.generation else { return }
                me.fail(.form, (error as? ServerSelection.ChangeError)?.message ?? "Couldn't switch servers.")
                return
            }
            guard g == me.generation else { return }
            me.step = .inviteCode
            me.issue = nil; me.notice = nil; me.suspension = nil
            me.invite = InviteCode.normalize(pending.url.absoluteString)
            guard me.invite.count == InviteCode.length else {
                me.fail(.invite, "Invite codes have 8 characters.")
                return
            }
            try await me.performLookup(code: InviteCode.dashed(me.invite), generation: g)
        }
    }

    private func lookUp(_ code: String) {
        start { me, g in try await me.performLookup(code: code, generation: g) }
    }

    private func performLookup(code: String, generation g: Int) async throws {
        let preview = try await api.lookupInvite(code: code)
        guard g == generation else { return }
        if let problem = Self.problem(with: preview.status) { fail(.invite, problem); return }
        step = .inviteAccept(preview: preview, code: code)
        issue = nil; notice = nil; busy = false
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
                        _ = try await self.api.finishPasskeyRegistration(ceremony: ceremony.id, credentialJSON: cred)
                        pending = false
                    } catch PasskeyFailure.cancelled { self.welcome = .prompt; return }
                    catch PasskeyFailure.unavailable { pending = true }
                }
                if pending {
                    let api = self.api
                    // `onDismiss` cancels this task directly (not `cancelPasskey()`, which only knows the sign-in
                    // and second-factor passkey steps and would otherwise leave `welcome` stuck at `.busy` with
                    // Skip and the sheet's own dismiss disabled until the server's 5-minute ceremony expires).
                    let _: Bool = try await self.pollPage(ceremony, onDismiss: { [weak self] in self?.task?.cancel() }) {
                        c, cred in try await api.finishPasskeyRegistration(ceremony: c, credentialJSON: cred) ? nil : true
                    }
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
        start { me, g in
            let ceremony = try await begin(me)
            var outcome: PasskeyOutcome?
            if let platform = me.passkeys, platform.canUseNative(host: me.server.host) {
                do {
                    let cred = try await platform.assertion(optionsJSON: ceremony.optionsJSON)
                    outcome = try await finish(me, ceremony.id, cred)
                } catch PasskeyFailure.cancelled {
                    if g == me.generation { me.endPasskey() }
                    return
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
            guard g == me.generation else { return }
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
    /// `onDismiss` runs instead of the default `cancelPasskey()` when given (the welcome add-passkey flow is not
    /// a sign-in step `cancelPasskey()` knows about).
    private func pollPage<T>(_ ceremony: PasskeyCeremony, onDismiss: (@MainActor () -> Void)? = nil, finish: (String, String) async throws -> T?) async throws -> T {
        guard let platform = passkeys, let url = ceremony.url else { throw PasskeyFailure.failed("no passkey support") }
        platform.openCeremonyPage(url, onDismiss: { [weak self] in
            if let onDismiss { onDismiss() } else { self?.cancelPasskey() }
        })
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

    /// `body` gets its own generation `g`, snapshotted before the call starts: a step change (`go`) bumps
    /// `generation` and cancels `task`, but a reply already in flight can still land after that (the cancellation
    /// is cooperative, and a fast reply can race it), so `body` must check `g == me.generation` itself before
    /// applying anything after an `await` — the catch blocks below only cover the error path.
    private func start(_ body: @escaping @MainActor (AuthFlow, Int) async throws -> Void) {
        busy = true; issue = nil; notice = nil
        task?.cancel()
        generation += 1
        let g = generation
        task = Task { [weak self] in
            guard let self else { return }
            do { try await body(self, g) }
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
            step = .secondFactor(SecondFactorState(challenge: c.id, methods: c.methods.isEmpty ? [.totp, .recoveryCode] : c.methods, expiresAt: c.expiresAt))
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
        // On the invite accept step a taken username is a one-field fix: keep the password typed so it is not
        // retyped. Every other refusal clears it, since it is the secret and the step may have been wrong about it.
        let keepsPassword = { if case .inviteAccept = self.step, case .alreadyExists = e { return true }; return false }()
        if !keepsPassword { password = "" }
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

/// Typed invite codes and invite links: the server mints only an 8-character code (`XXXX-XXXX`, Crockford
/// base32 — no `I`, `L`, `O` or `U`), passed to `LookupInvite`/`AcceptInvite` with dashes and case ignored.
public enum InviteCode {
    /// Characters a code may hold.
    public static let length = 8
    private static let alphabet = "0123456789ABCDEFGHJKMNPQRSTVWXYZ"

    /// One typed or pasted character as a code character: uppercased, `I`/`L` read as `1`, `O` as `0`, `nil`
    /// when the alphabet has no such character (including `-`, spaces and `_`).
    private static func codeChar(_ c: Character) -> Character? {
        let upper = c.uppercased().first ?? c
        let u: Character
        switch upper {
        case "I", "L": u = "1"
        case "O": u = "0"
        default: u = upper
        }
        return alphabet.contains(u) ? u : nil
    }

    /// The code characters in `raw`, in order, at most `length`: anything outside the alphabet (dashes,
    /// spaces, underscores, punctuation) is dropped, so typing, pasting a bare code or pasting a whole invite
    /// link (`.../invite/XXXX-XXXX`, counted only from after its last `/invite/`, up to the first `?`, `#` or
    /// `/` so a query string or another path segment never leaks into the code) all land the same way.
    public static func normalize(_ raw: String) -> String {
        guard let r = raw.range(of: "/invite/", options: .backwards) else {
            return String(raw.compactMap(codeChar).prefix(length))
        }
        let segment = raw[r.upperBound...].prefix { !"?#/".contains($0) }
        return String(segment.compactMap(codeChar).prefix(length))
    }

    /// `normalize`'s result written `XXXX-XXXX` once it is whole (what the server expects to read, though it
    /// ignores dashes and case); a shorter code comes back bare.
    public static func dashed(_ raw: String) -> String {
        let code = normalize(raw)
        guard code.count == length else { return code }
        let mid = code.index(code.startIndex, offsetBy: length / 2)
        return "\(code[..<mid])-\(code[mid...])"
    }

    /// Whether `raw` is a real invite link rather than a bare code: the custom scheme has `invite` as its
    /// authority (`thehideout://invite/<code>`); `http`/`https` has `invite` as the first path component
    /// (`https://host/invite/<code>`), followed by a code segment. Its code may still be short or malformed,
    /// so this alone does not mean it is usable.
    public static func isLink(_ raw: String) -> Bool {
        guard let url = URL(string: raw.trimmingCharacters(in: .whitespacesAndNewlines)),
              let scheme = url.scheme?.lowercased() else { return false }
        if scheme == "thehideout" {
            return url.host?.lowercased() == "invite" && !url.path.isEmpty && url.path != "/"
        }
        guard scheme == "http" || scheme == "https" else { return false }
        let parts = url.path.split(separator: "/", omittingEmptySubsequences: true)
        return parts.count >= 2 && parts[0].lowercased() == "invite"
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
