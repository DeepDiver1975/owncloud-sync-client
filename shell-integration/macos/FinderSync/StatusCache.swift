import Foundation

/// Thread-safe, in-process cache mapping absolute file-system paths to their
/// most recently known ocsyncd status tag (e.g. "OK", "SYNC", "ERROR").
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
