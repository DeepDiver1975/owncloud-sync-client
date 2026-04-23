import XCTest
@testable import FinderSync

// Individual test classes:
//   SocketClientTests  — extractLines/buffer parsing
//   StatusCacheTests   — set/get/remove/clear/concurrent safety
//   BadgeManagerTests  — badgeIdentifier mapping for all 6 inputs
//   MenuBuilderTests   — parseMenuItems / buildMenu

/// Combined smoke tests verifying all four sub-systems wire together.
final class FinderSyncIntegrationTests: XCTestCase {

    func testStatusCacheRoundTrip() {
        let cache = StatusCache()
        cache.set(path: "/sync/doc.txt", status: "SYNC")
        XCTAssertEqual(cache.get(path: "/sync/doc.txt"), "SYNC")
        cache.set(path: "/sync/doc.txt", status: "OK")
        XCTAssertEqual(cache.get(path: "/sync/doc.txt"), "OK")
    }

    func testBadgeManagerMappingCoversAllTags() {
        let knownTags = ["OK", "SYNC", "WARNING", "ERROR", "EXCLUDED"]
        let knownIds  = ["synced", "syncing", "warning", "error", "excluded"]
        for (tag, expectedId) in zip(knownTags, knownIds) {
            XCTAssertEqual(BadgeManager.badgeIdentifier(for: tag), expectedId,
                           "unexpected badge id for status '\(tag)'")
        }
    }

    func testMenuBuilderParseThenBuild() {
        let response =
            "GET_MENU_ITEMS:/sync/file.txt\u{1e}" +
            "Share:SHARE:enabled\u{1e}" +
            "Make Available Locally:MAKE_AVAILABLE_LOCALLY:disabled\u{1e}"
        let items = MenuBuilder.parseMenuItems(response)
        XCTAssertEqual(items.count, 2)
        let menu = MenuBuilder.buildMenu(items: items) { _ in }
        XCTAssertEqual(menu.items.count, 2)
        XCTAssertTrue(menu.items[0].isEnabled)
        XCTAssertFalse(menu.items[1].isEnabled)
    }

    func testSocketClientExtractLinesLargePayload() {
        let raw = (0..<1000).map { "STATUS:OK:/sync/file\($0).txt" }.joined(separator: "\n") + "\n"
        var data = Data(raw.utf8)
        let lines = SocketClient.extractLines(from: &data)
        XCTAssertEqual(lines.count, 1000)
        XCTAssertTrue(data.isEmpty)
    }
}
