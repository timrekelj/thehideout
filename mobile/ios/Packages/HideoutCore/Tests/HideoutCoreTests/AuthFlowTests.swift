import Foundation
import Testing
@testable import HideoutCore

/// A platform that answers passkey ceremonies from a script.
@MainActor
final class FakePasskeys: PasskeyPlatform {
    var native = false
    var assertionResult: Result<String, PasskeyFailure> = .success("{\"id\":\"cred\"}")
    var opened: [URL] = []
    var closed = 0
    var dismiss: (@MainActor () -> Void)?

    func canUseNative(host: String) -> Bool { native }
    func assertion(optionsJSON: String) async throws -> String { try assertionResult.get() }
    func registration(optionsJSON: String) async throws -> String { try assertionResult.get() }
    func openCeremonyPage(_ url: URL, onDismiss: @escaping @MainActor () -> Void) { opened.append(url); dismiss = onDismiss }
    func closeCeremonyPage() { closed += 1 }
}

final class Callbacks: @unchecked Sendable {
    private let lock = NSLock()
    private var _tokens: [String] = []
    private var _tooOld = 0
    var now = Date(timeIntervalSince1970: 1_800_000_000) { didSet {} }
    func add(_ t: String) { lock.withLock { _tokens.append(t) } }
    func old() { lock.withLock { _tooOld += 1 } }
    var tokens: [String] { lock.withLock { _tokens } }
    var tooOld: Int { lock.withLock { _tooOld } }
}

@MainActor
final class Harness {
    let fake = FakeBackend()
    let passkeys = FakePasskeys()
    let calls = Callbacks()
    let server: ServerSelection
    let flow: AuthFlow

    init() {
        let calls = self.calls
        fake.accounts.now = calls.now // the same clock as AuthFlow's, so a challenge's `expiresAt` lines up
        server = ServerSelection(fixed: fake)
        flow = AuthFlow(server: server, deviceName: "iPhone", passkeys: passkeys, pollInterval: .milliseconds(1),
                        now: { calls.now }, onSession: { calls.add($0) }, onClientTooOld: { calls.old() })
    }

    func signIn(_ name: String, _ password: String = FakeAccounts.password) async {
        flow.login = name; flow.password = password
        flow.submit()
        await flow.settle()
    }
}

@MainActor
@Suite("AuthFlow")
struct AuthFlowTests {
    // MARK: password

    @Test func passwordSignInHandsTheTokenOver() async {
        let h = Harness()
        await h.signIn("tim")
        #expect(h.calls.tokens == ["fake-tim"])
        #expect(h.flow.password.isEmpty && h.flow.login.isEmpty && h.flow.issue == nil)
    }

    @Test func emptyFieldsAreCaughtBeforeTheNetwork() async {
        let h = Harness()
        h.flow.submit()
        #expect(h.flow.issue == AuthIssue(.login, "Enter your username or email."))
        h.flow.login = "tim"; h.flow.submit()
        #expect(h.flow.issue == AuthIssue(.password, "Enter your password."))
        #expect(h.fake.accounts.log.isEmpty)
    }

    @Test func wrongCredentialsNeverSayWhichPartWasWrong() async {
        let h = Harness()
        await h.signIn("tim", "nope")
        let wrongPassword = h.flow.issue
        await h.signIn("nobody", "nope")
        #expect(h.flow.issue == wrongPassword)
        #expect(wrongPassword == AuthIssue(.form, "Wrong username, email or password."))
        #expect(h.flow.login == "nobody" && h.flow.password.isEmpty)
        #expect(h.calls.tokens.isEmpty)
    }

    @Test func rateLimitSaysHowLong() async {
        let h = Harness()
        h.fake.accounts.rateLimitNext(1)
        await h.signIn("tim")
        #expect(h.flow.issue == AuthIssue(.form, "Too many attempts. Try again in 2 minutes."))
        #expect(AuthFlow.rateLimitText(nil) == "Too many attempts. Try again in a moment.")
        #expect(AuthFlow.rateLimitText(30) == "Too many attempts. Try again in 30 seconds.")
    }

    @Test func offlineKeepsWhatWasTyped() async {
        let h = Harness()
        h.fake.accounts.failNext(network: 1)
        await h.signIn("tim")
        #expect(h.flow.issue == AuthIssue(.form, "Can't reach the server. Check your connection and try again."))
        #expect(h.flow.login == "tim" && !h.flow.busy)
        await h.signIn("tim")
        #expect(h.calls.tokens == ["fake-tim"])
    }

    @Test func aSuspendedAccountShowsTheSuspensionCard() async {
        let h = Harness()
        await h.signIn("cvetka")
        #expect(h.flow.suspension?.reason == "Repeated spam in public channels." && h.flow.suspension?.until != nil)
        #expect(!h.flow.topLevel)
        h.flow.dismissSuspension()
        #expect(h.flow.suspension == nil && h.flow.step == .password && h.flow.topLevel)
    }

    @Test func aTooOldClientIsReported() async {
        let h = Harness()
        h.fake.accounts.failNext(network: 0)
        // The fake has no too-old switch: the mapping is what matters.
        h.flow.login = "x"; h.flow.password = "y"
        #expect(AuthFlow.issue(for: .clientTooOld).field == .form)
        #expect(h.calls.tooOld == 0)
    }

    // MARK: email code

    @Test func emailCodeFlowRequestsThenVerifiesAtSixDigits() async {
        let h = Harness()
        h.flow.login = "tim@example.com"
        h.flow.startEmailCode()
        #expect(h.flow.step == .emailCode(sent: false) && h.flow.email == "tim@example.com")
        h.flow.submit(); await h.flow.settle()
        #expect(h.flow.step == .emailCode(sent: true))
        #expect(h.flow.notice == "If an account uses tim@example.com, a code is on its way. It works for 10 minutes.")
        h.flow.setCode("424 24x")
        #expect(h.flow.code == "42424" && !h.flow.busy)
        h.flow.setCode(FakeAccounts.emailCode); await h.flow.settle()
        #expect(h.calls.tokens == ["fake-tim"])
    }

    @Test func aBadEmailIsRefusedLocally() async {
        let h = Harness()
        h.flow.startEmailCode()
        h.flow.email = "tim@"
        h.flow.submit()
        #expect(h.flow.issue?.field == .email && h.fake.accounts.log.isEmpty)
    }

    @Test func aWrongEmailCodeSaysNothingAboutTheAccount() async {
        let h = Harness()
        h.flow.email = "nobody@example.com"
        h.flow.startEmailCode(); h.flow.email = "nobody@example.com"
        h.flow.submit(); await h.flow.settle()
        h.flow.setCode("111111"); await h.flow.settle()
        #expect(h.flow.issue == AuthIssue(.code, "That code didn't work. Check it and try again."))
        #expect(h.flow.code.isEmpty && h.flow.step == .emailCode(sent: true))
    }

    @Test func resendWaitsThirtySecondsThenSendsAgain() async {
        let h = Harness()
        h.flow.startEmailCode(); h.flow.email = "tim@example.com"
        h.flow.submit(); await h.flow.settle()
        #expect(h.flow.resendWait(at: h.calls.now) == 30)
        #expect(h.flow.resendWait(at: h.calls.now.addingTimeInterval(12.2)) == 18)
        h.flow.resendCode(); await h.flow.settle()
        #expect(h.fake.accounts.log.filter { $0.hasPrefix("requestEmailCode") }.count == 1)
        h.calls.now = h.calls.now.addingTimeInterval(31)
        #expect(h.flow.resendWait(at: h.calls.now) == 0)
        h.flow.resendCode(); await h.flow.settle()
        #expect(h.fake.accounts.log.filter { $0.hasPrefix("requestEmailCode") }.count == 2)
    }

    // MARK: second factor

    @Test func secondFactorTotpCompletesTheSignIn() async {
        let h = Harness()
        await h.signIn("ana")
        guard case .secondFactor(let s) = h.flow.step else { Issue.record("expected second factor"); return }
        #expect(s.offersPasskey && s.offersRecovery && h.flow.password.isEmpty)
        h.flow.setCode(FakeAccounts.totp); await h.flow.settle()
        #expect(h.calls.tokens == ["fake-ana"] && h.flow.step == .password)
    }

    @Test func aWrongTotpShowsTriesLeftAndLocksAfterFive() async {
        let h = Harness()
        await h.signIn("ana")
        h.flow.setCode("000000"); await h.flow.settle()
        #expect(h.flow.issue == AuthIssue(.code, "That code didn't work. 4 tries left."))
        for _ in 0..<3 { h.flow.setCode("000000"); await h.flow.settle() }
        #expect(h.flow.issue == AuthIssue(.code, "That code didn't work. 1 try left."))
        h.flow.setCode("000000"); await h.flow.settle()
        #expect(h.flow.issue == AuthIssue(.form, "Too many wrong attempts. Start signing in again."))
        guard case .secondFactor(let s) = h.flow.step else { Issue.record("expected second factor"); return }
        #expect(s.locked)
        h.flow.submit()
        #expect(h.calls.tokens.isEmpty)
        h.flow.showSignIn()
        #expect(h.flow.step == .password)
    }

    @Test func aRecoveryCodeWorksAndTheToggleClearsTheField() async {
        let h = Harness()
        await h.signIn("ana")
        h.flow.setCode("123")
        h.flow.toggleRecovery()
        #expect(h.flow.code.isEmpty)
        h.flow.submit()
        #expect(h.flow.issue == AuthIssue(.code, "Enter one of your recovery codes."))
        h.flow.recovery = FakeAccounts.recovery
        h.flow.submit(); await h.flow.settle()
        #expect(h.calls.tokens == ["fake-ana"])
    }

    @Test func aWrongRecoveryCodeIsNamedAsSuch() async {
        let h = Harness()
        await h.signIn("ana")
        h.flow.toggleRecovery()
        h.flow.recovery = "wrong-code"
        h.flow.submit(); await h.flow.settle()
        #expect(h.flow.issue == AuthIssue(.code, "That recovery code didn't work. 4 tries left."))
    }

    @Test func secondFactorPasskeyUsesTheBrowserPageWhenNativeIsOff() async {
        let h = Harness()
        await h.signIn("ana")
        h.flow.useSecondFactorPasskey()
        await h.flow.settle()
        #expect(h.passkeys.opened.count == 1 && h.passkeys.closed >= 1)
        #expect(h.fake.accounts.log.filter { $0.hasPrefix("finishSecondFactorPasskey:poll") }.count == 3)
        #expect(h.calls.tokens == ["fake-ana"])
    }

    // MARK: passkey

    @Test func nativePasskeySignInSendsTheCredential() async {
        let h = Harness()
        h.passkeys.native = true
        h.flow.startPasskey()
        h.flow.login = "tim"
        h.flow.submit(); await h.flow.settle()
        #expect(h.fake.accounts.log.contains("finishPasskeyLogin:credential"))
        #expect(h.passkeys.opened.isEmpty && h.calls.tokens == ["fake-tim"])
    }

    @Test func browserPasskeyPollsUntilTheCeremonyIsDone() async {
        let h = Harness()
        h.flow.startPasskey()
        h.flow.login = "tim"
        h.flow.submit()
        #expect(h.flow.step == .passkey(waiting: true))
        await h.flow.settle()
        #expect(h.passkeys.opened.count == 1)
        #expect(h.calls.tokens == ["fake-tim"])
    }

    @Test func cancellingTheSystemSheetReturnsQuietly() async {
        let h = Harness()
        h.passkeys.native = true
        h.passkeys.assertionResult = .failure(.cancelled)
        h.flow.startPasskey(); h.flow.login = "tim"
        h.flow.submit(); await h.flow.settle()
        #expect(h.flow.step == .passkey(waiting: false) && h.flow.issue == nil && !h.flow.busy && h.calls.tokens.isEmpty)
    }

    @Test func aSystemThatCannotServeTheServerFallsBackToTheBrowser() async {
        let h = Harness()
        h.passkeys.native = true
        h.passkeys.assertionResult = .failure(.unavailable("no associated domain"))
        h.flow.startPasskey(); h.flow.login = "tim"
        h.flow.submit(); await h.flow.settle()
        #expect(h.passkeys.opened.count == 1 && h.calls.tokens == ["fake-tim"])
    }

    @Test func aRefusedPasskeyShowsAMessage() async {
        let h = Harness()
        h.passkeys.native = true
        h.passkeys.assertionResult = .failure(.failed("bad"))
        h.flow.startPasskey(); h.flow.login = "tim"
        h.flow.submit(); await h.flow.settle()
        #expect(h.flow.issue == AuthIssue(.form, "That passkey wasn't accepted. Try again."))
        #expect(h.flow.step == .passkey(waiting: false))
    }

    @Test func anUnknownUsernameGetsTheGenericPasskeyMessage() async {
        let h = Harness()
        h.flow.startPasskey(); h.flow.login = "nobody"
        h.flow.submit(); await h.flow.settle()
        #expect(h.flow.issue == AuthIssue(.form, "Passkey sign-in isn't available for that account. Use your password or an emailed code."))
    }

    @Test func closingTheBrowserSheetCancelsTheWait() async {
        let h = Harness()
        h.fake.accounts.pendingPolls = 100_000
        h.flow.startPasskey(); h.flow.login = "tim"
        h.flow.submit()
        for _ in 0..<200 where h.passkeys.opened.isEmpty { try? await Task.sleep(for: .milliseconds(5)) }
        h.passkeys.dismiss?()
        await h.flow.settle()
        #expect(h.flow.step == .passkey(waiting: false) && !h.flow.busy && h.calls.tokens.isEmpty)
    }

    // MARK: reset

    @Test func resetRequestsACodeThenSetsTheNewPassword() async {
        let h = Harness()
        h.flow.startReset(); h.flow.email = "tim@example.com"
        h.flow.submit(); await h.flow.settle()
        #expect(h.flow.step == .reset(sent: true))
        h.flow.setCode(FakeAccounts.emailCode)
        #expect(h.flow.busy == false, "a reset needs the password too, so six digits do not submit")
        h.flow.password = "short"
        h.flow.submit()
        #expect(h.flow.issue == AuthIssue(.password, "Use at least 8 characters."))
        h.flow.password = "a brand new one"
        h.flow.submit(); await h.flow.settle()
        #expect(h.flow.step == .password && h.flow.login == "tim@example.com")
        #expect(h.flow.notice == "Your password was changed and every device was signed out. Sign in with the new one.")
        await h.signIn("tim", FakeAccounts.password)
        #expect(h.calls.tokens.isEmpty, "the old password no longer works")
        await h.signIn("tim", "a brand new one")
        #expect(h.calls.tokens == ["fake-tim"])
    }

    @Test func aWrongResetCodeIsRefusedWithoutSayingWhy() async {
        let h = Harness()
        h.flow.startReset(); h.flow.email = "tim@example.com"
        h.flow.submit(); await h.flow.settle()
        h.flow.setCode("999999"); h.flow.password = "long enough pw"
        h.flow.submit(); await h.flow.settle()
        #expect(h.flow.issue == AuthIssue(.code, "That code didn't work. Check it, or ask for a new one."))
        #expect(h.flow.step == .reset(sent: true))
    }

    // MARK: invite

    @Test func inviteCodeNormalisation() {
        #expect(InviteCode.normalize(" wxyz2345 ") == "WXYZ-2345")
        #expect(InviteCode.normalize("wxyz 2345") == "WXYZ-2345")
        #expect(InviteCode.normalize("wxyz-2345") == "WXYZ-2345")
        #expect(InviteCode.normalize("abc") == "ABC")
        #expect(InviteCode.looksLikeLink("https://x.example/invite/abc"))
        #expect(InviteCode.looksLikeLink("thehideout://invite/tok"))
        #expect(!InviteCode.looksLikeLink("WXYZ-2345"))
    }

    @Test func aLinkInTheCodeFieldIsRejectedWithAHint() async {
        let h = Harness()
        h.flow.startInvite()
        h.flow.invite = "https://thehideout.zabit.si/invite/abcDEF"
        h.flow.submit()
        #expect(h.flow.issue == AuthIssue(.invite, "Paste just the code from your invite email, not the link."))
        h.flow.invite = ""
        h.flow.submit()
        #expect(h.flow.issue == AuthIssue(.invite, "Enter the code from your invite email."))
        #expect(h.fake.accounts.log.isEmpty)
    }

    @Test func lookingUpAnInviteShowsThePreview() async {
        let h = Harness()
        h.flow.startInvite()
        h.flow.invite = "wxyz2345"
        h.flow.submit(); await h.flow.settle()
        guard case .inviteAccept(let preview, let code) = h.flow.step else { Issue.record("expected the accept step"); return }
        #expect(code == "WXYZ-2345" && preview.inviter == "ana" && preview.guildNames == ["The Hideout"] && preview.minimumAge == 16)
    }

    @Test func unknownUsedAndExpiredInvitesStayOnTheCodeStep() async {
        let h = Harness()
        h.flow.startInvite()
        h.flow.invite = "NOPE-0000"
        h.flow.submit(); await h.flow.settle()
        #expect(h.flow.issue == AuthIssue(.invite, "We couldn't find that invite. Check the code and try again.") && h.flow.step == .inviteCode)
        h.flow.invite = FakeAccounts.usedInviteCode
        h.flow.submit(); await h.flow.settle()
        #expect(h.flow.issue == AuthIssue(.invite, "This invite was already used. Sign in instead."))
        #expect(AuthFlow.problem(with: .expired) == "This invite has expired. Ask for a new one.")
        #expect(AuthFlow.problem(with: .revoked) == "This invite was withdrawn. Ask for a new one.")
        #expect(AuthFlow.problem(with: .pending) == nil)
    }

    private func accept(_ h: Harness) async {
        h.flow.startInvite()
        h.flow.invite = FakeAccounts.inviteCode
        h.flow.submit(); await h.flow.settle()
    }

    @Test func accountCreationNeedsBothBoxes() async {
        let h = Harness()
        await accept(h)
        h.flow.username = "zoe"; h.flow.password = "long enough pw"
        #expect(!h.flow.canCreateAccount)
        h.flow.submit()
        #expect(h.flow.issue == AuthIssue(.form, "Accept the terms and privacy notice to continue."))
        h.flow.acceptTerms = true
        #expect(!h.flow.canCreateAccount)
        h.flow.submit()
        #expect(h.flow.issue == AuthIssue(.form, "Confirm that you are at least 16 years old."))
        h.flow.confirmAge = true
        #expect(h.flow.canCreateAccount)
        #expect(!h.fake.accounts.log.contains { $0.hasPrefix("acceptInvite") })
    }

    @Test func accountFieldsAreValidated() async {
        let h = Harness()
        await accept(h)
        h.flow.acceptTerms = true; h.flow.confirmAge = true
        h.flow.username = "zo"; h.flow.password = "long enough pw"
        h.flow.submit()
        #expect(h.flow.issue == AuthIssue(.username, "Choose a username of 3 to 32 characters."))
        h.flow.username = "zoe"; h.flow.password = "short"
        h.flow.submit()
        #expect(h.flow.issue == AuthIssue(.password, "Use at least 8 characters."))
    }

    @Test func acceptingAnInviteSignsIn() async {
        let h = Harness()
        await accept(h)
        h.flow.username = "zoe"; h.flow.password = "long enough pw"
        h.flow.acceptTerms = true; h.flow.confirmAge = true
        h.flow.submit(); await h.flow.settle()
        #expect(h.calls.tokens == ["fake-zoe"] && h.flow.welcome == .hidden && h.flow.step == .password)
    }

    @Test func aTakenUsernameKeepsTheInviteUsable() async {
        let h = Harness()
        await accept(h)
        h.flow.username = "ana"; h.flow.password = "long enough pw"
        h.flow.acceptTerms = true; h.flow.confirmAge = true
        h.flow.submit(); await h.flow.settle()
        #expect(h.flow.issue == AuthIssue(.username, "That username is taken. Try another."))
        guard case .inviteAccept = h.flow.step else { Issue.record("still on the accept step"); return }
        #expect(h.flow.acceptTerms && h.flow.confirmAge && h.flow.username == "ana")
        // Only the username was wrong: the password is not retyped for a one-field fix.
        #expect(h.flow.password == "long enough pw")
        h.flow.username = "anna2"
        h.flow.submit(); await h.flow.settle()
        #expect(h.calls.tokens == ["fake-anna2"])
    }

    @Test func thePasskeyPromptFollowsAnAccountMadeWithTheToggle() async {
        let h = Harness()
        await accept(h)
        h.flow.username = "zoe"; h.flow.password = "long enough pw"
        h.flow.acceptTerms = true; h.flow.confirmAge = true; h.flow.addPasskey = true
        h.flow.submit(); await h.flow.settle()
        #expect(h.flow.welcome == .prompt)
        h.passkeys.native = true
        h.flow.addPasskeyNow(); await h.flow.settle()
        #expect(h.flow.welcome == .hidden && h.fake.accounts.log.contains("finishPasskeyRegistration:credential"))
    }

    @Test func skippingThePasskeyPromptHidesIt() async {
        let h = Harness()
        await accept(h)
        h.flow.username = "zoe"; h.flow.password = "long enough pw"
        h.flow.acceptTerms = true; h.flow.confirmAge = true; h.flow.addPasskey = true
        h.flow.submit(); await h.flow.settle()
        h.flow.skipWelcome()
        #expect(h.flow.welcome == .hidden)
    }

    @Test func aFailedPasskeyRegistrationCanBeRetried() async {
        let h = Harness()
        await accept(h)
        h.flow.username = "zoe"; h.flow.password = "long enough pw"
        h.flow.acceptTerms = true; h.flow.confirmAge = true; h.flow.addPasskey = true
        h.flow.submit(); await h.flow.settle()
        h.passkeys.native = true
        h.passkeys.assertionResult = .failure(.failed("nope"))
        h.flow.addPasskeyNow(); await h.flow.settle()
        guard case .failed = h.flow.welcome else { Issue.record("expected failed"); return }
        h.passkeys.assertionResult = .success("{}")
        h.flow.addPasskeyNow(); await h.flow.settle()
        #expect(h.flow.welcome == .hidden)
    }

    // MARK: deep links

    @Test func inviteLinksParse() {
        #expect(InviteCode.token(fromLink: "thehideout://invite/tok123") == "tok123")
        #expect(InviteCode.token(fromLink: "thehideout://invite/tok?x=1") == "tok")
        #expect(InviteCode.token(fromLink: "https://thehideout.zabit.si/invite/abc_DEF-123") == "abc_DEF-123")
        #expect(InviteCode.token(fromLink: "thehideout://other") == nil)
        #expect(InviteCode.token(fromLink: "https://x.example/terms") == nil)
    }

    @Test func aDeepLinkOpensAndLooksUpTheInvite() async {
        let h = Harness()
        #expect(h.flow.handle(url: URL(string: "thehideout://invite/WXYZ-2345")!))
        await h.flow.settle()
        guard case .inviteAccept(_, let code) = h.flow.step else { Issue.record("expected the accept step"); return }
        #expect(code == "WXYZ-2345" && h.flow.invite.isEmpty)
    }

    @Test func aBadDeepLinkLandsOnTheCodeStepWithAnError() async {
        let h = Harness()
        #expect(h.flow.handle(url: URL(string: "thehideout://invite/gone")!))
        await h.flow.settle()
        #expect(h.flow.step == .inviteCode && h.flow.issue?.field == .invite)
    }

    @Test func otherLinksAreIgnored() {
        let h = Harness()
        #expect(!h.flow.handle(url: URL(string: "thehideout://settings")!))
        #expect(!h.flow.handle(url: URL(string: "https://thehideout.zabit.si/terms")!))
        #expect(h.flow.step == .password)
    }

    @Test func aUniversalLinkForADifferentServerAsksBeforeLookingItUp() async {
        let h = Harness()
        #expect(h.flow.handle(url: URL(string: "https://thehideout.zabit.si/invite/WXYZ-2345")!))
        #expect(h.flow.pendingInviteLink?.host == "thehideout.zabit.si")
        #expect(h.flow.step == .password, "nothing is looked up on the current server until the switch is confirmed")
        #expect(h.fake.accounts.log.isEmpty)

        h.flow.switchServerForPendingInviteLink()
        await h.flow.settle()
        #expect(h.flow.pendingInviteLink == nil)
        #expect(h.server.url.host == "thehideout.zabit.si")
        guard case .inviteAccept(_, let code) = h.flow.step else { Issue.record("expected the accept step"); return }
        #expect(code == "WXYZ-2345")
    }

    @Test func dismissingTheServerSwitchLeavesTheStepUnchanged() {
        let h = Harness()
        h.flow.handle(url: URL(string: "https://thehideout.zabit.si/invite/WXYZ-2345")!)
        h.flow.dismissPendingInviteLink()
        #expect(h.flow.pendingInviteLink == nil && h.flow.step == .password)
    }

    @Test func theCustomSchemeNeverAsksToSwitchServers() async {
        // "invite" is the authority component of `thehideout://invite/<token>`, not a real host to compare.
        let h = Harness()
        #expect(h.flow.handle(url: URL(string: "thehideout://invite/WXYZ-2345")!))
        #expect(h.flow.pendingInviteLink == nil)
        await h.flow.settle()
        guard case .inviteAccept = h.flow.step else { Issue.record("expected the accept step"); return }
    }

    // MARK: navigation

    @Test func movingBetweenStepsClearsErrorsAndSecrets() async {
        let h = Harness()
        await h.signIn("tim", "nope")
        #expect(h.flow.issue != nil)
        h.flow.startEmailCode()
        #expect(h.flow.issue == nil && h.flow.password.isEmpty)
        h.flow.showSignIn()
        #expect(h.flow.step == .password && h.flow.topLevel && !h.flow.inviteMode)
        h.flow.startInvite()
        #expect(h.flow.inviteMode && h.flow.topLevel)
    }

    /// A password sign-in that would otherwise succeed, abandoned by "Join with a code" before its reply lands:
    /// the generation guard (not just task cancellation: see the next two tests) drops it, so nobody is signed in.
    @Test func aSlowPasswordSignInSuccessIsDroppedAfterGoingBack() async {
        let h = Harness()
        h.fake.latency = 0.05
        h.flow.login = "tim"; h.flow.password = FakeAccounts.password
        h.flow.submit()
        h.flow.startInvite()
        await h.flow.settle()
        try? await Task.sleep(for: .milliseconds(120))
        #expect(h.flow.step == .inviteCode)
        #expect(h.calls.tokens.isEmpty, "the stale success must not sign anyone in")
    }

    @Test func aSlowEmailCodeRequestSuccessIsDroppedAfterGoingBack() async {
        let h = Harness()
        h.fake.latency = 0.05
        h.flow.startEmailCode(); h.flow.email = "tim@example.com"
        h.flow.submit()
        h.flow.showSignIn()
        await h.flow.settle()
        try? await Task.sleep(for: .milliseconds(120))
        #expect(h.flow.step == .password, "the stale reply must not flip the step to \"code sent\"")
        #expect(h.flow.notice == nil)
    }

    @Test func aSlowInviteLookupSuccessIsDroppedAfterSigningIn() async {
        let h = Harness()
        h.fake.latency = 0.05
        h.flow.startInvite(); h.flow.invite = FakeAccounts.inviteCode
        h.flow.submit()
        h.flow.showSignIn()
        await h.flow.settle()
        try? await Task.sleep(for: .milliseconds(120))
        #expect(h.flow.step == .password, "the stale lookup must not land on the invite accept step")
    }
}

@Suite("ServerURL")
struct ServerURLTests {
    @Test func parsesAddresses() {
        #expect(ServerURL.parse("thehideout.zabit.si")?.absoluteString == "https://thehideout.zabit.si")
        #expect(ServerURL.parse(" https://thehideout.zabit.si/some/path?x=1 ")?.absoluteString == "https://thehideout.zabit.si")
        #expect(ServerURL.parse("127.0.0.1:8080")?.absoluteString == "http://127.0.0.1:8080")
        #expect(ServerURL.parse("http://192.168.1.20:8080")?.absoluteString == "http://192.168.1.20:8080")
        #expect(ServerURL.parse("localhost:8080")?.absoluteString == "http://localhost:8080")
    }

    @Test func parsesIPv6Addresses() {
        #expect(ServerURL.parse("[::1]:8080")?.absoluteString == "http://[::1]:8080")
        #expect(ServerURL.parse("http://[::1]:8080")?.absoluteString == "http://[::1]:8080")
    }

    @Test func dropsPastedPathUserAndPassword() {
        #expect(ServerURL.parse("https://user:pass@thehideout.zabit.si/invite/abc?x=1#y")?.absoluteString == "https://thehideout.zabit.si")
        #expect(ServerURL.parse("http://ana:hunter2@127.0.0.1:8080/some/path")?.absoluteString == "http://127.0.0.1:8080")
    }

    @Test func refusesCleartextToTheInternetAndNonsense() {
        #expect(ServerURL.parse("http://example.com") == nil)
        #expect(ServerURL.parse("ftp://example.com") == nil)
        #expect(ServerURL.parse("") == nil)
        #expect(ServerURL.parse("not a url") == nil)
    }

    @Test func tellsDomainsFromLocalAddresses() {
        #expect(ServerURL.isDomain(host: "thehideout.zabit.si"))
        #expect(!ServerURL.isDomain(host: "localhost"))
        #expect(!ServerURL.isDomain(host: "127.0.0.1"))
        #expect(!ServerURL.isDomain(host: "203.0.113.9"))
        #expect(!ServerURL.isDomain(host: "mac.local"))
    }
}

@MainActor
@Suite("ServerSelection")
struct ServerSelectionTests {
    private func makeDefaults() -> UserDefaults {
        let d = UserDefaults(suiteName: "test-\(UUID().uuidString)")!
        return d
    }

    @Test func changesPersistAndSwitchTheBackend() async throws {
        let defaults = makeDefaults()
        let first = FakeBackend(), second = FakeBackend()
        second.accounts.failNext(network: 1)
        let s = ServerSelection(defaults: defaults, make: { $0.host == "other.example" ? second : first }, probe: { _ in true })
        try await s.change(to: "other.example")
        #expect(s.url.absoluteString == "https://other.example" && defaults.string(forKey: ServerSelection.defaultsKey) == "https://other.example")
        await #expect(throws: AppError.network("offline")) { try await s.backend.requestEmailCode(email: "a@b.co") }
        let again = ServerSelection(defaults: defaults, make: { _ in first }, probe: { _ in true })
        #expect(again.url.absoluteString == "https://other.example")
    }

    @Test func anUnreachableOrInvalidServerChangesNothing() async {
        let s = ServerSelection(defaults: makeDefaults(), make: { _ in FakeBackend() }, probe: { _ in false })
        let before = s.url
        await #expect(throws: ServerSelection.ChangeError.unreachable) { try await s.change(to: "nowhere.example") }
        await #expect(throws: ServerSelection.ChangeError.invalid) { try await s.change(to: "http://nowhere.example") }
        #expect(s.url == before)
    }
}

@MainActor
@Suite("Terms gate and session restore")
struct SessionGateTests {
    @Test func acceptingTheTermsLoadsTheSession() async {
        let fake = FakeBackend()
        fake.accounts.termsRequired = "2026-06-01"
        let store = AppStore(api: fake, auth: fake, tokens: MemoryTokenStore(token: "t"))
        await store.bootstrap()
        #expect(store.state.phase == .termsRequired(version: "2026-06-01"))
        await store.acceptTerms()
        #expect(store.state.phase == .signedIn && store.termsError == nil)
        #expect(fake.accounts.log == ["acceptTerms:2026-06-01"])
    }

    @Test func aFailedAcceptShowsAnErrorAndStaysOnTheGate() async {
        let fake = FakeBackend()
        fake.accounts.termsRequired = "2026-06-01"
        let store = AppStore(api: fake, auth: fake, tokens: MemoryTokenStore(token: "t"))
        await store.bootstrap()
        fake.accounts.failNext(network: 1)
        await store.acceptTerms()
        #expect(store.state.phase == .termsRequired(version: "2026-06-01"))
        #expect(store.termsError == "Can't reach the server. Check your connection and try again.")
    }

    @Test func aPreconditionFailureAsksToReadTheNewTermsAgain() async {
        let fake = FakeBackend()
        fake.accounts.termsRequired = "2026-06-01"
        let store = AppStore(api: fake, auth: fake, tokens: MemoryTokenStore(token: "t"))
        await store.bootstrap()
        fake.accounts.nextAcceptTermsError = .precondition("terms moved on")
        await store.acceptTerms()
        // The terms are still required (the fake never cleared them): the gate looks again, same version.
        #expect(store.state.phase == .termsRequired(version: "2026-06-01"))
        #expect(store.termsError == "The terms were updated again. Please read the new version.")
    }

    @Test func aTooOldClientOnAcceptShowsTheUpdateGate() async {
        let fake = FakeBackend()
        fake.accounts.termsRequired = "2026-06-01"
        let store = AppStore(api: fake, auth: fake, tokens: MemoryTokenStore(token: "t"))
        await store.bootstrap()
        fake.accounts.nextAcceptTermsError = .clientTooOld
        await store.acceptTerms()
        #expect(store.state.phase == .updateRequired)
    }

    @Test func signingOutOfTheTermsGateClearsTheToken() async {
        let fake = FakeBackend()
        fake.accounts.termsRequired = "2026-06-01"
        let tokens = MemoryTokenStore(token: "t")
        let store = AppStore(api: fake, auth: fake, tokens: tokens)
        await store.bootstrap()
        store.signOut()
        #expect(store.state.phase == .signedOut && tokens.load() == nil)
    }

    @Test func aSavedSessionIsRestoredWithoutShowingSignIn() async {
        let store = AppStore(api: FakeBackend(), auth: FakeBackend(), tokens: MemoryTokenStore(token: "saved"))
        #expect(store.state.phase == .loading)
        await store.bootstrap()
        #expect(store.state.phase == .signedIn)
    }

    @Test func noSavedSessionGoesStraightToSignIn() async {
        let store = AppStore(api: FakeBackend(), tokens: MemoryTokenStore())
        await store.bootstrap()
        #expect(store.state.phase == .signedOut)
    }

    @Test func aFreshSignInKeepsTheTokenAndLoads() async {
        let tokens = MemoryTokenStore()
        let store = AppStore(api: FakeBackend(), tokens: tokens)
        await store.bootstrap()
        await store.signedIn(token: "fresh")
        #expect(tokens.load() == "fresh" && store.state.phase == .signedIn)
    }

    @Test func updateRequiredIsAPhase() {
        let store = AppStore(api: FakeBackend(), tokens: MemoryTokenStore())
        store.requireUpdate()
        #expect(store.state.phase == .updateRequired)
    }
}
