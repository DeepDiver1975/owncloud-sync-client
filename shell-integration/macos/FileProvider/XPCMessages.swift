import Foundation

// MARK: - Command type discriminator

enum XPCCommandType: String, Codable {
    case createPlaceholder = "create_placeholder"
    case updatePlaceholder = "update_placeholder"
    case hydrate           = "hydrate"
    case dehydrate         = "dehydrate"
    case isVirtual         = "is_virtual"
    case status            = "status"
    case setPinned         = "set_pinned"
}

// MARK: - Command

/// A command sent from the Rust daemon to the Swift FileProvider extension.
struct XPCCommand: Codable {
    let cmd:    XPCCommandType
    let path:   String
    let etag:   String?
    let size:   UInt64?
    let mtime:  Int64?
    let pinned: Bool?
}

// MARK: - Reply

/// A reply sent from the Swift extension back to the Rust daemon.
struct XPCReply: Codable {
    let ok:     Bool
    let error:  String?
    let bool:   Bool?
    let status: String?

    // MARK: Factory helpers

    static func success() -> XPCReply {
        XPCReply(ok: true, error: nil, bool: nil, status: nil)
    }

    static func failure(_ msg: String) -> XPCReply {
        XPCReply(ok: false, error: msg, bool: nil, status: nil)
    }

    static func boolResult(_ v: Bool) -> XPCReply {
        XPCReply(ok: true, error: nil, bool: v, status: nil)
    }

    static func statusResult(_ s: String) -> XPCReply {
        XPCReply(ok: true, error: nil, bool: nil, status: s)
    }
}
