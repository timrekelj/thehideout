import XCTest

/// The signed-out flows against the fake world (`--ui-testing`): sign in, the invite from code to shell, and the
/// inline error of a wrong code. `testAuthTour` additionally drives every auth screen and gate and writes a
/// screenshot of each to `TOUR_DIR` (default `/tmp/shots/tour`) for the design review; run it with
/// `-only-testing:HideoutUITests/AuthUITests/testAuthTour`.
final class AuthUITests: XCTestCase {
    private let password = "correct-horse"
    private let emailCode = "424242"
    private let inviteCode = "WXYZ-2345"

    // MARK: helpers

    private func launch(_ args: [String] = []) -> XCUIApplication {
        let app = XCUIApplication()
        app.launchArguments = ["--ui-testing"] + args
        app.launch()
        return app
    }

    private func shot(_ app: XCUIApplication, _ name: String) {
        let dir = ProcessInfo.processInfo.environment["TOUR_DIR"] ?? "/tmp/shots/tour"
        let suffix = ProcessInfo.processInfo.environment["TOUR_SUFFIX"] ?? ""
        let idiom = UIDevice.current.userInterfaceIdiom == .pad ? "ipad" : "iphone"
        try? FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
        let file = "\(dir)/\(idiom)-\(name)\(suffix).png"
        try? app.screenshot().pngRepresentation.write(to: URL(fileURLWithPath: file))
    }

    private func type(_ app: XCUIApplication, into label: String, secure: Bool = false, _ text: String) {
        let field = secure ? app.secureTextFields[label] : app.textFields[label]
        XCTAssertTrue(field.waitForExistence(timeout: 10), "missing field \(label)")
        field.tap()
        if secure {
            // The system's "Use Strong Password?" callout appears as soon as a `.newPassword` field is focused;
            // dismiss it and refocus before typing, or it can swallow part of the typed text.
            let closeCallout = app.buttons["Close"]
            if closeCallout.waitForExistence(timeout: 1) {
                closeCallout.tap()
                field.tap()
            }
        }
        app.typeText(text)
    }

    /// `CheckRow`'s checkbox carries `.isToggle`, so XCUITest exposes it as a switch, not a button; everything
    /// else (`PrimaryButton`, `QuietLink`, `BackLink`, ...) is a real button.
    private func tap(_ app: XCUIApplication, _ label: String) {
        if app.switches[label].exists {
            let s = app.switches[label]
            if !s.isHittable { app.swipeUp() }
            // The Terms/Privacy row's accessible frame spans the whole label (its text has markdown links),
            // wider than the 44pt checkbox at its leading edge: tap there, not the element's center.
            s.coordinate(withNormalizedOffset: CGVector(dx: 0.05, dy: 0.5)).tap()
            return
        }
        let b = app.buttons[label]
        XCTAssertTrue(b.waitForExistence(timeout: 10), "missing button \(label)")
        if !b.isHittable { app.swipeUp() }
        b.tap()
    }

    /// Dismisses the keyboard (a drag on the auth card's scroll view, which is `.scrollDismissesKeyboard(.interactively)`),
    /// so fields and checkboxes further down the invite accept form are hittable instead of sitting below the fold.
    private func dismissKeyboard(_ app: XCUIApplication) {
        if app.keyboards.element.exists { app.swipeUp() }
    }

    private func waitForText(_ app: XCUIApplication, containing needle: String, timeout: TimeInterval = 10) -> Bool {
        app.descendants(matching: .any).matching(NSPredicate(format: "label CONTAINS[c] %@", needle)).firstMatch
            .waitForExistence(timeout: timeout)
    }

    private var shellAppeared: (XCUIApplication) -> Bool {
        { app in app.tabBars.buttons["Servers"].waitForExistence(timeout: 15) }
    }

    // MARK: tests

    /// Sign in with a password: the tab shell appears.
    func testSignInWithPasswordShowsShell() {
        let app = launch(["--signed-out"])
        type(app, into: "Username or email", "tim")
        type(app, into: "Password", secure: true, password)
        tap(app, "Sign in")
        XCTAssertTrue(shellAppeared(app))
    }

    /// Invite code -> preview -> terms and age ticked -> create account -> the tab shell appears.
    func testInviteCodePreviewAcceptShowsShell() {
        let app = launch(["--signed-out"])
        tap(app, "Join with a code")
        type(app, into: "Invite code", inviteCode.replacingOccurrences(of: "-", with: ""))
        tap(app, "Continue")
        XCTAssertTrue(waitForText(app, containing: "invited you to join"), "the invite preview did not appear")
        type(app, into: "Choose a username", "zoe")
        type(app, into: "Choose a password", secure: true, "long-enough-pw")
        dismissKeyboard(app)
        tap(app, "I agree to the Terms and Privacy Policy")
        tap(app, "I'm 16 or older")
        tap(app, "Create account")
        XCTAssertTrue(shellAppeared(app))
    }

    /// A wrong emailed code shows the inline error under the code field.
    func testWrongEmailCodeShowsInlineError() {
        let app = launch(["--signed-out"])
        tap(app, "Email me a code")
        type(app, into: "Email", "tim@example.com")
        tap(app, "Send code")
        type(app, into: "6-digit code", "111111")
        XCTAssertTrue(waitForText(app, containing: "didn't work"), "no inline error for the wrong code")
    }

    // MARK: tour (screenshots for the design review)

    func testAuthTour() {
        // Sign in, the server sheet, the emailed code.
        do {
            let app = launch(["--signed-out"])
            XCTAssertTrue(app.textFields["Username or email"].waitForExistence(timeout: 10))
            shot(app, "01-signin")
            tap(app, "Change server")
            XCTAssertTrue(app.navigationBars["Server"].waitForExistence(timeout: 5))
            shot(app, "02-server-sheet")
            tap(app, "Cancel")
            tap(app, "Email me a code")
            shot(app, "03-email-request")
            type(app, into: "Email", "tim@example.com")
            tap(app, "Send code")
            XCTAssertTrue(app.textFields["6-digit code"].waitForExistence(timeout: 10))
            shot(app, "04-email-code")
            type(app, into: "6-digit code", "111111")
            XCTAssertTrue(waitForText(app, containing: "didn't work"))
            shot(app, "05-email-code-wrong")
            app.terminate()
        }

        // The passkey step and its waiting state (the browser fallback page opens for a non-domain host).
        do {
            let app = launch(["--signed-out"])
            tap(app, "Use a passkey")
            XCTAssertTrue(app.textFields["Username"].waitForExistence(timeout: 10))
            shot(app, "06-passkey")
            type(app, into: "Username", "tim")
            tap(app, "Continue with a passkey")
            Thread.sleep(forTimeInterval: 1.2)
            shot(app, "07-passkey-waiting")
            XCTAssertTrue(shellAppeared(app), "the polled passkey sign-in did not finish")
            shot(app, "08-shell-after-passkey")
            app.terminate()
        }

        // Password reset: request, code + new password, and the notice back on sign in.
        do {
            let app = launch(["--signed-out"])
            tap(app, "Forgot password?")
            XCTAssertTrue(app.textFields["Email"].waitForExistence(timeout: 10))
            shot(app, "09-reset-request")
            type(app, into: "Email", "tim@example.com")
            tap(app, "Send code")
            XCTAssertTrue(app.textFields["6-digit code"].waitForExistence(timeout: 10))
            type(app, into: "6-digit code", emailCode)
            type(app, into: "New password", secure: true, "a brand new one")
            shot(app, "10-reset-code")
            tap(app, "Set new password")
            XCTAssertTrue(waitForText(app, containing: "password was changed"))
            shot(app, "11-reset-notice")
            app.terminate()
        }

        // Invite: unknown code error, the preview, the filled form, the welcome passkey sheet, the shell.
        do {
            var app = launch(["--signed-out"])
            tap(app, "Join with a code")
            type(app, into: "Invite code", "0000-0000")
            tap(app, "Continue")
            XCTAssertTrue(waitForText(app, containing: "couldn't find that invite"))
            shot(app, "12-invite-unknown")
            app.terminate()

            app = launch(["--signed-out"])
            tap(app, "Join with a code")
            type(app, into: "Invite code", inviteCode.replacingOccurrences(of: "-", with: ""))
            tap(app, "Continue")
            XCTAssertTrue(waitForText(app, containing: "invited you to join"))
            shot(app, "13-invite-accept")
            type(app, into: "Choose a username", "zoe")
            type(app, into: "Choose a password", secure: true, "long-enough-pw")
            dismissKeyboard(app)
            tap(app, "Add a passkey after signing up")
            tap(app, "I agree to the Terms and Privacy Policy")
            tap(app, "I'm 16 or older")
            shot(app, "14-invite-accept-filled")
            tap(app, "Create account")
            XCTAssertTrue(app.buttons["Add a passkey"].waitForExistence(timeout: 15), "the welcome sheet did not appear")
            shot(app, "15-welcome-sheet")
            tap(app, "Skip")
            XCTAssertTrue(shellAppeared(app))
            shot(app, "16-shell-after-invite")
            app.terminate()
        }

        // Second factor: TOTP with a wrong code (tries left), the recovery toggle, the passkey option.
        do {
            let app = launch(["--signed-out"])
            type(app, into: "Username or email", "ana")
            type(app, into: "Password", secure: true, password)
            tap(app, "Sign in")
            XCTAssertTrue(app.textFields["Authenticator code"].waitForExistence(timeout: 10))
            shot(app, "17-second-factor")
            type(app, into: "Authenticator code", "000000")
            XCTAssertTrue(waitForText(app, containing: "tries left"))
            shot(app, "18-second-factor-wrong")
            tap(app, "Use a recovery code")
            XCTAssertTrue(app.textFields["Recovery code"].waitForExistence(timeout: 10))
            shot(app, "19-recovery")
            tap(app, "Use a passkey")
            Thread.sleep(forTimeInterval: 1.2)
            shot(app, "20-second-factor-passkey")
            XCTAssertTrue(shellAppeared(app), "the second-factor passkey did not finish")
            app.terminate()
        }

        // Suspended on sign in.
        do {
            let app = launch(["--signed-out"])
            type(app, into: "Username or email", "cvetka")
            type(app, into: "Password", secure: true, password)
            tap(app, "Sign in")
            XCTAssertTrue(waitForText(app, containing: "suspended"))
            shot(app, "21-suspended")
            app.terminate()
        }

        // The gates after a restored session, and the splash on a cold start.
        do {
            var app = launch(["--demo-terms"])
            XCTAssertTrue(waitForText(app, containing: "updated our Terms"))
            shot(app, "22-terms-gate")
            app.terminate()

            app = launch(["--demo-update"])
            XCTAssertTrue(waitForText(app, containing: "Update required"))
            shot(app, "23-update-gate")
            app.terminate()

            app = launch(["--demo-suspended"])
            XCTAssertTrue(waitForText(app, containing: "suspended"))
            shot(app, "24-suspended-gate")
            app.terminate()

            app = launch([])
            shot(app, "25-splash")
            XCTAssertTrue(shellAppeared(app))
            app.terminate()
        }
    }
}
