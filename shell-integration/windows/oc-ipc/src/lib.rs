//! oc-ipc: Named pipe client for the ownCloud sync daemon socket API.
//!
//! Connects to \\.\pipe\ownCloud-{USERNAME} and exchanges line-oriented
//! text commands with the ocsyncd process.

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("failed to connect to named pipe: {0}")]
    Connect(String),

    #[error("failed to write to named pipe: {0}")]
    Write(String),

    #[error("failed to read from named pipe: {0}")]
    Read(String),

    #[error("daemon response was not valid UTF-8 or had unexpected format")]
    InvalidResponse,

    #[error("USERNAME environment variable not set")]
    NoUsername,
}

#[cfg(windows)]
mod win_impl {
    use super::IpcError;
    use windows::Win32::Foundation::{
        CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, ReadFile, FILE_FLAG_OVERLAPPED, FILE_SHARE_NONE, OPEN_EXISTING,
    };
    use windows::Win32::System::IO::WriteFile;
    use windows::Win32::System::Pipes::{SetNamedPipeHandleState, PIPE_READMODE_MESSAGE};
    use windows::core::PCWSTR;

    /// A synchronous named pipe connection to the ocsyncd daemon.
    pub struct PipeConnection {
        handle: HANDLE,
    }

    // SAFETY: HANDLE is a raw pointer, but we guarantee exclusive ownership.
    unsafe impl Send for PipeConnection {}

    impl PipeConnection {
        pub fn connect() -> Result<Self, IpcError> {
            let username = std::env::var("USERNAME").map_err(|_| IpcError::NoUsername)?;
            let pipe_name = format!(r"\\.\pipe\ownCloud-{}", username);
            let pipe_name_wide: Vec<u16> =
                pipe_name.encode_utf16().chain(std::iter::once(0u16)).collect();

            // SAFETY: `pipe_name_wide` is a valid null-terminated wide string.
            let handle = unsafe {
                CreateFileW(
                    PCWSTR(pipe_name_wide.as_ptr()),
                    GENERIC_READ.0 | GENERIC_WRITE.0,
                    FILE_SHARE_NONE,
                    None,
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED,
                    None,
                )
                .map_err(|e| IpcError::Connect(e.to_string()))?
            };

            if handle == INVALID_HANDLE_VALUE {
                return Err(IpcError::Connect(
                    windows::core::Error::from_win32().to_string(),
                ));
            }

            let mut mode = PIPE_READMODE_MESSAGE;
            // SAFETY: `handle` is valid; `mode` is a local u32.
            unsafe {
                SetNamedPipeHandleState(handle, Some(&mut mode), None, None)
                    .map_err(|e| IpcError::Connect(e.to_string()))?;
            }

            Ok(PipeConnection { handle })
        }

        pub fn send_command(&mut self, cmd: &str) -> Result<String, IpcError> {
            let mut payload = cmd.as_bytes().to_vec();
            payload.push(b'\n');
            let mut bytes_written: u32 = 0;

            // SAFETY: `self.handle` is valid for the lifetime of `self`.
            unsafe {
                WriteFile(
                    self.handle,
                    Some(&payload),
                    Some(&mut bytes_written),
                    None,
                )
                .map_err(|e| IpcError::Write(e.to_string()))?;
            }

            let mut buf = [0u8; 4096];
            let mut bytes_read: u32 = 0;
            // SAFETY: `self.handle` is valid; `buf` is a 4096-byte local array.
            unsafe {
                ReadFile(
                    self.handle,
                    Some(&mut buf),
                    Some(&mut bytes_read),
                    None,
                )
                .map_err(|e| IpcError::Read(e.to_string()))?;
            }

            let raw = &buf[..bytes_read as usize];
            let line = std::str::from_utf8(raw)
                .map_err(|_| IpcError::InvalidResponse)?
                .trim_end_matches(['\n', '\r'])
                .to_owned();

            Ok(line)
        }
    }

    impl Drop for PipeConnection {
        fn drop(&mut self) {
            // SAFETY: `self.handle` was opened by `connect()` and is still valid.
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

#[cfg(windows)]
pub use win_impl::PipeConnection;

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(windows)]
    #[ignore = "requires Windows + running ocsyncd daemon"]
    fn test_connect_and_version() {
        use super::PipeConnection;
        let mut conn = PipeConnection::connect().expect("should connect");
        let response = conn.send_command("VERSION").expect("VERSION response");
        assert!(response.contains("VERSION:"), "got: {:?}", response);
    }

    #[test]
    #[cfg(windows)]
    #[ignore = "requires Windows + running ocsyncd daemon"]
    fn test_retrieve_file_status_none_for_unknown_path() {
        use super::PipeConnection;
        let mut conn = PipeConnection::connect().expect("connect");
        let response = conn
            .send_command(r"RETRIEVE_FILE_STATUS:C:\does-not-exist\file.txt")
            .expect("send_command");
        assert!(response.starts_with("STATUS:"), "got: {:?}", response);
    }
}
