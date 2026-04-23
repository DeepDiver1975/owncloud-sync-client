import AppKit
import FinderSync

/// Registers Finder badge images and applies them to file URLs.
enum BadgeManager {

    // MARK: - Badge registration

    /// Register all five sync-state badge images with the FinderSync controller.
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
    static func setBadge(forPath path: String, status: String) {
        let identifier = badgeIdentifier(for: status)
        FIFinderSyncController.default()
            .setBadgeIdentifier(identifier,
                                for: URL(fileURLWithPath: path))
    }

    // MARK: - Mapping

    /// Map an ocsyncd status tag to a Finder badge identifier.
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
        symbol.draw(in: rect, from: .zero, operation: .sourceOver, fraction: 1.0)
        image.unlockFocus()
        return image
    }
}
