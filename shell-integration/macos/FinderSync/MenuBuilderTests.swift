import XCTest
@testable import FinderSync

final class MenuBuilderTests: XCTestCase {

    private let sampleResponse =
        "GET_MENU_ITEMS:/sync/file.txt\u{1e}" +
        "Share:SHARE:enabled\u{1e}" +
        "Make Available Locally:MAKE_AVAILABLE_LOCALLY:enabled\u{1e}" +
        "Make Online Only:MAKE_ONLINE_ONLY:disabled\u{1e}"

    func testParseMenuItems_threeItems() {
        let items = MenuBuilder.parseMenuItems(sampleResponse)
        XCTAssertEqual(items.count, 3)
    }

    func testParseMenuItems_names() {
        let items = MenuBuilder.parseMenuItems(sampleResponse)
        XCTAssertEqual(items[0].name, "Share")
        XCTAssertEqual(items[1].name, "Make Available Locally")
        XCTAssertEqual(items[2].name, "Make Online Only")
    }

    func testParseMenuItems_commands() {
        let items = MenuBuilder.parseMenuItems(sampleResponse)
        XCTAssertEqual(items[0].command, "SHARE")
        XCTAssertEqual(items[1].command, "MAKE_AVAILABLE_LOCALLY")
        XCTAssertEqual(items[2].command, "MAKE_ONLINE_ONLY")
    }

    func testParseMenuItems_enabledStates() {
        let items = MenuBuilder.parseMenuItems(sampleResponse)
        XCTAssertTrue(items[0].enabled)
        XCTAssertTrue(items[1].enabled)
        XCTAssertFalse(items[2].enabled)
    }

    func testParseMenuItems_emptyResponse() {
        let items = MenuBuilder.parseMenuItems("")
        XCTAssertTrue(items.isEmpty)
    }

    func testParseMenuItems_headerOnly() {
        let items = MenuBuilder.parseMenuItems("GET_MENU_ITEMS:/path")
        XCTAssertTrue(items.isEmpty)
    }

    func testBuildMenu_itemCount() {
        let items = MenuBuilder.parseMenuItems(sampleResponse)
        let menu = MenuBuilder.buildMenu(items: items) { _ in }
        XCTAssertEqual(menu.items.count, 3)
    }

    func testBuildMenu_enabledStates() {
        let items = MenuBuilder.parseMenuItems(sampleResponse)
        let menu = MenuBuilder.buildMenu(items: items) { _ in }
        XCTAssertTrue(menu.items[0].isEnabled)
        XCTAssertTrue(menu.items[1].isEnabled)
        XCTAssertFalse(menu.items[2].isEnabled)
    }

    func testBuildMenu_actionFired() {
        let items: [(name: String, command: String, enabled: Bool)] = [
            (name: "Share", command: "SHARE", enabled: true)
        ]
        var firedCommand: String?
        let menu = MenuBuilder.buildMenu(items: items) { cmd in
            firedCommand = cmd
        }
        let menuItem = menu.items[0]
        _ = menuItem.target?.perform(menuItem.action, with: menuItem)
        XCTAssertEqual(firedCommand, "SHARE")
    }
}
