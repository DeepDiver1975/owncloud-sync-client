import FileProvider
import Foundation

// MARK: - FileProviderExtension

/// Main entry point for the ownCloud FileProvider app extension.
///
/// Conforms to `NSFileProviderReplicatedExtension` (macOS 12+).
final class FileProviderExtension: NSObject, NSFileProviderReplicatedExtension {

    let domain: NSFileProviderDomain
    private var xpcServer: XPCServer?

    private var syncRoot: URL {
        NSFileProviderManager(for: domain)?.documentStorageURL
            ?? FileManager.default.temporaryDirectory
    }

    // MARK: NSFileProviderReplicatedExtension

    required init(domain: NSFileProviderDomain) {
        self.domain = domain
        super.init()
        let server = XPCServer(provider: self)
        server.start()
        self.xpcServer = server
        NSLog("[FileProviderExtension] started, domain=\(domain.displayName)")
    }

    func invalidate() {
        xpcServer?.stop()
        xpcServer = nil
        NSLog("[FileProviderExtension] invalidated")
    }

    // MARK: Item lookup

    func item(
        for identifier: NSFileProviderItemIdentifier,
        request: NSFileProviderRequest,
        completionHandler: @escaping (NSFileProviderItem?, Error?) -> Void
    ) -> Progress {
        let url = localURL(for: identifier)
        let fm  = FileManager.default

        guard fm.fileExists(atPath: url.path) else {
            completionHandler(nil, NSFileProviderError(.noSuchItem))
            return Progress()
        }

        do {
            let resourceValues = try url.resourceValues(forKeys: [
                .isDirectoryKey, .fileSizeKey, .contentModificationDateKey,
            ])
            let item = FileProviderItem(
                identifier:       identifier,
                parent:           parentIdentifier(for: url),
                filename:         url.lastPathComponent,
                isDirectory:      resourceValues.isDirectory ?? false,
                size:             resourceValues.fileSize.map { Int64($0) },
                modificationDate: resourceValues.contentModificationDate,
                etag:             ""
            )
            completionHandler(item, nil)
        } catch {
            completionHandler(nil, error)
        }

        return Progress()
    }

    // MARK: Fetch contents (hydration)

    func fetchContents(
        for itemIdentifier: NSFileProviderItemIdentifier,
        version requestedVersion: NSFileProviderItemVersion?,
        request: NSFileProviderRequest,
        completionHandler: @escaping (URL?, NSFileProviderItem?, Error?) -> Void
    ) -> Progress {
        let localFileURL = localURL(for: itemIdentifier)
        let relativePath = relativePath(for: itemIdentifier)

        sendHydrationNeededEvent(path: relativePath, domainID: domain.identifier.rawValue)

        let progress = Progress(totalUnitCount: 100)
        let deadline = DispatchTime.now() + .seconds(30)
        let queue    = DispatchQueue.global(qos: .userInitiated)

        queue.async {
            var elapsed: Double = 0
            while !FileManager.default.fileExists(atPath: localFileURL.path) {
                if DispatchTime.now() > deadline {
                    completionHandler(nil, nil, NSFileProviderError(.serverUnreachable))
                    return
                }
                Thread.sleep(forTimeInterval: 0.1)
                elapsed += 0.1
                progress.completedUnitCount = Int64(min(99, elapsed / 30.0 * 100))
            }

            progress.completedUnitCount = 100

            do {
                let rv = try localFileURL.resourceValues(forKeys: [
                    .isDirectoryKey, .fileSizeKey, .contentModificationDateKey,
                ])
                let item = FileProviderItem(
                    identifier:       itemIdentifier,
                    parent:           self.parentIdentifier(for: localFileURL),
                    filename:         localFileURL.lastPathComponent,
                    isDirectory:      rv.isDirectory ?? false,
                    size:             rv.fileSize.map { Int64($0) },
                    modificationDate: rv.contentModificationDate,
                    etag:             ""
                )
                completionHandler(localFileURL, item, nil)
            } catch {
                completionHandler(nil, nil, error)
            }
        }

        return progress
    }

    // MARK: Mutations (stub implementations)

    func createItem(
        basedOn itemTemplate: NSFileProviderItem,
        fields: NSFileProviderItemFields,
        contents url: URL?,
        options: NSFileProviderCreateItemOptions = [],
        request: NSFileProviderRequest,
        completionHandler: @escaping (NSFileProviderItem?, NSFileProviderItemFields, Bool, Error?) -> Void
    ) -> Progress {
        completionHandler(itemTemplate, [], false, nil)
        return Progress()
    }

    func modifyItem(
        _ item: NSFileProviderItem,
        baseVersion version: NSFileProviderItemVersion,
        changedFields: NSFileProviderItemFields,
        contents newContents: URL?,
        options: NSFileProviderModifyItemOptions = [],
        request: NSFileProviderRequest,
        completionHandler: @escaping (NSFileProviderItem?, NSFileProviderItemFields, Bool, Error?) -> Void
    ) -> Progress {
        completionHandler(item, [], false, nil)
        return Progress()
    }

    func deleteItem(
        identifier: NSFileProviderItemIdentifier,
        baseVersion version: NSFileProviderItemVersion,
        options: NSFileProviderDeleteItemOptions = [],
        request: NSFileProviderRequest,
        completionHandler: @escaping (Error?) -> Void
    ) -> Progress {
        completionHandler(nil)
        return Progress()
    }

    // MARK: Enumeration

    func enumerator(
        for containerItemIdentifier: NSFileProviderItemIdentifier,
        request: NSFileProviderRequest
    ) throws -> NSFileProviderEnumerator {
        let containerURL: URL

        switch containerItemIdentifier {
        case .rootContainer:
            containerURL = syncRoot
        case .workingSet:
            throw NSFileProviderError(.noSuchItem)
        default:
            containerURL = localURL(for: containerItemIdentifier)
        }

        return FileProviderEnumerator(
            directoryURL:        containerURL,
            containerIdentifier: containerItemIdentifier
        )
    }

    // MARK: - XPC command methods

    func xpcCreatePlaceholder(path: String, etag: String, size: UInt64, mtime: Int64) -> XPCReply {
        let url = syncRoot.appendingPathComponent(path)
        do {
            try FileManager.default.createDirectory(
                at: url.deletingLastPathComponent(),
                withIntermediateDirectories: true
            )
            if !FileManager.default.fileExists(atPath: url.path) {
                FileManager.default.createFile(atPath: url.path, contents: nil)
            }
            try url.setExtendedAttribute("com.owncloud.etag", value: etag.data(using: .utf8)!)
            return .success()
        } catch {
            return .failure(error.localizedDescription)
        }
    }

    func xpcUpdatePlaceholder(path: String, etag: String, size: UInt64, mtime: Int64) -> XPCReply {
        let url = syncRoot.appendingPathComponent(path)
        do {
            try url.setExtendedAttribute("com.owncloud.etag", value: etag.data(using: .utf8)!)
            return .success()
        } catch {
            return .failure(error.localizedDescription)
        }
    }

    func xpcHydrate(path: String) -> XPCReply {
        return .success()
    }

    func xpcDehydrate(path: String) -> XPCReply {
        let url = syncRoot.appendingPathComponent(path)
        do {
            try Data().write(to: url)
            return .success()
        } catch {
            return .failure(error.localizedDescription)
        }
    }

    func xpcIsVirtual(path: String) -> XPCReply {
        let url  = syncRoot.appendingPathComponent(path)
        let size = (try? url.resourceValues(forKeys: [.fileSizeKey]))?.fileSize ?? -1
        let isVirtual = FileManager.default.fileExists(atPath: url.path) && size == 0
        return .boolResult(isVirtual)
    }

    func xpcStatus(path: String) -> XPCReply {
        let url = syncRoot.appendingPathComponent(path)
        guard FileManager.default.fileExists(atPath: url.path) else {
            return .failure("file not found: \(path)")
        }
        let size = (try? url.resourceValues(forKeys: [.fileSizeKey]))?.fileSize ?? -1
        let statusString = size == 0 ? "Dehydrated" : "Hydrated"
        return .statusResult(statusString)
    }

    func xpcSetPinned(path: String, pinned: Bool) -> XPCReply {
        let url = syncRoot.appendingPathComponent(path)
        do {
            let pinValue = pinned ? "1" : "0"
            try url.setExtendedAttribute(
                "com.owncloud.pinned",
                value: pinValue.data(using: .utf8)!
            )
            return .success()
        } catch {
            return .failure(error.localizedDescription)
        }
    }

    // MARK: - Private helpers

    private func localURL(for identifier: NSFileProviderItemIdentifier) -> URL {
        if identifier == .rootContainer {
            return syncRoot
        }
        return URL(fileURLWithPath: identifier.rawValue)
    }

    private func parentIdentifier(for url: URL) -> NSFileProviderItemIdentifier {
        let parent = url.deletingLastPathComponent()
        if parent == syncRoot {
            return .rootContainer
        }
        return NSFileProviderItemIdentifier(parent.path)
    }

    private func relativePath(for identifier: NSFileProviderItemIdentifier) -> String {
        let absPath  = identifier.rawValue
        let rootPath = syncRoot.path
        if absPath.hasPrefix(rootPath) {
            return String(absPath.dropFirst(rootPath.count + 1))
        }
        return absPath
    }

    private func sendHydrationNeededEvent(path: String, domainID: String) {
        let eventDict: [String: Any] = [
            "event":     "hydration_needed",
            "path":      path,
            "domain_id": domainID,
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: eventDict) else { return }

        let backchannelService = "org.owncloud.owncloud-sync.daemon-xpc"
        let connection = NSXPCConnection(machServiceName: backchannelService, options: [])
        connection.resume()

        let msg     = xpc_dictionary_create(nil, nil, 0)
        let bytes   = (data as NSData).bytes
        let xpcData = xpc_data_create(bytes, data.count)
        xpc_dictionary_set_value(msg, "data", xpcData)
        xpc_connection_send_message(
            connection.value(forKey: "_xpcConnection") as! xpc_connection_t,
            msg
        )

        NSLog("[FileProviderExtension] sent hydration_needed for path=\(path)")
    }
}

// MARK: - URL extended attribute helpers

private extension URL {
    func setExtendedAttribute(_ name: String, value: Data) throws {
        try value.withUnsafeBytes { ptr in
            let rc = setxattr(self.path, name, ptr.baseAddress, value.count, 0, 0)
            if rc != 0 {
                throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EPERM)
            }
        }
    }
}
