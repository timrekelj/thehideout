import HideoutCore
import HideoutUI
import SwiftUI
import XCTest

/// Every signed-out screen and gate, dark and light on iPhone and iPad: sign in, email code, second factor, passkey, reset,
/// invite, suspended, the terms, update and splash screens and the passkey prompt. The flows are driven through
/// `AuthFlow` against the fake accounts, so the pictures show real states (errors from the fake server, tries left).
@MainActor
final class AuthSnapshotTests: SnapshotCase {
    struct Rig {
        let flow: AuthFlow
        let server: ServerSelection
        let store: AppStore
        let fake: FakeBackend
    }

    func rig(token: String? = nil) async -> Rig {
        registerFonts()
        let fake = FakeBackend(now: snapshotNow)
        let server = ServerSelection(fixed: fake)
        let store = AppStore(api: server.backend, auth: server.backend, tokens: MemoryTokenStore(token: token), now: { snapshotNow })
        let flow = AuthFlow(server: server, deviceName: "iPhone", now: { snapshotNow }, onSession: { _ in })
        return Rig(flow: flow, server: server, store: store, fake: fake)
    }

    /// Dark and light on the phone, and the iPad card in both.
    func shoot(_ r: Rig, name: String = #function, file: StaticString = #filePath, line: UInt = #line) {
        assertScreen(AuthScreen(), store: r.store, auth: r.flow, server: r.server, testName: name)
        assertScreen(AuthScreen(), store: r.store, auth: r.flow, server: r.server, device: .iPad, testName: name)
    }

    func testSignIn() async {
        let r = await rig()
        shoot(r)
    }

    func testSignInWrongPassword() async {
        let r = await rig()
        r.flow.login = "tim"; r.flow.password = "nope"
        r.flow.submit(); await r.flow.settle()
        shoot(r)
    }

    func testSignInRateLimited() async {
        let r = await rig()
        r.fake.accounts.rateLimitNext(1)
        r.flow.login = "tim"; r.flow.password = FakeAccounts.password
        r.flow.submit(); await r.flow.settle()
        shoot(r)
    }

    func testSignInOffline() async {
        let r = await rig()
        r.fake.accounts.failNext(network: 1)
        r.flow.login = "tim"; r.flow.password = FakeAccounts.password
        r.flow.submit(); await r.flow.settle()
        shoot(r)
    }

    func testEmailRequest() async {
        let r = await rig()
        r.flow.startEmailCode()
        shoot(r)
    }

    func testEmailCode() async {
        let r = await rig()
        r.flow.login = "tim@example.com"
        r.flow.startEmailCode()
        r.flow.submit(); await r.flow.settle()
        r.flow.setCode("424")
        shoot(r)
    }

    func testEmailCodeWrong() async {
        let r = await rig()
        r.flow.login = "tim@example.com"
        r.flow.startEmailCode()
        r.flow.submit(); await r.flow.settle()
        r.flow.setCode("111111"); await r.flow.settle()
        shoot(r)
    }

    func testSecondFactor() async {
        let r = await rig()
        r.flow.login = "ana"; r.flow.password = FakeAccounts.password
        r.flow.submit(); await r.flow.settle()
        shoot(r)
    }

    func testSecondFactorTriesLeft() async {
        let r = await rig()
        r.flow.login = "ana"; r.flow.password = FakeAccounts.password
        r.flow.submit(); await r.flow.settle()
        r.flow.setCode("000000"); await r.flow.settle()
        r.flow.setCode("12")
        shoot(r)
    }

    func testRecoveryCode() async {
        let r = await rig()
        r.flow.login = "ana"; r.flow.password = FakeAccounts.password
        r.flow.submit(); await r.flow.settle()
        r.flow.toggleRecovery()
        r.flow.recovery = "abcde-fghi"
        shoot(r)
    }

    func testPasskey() async {
        let r = await rig()
        r.flow.startPasskey()
        shoot(r)
    }

    func testResetRequest() async {
        let r = await rig()
        r.flow.startReset()
        shoot(r)
    }

    func testResetCode() async {
        let r = await rig()
        r.flow.startReset(); r.flow.email = "tim@example.com"
        r.flow.submit(); await r.flow.settle()
        r.flow.setCode("424242")
        r.flow.password = "a brand new one"
        shoot(r)
    }

    func testInviteCode() async {
        let r = await rig()
        r.flow.startInvite()
        shoot(r)
    }

    func testInviteCodeFromPastedLink() async {
        let r = await rig()
        r.flow.startInvite()
        r.flow.setInvite("https://thehideout.zabit.si/invite/NOPE-0000")
        await r.flow.settle()
        shoot(r)
    }

    private func acceptStep(_ r: Rig) async {
        r.flow.startInvite()
        r.flow.setInvite(FakeAccounts.inviteCode)
        await r.flow.settle()
    }

    func testInviteAccept() async {
        let r = await rig()
        await acceptStep(r)
        shoot(r)
    }

    func testInviteAcceptFilled() async {
        let r = await rig()
        await acceptStep(r)
        r.flow.username = "zoe"; r.flow.password = "long enough pw"
        r.flow.addPasskey = true; r.flow.acceptTerms = true; r.flow.confirmAge = true
        shoot(r)
    }

    func testInviteAcceptUsernameTaken() async {
        let r = await rig()
        await acceptStep(r)
        r.flow.username = "ana"; r.flow.password = "long enough pw"
        r.flow.acceptTerms = true; r.flow.confirmAge = true
        r.flow.submit(); await r.flow.settle()
        shoot(r)
    }

    func testSuspendedOnSignIn() async {
        let r = await rig()
        r.flow.login = "cvetka"; r.flow.password = FakeAccounts.password
        r.flow.submit(); await r.flow.settle()
        shoot(r)
    }

    // MARK: gates

    func testSplash() async {
        let r = await rig(token: "t")
        assertScreen(RootView(), store: r.store, auth: r.flow, server: r.server)
    }

    func testSplashOffline() async {
        let r = await rig(token: "t")
        let store = AppStore(api: BrokenApi(), tokens: MemoryTokenStore(token: "t"))
        await store.bootstrap()
        assertScreen(RootView(), store: store, auth: r.flow, server: r.server)
    }

    func testTermsGate() async {
        let r = await rig(token: "t")
        r.fake.accounts.termsRequired = "2026-06-01"
        await r.store.bootstrap()
        assertScreen(RootView(), store: r.store, auth: r.flow, server: r.server)
        assertScreen(RootView(), store: r.store, auth: r.flow, server: r.server, device: .iPad)
    }

    func testTermsGateError() async {
        let r = await rig(token: "t")
        r.fake.accounts.termsRequired = "2026-06-01"
        await r.store.bootstrap()
        r.fake.accounts.failNext(network: 1)
        await r.store.acceptTerms()
        assertScreen(RootView(), store: r.store, auth: r.flow, server: r.server)
    }

    func testSuspendedGate() async {
        let r = await rig(token: "t")
        r.fake.accounts.suspendMe = true
        await r.store.bootstrap()
        assertScreen(RootView(), store: r.store, auth: r.flow, server: r.server)
        assertScreen(RootView(), store: r.store, auth: r.flow, server: r.server, device: .iPad)
    }

    func testUpdateRequired() async {
        let r = await rig(token: "t")
        r.store.requireUpdate()
        assertScreen(RootView(), store: r.store, auth: r.flow, server: r.server)
        assertScreen(RootView(), store: r.store, auth: r.flow, server: r.server, device: .iPad)
    }

    func testWelcomePasskeySheet() async {
        let r = await rig(token: "t")
        await r.store.bootstrap()
        await acceptStep(r)
        r.flow.username = "zoe"; r.flow.password = "long enough pw"
        r.flow.addPasskey = true; r.flow.acceptTerms = true; r.flow.confirmAge = true
        r.flow.submit(); await r.flow.settle()
        // `RootView` only presents this as a `.sheet`, which an offscreen `UIHostingController` cannot reliably
        // capture; snapshot the sheet's own content view, the same way `testServerSheet` does.
        assertScreen(WelcomeSheet(), store: r.store, auth: r.flow, server: r.server)
    }

    func testServerSheet() async {
        let r = await rig()
        assertScreen(ServerSheet(), store: r.store, auth: r.flow, server: r.server)
    }

    func testLargeText() async {
        let r = await rig()
        r.flow.login = "tim"; r.flow.password = "nope"
        r.flow.submit(); await r.flow.settle()
        assertScreen(AuthScreen(), store: r.store, auth: r.flow, server: r.server, looks: [Looks.text125], testName: "testLargeText")
    }
}

/// An api that never answers: the splash's offline state.
private struct BrokenApi: ApiClient {
    func me() async throws -> Me { throw AppError.network("offline") }
    func snapshot() async throws -> Snapshot { throw AppError.network("offline") }
    func listChannels(guild: ID) async throws -> (channels: [Channel], layout: HideoutCore.Layout) { throw AppError.network("offline") }
    func listMembers(guild: ID) async throws -> [Member] { throw AppError.network("offline") }
    func listRoles(guild: ID) async throws -> (roles: [Role], version: UInt64) { throw AppError.network("offline") }
    func listEmojis(guild: ID) async throws -> [Emoji] { throw AppError.network("offline") }
    func listMessages(channel: ID, before: ID?, limit: Int) async throws -> MessagePage { throw AppError.network("offline") }
    func sendMessage(channel: ID, content: String, nonce: String, attachmentIDs: [ID]) async throws -> Message { throw AppError.network("offline") }
    func editMessage(channel: ID, message: ID, content: String) async throws -> Message { throw AppError.network("offline") }
    func deleteMessage(channel: ID, message: ID) async throws { throw AppError.network("offline") }
    func addReaction(channel: ID, message: ID, emoji: ReactionEmoji) async throws { throw AppError.network("offline") }
    func removeReaction(channel: ID, message: ID, emoji: ReactionEmoji) async throws { throw AppError.network("offline") }
    func updateLayout(guild: ID, expectedVersion: UInt64, sectionIDs: [ID], entries: [LayoutEntry]) async throws -> HideoutCore.Layout { throw AppError.network("offline") }
    func startDm(user: ID) async throws -> DMChannel { throw AppError.network("offline") }
    func subscribe() -> AsyncThrowingStream<GatewayEvent, Error> { AsyncThrowingStream { $0.finish(throwing: AppError.network("offline")) } }
}
