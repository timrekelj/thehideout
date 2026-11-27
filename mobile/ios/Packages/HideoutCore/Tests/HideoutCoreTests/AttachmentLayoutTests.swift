import Foundation
import XCTest
@testable import HideoutCore

final class AttachmentLayoutTests: XCTestCase {
    // MARK: fileKind

    func testFileKindByContentType() {
        XCTAssertEqual(fileKind(contentType: "image/png", filename: "a"), .image)
        XCTAssertEqual(fileKind(contentType: "video/mp4", filename: "a"), .video)
        XCTAssertEqual(fileKind(contentType: "audio/mpeg", filename: "a"), .audio)
        XCTAssertEqual(fileKind(contentType: "application/pdf", filename: "a"), .pdf)
        XCTAssertEqual(fileKind(contentType: "application/zip", filename: "a"), .archive)
        XCTAssertEqual(fileKind(contentType: "application/json", filename: "a"), .code)
        XCTAssertEqual(fileKind(contentType: "text/plain", filename: "a"), .text)
        XCTAssertEqual(fileKind(contentType: "application/msword", filename: "a"), .doc)
        XCTAssertEqual(fileKind(contentType: "application/octet-stream", filename: "a"), .other)
    }

    func testFileKindFallsBackToExtension() {
        XCTAssertEqual(fileKind(contentType: "application/octet-stream", filename: "clip.mov"), .video)
        XCTAssertEqual(fileKind(contentType: "application/octet-stream", filename: "note.m4a"), .audio)
        XCTAssertEqual(fileKind(contentType: "application/octet-stream", filename: "archive.7z"), .archive)
        XCTAssertEqual(fileKind(contentType: "application/octet-stream", filename: "main.rs"), .code)
        XCTAssertEqual(fileKind(contentType: "application/octet-stream", filename: "readme.md"), .text)
        XCTAssertEqual(fileKind(contentType: "application/octet-stream", filename: "sheet.xlsx"), .doc)
    }

    func testOpensInline() {
        XCTAssertTrue(opensInline(.image))
        XCTAssertTrue(opensInline(.video))
        XCTAssertTrue(opensInline(.audio))
        XCTAssertTrue(opensInline(.pdf))
        XCTAssertFalse(opensInline(.archive))
        XCTAssertFalse(opensInline(.other))
    }

    // MARK: formatting

    func testHumanSize() {
        XCTAssertEqual(humanSize(12), "12 B")
        XCTAssertEqual(humanSize(1024), "1.0 KB")
        XCTAssertEqual(humanSize(3 * 1024 + 410), "3.4 KB")
        XCTAssertEqual(humanSize(5 * 1024 * 1024), "5.0 MB")
        XCTAssertEqual(humanSize(UInt64(1.2 * 1024 * 1024 * 1024)), "1.2 GB")
    }

    func testClockLabel() {
        XCTAssertEqual(clockLabel(7), "0:07")
        XCTAssertEqual(clockLabel(12 * 60 + 3), "12:03")
        XCTAssertEqual(clockLabel(3600 + 2 * 60 + 3), "1:02:03")
        XCTAssertEqual(clockLabel(.nan), "0:00")
        XCTAssertEqual(clockLabel(-5), "0:00")
    }

    // MARK: fitSize

    func testFitSizeKeepsAspectAndNeverUpscales() {
        let (w, h) = fitSize(natural: (1200, 800), maxW: 400, maxH: 300)
        XCTAssertEqual(w, 400)
        XCTAssertEqual(h, 267)

        let (w2, h2) = fitSize(natural: (100, 50), maxW: 400, maxH: 300)
        XCTAssertEqual(w2, 100, "smaller than the box: never scaled up past natural size")
        XCTAssertEqual(h2, 50)
    }

    func testFitSizeFallsBackTo16by9WithoutNaturalSize() {
        let (w, h) = fitSize(natural: nil, maxW: 400, maxH: 300)
        XCTAssertEqual(w, 400)
        XCTAssertEqual(h, (400 * 9 / 16).rounded())
    }

    func testFitSizeGivesATinyPictureALegibleBox() {
        let (w, h) = fitSize(natural: (10, 10), maxW: 400, maxH: 300)
        XCTAssertEqual(w, 96)
        XCTAssertEqual(h, 96)
    }

    // MARK: imageGrid

    func testImageGridOne() {
        let (size, cells) = imageGrid(n: 1, single: (1200, 800))
        XCTAssertEqual(cells.count, 1)
        XCTAssertEqual(size.0, 400)
        XCTAssertEqual(size.1, 267)
        XCTAssertEqual(cells[0], Cell(x: 0, y: 0, w: 400, h: 267))
    }

    func testImageGridTwoSideBySide() {
        let (size, cells) = imageGrid(n: 2)
        XCTAssertEqual(size.0, 400)
        XCTAssertEqual(size.1, 240)
        XCTAssertEqual(cells.count, 2)
        XCTAssertEqual(cells[0], Cell(x: 0, y: 0, w: 198, h: 240))
        XCTAssertEqual(cells[1], Cell(x: 202, y: 0, w: 198, h: 240))
    }

    func testImageGridThreePutsTheFirstTall() {
        let (_, cells) = imageGrid(n: 3)
        XCTAssertEqual(cells.count, 3)
        XCTAssertEqual(cells[0], Cell(x: 0, y: 0, w: 198, h: 240), "the first tile spans the full column height")
        XCTAssertEqual(cells[1].h, cells[2].h, "the other two are stacked at half height")
        XCTAssertEqual(cells[1].y, 0)
        XCTAssertGreaterThan(cells[2].y, cells[1].y)
    }

    func testImageGridFourOrMoreIsA2x2() {
        for n in [4, 5, 7] {
            let (size, cells) = imageGrid(n: n)
            XCTAssertEqual(cells.count, 4, "n=\(n) still lays out 4 tiles; a fifth and beyond are reached through the lightbox")
            XCTAssertEqual(size.0, 400)
            XCTAssertEqual(size.1, 240)
            XCTAssertEqual(Set(cells.map(\.w)), [198])
            XCTAssertEqual(Set(cells.map(\.h)), [118])
        }
    }

    func testImageGridZero() {
        let (size, cells) = imageGrid(n: 0)
        XCTAssertEqual(size.0, 0)
        XCTAssertEqual(size.1, 0)
        XCTAssertTrue(cells.isEmpty)
    }

    func testImageGridWidthAdaptsToAColumn() {
        let (size, cells) = imageGrid(n: 2, maxW: 300)
        XCTAssertEqual(size.0, 300)
        XCTAssertEqual(cells[0].w, 148)
    }

    // MARK: mediaSource / URL building

    func testMediaSourceBuildsTheServerURL() throws {
        let base = URL(string: "https://thehideout.zabit.si")!
        guard case .remote(let url) = try mediaSource(base: base, key: "abc123") else { return XCTFail() }
        XCTAssertEqual(url.absoluteString, "https://thehideout.zabit.si/media/abc123")
    }

    func testMediaSourcePassesThroughTheFakeAssetPrefix() throws {
        let base = URL(string: "https://thehideout.zabit.si")!
        guard case .fakeAsset(let name) = try mediaSource(base: base, key: "fakeasset:landscape-1.jpg") else { return XCTFail() }
        XCTAssertEqual(name, "landscape-1.jpg")
    }

    func testMediaSourceRejectsAForeignScheme() {
        let base = URL(string: "https://thehideout.zabit.si")!
        XCTAssertThrowsError(try mediaSource(base: base, key: "file:///etc/passwd")) { error in
            XCTAssertEqual(error as? MediaKeyError, .foreignScheme("file:///etc/passwd"))
        }
        XCTAssertThrowsError(try mediaSource(base: base, key: "content://evil/thing"))
    }
}
