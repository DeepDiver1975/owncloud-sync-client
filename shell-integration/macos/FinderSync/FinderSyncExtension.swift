import AppKit
import FinderSync

/// Principal class of the FinderSync App Extension.
///
/// macOS instantiates exactly one `FinderSyncExtension` per Finder process.
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
        socket.onStatusBroadcast = { [weak self] tag, path in
            guard let self else { return }
            self.cache.set(path: path, status: tag)
            DispatchQueue.main.async {
                BadgeManager.setBadge(forPath: path, status: tag)
            }
        }

        socket.onRegisterPath = { path in
            DispatchQueue.main.async {
                var urls = FIFinderSyncController.default().directoryURLs ?? []
                urls.insert(URL(fileURLWithPath: path))
                FIFinderSyncController.default().directoryURLs = urls
            }
        }

        socket.onUpdateView = { path in
            _ = path
        }
    }

    // MARK: - FIFinderSync overrides

    override func requestBadgeIdentifier(for url: URL) {
        let path = url.path

        if let cached = cache.get(path: path) {
            BadgeManager.setBadge(forPath: path, status: cached)
            return
        }

        socket.sendCommand("RETRIEVE_FILE_STATUS:\(path)") { [weak self] response in
            guard let self, let response else { return }
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
