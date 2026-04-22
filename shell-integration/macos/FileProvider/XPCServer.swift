import Foundation
import FileProvider

// MARK: - XPCServer

/// Listens on the XPC mach service and dispatches decoded commands to the
/// FileProvider extension.
final class XPCServer: NSObject, NSXPCListenerDelegate {

    // MARK: Properties

    private let listener: NSXPCListener
    private weak var provider: FileProviderExtension?

    private let encoder = JSONEncoder()
    private let decoder = JSONDecoder()

    // MARK: Init

    init(provider: FileProviderExtension) {
        self.listener = NSXPCListener(
            machServiceName: "org.owncloud.owncloud-sync.fileprovider-xpc"
        )
        self.provider = provider
        super.init()
        listener.delegate = self
    }

    // MARK: Lifecycle

    func start() {
        listener.resume()
    }

    func stop() {
        listener.invalidate()
    }

    // MARK: NSXPCListenerDelegate

    func listener(
        _ listener: NSXPCListener,
        shouldAcceptNewConnection connection: NSXPCConnection
    ) -> Bool {
        connection.exportedInterface = NSXPCInterface(with: XPCServerProtocol.self)
        connection.exportedObject    = self
        connection.resume()
        return true
    }

    // MARK: Command dispatch

    /// Decode an `XPCCommand` from `data`, execute it against the provider,
    /// and return an encoded `XPCReply`.
    func handleCommand(_ data: Data) -> Data {
        let fallback = encodeReply(.failure("internal error encoding reply"))

        guard let cmd = try? decoder.decode(XPCCommand.self, from: data) else {
            return encode(.failure("could not decode XPCCommand")) ?? fallback
        }

        guard let provider = provider else {
            return encode(.failure("provider is unavailable")) ?? fallback
        }

        let reply: XPCReply

        switch cmd.cmd {

        case .createPlaceholder:
            guard let etag  = cmd.etag,
                  let size  = cmd.size,
                  let mtime = cmd.mtime else {
                return encode(.failure("createPlaceholder missing etag/size/mtime")) ?? fallback
            }
            reply = provider.xpcCreatePlaceholder(
                path: cmd.path, etag: etag, size: size, mtime: mtime
            )

        case .updatePlaceholder:
            guard let etag  = cmd.etag,
                  let size  = cmd.size,
                  let mtime = cmd.mtime else {
                return encode(.failure("updatePlaceholder missing etag/size/mtime")) ?? fallback
            }
            reply = provider.xpcUpdatePlaceholder(
                path: cmd.path, etag: etag, size: size, mtime: mtime
            )

        case .hydrate:
            reply = provider.xpcHydrate(path: cmd.path)

        case .dehydrate:
            reply = provider.xpcDehydrate(path: cmd.path)

        case .isVirtual:
            reply = provider.xpcIsVirtual(path: cmd.path)

        case .status:
            reply = provider.xpcStatus(path: cmd.path)

        case .setPinned:
            guard let pinned = cmd.pinned else {
                return encode(.failure("setPinned missing 'pinned' field")) ?? fallback
            }
            reply = provider.xpcSetPinned(path: cmd.path, pinned: pinned)
        }

        return encode(reply) ?? fallback
    }

    // MARK: Private helpers

    private func encode(_ reply: XPCReply) -> Data? {
        try? encoder.encode(reply)
    }

    private func encodeReply(_ reply: XPCReply) -> Data {
        let json = reply.ok
            ? #"{"ok":true}"#
            : #"{"ok":false,"error":"\#(reply.error ?? "unknown")"}"#
        return Data(json.utf8)
    }
}

// MARK: - XPCServerProtocol (Objective-C compatible)

@objc protocol XPCServerProtocol {
    /// Entry point called by the Rust side; `data` is a JSON-encoded
    /// `XPCCommand`; the return value is a JSON-encoded `XPCReply`.
    func handleCommand(_ data: Data, reply: @escaping (Data) -> Void)
}

extension XPCServer: XPCServerProtocol {
    func handleCommand(_ data: Data, reply: @escaping (Data) -> Void) {
        reply(handleCommand(data))
    }
}
