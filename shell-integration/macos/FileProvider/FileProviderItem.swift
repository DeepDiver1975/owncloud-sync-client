import FileProvider
import UniformTypeIdentifiers

// MARK: - FileProviderItem

/// Represents a single file or directory in the ownCloud sync domain.
final class FileProviderItem: NSObject, NSFileProviderItem {

    // MARK: Stored properties

    private let _identifier:              NSFileProviderItemIdentifier
    private let _parentIdentifier:        NSFileProviderItemIdentifier
    private let _filename:                String
    private let _isDirectory:             Bool
    private let _documentSize:            NSNumber?
    private let _contentModificationDate: Date?
    private let _etag:                    String

    // MARK: Init

    init(
        identifier:       NSFileProviderItemIdentifier,
        parent:           NSFileProviderItemIdentifier,
        filename:         String,
        isDirectory:      Bool,
        size:             Int64?,
        modificationDate: Date?,
        etag:             String
    ) {
        self._identifier            = identifier
        self._parentIdentifier      = parent
        self._filename              = filename
        self._isDirectory           = isDirectory
        self._documentSize          = size.map { NSNumber(value: $0) }
        self._contentModificationDate = modificationDate
        self._etag                  = etag
        super.init()
    }

    // MARK: NSFileProviderItem — required

    var itemIdentifier: NSFileProviderItemIdentifier { _identifier }
    var parentItemIdentifier: NSFileProviderItemIdentifier { _parentIdentifier }
    var filename: String { _filename }

    var contentType: UTType {
        _isDirectory
            ? .folder
            : UTType(filenameExtension: (_filename as NSString).pathExtension) ?? .data
    }

    // MARK: NSFileProviderItem — optional but recommended

    var documentSize: NSNumber? { _documentSize }
    var contentModificationDate: Date? { _contentModificationDate }

    /// Use the ETag bytes as the version identifier.
    var versionIdentifier: Data? {
        _etag.data(using: .utf8)
    }

    /// Capabilities for this item.
    var capabilities: NSFileProviderItemCapabilities {
        if _isDirectory {
            return [.allowsContentEnumerating, .allowsAddingSubItems, .allowsRenaming, .allowsDeleting]
        }
        return [.allowsReading, .allowsWriting, .allowsRenaming, .allowsDeleting]
    }
}
