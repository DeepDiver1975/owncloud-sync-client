import XCTest
@testable import FinderSync

final class BadgeManagerTests: XCTestCase {

    func testBadgeIdentifier_OK() {
        XCTAssertEqual(BadgeManager.badgeIdentifier(for: "OK"), "synced")
    }

    func testBadgeIdentifier_SYNC() {
        XCTAssertEqual(BadgeManager.badgeIdentifier(for: "SYNC"), "syncing")
    }

    func testBadgeIdentifier_WARNING() {
        XCTAssertEqual(BadgeManager.badgeIdentifier(for: "WARNING"), "warning")
    }

    func testBadgeIdentifier_ERROR() {
        XCTAssertEqual(BadgeManager.badgeIdentifier(for: "ERROR"), "error")
    }

    func testBadgeIdentifier_EXCLUDED() {
        XCTAssertEqual(BadgeManager.badgeIdentifier(for: "EXCLUDED"), "excluded")
    }

    func testBadgeIdentifier_unknown_returnsEmpty() {
        XCTAssertEqual(BadgeManager.badgeIdentifier(for: "NONE"), "")
        XCTAssertEqual(BadgeManager.badgeIdentifier(for: ""), "")
        XCTAssertEqual(BadgeManager.badgeIdentifier(for: "BOGUS"), "")
    }
}
