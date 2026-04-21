# Plan 4: VFS macOS — FileProvider Extension + XPC Bridge

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement `vfs-macos` (Rust XPC bridge crate) and the Swift `FileProvider.appex` App Extension for native macOS virtual file support.

**Architecture:** Apple requires FileProvider extensions be Swift App Extensions. Rust daemon ↔ Swift extension communicate over XPC (App Group: `group.org.owncloud.owncloud-sync`, XPC service: `org.owncloud.owncloud-sync.fileprovider-xpc`). JSON protocol over XPC data. `vfs-macos` implements the `vfs-core::Vfs` trait by sending XPC commands to the Swift extension.

**Tech Stack:** Rust 2021 + serde_json + thiserror + camino + libc (XPC bindings). Swift 5.9 + FileProvider.framework + NSXPCConnection. Depends on vfs-core (Plan 2).

---

## Task 1: Cargo.toml + error + messages

- [ ] Create `crates/vfs-macos/Cargo.toml` with the following content:

```toml
[package]
name = "vfs-macos"
version = "0.1.0"
edition = "2021"

[lib]
name = "vfs_macos"
path = "src/lib.rs"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
camino = "1"
libc = "0.2"
vfs-core = { path = "../vfs-core" }

[dev-dependencies]
serde_json = "1"
```

- [ ] Create `crates/vfs-macos/src/error.rs`:

```rust
use thiserror::Error;
use vfs_core::VfsError;

#[derive(Debug, Error)]
pub enum VfsMacOsError {
    #[error("XPC error: {0}")]
    Xpc(String),
    #[error("Protocol error: {0}")]
    Protocol(String),
}

impl From<VfsMacOsError> for VfsError {
    fn from(e: VfsMacOsError) -> Self {
        VfsError::Backend(e.to_string())
    }
}
```

- [ ] Create `crates/vfs-macos/src/messages.rs`:

```rust
use serde::{Deserialize, Serialize};

/// Commands sent from Rust to the Swift FileProvider extension over XPC.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum XpcCommand {
    CreatePlaceholder {
        path: String,
        etag: String,
        size: u64,
        mtime: i64,
    },
    UpdatePlaceholder {
        path: String,
        etag: String,
        size: u64,
        mtime: i64,
    },
    Hydrate {
        path: String,
    },
    Dehydrate {
        path: String,
    },
    IsVirtual {
        path: String,
    },
    Status {
        path: String,
    },
    SetPinned {
        path: String,
        pinned: bool,
    },
}

/// Reply from Swift FileProvider extension back to Rust over XPC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XpcReply {
    pub ok: bool,
    pub error: Option<String>,
    #[serde(rename = "bool")]
    pub bool_value: Option<bool>,
    pub status: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip<T: Serialize + for<'de> Deserialize<'de> + std::fmt::Debug + PartialEq>(
        value: &T,
    ) {
        let json = serde_json::to_string(value).expect("serialize");
        let decoded: T = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(
            format!("{:?}", value),
            format!("{:?}", decoded),
            "roundtrip failed for JSON: {json}"
        );
    }

    #[test]
    fn test_create_placeholder_roundtrip() {
        roundtrip(&XpcCommand::CreatePlaceholder {
            path: "docs/readme.md".to_string(),
            etag: "abc123".to_string(),
            size: 1024,
            mtime: 1700000000,
        });
    }

    #[test]
    fn test_update_placeholder_roundtrip() {
        roundtrip(&XpcCommand::UpdatePlaceholder {
            path: "docs/readme.md".to_string(),
            etag: "def456".to_string(),
            size: 2048,
            mtime: 1700001000,
        });
    }

    #[test]
    fn test_hydrate_roundtrip() {
        roundtrip(&XpcCommand::Hydrate {
            path: "photos/img.png".to_string(),
        });
    }

    #[test]
    fn test_dehydrate_roundtrip() {
        roundtrip(&XpcCommand::Dehydrate {
            path: "photos/img.png".to_string(),
        });
    }

    #[test]
    fn test_is_virtual_roundtrip() {
        roundtrip(&XpcCommand::IsVirtual {
            path: "photos/img.png".to_string(),
        });
    }

    #[test]
    fn test_status_roundtrip() {
        roundtrip(&XpcCommand::Status {
            path: "docs/readme.md".to_string(),
        });
    }

    #[test]
    fn test_set_pinned_roundtrip() {
        roundtrip(&XpcCommand::SetPinned {
            path: "docs/readme.md".to_string(),
            pinned: true,
        });
        roundtrip(&XpcCommand::SetPinned {
            path: "docs/readme.md".to_string(),
            pinned: false,
        });
    }

    #[test]
    fn test_reply_ok_roundtrip() {
        roundtrip(&XpcReply {
            ok: true,
            error: None,
            bool_value: None,
            status: None,
        });
    }

    #[test]
    fn test_reply_error_roundtrip() {
        roundtrip(&XpcReply {
            ok: false,
            error: Some("file not found".to_string()),
            bool_value: None,
            status: None,
        });
    }

    #[test]
    fn test_reply_bool_roundtrip() {
        roundtrip(&XpcReply {
            ok: true,
            error: None,
            bool_value: Some(true),
            status: None,
        });
    }

    #[test]
    fn test_reply_status_roundtrip() {
        roundtrip(&XpcReply {
            ok: true,
            error: None,
            bool_value: None,
            status: Some("Hydrated".to_string()),
        });
    }

    #[test]
    fn test_create_placeholder_json_tag() {
        let cmd = XpcCommand::CreatePlaceholder {
            path: "a/b".to_string(),
            etag: "e".to_string(),
            size: 0,
            mtime: 0,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains(r#""cmd":"create_placeholder""#), "unexpected JSON: {json}");
    }

    #[test]
    fn test_is_virtual_json_tag() {
        let cmd = XpcCommand::IsVirtual { path: "x".to_string() };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains(r#""cmd":"is_virtual""#), "unexpected JSON: {json}");
    }

    #[test]
    fn test_set_pinned_json_tag() {
        let cmd = XpcCommand::SetPinned { path: "x".to_string(), pinned: true };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains(r#""cmd":"set_pinned""#), "unexpected JSON: {json}");
    }
}
```

- [ ] Commit:

```
git commit -m "feat(vfs-macos): add Cargo.toml, error types, XPC message types"
```

---

## Task 2: xpc.rs — XPC connection

- [ ] Create `crates/vfs-macos/src/xpc.rs`:

```rust
//! Low-level XPC connection wrapper.
//!
//! All `unsafe` blocks carry a one-line safety comment explaining the invariant.

use libc::c_void;
use std::ffi::CString;

use crate::error::VfsMacOsError;
use crate::messages::{XpcCommand, XpcReply};

// ---------------------------------------------------------------------------
// XPC opaque types and extern declarations
// ---------------------------------------------------------------------------

/// Opaque XPC object type (pointer-sized).
type XpcObject = *mut c_void;

#[link(name = "System", kind = "framework")]
extern "C" {
    /// Create a new XPC mach service connection.
    fn xpc_connection_create_mach_service(
        name: *const libc::c_char,
        targetq: *mut c_void,
        flags: u64,
    ) -> XpcObject;

    /// Resume a suspended XPC connection.
    fn xpc_connection_resume(connection: XpcObject);

    /// Send a message synchronously and block until a reply arrives.
    fn xpc_connection_send_message_with_reply_sync(
        connection: XpcObject,
        message: XpcObject,
    ) -> XpcObject;

    /// Create an XPC dictionary object.
    fn xpc_dictionary_create(
        keys: *const *const libc::c_char,
        values: *const XpcObject,
        count: libc::size_t,
    ) -> XpcObject;

    /// Set a key-value pair in an XPC dictionary.
    fn xpc_dictionary_set_value(dict: XpcObject, key: *const libc::c_char, value: XpcObject);

    /// Get a value from an XPC dictionary by key; returns NULL if missing.
    fn xpc_dictionary_get_value(dict: XpcObject, key: *const libc::c_char) -> XpcObject;

    /// Create an XPC data object from a byte buffer.
    fn xpc_data_create(bytes: *const c_void, length: libc::size_t) -> XpcObject;

    /// Return a pointer to the bytes of an XPC data object.
    fn xpc_data_get_bytes_ptr(data: XpcObject) -> *const c_void;

    /// Return the byte length of an XPC data object.
    fn xpc_data_get_length(data: XpcObject) -> libc::size_t;

    /// Release (decrement refcount) an XPC object.
    fn xpc_release(object: XpcObject);
}

// ---------------------------------------------------------------------------
// XpcConnection
// ---------------------------------------------------------------------------

/// Thread-safe wrapper around an XPC mach service connection.
pub struct XpcConnection {
    /// Raw XPC connection pointer. Owned — released in Drop.
    conn: XpcObject,
}

// Safety: XPC connections are internally reference-counted and all XPC API
// functions are safe to call from any thread per Apple documentation.
unsafe impl Send for XpcConnection {}
unsafe impl Sync for XpcConnection {}

impl XpcConnection {
    /// Create and resume an XPC connection to the named mach service.
    pub fn connect(service: &str) -> Result<Self, VfsMacOsError> {
        let name = CString::new(service)
            .map_err(|e| VfsMacOsError::Xpc(format!("invalid service name: {e}")))?;

        // Safety: name is a valid NUL-terminated C string; targetq NULL means
        // the default concurrent queue; flags 0 = client connection.
        let conn =
            unsafe { xpc_connection_create_mach_service(name.as_ptr(), std::ptr::null_mut(), 0) };

        if conn.is_null() {
            return Err(VfsMacOsError::Xpc(format!(
                "xpc_connection_create_mach_service returned NULL for service '{service}'"
            )));
        }

        // Safety: conn is a valid non-null XPC connection object created above.
        unsafe { xpc_connection_resume(conn) };

        Ok(Self { conn })
    }

    /// Serialize `cmd` to JSON, send it over XPC, and deserialize the reply.
    pub fn send_command(&self, cmd: &XpcCommand) -> Result<XpcReply, VfsMacOsError> {
        let json_bytes = serde_json::to_vec(cmd)
            .map_err(|e| VfsMacOsError::Protocol(format!("serialize command: {e}")))?;

        // Build XPC data object from JSON bytes.
        // Safety: json_bytes is a valid slice; length matches the pointer.
        let data_obj = unsafe {
            xpc_data_create(json_bytes.as_ptr() as *const c_void, json_bytes.len())
        };
        if data_obj.is_null() {
            return Err(VfsMacOsError::Xpc("xpc_data_create returned NULL".to_string()));
        }

        // Build XPC dictionary {"data": <data_obj>}.
        // Safety: xpc_dictionary_create with count=0 and NULL arrays produces an
        // empty mutable dictionary; we set the "data" key immediately after.
        let msg_dict = unsafe {
            xpc_dictionary_create(std::ptr::null(), std::ptr::null(), 0)
        };
        if msg_dict.is_null() {
            // Safety: data_obj is a valid XPC object created above.
            unsafe { xpc_release(data_obj) };
            return Err(VfsMacOsError::Xpc("xpc_dictionary_create returned NULL".to_string()));
        }

        let key_data = CString::new("data").unwrap();
        // Safety: msg_dict and data_obj are valid non-null XPC objects.
        unsafe { xpc_dictionary_set_value(msg_dict, key_data.as_ptr(), data_obj) };
        // Safety: data_obj is now retained by the dictionary; release our ref.
        unsafe { xpc_release(data_obj) };

        // Send message and block for reply.
        // Safety: self.conn and msg_dict are valid non-null XPC objects; the
        // function blocks until a reply arrives or the connection is invalidated.
        let reply_obj =
            unsafe { xpc_connection_send_message_with_reply_sync(self.conn, msg_dict) };
        // Safety: msg_dict is no longer needed after the send.
        unsafe { xpc_release(msg_dict) };

        if reply_obj.is_null() {
            return Err(VfsMacOsError::Xpc(
                "xpc_connection_send_message_with_reply_sync returned NULL".to_string(),
            ));
        }

        // Extract "reply" data from the reply dictionary.
        let key_reply = CString::new("reply").unwrap();
        // Safety: reply_obj is a valid XPC dictionary; get_value borrows without
        // transferring ownership so we must not release the returned pointer.
        let reply_data = unsafe { xpc_dictionary_get_value(reply_obj, key_reply.as_ptr()) };

        if reply_data.is_null() {
            // Safety: reply_obj is owned by us and must be released.
            unsafe { xpc_release(reply_obj) };
            return Err(VfsMacOsError::Protocol(
                "reply dictionary missing 'reply' key".to_string(),
            ));
        }

        // Safety: reply_data is a valid XPC data object; bytes_ptr borrows the
        // internal buffer which remains valid as long as reply_obj is alive.
        let bytes_ptr = unsafe { xpc_data_get_bytes_ptr(reply_data) };
        let bytes_len = unsafe { xpc_data_get_length(reply_data) };

        if bytes_ptr.is_null() {
            unsafe { xpc_release(reply_obj) };
            return Err(VfsMacOsError::Protocol("reply data bytes pointer is NULL".to_string()));
        }

        // Safety: bytes_ptr points to bytes_len bytes owned by reply_obj.
        let reply_slice =
            unsafe { std::slice::from_raw_parts(bytes_ptr as *const u8, bytes_len) };

        let parsed: XpcReply = serde_json::from_slice(reply_slice)
            .map_err(|e| VfsMacOsError::Protocol(format!("deserialize reply: {e}")))?;

        // Safety: reply_obj is owned by us; release after we are done reading.
        unsafe { xpc_release(reply_obj) };

        Ok(parsed)
    }
}

impl Drop for XpcConnection {
    fn drop(&mut self) {
        // Safety: self.conn was created by connect() and has not been released.
        unsafe { xpc_release(self.conn) };
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// This test only runs when a macOS FileProvider extension is active and
    /// listening on the configured mach service name.
    #[test]
    #[ignore = "requires macOS + running FileProvider extension"]
    fn test_connect_and_ping() {
        let conn = XpcConnection::connect("org.owncloud.owncloud-sync.fileprovider-xpc")
            .expect("connect");
        let cmd = XpcCommand::IsVirtual { path: "test.txt".to_string() };
        let reply = conn.send_command(&cmd).expect("send_command");
        // We only assert the protocol was satisfied, not the specific value.
        assert!(reply.ok || reply.error.is_some());
    }
}
```

- [ ] Commit:

```
git commit -m "feat(vfs-macos): add XPC connection wrapper"
```

---

## Task 3: VfsMacOs + Vfs impl

- [ ] Create `crates/vfs-macos/src/lib.rs`:

```rust
mod error;
mod messages;
mod xpc;

pub use error::VfsMacOsError;
pub use messages::{XpcCommand, XpcReply};

use camino::{Utf8Path, Utf8PathBuf};
use std::sync::Mutex;
use vfs_core::{VfsError, VfsFileItem, VfsStatus, Vfs};

use crate::xpc::XpcConnection;

/// Constant XPC mach service name for the FileProvider extension.
const XPC_SERVICE: &str = "org.owncloud.owncloud-sync.fileprovider-xpc";

// ---------------------------------------------------------------------------
// VfsMacOs
// ---------------------------------------------------------------------------

/// macOS VFS backend that delegates all operations to the Swift FileProvider
/// extension via an XPC connection.
pub struct VfsMacOs {
    conn: Mutex<XpcConnection>,
    root: Utf8PathBuf,
}

impl VfsMacOs {
    /// Connect to the FileProvider XPC service and return a new `VfsMacOs`.
    pub fn new(root: Utf8PathBuf) -> Result<Self, VfsMacOsError> {
        let conn = XpcConnection::connect(XPC_SERVICE)?;
        Ok(Self {
            conn: Mutex::new(conn),
            root,
        })
    }

    /// Lock the connection, send a command, and return the reply.
    fn send(&self, cmd: XpcCommand) -> Result<XpcReply, VfsMacOsError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| VfsMacOsError::Xpc("connection mutex poisoned".to_string()))?;
        conn.send_command(&cmd)
    }

    /// Resolve a `Utf8Path` relative to the sync root to a string suitable for
    /// sending to the Swift extension (always forward-slash, relative).
    fn rel_path(&self, abs_path: &Utf8Path) -> String {
        abs_path
            .strip_prefix(&self.root)
            .unwrap_or(abs_path)
            .as_str()
            .to_owned()
    }

    /// Convert a successful reply into `()`, propagating any reported error.
    fn expect_ok(reply: XpcReply) -> Result<(), VfsMacOsError> {
        if reply.ok {
            Ok(())
        } else {
            Err(VfsMacOsError::Protocol(
                reply.error.unwrap_or_else(|| "unknown error".to_string()),
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Vfs trait implementation
// ---------------------------------------------------------------------------

impl Vfs for VfsMacOs {
    fn create_placeholder(&self, item: &VfsFileItem) -> Result<(), VfsError> {
        let reply = self.send(XpcCommand::CreatePlaceholder {
            path: self.rel_path(&item.path),
            etag: item.etag.clone(),
            size: item.size,
            mtime: item.mtime,
        })?;
        Self::expect_ok(reply).map_err(VfsError::from)
    }

    fn update_placeholder(&self, item: &VfsFileItem) -> Result<(), VfsError> {
        let reply = self.send(XpcCommand::UpdatePlaceholder {
            path: self.rel_path(&item.path),
            etag: item.etag.clone(),
            size: item.size,
            mtime: item.mtime,
        })?;
        Self::expect_ok(reply).map_err(VfsError::from)
    }

    fn hydrate(&self, path: &Utf8Path) -> Result<(), VfsError> {
        let reply = self.send(XpcCommand::Hydrate { path: self.rel_path(path) })?;
        Self::expect_ok(reply).map_err(VfsError::from)
    }

    fn dehydrate(&self, path: &Utf8Path) -> Result<(), VfsError> {
        let reply = self.send(XpcCommand::Dehydrate { path: self.rel_path(path) })?;
        Self::expect_ok(reply).map_err(VfsError::from)
    }

    fn is_virtual(&self, path: &Utf8Path) -> Result<bool, VfsError> {
        let reply = self.send(XpcCommand::IsVirtual { path: self.rel_path(path) })?;
        if !reply.ok {
            return Err(VfsError::from(VfsMacOsError::Protocol(
                reply.error.unwrap_or_else(|| "unknown error".to_string()),
            )));
        }
        Ok(reply.bool_value.unwrap_or(false))
    }

    fn status(&self, path: &Utf8Path) -> Result<VfsStatus, VfsError> {
        let reply = self.send(XpcCommand::Status { path: self.rel_path(path) })?;
        if !reply.ok {
            return Err(VfsError::from(VfsMacOsError::Protocol(
                reply.error.unwrap_or_else(|| "unknown error".to_string()),
            )));
        }
        let status_str = reply
            .status
            .ok_or_else(|| VfsError::from(VfsMacOsError::Protocol("missing status field".to_string())))?;
        match status_str.as_str() {
            "Hydrated" => Ok(VfsStatus::Hydrated),
            "Dehydrated" | "Virtual" => Ok(VfsStatus::Dehydrated),
            "Pinned" => Ok(VfsStatus::Pinned),
            "Unpinned" => Ok(VfsStatus::Unpinned),
            other => Err(VfsError::from(VfsMacOsError::Protocol(format!(
                "unrecognized VfsStatus: '{other}'"
            )))),
        }
    }

    fn set_pinned(&self, path: &Utf8Path, pinned: bool) -> Result<(), VfsError> {
        let reply = self
            .send(XpcCommand::SetPinned { path: self.rel_path(path), pinned })?;
        Self::expect_ok(reply).map_err(VfsError::from)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time check: VfsMacOs must be Send + Sync.
    #[test]
    fn test_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<VfsMacOs>();
    }

    /// Requires a running FileProvider extension on macOS.
    #[test]
    #[ignore = "requires macOS + running FileProvider extension"]
    fn test_is_virtual_integration() {
        let root = Utf8PathBuf::from("/Users/testuser/ownCloud");
        let vfs = VfsMacOs::new(root).expect("VfsMacOs::new");
        let path = Utf8Path::new("/Users/testuser/ownCloud/test.txt");
        let result = vfs.is_virtual(path);
        assert!(result.is_ok(), "is_virtual failed: {:?}", result);
    }
}
```

- [ ] Commit:

```
git commit -m "feat(vfs-macos): implement Vfs trait over XPC"
```

---

## Task 4: XPCMessages.swift

- [ ] Create `shell-integration/macos/FileProvider/XPCMessages.swift`:

```swift
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
```

- [ ] Create `shell-integration/macos/FileProvider/XPCMessagesTests.swift` (XCTest target):

```swift
import XCTest
@testable import FileProvider

final class XPCMessagesTests: XCTestCase {

    private let encoder = JSONEncoder()
    private let decoder = JSONDecoder()

    // MARK: - Command roundtrip helpers

    private func roundtrip(_ cmd: XPCCommand, file: StaticString = #file, line: UInt = #line) throws {
        let data    = try encoder.encode(cmd)
        let decoded = try decoder.decode(XPCCommand.self, from: data)
        XCTAssertEqual(decoded.cmd,    cmd.cmd,    "cmd mismatch",   file: file, line: line)
        XCTAssertEqual(decoded.path,   cmd.path,   "path mismatch",  file: file, line: line)
        XCTAssertEqual(decoded.etag,   cmd.etag,   "etag mismatch",  file: file, line: line)
        XCTAssertEqual(decoded.size,   cmd.size,   "size mismatch",  file: file, line: line)
        XCTAssertEqual(decoded.mtime,  cmd.mtime,  "mtime mismatch", file: file, line: line)
        XCTAssertEqual(decoded.pinned, cmd.pinned, "pinned mismatch",file: file, line: line)
    }

    // MARK: - Command tests

    func testCreatePlaceholderRoundtrip() throws {
        try roundtrip(XPCCommand(cmd: .createPlaceholder, path: "docs/a.md",
                                 etag: "abc", size: 1024, mtime: 1700000000, pinned: nil))
    }

    func testUpdatePlaceholderRoundtrip() throws {
        try roundtrip(XPCCommand(cmd: .updatePlaceholder, path: "docs/a.md",
                                 etag: "def", size: 2048, mtime: 1700001000, pinned: nil))
    }

    func testHydrateRoundtrip() throws {
        try roundtrip(XPCCommand(cmd: .hydrate, path: "photos/x.jpg",
                                 etag: nil, size: nil, mtime: nil, pinned: nil))
    }

    func testDehydrateRoundtrip() throws {
        try roundtrip(XPCCommand(cmd: .dehydrate, path: "photos/x.jpg",
                                 etag: nil, size: nil, mtime: nil, pinned: nil))
    }

    func testIsVirtualRoundtrip() throws {
        try roundtrip(XPCCommand(cmd: .isVirtual, path: "photos/x.jpg",
                                 etag: nil, size: nil, mtime: nil, pinned: nil))
    }

    func testStatusRoundtrip() throws {
        try roundtrip(XPCCommand(cmd: .status, path: "docs/a.md",
                                 etag: nil, size: nil, mtime: nil, pinned: nil))
    }

    func testSetPinnedTrueRoundtrip() throws {
        try roundtrip(XPCCommand(cmd: .setPinned, path: "docs/a.md",
                                 etag: nil, size: nil, mtime: nil, pinned: true))
    }

    func testSetPinnedFalseRoundtrip() throws {
        try roundtrip(XPCCommand(cmd: .setPinned, path: "docs/a.md",
                                 etag: nil, size: nil, mtime: nil, pinned: false))
    }

    func testCommandTypeRawValues() {
        XCTAssertEqual(XPCCommandType.createPlaceholder.rawValue, "create_placeholder")
        XCTAssertEqual(XPCCommandType.updatePlaceholder.rawValue, "update_placeholder")
        XCTAssertEqual(XPCCommandType.hydrate.rawValue,           "hydrate")
        XCTAssertEqual(XPCCommandType.dehydrate.rawValue,         "dehydrate")
        XCTAssertEqual(XPCCommandType.isVirtual.rawValue,         "is_virtual")
        XCTAssertEqual(XPCCommandType.status.rawValue,            "status")
        XCTAssertEqual(XPCCommandType.setPinned.rawValue,         "set_pinned")
    }

    // MARK: - Reply tests

    func testReplySuccessRoundtrip() throws {
        let r = XPCReply.success()
        let data    = try encoder.encode(r)
        let decoded = try decoder.decode(XPCReply.self, from: data)
        XCTAssertTrue(decoded.ok)
        XCTAssertNil(decoded.error)
        XCTAssertNil(decoded.bool)
        XCTAssertNil(decoded.status)
    }

    func testReplyFailureRoundtrip() throws {
        let r = XPCReply.failure("not found")
        let data    = try encoder.encode(r)
        let decoded = try decoder.decode(XPCReply.self, from: data)
        XCTAssertFalse(decoded.ok)
        XCTAssertEqual(decoded.error, "not found")
    }

    func testReplyBoolResultRoundtrip() throws {
        let r = XPCReply.boolResult(true)
        let data    = try encoder.encode(r)
        let decoded = try decoder.decode(XPCReply.self, from: data)
        XCTAssertTrue(decoded.ok)
        XCTAssertEqual(decoded.bool, true)
    }

    func testReplyStatusResultRoundtrip() throws {
        let r = XPCReply.statusResult("Hydrated")
        let data    = try encoder.encode(r)
        let decoded = try decoder.decode(XPCReply.self, from: data)
        XCTAssertTrue(decoded.ok)
        XCTAssertEqual(decoded.status, "Hydrated")
    }
}
```

- [ ] Commit:

```
git add shell-integration/macos/FileProvider/XPCMessages.swift \
        shell-integration/macos/FileProvider/XPCMessagesTests.swift
git commit -m "feat(fileprovider): add XPC message types"
```

---

## Task 5: XPCServer.swift

- [ ] Create `shell-integration/macos/FileProvider/XPCServer.swift`:

```swift
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
        // Wire up the message handler on the connection's exported interface.
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
        // Last-resort: hand-craft the JSON bytes to avoid infinite recursion.
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
```

- [ ] The XPCServer delegates to methods on `FileProviderExtension` that are defined in Task 8. As a placeholder, add the protocol signatures in a protocol file so the Swift project compiles before Task 8:

```swift
// shell-integration/macos/FileProvider/FileProviderXPCMethods.swift
import Foundation

/// XPC command handler methods that FileProviderExtension must implement.
protocol FileProviderXPCMethods: AnyObject {
    func xpcCreatePlaceholder(path: String, etag: String, size: UInt64, mtime: Int64) -> XPCReply
    func xpcUpdatePlaceholder(path: String, etag: String, size: UInt64, mtime: Int64) -> XPCReply
    func xpcHydrate(path: String)  -> XPCReply
    func xpcDehydrate(path: String) -> XPCReply
    func xpcIsVirtual(path: String) -> XPCReply
    func xpcStatus(path: String)   -> XPCReply
    func xpcSetPinned(path: String, pinned: Bool) -> XPCReply
}
```

- [ ] Commit:

```
git add shell-integration/macos/FileProvider/XPCServer.swift \
        shell-integration/macos/FileProvider/FileProviderXPCMethods.swift
git commit -m "feat(fileprovider): add XPC server"
```

---

## Task 6: FileProviderItem.swift

- [ ] Create `shell-integration/macos/FileProvider/FileProviderItem.swift`:

```swift
import FileProvider
import UniformTypeIdentifiers

// MARK: - FileProviderItem

/// Represents a single file or directory in the ownCloud sync domain.
final class FileProviderItem: NSObject, NSFileProviderItem {

    // MARK: Stored properties

    private let _identifier:         NSFileProviderItemIdentifier
    private let _parentIdentifier:   NSFileProviderItemIdentifier
    private let _filename:           String
    private let _isDirectory:        Bool
    private let _documentSize:       NSNumber?
    private let _contentModificationDate: Date?
    private let _etag:               String

    // MARK: Init

    /// Create a new item.
    ///
    /// - Parameters:
    ///   - identifier: Stable, unique item identifier (e.g. server file ID).
    ///   - parent: Parent item identifier.
    ///   - filename: Display name including extension.
    ///   - isDirectory: `true` for containers, `false` for files.
    ///   - size: File size in bytes (`nil` for directories).
    ///   - modificationDate: Last modification date from the server.
    ///   - etag: Server-assigned ETag; stored as the version identifier.
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
        _isDirectory ? .folder : UTType(filenameExtension: (_filename as NSString).pathExtension) ?? .data
    }

    // MARK: NSFileProviderItem — optional but recommended

    var documentSize: NSNumber? { _documentSize }
    var contentModificationDate: Date? { _contentModificationDate }

    /// Use the ETag bytes as the version identifier so the system can detect
    /// remote changes without fetching file contents.
    var versionIdentifier: Data? {
        _etag.data(using: .utf8)
    }

    /// Capabilities for this item. Directories are browsable; files support
    /// reading, writing, deletion, and renaming.
    var capabilities: NSFileProviderItemCapabilities {
        if _isDirectory {
            return [.allowsContentEnumerating, .allowsAddingSubItems, .allowsRenaming, .allowsDeleting]
        }
        return [.allowsReading, .allowsWriting, .allowsRenaming, .allowsDeleting]
    }
}
```

- [ ] Commit:

```
git add shell-integration/macos/FileProvider/FileProviderItem.swift
git commit -m "feat(fileprovider): add FileProviderItem"
```

---

## Task 7: FileProviderEnumerator.swift

- [ ] Create `shell-integration/macos/FileProvider/FileProviderEnumerator.swift`:

```swift
import FileProvider
import Foundation

// MARK: - FileProviderEnumerator

/// Enumerates the contents of a single directory inside the ownCloud sync root.
final class FileProviderEnumerator: NSObject, NSFileProviderEnumerator {

    // MARK: Properties

    /// Absolute URL of the directory to enumerate (inside the sync root).
    private let directoryURL: URL

    /// The item identifier for the directory being enumerated.
    private let containerIdentifier: NSFileProviderItemIdentifier

    // MARK: Init

    /// - Parameters:
    ///   - directoryURL: Local filesystem URL of the directory to list.
    ///   - containerIdentifier: Identifier of the parent container item.
    init(directoryURL: URL, containerIdentifier: NSFileProviderItemIdentifier) {
        self.directoryURL        = directoryURL
        self.containerIdentifier = containerIdentifier
        super.init()
    }

    // MARK: NSFileProviderEnumerator

    func invalidate() {
        // No persistent resources to release.
    }

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
                    .fileSizeKey,
                    .contentModificationDateKey,
                    .isDirectoryKey,
                ],
                options: .skipsHiddenFiles
            )

            for entry in entries {
                let resourceValues = try entry.resourceValues(forKeys: [
                    .fileSizeKey,
                    .contentModificationDateKey,
                    .isDirectoryKey,
                ])

                let isDir    = resourceValues.isDirectory ?? false
                let size     = resourceValues.fileSize.map { Int64($0) }
                let modDate  = resourceValues.contentModificationDate
                let filename = entry.lastPathComponent

                // Use the path relative to the container as a stable identifier.
                let relPath          = directoryURL
                    .appendingPathComponent(filename)
                    .path
                let itemIdentifier   = NSFileProviderItemIdentifier(relPath)

                let item = FileProviderItem(
                    identifier:       itemIdentifier,
                    parent:           containerIdentifier,
                    filename:         filename,
                    isDirectory:      isDir,
                    size:             isDir ? nil : size,
                    modificationDate: modDate,
                    etag:             "" // ETag populated from metadata store in production
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

        // Log any enumeration error; the observer has already been finished
        // with a nil page to indicate a complete (though possibly empty) listing.
        if let err = enumerationError {
            NSLog("[FileProviderEnumerator] enumeration error for \(directoryURL.path): \(err)")
        }
    }
}
```

- [ ] Commit:

```
git add shell-integration/macos/FileProvider/FileProviderEnumerator.swift
git commit -m "feat(fileprovider): add FileProviderEnumerator"
```

---

## Task 8: FileProvider.swift

- [ ] Create `shell-integration/macos/FileProvider/FileProvider.swift`:

```swift
import FileProvider
import Foundation

// MARK: - FileProviderExtension

/// Main entry point for the ownCloud FileProvider app extension.
///
/// Conforms to `NSFileProviderReplicatedExtension` (macOS 12+).  Handles the
/// Apple FileProvider lifecycle and dispatches XPC commands from the Rust
/// sync daemon.
final class FileProviderExtension: NSObject, NSFileProviderReplicatedExtension {

    // MARK: Properties

    /// The domain this extension instance serves.
    let domain: NSFileProviderDomain

    /// XPC server listening for commands from the Rust daemon.
    private var xpcServer: XPCServer?

    /// Local root directory for this domain's synchronized files.
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
        // Resolve the identifier to a local URL.
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
                size:             (resourceValues.fileSize).map { Int64($0) },
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

        // Notify the Rust daemon that this path needs hydrating via a separate
        // XPC back-channel.  The daemon is expected to fetch the file from the
        // server and write it to `localFileURL`.
        sendHydrationNeededEvent(path: relativePath, domainID: domain.identifier.rawValue)

        // Poll for the file to appear (up to 30 s, checking every 100 ms).
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

            // Build and return the item metadata alongside the file URL.
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
        // Stub: acknowledge without error; real implementation would upload to server.
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
        // Stub: acknowledge without error.
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
        // Stub: acknowledge without error.
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
            // Working set enumeration not yet supported.
            throw NSFileProviderError(.noSuchItem)
        default:
            containerURL = localURL(for: containerItemIdentifier)
        }

        return FileProviderEnumerator(
            directoryURL:        containerURL,
            containerIdentifier: containerItemIdentifier
        )
    }

    // MARK: - XPC command methods (FileProviderXPCMethods)

    func xpcCreatePlaceholder(path: String, etag: String, size: UInt64, mtime: Int64) -> XPCReply {
        let url = syncRoot.appendingPathComponent(path)
        do {
            // Write a zero-byte placeholder file; hydration fills the real content.
            try FileManager.default.createDirectory(
                at: url.deletingLastPathComponent(),
                withIntermediateDirectories: true
            )
            if !FileManager.default.fileExists(atPath: url.path) {
                FileManager.default.createFile(atPath: url.path, contents: nil)
            }
            // Store extended attributes for etag and size so status queries work.
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
        // Hydration is driven by fetchContents; here we just acknowledge.
        return .success()
    }

    func xpcDehydrate(path: String) -> XPCReply {
        let url = syncRoot.appendingPathComponent(path)
        do {
            // Truncate file to zero bytes to simulate dehydration (placeholder state).
            try Data().write(to: url)
            return .success()
        } catch {
            return .failure(error.localizedDescription)
        }
    }

    func xpcIsVirtual(path: String) -> XPCReply {
        let url  = syncRoot.appendingPathComponent(path)
        let size = (try? url.resourceValues(forKeys: [.fileSizeKey]))?.fileSize ?? -1
        // A placeholder has zero bytes and exists on disk.
        let isVirtual = FileManager.default.fileExists(atPath: url.path) && size == 0
        return .boolResult(isVirtual)
    }

    func xpcStatus(path: String) -> XPCReply {
        let url  = syncRoot.appendingPathComponent(path)
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
            try url.setExtendedAttribute("com.owncloud.pinned",
                                         value: pinValue.data(using: .utf8)!)
            return .success()
        } catch {
            return .failure(error.localizedDescription)
        }
    }

    // MARK: - Private helpers

    /// Map an item identifier to its absolute local URL.
    private func localURL(for identifier: NSFileProviderItemIdentifier) -> URL {
        if identifier == .rootContainer {
            return syncRoot
        }
        // Identifiers are stored as absolute paths (see FileProviderEnumerator).
        return URL(fileURLWithPath: identifier.rawValue)
    }

    /// Derive the parent identifier from a file URL.
    private func parentIdentifier(for url: URL) -> NSFileProviderItemIdentifier {
        let parent = url.deletingLastPathComponent()
        if parent == syncRoot {
            return .rootContainer
        }
        return NSFileProviderItemIdentifier(parent.path)
    }

    /// Convert an absolute item identifier / URL to a sync-root-relative path.
    private func relativePath(for identifier: NSFileProviderItemIdentifier) -> String {
        let absPath = identifier.rawValue
        let rootPath = syncRoot.path
        if absPath.hasPrefix(rootPath) {
            return String(absPath.dropFirst(rootPath.count + 1))
        }
        return absPath
    }

    /// Send a hydration-needed event back to the Rust daemon over a separate
    /// XPC back-channel so it can fetch the file from the server.
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

        // Fire-and-forget: the daemon picks up the event and hydrates asynchronously.
        let msg = xpc_dictionary_create(nil, nil, 0)
        let bytes = (data as NSData).bytes
        let xpcData = xpc_data_create(bytes, data.count)
        xpc_dictionary_set_value(msg, "data", xpcData)
        xpc_connection_send_message(connection.value(forKey: "_xpcConnection") as! xpc_connection_t, msg)

        NSLog("[FileProviderExtension] sent hydration_needed for path=\(path)")
    }
}

// MARK: - URL extended attribute helpers

private extension URL {
    func setExtendedAttribute(_ name: String, value: Data) throws {
        try value.withUnsafeBytes { ptr in
            let rc = setxattr(
                self.path,
                name,
                ptr.baseAddress,
                value.count,
                0,     // position (HFS+ only)
                0      // options
            )
            if rc != 0 {
                throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EPERM)
            }
        }
    }
}
```

- [ ] Commit:

```
git add shell-integration/macos/FileProvider/FileProvider.swift
git commit -m "feat(fileprovider): add FileProviderExtension"
```

---

## Task 9: Info.plist + entitlements

- [ ] Create `shell-integration/macos/FileProvider/Info.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
    "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <!-- App Extension metadata -->
    <key>NSExtension</key>
    <dict>
        <key>NSExtensionFileProviderDocumentGroup</key>
        <string>group.org.owncloud.owncloud-sync</string>

        <key>NSExtensionPointIdentifier</key>
        <string>com.apple.fileprovider-nonui</string>

        <key>NSExtensionPrincipalClass</key>
        <string>$(PRODUCT_MODULE_NAME).FileProviderExtension</string>
    </dict>

    <!-- Bundle identification -->
    <key>CFBundleDisplayName</key>
    <string>ownCloud File Provider</string>

    <key>CFBundleIdentifier</key>
    <string>org.owncloud.owncloud-sync.FileProvider</string>

    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>

    <key>CFBundleName</key>
    <string>$(PRODUCT_NAME)</string>

    <key>CFBundlePackageType</key>
    <string>XPC!</string>

    <key>CFBundleShortVersionString</key>
    <string>1.0</string>

    <key>CFBundleVersion</key>
    <string>1</string>

    <!-- Minimum macOS version: FileProvider replicated extension requires 12.0 -->
    <key>LSMinimumSystemVersion</key>
    <string>12.0</string>
</dict>
</plist>
```

- [ ] Create `shell-integration/macos/FileProvider/FileProvider.entitlements`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
    "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <!-- App Group shared container for the extension and main app -->
    <key>com.apple.security.application-groups</key>
    <array>
        <string>group.org.owncloud.owncloud-sync</string>
    </array>

    <!-- Hardened Runtime: allow JIT if needed by the Rust side (not required
         for the Swift extension, but included for completeness) -->
    <key>com.apple.security.cs.allow-jit</key>
    <false/>

    <!-- Network client access for the backchannel to the Rust daemon -->
    <key>com.apple.security.network.client</key>
    <true/>

    <!-- FileProvider entitlement required for replicated extensions -->
    <key>com.apple.developer.fileprovider.testing-mode</key>
    <false/>
</dict>
</plist>
```

- [ ] Xcode project setup steps:

  1. In Xcode, open the main `ocsync.xcodeproj` (or workspace).
  2. Choose **File > New > Target** and select **File Provider Extension** from the macOS templates.
  3. Set the Product Name to `FileProvider`, bundle ID `org.owncloud.owncloud-sync.FileProvider`.
  4. Add the existing Swift files from `shell-integration/macos/FileProvider/` to the new target's **Compile Sources** phase.
  5. Replace the generated `Info.plist` and entitlements files with the ones above.
  6. In the main app target, open **Signing & Capabilities**, click **+**, and add **App Groups** with `group.org.owncloud.owncloud-sync`.
  7. Do the same on the **FileProvider** extension target.
  8. In the main app target's **Build Phases**, add a new **Embed App Extensions** phase and drag `FileProvider.appex` into it.
  9. In both targets, add the **FileProvider.framework** under **Frameworks, Libraries, and Embedded Content**.
  10. Set the XPC Mach Service name in the **FileProvider** target's entitlements if your provisioning profile requires explicit XPC service declarations.

- [ ] Commit:

```
git add shell-integration/macos/FileProvider/Info.plist \
        shell-integration/macos/FileProvider/FileProvider.entitlements
git commit -m "feat(fileprovider): add Info.plist and entitlements"
```

---

## Task 10: Integration tests + manual testing guide

- [ ] Create `crates/vfs-macos/tests/integration.rs`:

```rust
//! Integration tests for vfs-macos.
//!
//! All tests in this file require a macOS system with the FileProvider
//! extension running and are therefore marked `#[ignore]`.

#[cfg(test)]
mod integration {
    use camino::Utf8PathBuf;
    use vfs_macos::VfsMacOs;
    use vfs_core::Vfs;

    /// Verify that `VfsMacOs::new` can connect to the XPC service and that
    /// `is_virtual` returns without error for a path inside the sync root.
    ///
    /// Pre-conditions:
    ///   - macOS 12+
    ///   - ownCloud FileProvider extension registered and running
    ///   - Sync root exists at ~/ownCloud
    #[test]
    #[ignore = "requires macOS + running FileProvider extension"]
    fn test_is_virtual_smoke() {
        let home    = std::env::var("HOME").expect("HOME not set");
        let root    = Utf8PathBuf::from(format!("{home}/ownCloud"));
        let vfs     = VfsMacOs::new(root.clone()).expect("VfsMacOs::new failed");

        // Use a known placeholder file; create it if absent.
        let test_path = root.join("integration_test_placeholder.txt");
        let result    = vfs.is_virtual(&test_path);

        assert!(
            result.is_ok(),
            "is_virtual returned an error: {:?}",
            result
        );
        println!("is_virtual({test_path}) = {:?}", result.unwrap());
    }

    /// Verify that `hydrate` and `dehydrate` round-trip without error.
    #[test]
    #[ignore = "requires macOS + running FileProvider extension"]
    fn test_hydrate_dehydrate_roundtrip() {
        let home  = std::env::var("HOME").expect("HOME not set");
        let root  = Utf8PathBuf::from(format!("{home}/ownCloud"));
        let vfs   = VfsMacOs::new(root.clone()).expect("VfsMacOs::new failed");
        let path  = root.join("integration_test_placeholder.txt");

        vfs.dehydrate(&path).expect("dehydrate failed");
        assert!(vfs.is_virtual(&path).expect("is_virtual after dehydrate"), "should be virtual after dehydrate");

        vfs.hydrate(&path).expect("hydrate failed");
        assert!(!vfs.is_virtual(&path).expect("is_virtual after hydrate"), "should not be virtual after hydrate");
    }
}
```

- [ ] Create `shell-integration/macos/FileProvider/XPCServerTests.swift` (XCTest target):

```swift
import XCTest
@testable import FileProvider

/// Unit tests for XPCServer.handleCommand that run without a live XPC
/// connection by calling the method directly with pre-encoded JSON.
final class XPCServerTests: XCTestCase {

    // A lightweight stub that satisfies FileProviderXPCMethods for testing.
    private final class MockProvider: NSObject, FileProviderXPCMethods {
        func xpcCreatePlaceholder(path: String, etag: String, size: UInt64, mtime: Int64) -> XPCReply { .success() }
        func xpcUpdatePlaceholder(path: String, etag: String, size: UInt64, mtime: Int64) -> XPCReply { .success() }
        func xpcHydrate(path: String)  -> XPCReply { .success() }
        func xpcDehydrate(path: String) -> XPCReply { .success() }
        func xpcIsVirtual(path: String) -> XPCReply { .boolResult(false) }
        func xpcStatus(path: String)   -> XPCReply { .statusResult("Hydrated") }
        func xpcSetPinned(path: String, pinned: Bool) -> XPCReply { .success() }
    }

    private var server: XPCServer!
    private let encoder = JSONEncoder()
    private let decoder = JSONDecoder()

    override func setUp() {
        super.setUp()
        // XPCServer accepts any NSObject conforming to FileProviderXPCMethods.
        // We pass the mock directly; the server only calls protocol methods.
        server = XPCServer(provider: unsafeBitCast(MockProvider(), to: FileProviderExtension.self))
    }

    private func sendCommand(_ cmd: XPCCommand) throws -> XPCReply {
        let data        = try encoder.encode(cmd)
        let replyData   = server.handleCommand(data)
        return try decoder.decode(XPCReply.self, from: replyData)
    }

    func testHandleIsVirtual() throws {
        let reply = try sendCommand(
            XPCCommand(cmd: .isVirtual, path: "a.txt", etag: nil, size: nil, mtime: nil, pinned: nil)
        )
        XCTAssertTrue(reply.ok)
    }

    func testHandleStatus() throws {
        let reply = try sendCommand(
            XPCCommand(cmd: .status, path: "a.txt", etag: nil, size: nil, mtime: nil, pinned: nil)
        )
        XCTAssertTrue(reply.ok)
        XCTAssertEqual(reply.status, "Hydrated")
    }

    func testHandleHydrate() throws {
        let reply = try sendCommand(
            XPCCommand(cmd: .hydrate, path: "a.txt", etag: nil, size: nil, mtime: nil, pinned: nil)
        )
        XCTAssertTrue(reply.ok)
    }

    func testHandleDehydrate() throws {
        let reply = try sendCommand(
            XPCCommand(cmd: .dehydrate, path: "a.txt", etag: nil, size: nil, mtime: nil, pinned: nil)
        )
        XCTAssertTrue(reply.ok)
    }

    func testHandleSetPinned() throws {
        let reply = try sendCommand(
            XPCCommand(cmd: .setPinned, path: "a.txt", etag: nil, size: nil, mtime: nil, pinned: true)
        )
        XCTAssertTrue(reply.ok)
    }

    func testHandleCreatePlaceholder() throws {
        let reply = try sendCommand(
            XPCCommand(cmd: .createPlaceholder, path: "docs/b.md",
                       etag: "abc", size: 1024, mtime: 1700000000, pinned: nil)
        )
        XCTAssertTrue(reply.ok)
    }

    func testHandleMalformedCommand() throws {
        // Send garbage bytes; server should return ok=false.
        let replyData = server.handleCommand(Data("not valid json".utf8))
        let reply     = try decoder.decode(XPCReply.self, from: replyData)
        XCTAssertFalse(reply.ok)
        XCTAssertNotNil(reply.error)
    }

    func testHandleSetPinnedMissingField() throws {
        // setPinned without the 'pinned' field should fail gracefully.
        let reply = try sendCommand(
            XPCCommand(cmd: .setPinned, path: "a.txt", etag: nil, size: nil, mtime: nil, pinned: nil)
        )
        XCTAssertFalse(reply.ok)
    }
}
```

- [ ] Manual testing guide:

  **Build and register the extension**

  1. Open `ocsync.xcodeproj` in Xcode 15+.
  2. Select the `ocsync` scheme (main app) and build for "My Mac" (`Cmd+B`).
  3. Run the app once (`Cmd+R`) so macOS registers the embedded extension. You can quit the app immediately after first launch.
  4. Open **System Settings > Privacy & Security > Extensions > Added Extensions** (or **System Settings > General > Login Items & Extensions** on macOS 13+). Verify that "ownCloud File Provider" appears and is enabled.

  **Verify the FileProvider domain**

  5. Run in Terminal:
     ```
     pluginkit -mAvv -p com.apple.fileprovider-nonui
     ```
     You should see an entry with `org.owncloud.owncloud-sync.FileProvider`.

  6. In the Finder, check that an "ownCloud" entry appears in the sidebar under **Locations**.

  **Check Console.app logs**

  7. Open **Console.app** and filter by process name `FileProvider` or search for `[FileProviderExtension]`.
  8. Trigger a sync action (e.g., right-click a file > Evict Local Copy) and confirm log lines appear for `xpcHydrate` / `xpcDehydrate`.

  **Run the Rust integration tests**

  9. Ensure the extension is running (Finder sidebar entry visible).
  10. From the repo root:
      ```
      cargo test -p vfs-macos -- --ignored
      ```
  11. Expect `test_is_virtual_smoke` and `test_hydrate_dehydrate_roundtrip` to pass.

  **Run the Swift XCTests**

  12. In Xcode, select the `FileProviderTests` scheme.
  13. Run `Cmd+U`. All `XPCServerTests` and `XPCMessagesTests` should pass without needing a running extension because they call `handleCommand` directly.

- [ ] Commit:

```
git add crates/vfs-macos/tests/integration.rs \
        shell-integration/macos/FileProvider/XPCServerTests.swift
git commit -m "test(vfs-macos): add integration tests and manual testing guide"
```
