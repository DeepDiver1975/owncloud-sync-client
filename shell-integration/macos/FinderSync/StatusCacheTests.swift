import XCTest
@testable import FinderSync

final class StatusCacheTests: XCTestCase {

    func testSetAndGet() {
        let cache = StatusCache()
        cache.set(path: "/sync/file.txt", status: "OK")
        XCTAssertEqual(cache.get(path: "/sync/file.txt"), "OK")
    }

    func testGetMissingReturnsNil() {
        let cache = StatusCache()
        XCTAssertNil(cache.get(path: "/does/not/exist"))
    }

    func testRemove() {
        let cache = StatusCache()
        cache.set(path: "/sync/a", status: "SYNC")
        cache.remove(path: "/sync/a")
        XCTAssertNil(cache.get(path: "/sync/a"))
    }

    func testClear() {
        let cache = StatusCache()
        cache.set(path: "/sync/a", status: "OK")
        cache.set(path: "/sync/b", status: "ERROR")
        cache.clear()
        XCTAssertTrue(cache.allPaths().isEmpty)
    }

    func testAllPaths() {
        let cache = StatusCache()
        cache.set(path: "/sync/a", status: "OK")
        cache.set(path: "/sync/b", status: "SYNC")
        let paths = cache.allPaths()
        XCTAssertEqual(Set(paths), Set(["/sync/a", "/sync/b"]))
    }

    func testConcurrentWriteSafety() {
        let cache = StatusCache()
        let iterations = 100
        DispatchQueue.concurrentPerform(iterations: iterations) { i in
            cache.set(path: "/sync/file\(i).txt", status: "OK")
        }
        XCTAssertEqual(cache.allPaths().count, iterations)
    }
}
