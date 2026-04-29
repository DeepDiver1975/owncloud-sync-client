import AppKit

/// Parses the `GET_MENU_ITEMS` daemon response and builds an `NSMenu`.
enum MenuBuilder {

    // MARK: - Parsing

    /// Parse a raw `GET_MENU_ITEMS` response into structured tuples.
    ///
    /// Wire format: `GET_MENU_ITEMS:path\x1ename:cmd:state\x1e...\n`
    static func parseMenuItems(
        _ response: String
    ) -> [(name: String, command: String, enabled: Bool)] {
        var records = response.components(separatedBy: "\u{1e}")
        guard !records.isEmpty else { return [] }
        records.removeFirst()  // discard "GET_MENU_ITEMS:path" header

        return records.compactMap { record in
            let trimmed = record.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmed.isEmpty else { return nil }
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
            let target = MenuItemTarget(command: item.command, action: action)
            menuItem.target = target
            menuItem.representedObject = target
            menu.addItem(menuItem)
        }
        return menu
    }
}

// MARK: - Private helper

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
