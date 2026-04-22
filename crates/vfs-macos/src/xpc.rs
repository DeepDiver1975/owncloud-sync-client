//! Low-level XPC connection wrapper.
//!
//! All `unsafe` blocks carry a one-line safety comment explaining the invariant.

#[cfg(target_os = "macos")]
mod imp {
    use libc::c_void;
    use std::ffi::CString;

    use crate::error::VfsMacOsError;
    use crate::messages::{XpcCommand, XpcReply};

    // ── XPC opaque types and extern declarations ──────────────────────────────

    type XpcObject = *mut c_void;

    #[link(name = "System", kind = "framework")]
    extern "C" {
        fn xpc_connection_create_mach_service(
            name: *const libc::c_char,
            targetq: *mut c_void,
            flags: u64,
        ) -> XpcObject;

        fn xpc_connection_resume(connection: XpcObject);

        fn xpc_connection_send_message_with_reply_sync(
            connection: XpcObject,
            message: XpcObject,
        ) -> XpcObject;

        fn xpc_dictionary_create(
            keys: *const *const libc::c_char,
            values: *const XpcObject,
            count: libc::size_t,
        ) -> XpcObject;

        fn xpc_dictionary_set_value(
            dict: XpcObject,
            key: *const libc::c_char,
            value: XpcObject,
        );

        fn xpc_dictionary_get_value(
            dict: XpcObject,
            key: *const libc::c_char,
        ) -> XpcObject;

        fn xpc_data_create(bytes: *const c_void, length: libc::size_t) -> XpcObject;
        fn xpc_data_get_bytes_ptr(data: XpcObject) -> *const c_void;
        fn xpc_data_get_length(data: XpcObject) -> libc::size_t;
        fn xpc_release(object: XpcObject);
    }

    // ── XpcConnection ─────────────────────────────────────────────────────────

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
            let conn = unsafe {
                xpc_connection_create_mach_service(name.as_ptr(), std::ptr::null_mut(), 0)
            };

            if conn.is_null() {
                return Err(VfsMacOsError::Xpc(format!(
                    "xpc_connection_create_mach_service returned NULL for service '{service}'"
                )));
            }

            // Safety: conn is a valid non-null XPC connection created above.
            unsafe { xpc_connection_resume(conn) };

            Ok(Self { conn })
        }

        /// Serialize `cmd` to JSON, send it over XPC, and deserialize the reply.
        pub fn send_command(&self, cmd: &XpcCommand) -> Result<XpcReply, VfsMacOsError> {
            let json_bytes = serde_json::to_vec(cmd)
                .map_err(|e| VfsMacOsError::Protocol(format!("serialize command: {e}")))?;

            // Safety: json_bytes is a valid slice; length matches the pointer.
            let data_obj = unsafe {
                xpc_data_create(json_bytes.as_ptr() as *const c_void, json_bytes.len())
            };
            if data_obj.is_null() {
                return Err(VfsMacOsError::Xpc(
                    "xpc_data_create returned NULL".to_string(),
                ));
            }

            // Safety: xpc_dictionary_create with count=0 produces an empty mutable dict.
            let msg_dict =
                unsafe { xpc_dictionary_create(std::ptr::null(), std::ptr::null(), 0) };
            if msg_dict.is_null() {
                // Safety: data_obj is a valid XPC object created above.
                unsafe { xpc_release(data_obj) };
                return Err(VfsMacOsError::Xpc(
                    "xpc_dictionary_create returned NULL".to_string(),
                ));
            }

            let key_data = CString::new("data").unwrap();
            // Safety: msg_dict and data_obj are valid non-null XPC objects.
            unsafe { xpc_dictionary_set_value(msg_dict, key_data.as_ptr(), data_obj) };
            // Safety: data_obj is now retained by the dictionary; release our ref.
            unsafe { xpc_release(data_obj) };

            // Safety: self.conn and msg_dict are valid non-null XPC objects.
            let reply_obj = unsafe {
                xpc_connection_send_message_with_reply_sync(self.conn, msg_dict)
            };
            // Safety: msg_dict is no longer needed after the send.
            unsafe { xpc_release(msg_dict) };

            if reply_obj.is_null() {
                return Err(VfsMacOsError::Xpc(
                    "xpc_connection_send_message_with_reply_sync returned NULL".to_string(),
                ));
            }

            let key_reply = CString::new("reply").unwrap();
            // Safety: reply_obj is a valid XPC dict; get_value borrows without ownership.
            let reply_data =
                unsafe { xpc_dictionary_get_value(reply_obj, key_reply.as_ptr()) };

            if reply_data.is_null() {
                // Safety: reply_obj is owned by us and must be released.
                unsafe { xpc_release(reply_obj) };
                return Err(VfsMacOsError::Protocol(
                    "reply dictionary missing 'reply' key".to_string(),
                ));
            }

            // Safety: reply_data is a valid XPC data object owned by reply_obj.
            let bytes_ptr = unsafe { xpc_data_get_bytes_ptr(reply_data) };
            let bytes_len = unsafe { xpc_data_get_length(reply_data) };

            if bytes_ptr.is_null() {
                unsafe { xpc_release(reply_obj) };
                return Err(VfsMacOsError::Protocol(
                    "reply data bytes pointer is NULL".to_string(),
                ));
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

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        #[ignore = "requires macOS + running FileProvider extension"]
        fn test_connect_and_ping() {
            let conn =
                XpcConnection::connect("org.owncloud.owncloud-sync.fileprovider-xpc")
                    .expect("connect");
            let cmd = XpcCommand::IsVirtual {
                path: "test.txt".to_string(),
            };
            let reply = conn.send_command(&cmd).expect("send_command");
            assert!(reply.ok || reply.error.is_some());
        }
    }
}

#[cfg(target_os = "macos")]
pub use imp::XpcConnection;
