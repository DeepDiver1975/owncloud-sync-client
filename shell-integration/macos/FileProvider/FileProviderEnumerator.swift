import FileProvider
import Foundation

// MARK: - FileProviderEnumerator

/// Enumerates the contents of a single directory inside the ownCloud sync root.
final class FileProviderEnumerator: NSObject, NSFileProviderEnumerator {

    private let directoryURL:        URL
    private let containerIdentifier: NSFileProviderItemIdentifier

    init(directoryURL: URL, containerIdentifier: NSFileProviderItemIdentifier) {
        self.directoryURL        = directoryURL
        self.containerIdentifier = containerIdentifier
        super.init()
    }

    // MARK: NSFileProviderEnumerator

    func invalidate() {}

    func enumerateItems(
        for observer: NSFileProviderEnumerationObserver,
        startingAt page: NSFileProviderPage
    ) {
        let fm = FileManager.default
        var items: [NSFileProviderItem] = []
        var enumerationError: Error?

        do {
            let entries = try fm.contentsOfDirectory(
                at: directoryURL,
                includingPropertiesForKeys: [
                    .fileSizeKey, .contentModificationDateKey, .isDirectoryKey,
                ],
                options: .skipsHiddenFiles
            )

            for entry in entries {
                let resourceValues = try entry.resourceValues(forKeys: [
                    .fileSizeKey, .contentModificationDateKey, .isDirectoryKey,
                ])

                let isDir    = resourceValues.isDirectory ?? false
                let size     = resourceValues.fileSize.map { Int64($0) }
                let modDate  = resourceValues.contentModificationDate
                let filename = entry.lastPathComponent

                let relPath = directoryURL.appendingPathComponent(filename).path
                let itemIdentifier = NSFileProviderItemIdentifier(relPath)

                let item = FileProviderItem(
                    identifier:       itemIdentifier,
                    parent:           containerIdentifier,
                    filename:         filename,
                    isDirectory:      isDir,
                    size:             isDir ? nil : size,
                    modificationDate: modDate,
                    etag:             ""
                )

                items.append(item)
            }
        } catch {
            enumerationError = error
        }

        if !items.isEmpty {
            observer.didEnumerate(items)
        }

        observer.finishEnumerating(upTo: nil)

        if let err = enumerationError {
            NSLog("[FileProviderEnumerator] enumeration error for \(directoryURL.path): \(err)")
        }
    }
}
