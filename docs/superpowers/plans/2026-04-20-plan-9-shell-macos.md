# Plan 9: Shell Integration — macOS FinderSync Extension (Swift)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the Swift `FinderSync.appex` App Extension that provides sync status badge icons and context menu items in macOS Finder for files inside sync folders.

**Architecture:** `FINCFinderSync` protocol extension. Connects to ocsyncd Unix socket via `NWConnection` to send status queries and receive broadcast updates. Maps socket API status tags to Finder badge identifiers. Adds toolbar items (Share, Make Available Locally, Make Online Only).

**Tech Stack:** Swift 5.9, FinderSync.framework, Network.framework (NWConnection), Foundation. Socket path: `~/Library/Group Containers/$(APP_GROUP_ID)/owncloud.sock`. Depends on socket API wire protocol (Plan 5) — NOT the Rust crate.

---

## Context — Socket API Wire Protocol

The FinderSync extension connects to the same Unix socket as the Windows shell integration (Plan 5 defines it). Wire protocol:
- Send: `RETRIEVE_FILE_STATUS:path\n` → receive: `STATUS:OK:path\n` (tags: OK, SYNC, WARNING, ERROR, EXCLUDED, NONE)
- Send: `RETRIEVE_FOLDER_STATUS:path\n` → receive: `STATUS:tag:path\n`
- Send: `GET_STRINGS\n` → receive localized UI strings
- Send: `GET_MENU_ITEMS:path\n` → receive: `GET_MENU_ITEMS:path\x1ename:cmd:state\x1e...\n`
- Send: `MAKE_AVAILABLE_LOCALLY:path\n`, `MAKE_ONLINE_ONLY:path\n`, `SHARE:path\n`, `COPY_PRIVATE_LINK:path\n`
- Server broadcasts: `REGISTER_PATH:/sync/root\n`, `STATUS:tag:path\n`, `UPDATE_VIEW:/path\n`

Badge identifiers: `"synced"`, `"syncing"`, `"warning"`, `"error"`, `"excluded"`

## File map

```
shell-integration/macos/FinderSync/
  FinderSyncExtension.swift      # FINCFinderSync principal class
  SocketClient.swift             # NWConnection to Unix socket, send/receive
  StatusCache.swift              # thread-safe path→status cache
  BadgeManager.swift             # register badges, set badge for path
  MenuBuilder.swift              # build NSMenu from GET_MENU_ITEMS response
  FinderSync.entitlements        # App Group entitlement
  Info.plist                     # extension plist
```

---

## Tasks

### Task 1: Info.plist + entitlements

- [ ] Create `shell-integration/macos/FinderSync/Info.plist` with the complete XML:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>$(DEVELOPMENT_LANGUAGE)</string>
    <key>CFBundleDisplayName</key>
    <string>ownCloud FinderSync</string>
    <key>CFBundleExecutable</key>
    <string>$(EXECUTABLE_NAME)</string>
    <key>CFBundleIdentifier</key>
    <string>org.owncloud.owncloud-sync.FinderSync</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>$(PRODUCT_NAME)</string>
    <key>CFBundlePackageType</key>
    <string>XPC!</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>NSExtension</key>
    <dict>
        <key>NSExtensionPointIdentifier</key>
        <string>com.apple.FinderSync</string>
        <key>NSExtensionPrincipalClass</key>
        <string>$(PRODUCT_MODULE_NAME).FinderSyncExtension</string>
    </dict>
</dict>
</plist>
```

- [ ] Create `shell-integration/macos/FinderSync/FinderSync.entitlements`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>com.apple.security.application-groups</key>
    <array>
        <string>group.org.owncloud.owncloud-sync</string>
    </array>
</dict>
</plist>
```

- [ ] Xcode target setup:
  - In the main `ocsync` project, add a new target: File > New > Target > Finder Sync Extension.
  - Set bundle identifier to `org.owncloud.owncloud-sync.FinderSync`.
  - Set the principal class in the generated `Info.plist` to `$(PRODUCT_MODULE_NAME).FinderSyncExtension`.
  - In the main app target's "Embed App Extensions" build phase, add `FinderSync.appex`.
  - In both the main app target and the extension target, set App Group entitlement to `group.org.owncloud.owncloud-sync` under Signing & Capabilities.
  - Swift Language Version: Swift 5.9. Minimum deployment: macOS 13.0.

Commit: `git add shell-integration/macos/FinderSync/ && git commit -m "feat(findersync): add extension plist and entitlements"`

---

### Task 2: SocketClient.swift — NWConnection to Unix socket

- [ ] Create `shell-integration/macos/FinderSync/SocketClient.swift`:

```swift
import Foundation
import Network

/// Async Unix-socket client for the ocsyncd wire protocol.
///
/// A single `SocketClient` instance lives for the lifetime of the
/// `FinderSyncExtension`. It maintains a persistent `NWConnection` to the
/// ocsyncd Unix socket, dispatches broadcast lines to registered callbacks,
/// and routes command responses to their per-call completion handlers.
final class SocketClient {

    // MARK: - Public callbacks (set before calling connect)

    /// Called for every `STATUS:tag:path` broadcast line.
    var onStatusBroadcast: ((String, String) -> Void)?  // (tag, path)
    /// Called for every `REGISTER_PATH:path` broadcast line.
    var onRegisterPath: ((String) -> Void)?
    /// Called for every `UPDATE_VIEW:path` broadcast line.
    var onUpdateView: ((String) -> Void)?

    // MARK: - Private state

    private var connection: NWConnection?
    private let queue = DispatchQueue(label: "org.owncloud.findersync.socket",
                                      qos: .utility)
    /// Accumulates bytes received from the socket until a complete line arrives.
    private var receiveBuffer = Data()
    /// FIFO queue of pending one-shot completions for sendCommand(_:completion:).
    /// Each entry is consumed by the first response line that arrives.
    private var pendingCompletions: [(String?) -> Void] = []

    // MARK: - Connection lifecycle

    /// Open a persistent connection to the ocsyncd Unix socket at `socketPath`.
    ///
    /// On successful connection the client sends `GET_STRINGS` and starts the
    /// continuous receive loop. If the connection drops it is not automatically
    /// re-established; callers should create a new `SocketClient`.
    func connect(socketPath: String) {
        let endpoint = NWEndpoint.unix(path: socketPath)
        let params = NWParameters()
        params.allowLocalEndpointReuse = true
        let conn = NWConnection(to: endpoint, using: params)
        self.connection = conn

        conn.stateUpdateHandler = { [weak self] state in
            guard let self else { return }
            switch state {
            case .ready:
                // Warm up: fetch localized strings used for toolbar/menu labels.
                self.sendAndForget("GET_STRINGS")
                self.startReceiving()
            case .failed(let error):
                NSLog("[FinderSync] socket error: \(error)")
                self.drainPendingCompletions()
            case .cancelled:
                self.drainPendingCompletions()
            default:
                break
            }
        }
        conn.start(queue: queue)
    }

    /// Send `cmd` (newline appended automatically), collect the first response
    /// line and deliver it to `completion`.  Passes `nil` on send failure.
    func sendCommand(_ cmd: String, completion: @escaping (String?) -> Void) {
        guard let connection, connection.state == .ready else {
            completion(nil)
            return
        }
        // Register the completion *before* sending so the response cannot race
        // past an empty pending queue.
        pendingCompletions.append(completion)

        let payload = Data((cmd + "\n").utf8)
        connection.send(content: payload, completion: .contentProcessed { [weak self] error in
            if let error {
                NSLog("[FinderSync] send error for '\(cmd)': \(error)")
                // Remove the completion we just appended and call it with nil.
                self?.queue.async {
                    if let idx = self?.pendingCompletions.indices.first {
                        let cb = self!.pendingCompletions.remove(at: idx)
                        cb(nil)
                    }
                }
            }
        })
    }

    /// Send `cmd` and ignore the response (fire-and-forget commands such as
    /// `SHARE`, `MAKE_AVAILABLE_LOCALLY`, etc.).
    func sendAndForget(_ cmd: String) {
        guard let connection, connection.state == .ready else { return }
        let payload = Data((cmd + "\n").utf8)
        connection.send(content: payload, completion: .idempotent)
    }

    /// Close the connection and cancel any in-flight completions.
    func disconnect() {
        connection?.cancel()
        connection = nil
        drainPendingCompletions()
    }

    // MARK: - Receive loop

    private func startReceiving() {
        connection?.receive(minimumIncompleteLength: 1,
                             maximumLength: 65_536) { [weak self] data, _, isComplete, error in
            guard let self else { return }
            if let data, !data.isEmpty {
                self.receiveBuffer.append(data)
                let lines = Self.extractLines(from: &self.receiveBuffer)
                for line in lines {
                    self.dispatch(line: line)
                }
            }
            if let error {
                NSLog("[FinderSync] receive error: \(error)")
                self.drainPendingCompletions()
                return
            }
            if !isComplete {
                self.startReceiving()  // schedule next read
            }
        }
    }

    /// Extract all complete newline-terminated lines from `buffer`, leaving
    /// any partial line in place.
    ///
    /// This is a pure function exposed as `internal` for unit testing.
    static func extractLines(from buffer: inout Data) -> [String] {
        var lines: [String] = []
        while let nlIndex = buffer.firstIndex(of: UInt8(ascii: "\n")) {
            let lineData = buffer[buffer.startIndex ..< nlIndex]
            if let line = String(data: lineData, encoding: .utf8) {
                let trimmed = line.trimmingCharacters(in: .init(charactersIn: "\r"))
                if !trimmed.isEmpty {
                    lines.append(trimmed)
                }
            }
            buffer.removeSubrange(buffer.startIndex ... nlIndex)
        }
        return lines
    }

    // MARK: - Dispatch

    /// Route a received line either to a broadcast callback or to the first
    /// pending command completion.
    private func dispatch(line: String) {
        if line.hasPrefix("STATUS:") {
            // Format: STATUS:tag:path
            let parts = line.components(separatedBy: ":")
            if parts.count >= 3 {
                let tag = parts[1]
                // Path may itself contain colons; rejoin everything after tag.
                let path = parts.dropFirst(2).joined(separator: ":")
                onStatusBroadcast?(tag, path)
            }
            return
        }
        if line.hasPrefix("REGISTER_PATH:") {
            let path = String(line.dropFirst("REGISTER_PATH:".count))
            onRegisterPath?(path)
            return
        }
        if line.hasPrefix("UPDATE_VIEW:") {
            let path = String(line.dropFirst("UPDATE_VIEW:".count))
            onUpdateView?(path)
            return
        }
        // Not a broadcast → deliver to first pending completion.
        if !pendingCompletions.isEmpty {
            let cb = pendingCompletions.removeFirst()
            cb(line)
        }
    }

    private func drainPendingCompletions() {
        let cbs = pendingCompletions
        pendingCompletions = []
        cbs.forEach { $0(nil) }
    }
}
```

- [ ] Create `shell-integration/macos/FinderSync/SocketClientTests.swift` (XCTest):

```swift
import XCTest
@testable import FinderSync

final class SocketClientTests: XCTestCase {

    // MARK: - extractLines

    func testExtractLines_threeCompleteLines() {
        var data = Data("STATUS:OK:/a\nSTATUS:SYNC:/b\nSTATUS:ERROR:/c\n".utf8)
        let lines = SocketClient.extractLines(from: &data)
        XCTAssertEqual(lines.count, 3)
        XCTAssertEqual(lines[0], "STATUS:OK:/a")
        XCTAssertEqual(lines[1], "STATUS:SYNC:/b")
        XCTAssertEqual(lines[2], "STATUS:ERROR:/c")
        XCTAssertTrue(data.isEmpty, "buffer should be empty after all lines consumed")
    }

    func testExtractLines_partialLineRemainsInBuffer() {
        var data = Data("STATUS:OK:/a\nSTATUS:SYNC:/b\nSTATUS:".utf8)
        let lines = SocketClient.extractLines(from: &data)
        XCTAssertEqual(lines.count, 2)
        XCTAssertEqual(lines[0], "STATUS:OK:/a")
        XCTAssertEqual(lines[1], "STATUS:SYNC:/b")
        XCTAssertEqual(String(data: data, encoding: .utf8), "STATUS:",
                       "partial line must remain in buffer unchanged")
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
```

Commit: `git commit -m "feat(findersync): add SocketClient"`

---

### Task 3: StatusCache.swift

- [ ] Create `shell-integration/macos/FinderSync/StatusCache.swift`:

```swift
import Foundation

/// Thread-safe, in-process cache mapping absolute file-system paths to their
/// most recently known ocsyncd status tag (e.g. "OK", "SYNC", "ERROR").
///
/// All public methods may be called from any thread.
final class StatusCache {

    private var cache: [String: String] = [:]
    private let lock = NSLock()

    /// Store or overwrite the status for `path`.
    func set(path: String, status: String) {
        lock.lock()
        defer { lock.unlock() }
        cache[path] = status
    }

    /// Return the cached status for `path`, or `nil` if not yet known.
    func get(path: String) -> String? {
        lock.lock()
        defer { lock.unlock() }
        return cache[path]
    }

    /// Remove the entry for `path` (e.g. after a file is deleted).
    func remove(path: String) {
        lock.lock()
        defer { lock.unlock() }
        cache.removeValue(forKey: path)
    }

    /// Discard all cached entries (e.g. when the socket reconnects).
    func clear() {
        lock.lock()
        defer { lock.unlock() }
        cache.removeAll()
    }

    /// Snapshot of all currently tracked paths.
    func allPaths() -> [String] {
        lock.lock()
        defer { lock.unlock() }
        return Array(cache.keys)
    }
}
```

- [ ] Create `shell-integration/macos/FinderSync/StatusCacheTests.swift` (XCTest):

```swift
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
        // Write 100 distinct keys from concurrent threads, then verify all survive.
        DispatchQueue.concurrentPerform(iterations: iterations) { i in
            cache.set(path: "/sync/file\(i).txt", status: "OK")
        }
        XCTAssertEqual(cache.allPaths().count, iterations)
    }
}
```

Commit: `git commit -m "feat(findersync): add StatusCache"`

---

### Task 4: BadgeManager.swift

- [ ] Create `shell-integration/macos/FinderSync/BadgeManager.swift`:

```swift
import AppKit
import FinderSync

/// Registers Finder badge images and applies them to file URLs.
///
/// `registerBadges()` must be called once during extension initialisation
/// (from `FinderSyncExtension.init()`).  `setBadge(forPath:status:)` may be
/// called from any thread — `FIFinderSyncController` is thread-safe.
enum BadgeManager {

    // MARK: - Badge registration

    /// Register all five sync-state badge images with the FinderSync controller.
    ///
    /// Uses SF Symbols available on macOS 11+; the tint colour is applied by
    /// rendering the symbol into an `NSImage` with the appropriate `NSColor`.
    static func registerBadges() {
        let badges: [(identifier: String, symbol: String, color: NSColor, label: String)] = [
            ("synced",   "checkmark.circle.fill",           .systemGreen,  "Synced"),
            ("syncing",  "arrow.triangle.2.circlepath",     .systemBlue,   "Syncing"),
            ("warning",  "exclamationmark.triangle.fill",   .systemYellow, "Warning"),
            ("error",    "xmark.circle.fill",               .systemRed,    "Error"),
            ("excluded", "minus.circle",                    .systemGray,   "Excluded"),
        ]
        let controller = FIFinderSyncController.default()
        for badge in badges {
            if let image = tintedSymbol(named: badge.symbol, color: badge.color) {
                controller.setBadgeImage(image,
                                         label: badge.label,
                                         forBadgeIdentifier: badge.identifier)
            }
        }
    }

    // MARK: - Badge application

    /// Apply the badge that corresponds to `status` to the file at `path`.
    ///
    /// Passing an unrecognised status clears the badge (empty identifier).
    static func setBadge(forPath path: String, status: String) {
        let identifier = badgeIdentifier(for: status)
        FIFinderSyncController.default()
            .setBadgeIdentifier(identifier,
                                for: URL(fileURLWithPath: path))
    }

    // MARK: - Mapping (internal for testability)

    /// Map an ocsyncd status tag to a Finder badge identifier.
    ///
    /// Returns `""` (clear badge) for unknown tags so callers never
    /// display a stale icon.
    static func badgeIdentifier(for status: String) -> String {
        switch status {
        case "OK":       return "synced"
        case "SYNC":     return "syncing"
        case "WARNING":  return "warning"
        case "ERROR":    return "error"
        case "EXCLUDED": return "excluded"
        default:         return ""
        }
    }

    // MARK: - Helpers

    private static func tintedSymbol(named symbolName: String,
                                      color: NSColor) -> NSImage? {
        guard let symbol = NSImage(systemSymbolName: symbolName,
                                   accessibilityDescription: nil) else { return nil }
        let size = NSSize(width: 16, height: 16)
        let image = NSImage(size: size)
        image.lockFocus()
        color.setFill()
        let rect = NSRect(origin: .zero, size: size)
        symbol.draw(in: rect,
                    from: .zero,
                    operation: .sourceOver,
                    fraction: 1.0)
        image.unlockFocus()
        return image
    }
}
```

- [ ] Create `shell-integration/macos/FinderSync/BadgeManagerTests.swift` (XCTest):

```swift
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
```

Commit: `git commit -m "feat(findersync): add BadgeManager"`

---

### Task 5: MenuBuilder.swift

- [ ] Create `shell-integration/macos/FinderSync/MenuBuilder.swift`:

```swift
import AppKit

/// Parses the `GET_MENU_ITEMS` daemon response and builds an `NSMenu`.
enum MenuBuilder {

    // MARK: - Parsing

    /// Parse a raw `GET_MENU_ITEMS` response into structured tuples.
    ///
    /// Wire format:
    ///   `GET_MENU_ITEMS:path\x1ename:cmd:state\x1e...\n`
    ///
    /// - Parameter response: The full response string, including the header.
    /// - Returns: Array of `(name, command, enabled)` tuples.  Empty on error.
    static func parseMenuItems(
        _ response: String
    ) -> [(name: String, command: String, enabled: Bool)] {
        // Split on the record separator (U+001E ASCII unit separator).
        var records = response.components(separatedBy: "\u{1e}")
        guard !records.isEmpty else { return [] }
        records.removeFirst()  // discard "GET_MENU_ITEMS:path" header

        return records.compactMap { record in
            let trimmed = record.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmed.isEmpty else { return nil }
            // Each record: "name:cmd:state"
            let fields = trimmed.components(separatedBy: ":")
            guard fields.count >= 2 else { return nil }
            let name = fields[0]
            let command = fields[1]
            let enabled = fields.count < 3 || fields[2] != "disabled"
            return (name: name, command: command, enabled: enabled)
        }
    }

    // MARK: - Menu building

    /// Build an `NSMenu` from a list of parsed items.
    ///
    /// - Parameters:
    ///   - items: Items as returned by `parseMenuItems(_:)`.
    ///   - action: Called with the `command` string when the user clicks an item.
    /// - Returns: A populated `NSMenu` (may be empty if `items` is empty).
    static func buildMenu(
        items: [(name: String, command: String, enabled: Bool)],
        action: @escaping (String) -> Void
    ) -> NSMenu {
        let menu = NSMenu(title: "ownCloud")
        for item in items {
            if item.name == "-" {
                menu.addItem(.separator())
                continue
            }
            let menuItem = NSMenuItem(title: item.name,
                                       action: #selector(MenuItemTarget.invoke(_:)),
                                       keyEquivalent: "")
            menuItem.isEnabled = item.enabled
            // Wrap the action closure in a helper target object.
            let target = MenuItemTarget(command: item.command, action: action)
            menuItem.target = target
            // Retain the target via representedObject so it lives with the item.
            menuItem.representedObject = target
            menu.addItem(menuItem)
        }
        return menu
    }
}

// MARK: - Private helper

/// Lightweight NSObject that acts as the target for a single menu item.
private final class MenuItemTarget: NSObject {
    private let command: String
    private let action: (String) -> Void

    init(command: String, action: @escaping (String) -> Void) {
        self.command = command
        self.action = action
    }

    @objc func invoke(_ sender: NSMenuItem) {
        action(command)
    }
}
```

- [ ] Create `shell-integration/macos/FinderSync/MenuBuilderTests.swift` (XCTest):

```swift
import XCTest
@testable import FinderSync

final class MenuBuilderTests: XCTestCase {

    private let sampleResponse =
        "GET_MENU_ITEMS:/sync/file.txt\u{1e}" +
        "Share:SHARE:enabled\u{1e}" +
        "Make Available Locally:MAKE_AVAILABLE_LOCALLY:enabled\u{1e}" +
        "Make Online Only:MAKE_ONLINE_ONLY:disabled\u{1e}"

    // MARK: - parseMenuItems

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
        // Response with no items after the header separator.
        let items = MenuBuilder.parseMenuItems("GET_MENU_ITEMS:/path")
        XCTAssertTrue(items.isEmpty)
    }

    // MARK: - buildMenu

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
        // Simulate a click by sending the action to the target.
        let menuItem = menu.items[0]
        _ = menuItem.target?.perform(menuItem.action, with: menuItem)
        XCTAssertEqual(firedCommand, "SHARE")
    }
}
```

Commit: `git commit -m "feat(findersync): add MenuBuilder"`

---

### Task 6: FinderSyncExtension.swift — FINCFinderSync impl

- [ ] Create `shell-integration/macos/FinderSync/FinderSyncExtension.swift`:

```swift
import AppKit
import FinderSync

/// Principal class of the FinderSync App Extension.
///
/// macOS instantiates exactly one `FinderSyncExtension` per Finder process.
/// On init it:
///  1. Registers the five badge images.
///  2. Connects to the ocsyncd Unix socket via the shared App Group container.
///  3. Starts the receive loop; incoming broadcasts update the `StatusCache`
///     and call `FIFinderSyncController` to refresh badges.
class FinderSyncExtension: FIFinderSync {

    private let socket = SocketClient()
    private let cache  = StatusCache()

    override init() {
        super.init()
        BadgeManager.registerBadges()
        setupSocketCallbacks()
        connectToSocket()
    }

    // MARK: - Socket setup

    private func connectToSocket() {
        let appGroup = "group.org.owncloud.owncloud-sync"
        guard let containerURL = FileManager.default
                .containerURL(forSecurityApplicationGroupIdentifier: appGroup) else {
            NSLog("[FinderSync] cannot resolve App Group container URL")
            return
        }
        let socketPath = containerURL.appendingPathComponent("owncloud.sock").path
        socket.connect(socketPath: socketPath)
    }

    private func setupSocketCallbacks() {
        // Broadcast: a file's status changed.
        socket.onStatusBroadcast = { [weak self] tag, path in
            guard let self else { return }
            self.cache.set(path: path, status: tag)
            DispatchQueue.main.async {
                BadgeManager.setBadge(forPath: path, status: tag)
            }
        }

        // Broadcast: the daemon is asking us to monitor a new sync root.
        socket.onRegisterPath = { path in
            DispatchQueue.main.async {
                var urls = FIFinderSyncController.default().directoryURLs ?? []
                urls.insert(URL(fileURLWithPath: path))
                FIFinderSyncController.default().directoryURLs = urls
            }
        }

        // Broadcast: Finder should refresh its view of this path.
        socket.onUpdateView = { path in
            // No FinderSync API to force a view refresh; badge re-set has the
            // same visual effect for the common case.
            _ = path
        }
    }

    // MARK: - FIFinderSync overrides

    /// Called by Finder when it needs to display a badge for `url`.
    ///
    /// Returns a cached value immediately; if no entry exists, sends
    /// `RETRIEVE_FILE_STATUS` and updates the badge asynchronously.
    override func requestBadgeIdentifier(for url: URL) {
        let path = url.path

        if let cached = cache.get(path: path) {
            BadgeManager.setBadge(forPath: path, status: cached)
            return
        }

        socket.sendCommand("RETRIEVE_FILE_STATUS:\(path)") { [weak self] response in
            guard let self, let response else { return }
            // Response: STATUS:tag:path
            let parts = response.components(separatedBy: ":")
            guard parts.count >= 2 else { return }
            let tag = parts[1]
            self.cache.set(path: path, status: tag)
            DispatchQueue.main.async {
                BadgeManager.setBadge(forPath: path, status: tag)
            }
        }
    }

    // MARK: - Toolbar

    override var toolbarItemName: String {
        return "ownCloud"
    }

    override var toolbarItemToolTip: String {
        return "ownCloud sync actions"
    }

    override var toolbarItemImage: NSImage {
        return NSImage(systemSymbolName: "cloud",
                       accessibilityDescription: "ownCloud") ?? NSImage()
    }

    // MARK: - Context menu

    /// Build and return the context menu for the selected items.
    ///
    /// The call is synchronous; `GET_MENU_ITEMS` is sent synchronously via a
    /// short-lived semaphore wait (500 ms timeout) to satisfy Finder's
    /// requirement that `menu(for:)` returns before the event loop continues.
    override func menu(for menuKind: FIMenuKind) -> NSMenu {
        let fallback = NSMenu(title: "ownCloud")
        guard let selectedURLs = FIFinderSyncController.default().selectedItemURLs(),
              let firstURL = selectedURLs.first else {
            return fallback
        }

        let path = firstURL.path
        let semaphore = DispatchSemaphore(value: 0)
        var receivedResponse: String?

        socket.sendCommand("GET_MENU_ITEMS:\(path)") { response in
            receivedResponse = response
            semaphore.signal()
        }
        _ = semaphore.wait(timeout: .now() + 0.5)

        guard let response = receivedResponse else { return fallback }
        let items = MenuBuilder.parseMenuItems(response)

        return MenuBuilder.buildMenu(items: items) { [weak self] command in
            guard let self else { return }
            let paths = selectedURLs.map(\.path).joined(separator: "\u{1e}")
            self.socket.sendAndForget("\(command):\(paths)")
        }
    }
}
```

Commit: `git commit -m "feat(findersync): implement FINCFinderSync principal class"`

---

### Task 7: XCTest integration tests

- [ ] Create `shell-integration/macos/FinderSync/FinderSyncTests.swift` collecting all unit tests:

```swift
import XCTest
@testable import FinderSync

// All tests are defined in individual *Tests.swift files and executed by the
// FinderSyncTests scheme target. This file documents the test suite and
// provides combined helpers.

// Individual test classes:
//   SocketClientTests  — extractLines/buffer parsing (SocketClientTests.swift)
//   StatusCacheTests   — set/get/remove/clear/concurrent safety (StatusCacheTests.swift)
//   BadgeManagerTests  — badgeIdentifier mapping for all 6 inputs (BadgeManagerTests.swift)
//   MenuBuilderTests   — parseMenuItems / buildMenu (MenuBuilderTests.swift)

/// Combined smoke test verifying all four sub-systems wire together
/// without crashing when creating a FinderSyncExtension-like object
/// (no actual Finder host process needed for pure-logic paths).
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
        // 1000 lines, each ~30 bytes — verifies the loop terminates correctly.
        let raw = (0..<1000).map { "STATUS:OK:/sync/file\($0).txt" }.joined(separator: "\n") + "\n"
        var data = Data(raw.utf8)
        let lines = SocketClient.extractLines(from: &data)
        XCTAssertEqual(lines.count, 1000)
        XCTAssertTrue(data.isEmpty)
    }
}
```

- [ ] Run tests:

```bash
xcodebuild test \
  -scheme FinderSyncTests \
  -destination 'platform=macOS' \
  | xcpretty
```

Commit: `git commit -m "test(findersync): add unit tests"`

---

### Task 8: Manual testing guide

- [ ] Build `ocsync.app` with the FinderSync extension embedded:

```bash
xcodebuild \
  -project ocsync.xcodeproj \
  -scheme ocsync \
  -configuration Release \
  -derivedDataPath build/DerivedData \
  build
```

- [ ] Launch `ocsync.app` at least once so macOS registers the embedded extension (the system reads `FinderSync.appex` from the app bundle during first launch).
- [ ] Open **System Settings > Privacy & Security > Extensions > Finder Extensions** and verify **ownCloud** is listed.
- [ ] Enable the extension by toggling it on in System Settings.
- [ ] Open Finder and navigate to the configured sync folder root.
- [ ] Verify that files display badge overlays:
  - Green checkmark for `OK`
  - Blue circular arrows for `SYNC`
  - Yellow triangle for `WARNING`
  - Red X circle for `ERROR`
  - Grey minus circle for `EXCLUDED`
- [ ] Right-click a file inside the sync folder. Verify an **ownCloud** submenu appears in the context menu.
- [ ] Verify the submenu contains the items returned by `GET_MENU_ITEMS` for that file (e.g. Share, Make Available Locally, Make Online Only).
- [ ] Verify that disabled items appear greyed out and cannot be clicked.
- [ ] Click **Share** on a synced file. Verify `SHARE:path` is sent to the daemon (check daemon logs or Console.app filtered to process `ocsyncd`).
- [ ] Click **Make Available Locally** on a file with `EXCLUDED` status. Verify the badge changes to `SYNC` and then to `OK` as the daemon processes the request.
- [ ] Click **Make Online Only** on a fully synced file. Verify the badge changes from `synced` to `excluded`.
- [ ] Open **Console.app**, filter on process name `FinderSync`, and confirm no `[FinderSync]` error lines appear during normal operation.
- [ ] Stop `ocsyncd`. Verify the extension degrades gracefully (no crash; badges may remain stale).
- [ ] Restart `ocsyncd`. Verify the extension reconnects and begins applying fresh badges again.
- [ ] Verify no badge is shown for files outside any registered sync folder.
