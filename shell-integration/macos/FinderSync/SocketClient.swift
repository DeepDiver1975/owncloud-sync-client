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
    private var pendingCompletions: [(String?) -> Void] = []

    // MARK: - Connection lifecycle

    /// Open a persistent connection to the ocsyncd Unix socket at `socketPath`.
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
    /// line and deliver it to `completion`. Passes `nil` on send failure.
    func sendCommand(_ cmd: String, completion: @escaping (String?) -> Void) {
        guard let connection, connection.state == .ready else {
            completion(nil)
            return
        }
        pendingCompletions.append(completion)

        let payload = Data((cmd + "\n").utf8)
        connection.send(content: payload, completion: .contentProcessed { [weak self] error in
            if let error {
                NSLog("[FinderSync] send error for '\(cmd)': \(error)")
                self?.queue.async {
                    if let idx = self?.pendingCompletions.indices.first {
                        let cb = self!.pendingCompletions.remove(at: idx)
                        cb(nil)
                    }
                }
            }
        })
    }

    /// Send `cmd` and ignore the response (fire-and-forget).
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
                self.startReceiving()
            }
        }
    }

    /// Extract all complete newline-terminated lines from `buffer`, leaving
    /// any partial line in place.
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

    private func dispatch(line: String) {
        if line.hasPrefix("STATUS:") {
            let parts = line.components(separatedBy: ":")
            if parts.count >= 3 {
                let tag = parts[1]
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
