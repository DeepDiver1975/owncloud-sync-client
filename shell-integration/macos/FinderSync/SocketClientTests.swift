import XCTest
@testable import FinderSync

final class SocketClientTests: XCTestCase {

    func testExtractLines_threeCompleteLines() {
        var data = Data("STATUS:OK:/a\nSTATUS:SYNC:/b\nSTATUS:ERROR:/c\n".utf8)
        let lines = SocketClient.extractLines(from: &data)
        XCTAssertEqual(lines.count, 3)
        XCTAssertEqual(lines[0], "STATUS:OK:/a")
        XCTAssertEqual(lines[1], "STATUS:SYNC:/b")
        XCTAssertEqual(lines[2], "STATUS:ERROR:/c")
        XCTAssertTrue(data.isEmpty)
    }

    func testExtractLines_partialLineRemainsInBuffer() {
        var data = Data("STATUS:OK:/a\nSTATUS:SYNC:/b\nSTATUS:".utf8)
        let lines = SocketClient.extractLines(from: &data)
        XCTAssertEqual(lines.count, 2)
        XCTAssertEqual(lines[0], "STATUS:OK:/a")
        XCTAssertEqual(lines[1], "STATUS:SYNC:/b")
        XCTAssertEqual(String(data: data, encoding: .utf8), "STATUS:")
    }

    func testExtractLines_emptyBuffer() {
        var data = Data()
        let lines = SocketClient.extractLines(from: &data)
        XCTAssertTrue(lines.isEmpty)
    }

    func testExtractLines_crlfStripped() {
        var data = Data("STATUS:OK:/a\r\n".utf8)
        let lines = SocketClient.extractLines(from: &data)
        XCTAssertEqual(lines.count, 1)
        XCTAssertEqual(lines[0], "STATUS:OK:/a")
    }

    func testExtractLines_blankLinesSkipped() {
        var data = Data("\nSTATUS:OK:/a\n\n".utf8)
        let lines = SocketClient.extractLines(from: &data)
        XCTAssertEqual(lines.count, 1)
        XCTAssertEqual(lines[0], "STATUS:OK:/a")
    }
}
