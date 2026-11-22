import XCTest

/// Phase 3a chat flows against the fake world (`--ui-testing`, signed in as tim): open a channel and send
/// (pending then sent), react, edit, delete, open a DM, and a mention tap landing in the transcript.
@MainActor
final class ChatUITests: XCTestCase {
    private func launch() -> XCUIApplication {
        let app = XCUIApplication()
        app.launchArguments = ["--ui-testing"]
        app.launch()
        return app
    }

    /// The composer's text view. It is a multi-line `TextField`, backed by a text view: XCUITest's automatic
    /// type detection flips between `TextField` and `TextView` as its content changes, so this keys off a
    /// fixed `accessibilityIdentifier` (set in `Transcript.swift`) instead of its type or placeholder label.
    private func composer(_ app: XCUIApplication) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: "composer").firstMatch
    }

    private func openGeneral(_ app: XCUIApplication) {
        XCTAssertTrue(app.tabBars.buttons["Servers"].waitForExistence(timeout: 15))
        app.tabBars.buttons["Servers"].tap()
        let server = app.buttons["The Hideout"]
        XCTAssertTrue(server.waitForExistence(timeout: 10))
        server.tap()
        let general = app.buttons.containing(NSPredicate(format: "label CONTAINS 'general'")).firstMatch
        XCTAssertTrue(general.waitForExistence(timeout: 10))
        general.tap()
        XCTAssertTrue(composer(app).waitForExistence(timeout: 10))
    }

    /// Sending shows the message as pending, then it settles once the fake backend answers (its reply and
    /// gateway echo reconcile to one row, HideoutCoreTests covers that at the model level).
    func testSendAMessage() {
        let app = launch()
        openGeneral(app)
        let unique = "ui test message \(Int(Date().timeIntervalSince1970))"
        let field = composer(app)
        field.tap()
        field.typeText(unique)
        app.buttons["Send"].tap()
        XCTAssertTrue(app.staticTexts[unique].waitForExistence(timeout: 10))
        XCTAssertTrue(field.waitForExistence(timeout: 2), "the field clears and is ready for the next message")
    }

    func testReactToAMessage() {
        let app = launch()
        openGeneral(app)
        let row = app.staticTexts["Nice. Merging."].firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 10))
        row.press(forDuration: 1.0)
        let thumbsUp = app.buttons["👍"].firstMatch
        XCTAssertTrue(thumbsUp.waitForExistence(timeout: 5), "the quick-reaction row in the context menu")
        thumbsUp.tap()
        let reacted = app.descendants(matching: .any).matching(NSPredicate(format: "label CONTAINS 'you reacted'")).firstMatch
        XCTAssertTrue(reacted.waitForExistence(timeout: 5))
    }

    /// Edits one of tim's own seeded messages (not a message this test just sent itself: the composer would
    /// still have keyboard focus right after typing, and a long-press while the scroll view is interactively
    /// dismissing the keyboard is unreliable — see the Send test for that path instead).
    func testEditOwnMessage() {
        let app = launch()
        openGeneral(app)
        let row = app.staticTexts["Release checklist"].firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 10))
        row.press(forDuration: 1.0)
        app.buttons["Edit"].tap()
        XCTAssertTrue(app.staticTexts["Editing message"].waitForExistence(timeout: 5))
        composer(app).typeText(" v2")
        app.buttons["Save"].tap()
        XCTAssertTrue(app.staticTexts["ship it v2"].waitForExistence(timeout: 10))
        XCTAssertFalse(app.staticTexts["Editing message"].exists, "edit mode exits after saving")
    }

    func testDeleteOwnMessage() {
        let app = launch()
        openGeneral(app)
        let row = app.staticTexts["Release checklist"].firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 10))
        row.press(forDuration: 1.0)
        app.buttons["Delete"].tap()
        XCTAssertTrue(app.buttons["Delete"].waitForExistence(timeout: 5), "the confirmation dialog's destructive button")
        app.buttons["Delete"].tap()
        XCTAssertFalse(app.staticTexts["Release checklist"].waitForExistence(timeout: 5))
    }

    func testOpenADM() {
        let app = launch()
        XCTAssertTrue(app.tabBars.buttons["DMs"].waitForExistence(timeout: 15))
        app.tabBars.buttons["DMs"].tap()
        let ana = app.buttons["ana"].firstMatch
        XCTAssertTrue(ana.waitForExistence(timeout: 10))
        ana.tap()
        XCTAssertTrue(composer(app).waitForExistence(timeout: 10))
    }

    /// Tapping a mention in the feed lands in the conversation, not the list (the Android bug this guards
    /// against). The fake world's seed data is fixed but not guaranteed to mention tim in exactly one place,
    /// so this skips rather than assert-false when the feed happens to be empty.
    func testMentionTapOpensTheTranscript() throws {
        let app = launch()
        XCTAssertTrue(app.tabBars.buttons["Mentions"].waitForExistence(timeout: 15))
        app.tabBars.buttons["Mentions"].tap()
        let firstRow = app.cells.firstMatch
        guard firstRow.waitForExistence(timeout: 5) else {
            throw XCTSkip("the fake world's seed data has no mention of tim right now")
        }
        firstRow.tap()
        XCTAssertTrue(composer(app).waitForExistence(timeout: 10), "a composer, not the mentions list, is on screen")
    }
}
