import Foundation

/// XPC command handler methods that FileProviderExtension must implement.
protocol FileProviderXPCMethods: AnyObject {
    func xpcCreatePlaceholder(path: String, etag: String, size: UInt64, mtime: Int64) -> XPCReply
    func xpcUpdatePlaceholder(path: String, etag: String, size: UInt64, mtime: Int64) -> XPCReply
    func xpcHydrate(path: String) -> XPCReply
    func xpcDehydrate(path: String) -> XPCReply
    func xpcIsVirtual(path: String) -> XPCReply
    func xpcStatus(path: String) -> XPCReply
    func xpcSetPinned(path: String, pinned: Bool) -> XPCReply
}
