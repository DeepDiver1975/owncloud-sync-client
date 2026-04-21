# Plan 5: Socket API Server

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the `socket-api` crate — the IPC server that shell extensions (Windows Explorer, macOS Finder, Linux Nautilus/Dolphin) connect to for file status queries and sync actions.

**Architecture:** Per-platform transport (named pipe on Windows, Unix socket on macOS/Linux). Text-based wire protocol: newline-delimited commands, `\x1e` field separator. Server holds `Arc<RwLock<SyncState>>` read-only. Commands dispatched to typed handlers. Broadcasts sent to all relevant connections on sync state changes.

**Tech Stack:** Rust 2021, tokio (async), camino, thiserror. Depends on sync-engine (Plan 2) for SyncState and FileStatus types.

---

## Task 1: Cargo.toml + error.rs

- [ ] Add `crates/socket-api` to the workspace members in the root `Cargo.toml`:

```toml
members = [
    "crates/sync-db",
    "crates/ocis-client",
    "crates/vfs-core",
    "crates/vfs-off",
    "crates/sync-engine",
    "crates/socket-api",
]
```

- [ ] Create directories:

```bash
mkdir -p crates/socket-api/src/transport
mkdir -p crates/socket-api/src/commands
mkdir -p crates/socket-api/tests
```

- [ ] Write a failing test first. Create `crates/socket-api/tests/error_variants.rs`:

```rust
use socket_api::error::SocketApiError;

#[test]
fn all_variants_exist() {
    let _: SocketApiError = SocketApiError::Io(std::io::Error::new(
        std::io::ErrorKind::Other,
        "io error",
    ));
    let _: SocketApiError = SocketApiError::Transport("bad transport".into());
    let _: SocketApiError = SocketApiError::Protocol("bad protocol".into());
    let _: SocketApiError = SocketApiError::Vfs(vfs_core::VfsError::NotSupported);
}

#[test]
fn socket_api_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SocketApiError>();
}

#[test]
fn error_display_is_informative() {
    let e = SocketApiError::Protocol("unexpected EOF".into());
    assert!(e.to_string().contains("unexpected EOF"));
}
```

- [ ] Run (expect failure — crate missing):

```bash
cargo test -p socket-api --test error_variants 2>&1 | head -20
# Expected: error[E0432]: unresolved import `socket_api`
```

- [ ] Create `crates/socket-api/Cargo.toml`:

```toml
[package]
name = "socket-api"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true

[dependencies]
tokio = { workspace = true, features = ["full"] }
camino = { workspace = true }
thiserror = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
uuid = { workspace = true }
tracing = "0.1"
vfs-core = { path = "../vfs-core" }
sync-engine = { path = "../sync-engine" }

[target.'cfg(windows)'.dependencies]
# tokio named pipe support is included in tokio "full" features

[dev-dependencies]
tokio = { workspace = true, features = ["macros", "rt-multi-thread"] }
tempfile = { workspace = true }
```

- [ ] Create `crates/socket-api/src/lib.rs` (minimal, grows over tasks):

```rust
pub mod commands;
pub mod error;
pub mod protocol;
pub mod broadcast;
pub mod server;
pub mod status_resolver;
pub mod transport;
```

- [ ] Create `crates/socket-api/src/error.rs`:

```rust
//! Error types for the socket-api crate.

use thiserror::Error;

/// All errors that can occur inside the socket API server.
#[derive(Debug, Error)]
pub enum SocketApiError {
    /// An I/O error from the OS or tokio.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A transport-layer error (e.g. named pipe or Unix socket failure).
    #[error("Transport error: {0}")]
    Transport(String),

    /// A protocol-level error (malformed command, unexpected field count, …).
    #[error("Protocol error: {0}")]
    Protocol(String),

    /// A VFS operation forwarded by a command handler failed.
    #[error("VFS error: {0}")]
    Vfs(#[from] vfs_core::VfsError),
}

/// Convenience alias.
pub type Result<T, E = SocketApiError> = std::result::Result<T, E>;
```

- [ ] Add stub modules so the crate compiles. Create each file with a single `// TODO` comment:

`crates/socket-api/src/protocol.rs` — `// TODO`  
`crates/socket-api/src/broadcast.rs` — `// TODO`  
`crates/socket-api/src/server.rs` — `// TODO`  
`crates/socket-api/src/status_resolver.rs` — `// TODO`  
`crates/socket-api/src/transport/mod.rs` — `// TODO`  
`crates/socket-api/src/commands/mod.rs` — `// TODO`  

- [ ] Run (expect pass):

```bash
cargo test -p socket-api --test error_variants 2>&1
# Expected:
# test all_variants_exist ... ok
# test socket_api_error_is_send_sync ... ok
# test error_display_is_informative ... ok
```

- [ ] Commit:

```bash
git add crates/socket-api/ Cargo.toml
git commit -m "feat(socket-api): scaffold crate, SocketApiError with all variants"
```

---

## Task 2: protocol.rs — command parsing

- [ ] Write the failing test first. Create `crates/socket-api/tests/protocol_tests.rs`:

```rust
use socket_api::protocol::{parse_command, format_response, Command};
use socket_api::error::SocketApiError;

// ── parse_command: happy paths ────────────────────────────────────────────────

#[test]
fn parse_version() {
    match parse_command("VERSION").unwrap() {
        Command::Version => {}
        other => panic!("expected Version, got {other:?}"),
    }
}

#[test]
fn parse_get_strings() {
    match parse_command("GET_STRINGS").unwrap() {
        Command::GetStrings => {}
        other => panic!("expected GetStrings, got {other:?}"),
    }
}

#[test]
fn parse_get_menu_items() {
    match parse_command("GET_MENU_ITEMS:/sync/root/file.txt").unwrap() {
        Command::GetMenuItems { path } => assert_eq!(path, "/sync/root/file.txt"),
        other => panic!("expected GetMenuItems, got {other:?}"),
    }
}

#[test]
fn parse_retrieve_file_status() {
    match parse_command("RETRIEVE_FILE_STATUS:/home/user/docs/a.pdf").unwrap() {
        Command::RetrieveFileStatus { path } => {
            assert_eq!(path, "/home/user/docs/a.pdf")
        }
        other => panic!("expected RetrieveFileStatus, got {other:?}"),
    }
}

#[test]
fn parse_retrieve_folder_status() {
    match parse_command("RETRIEVE_FOLDER_STATUS:/home/user/docs").unwrap() {
        Command::RetrieveFolderStatus { path } => {
            assert_eq!(path, "/home/user/docs")
        }
        other => panic!("expected RetrieveFolderStatus, got {other:?}"),
    }
}

#[test]
fn parse_share() {
    match parse_command("SHARE:/tmp/foo.txt").unwrap() {
        Command::Share { path } => assert_eq!(path, "/tmp/foo.txt"),
        other => panic!("expected Share, got {other:?}"),
    }
}

#[test]
fn parse_make_available_locally_single() {
    match parse_command("MAKE_AVAILABLE_LOCALLY:/a/b.txt").unwrap() {
        Command::MakeAvailableLocally { paths } => {
            assert_eq!(paths, vec!["/a/b.txt"]);
        }
        other => panic!("expected MakeAvailableLocally, got {other:?}"),
    }
}

#[test]
fn parse_make_available_locally_multiple() {
    // Multiple paths separated by \x1e
    let line = "MAKE_AVAILABLE_LOCALLY:/a/b.txt\x1e/c/d.txt\x1e/e/f.txt";
    match parse_command(line).unwrap() {
        Command::MakeAvailableLocally { paths } => {
            assert_eq!(paths, vec!["/a/b.txt", "/c/d.txt", "/e/f.txt"]);
        }
        other => panic!("expected MakeAvailableLocally, got {other:?}"),
    }
}

#[test]
fn parse_make_online_only_multiple() {
    let line = "MAKE_ONLINE_ONLY:/x/y.txt\x1e/z.txt";
    match parse_command(line).unwrap() {
        Command::MakeOnlineOnly { paths } => {
            assert_eq!(paths, vec!["/x/y.txt", "/z.txt"]);
        }
        other => panic!("expected MakeOnlineOnly, got {other:?}"),
    }
}

#[test]
fn parse_copy_private_link() {
    match parse_command("COPY_PRIVATE_LINK:/share/me.txt").unwrap() {
        Command::CopyPrivateLink { path } => {
            assert_eq!(path, "/share/me.txt")
        }
        other => panic!("expected CopyPrivateLink, got {other:?}"),
    }
}

#[test]
fn parse_v2_command() {
    match parse_command("V2/GET_CLIENT_ICON").unwrap() {
        Command::V2 { name, body } => {
            assert_eq!(name, "GET_CLIENT_ICON");
            assert_eq!(body, "");
        }
        other => panic!("expected V2, got {other:?}"),
    }
}

// ── parse_command: error cases ────────────────────────────────────────────────

#[test]
fn parse_empty_line_is_error() {
    let result = parse_command("");
    assert!(
        matches!(result, Err(SocketApiError::Protocol(_))),
        "empty line should be a protocol error"
    );
}

#[test]
fn parse_unknown_command_is_error() {
    let result = parse_command("TOTALLY_UNKNOWN_CMD:arg");
    assert!(
        matches!(result, Err(SocketApiError::Protocol(_))),
        "unknown command should be a protocol error"
    );
}

#[test]
fn parse_get_menu_items_missing_path_is_error() {
    // GET_MENU_ITEMS requires a colon and a non-empty path
    let result = parse_command("GET_MENU_ITEMS:");
    assert!(
        matches!(result, Err(SocketApiError::Protocol(_))),
        "GET_MENU_ITEMS with empty path should fail"
    );
}

// ── format_response ───────────────────────────────────────────────────────────

#[test]
fn format_response_single_part() {
    let resp = format_response("VERSION", &["1.1"]);
    assert_eq!(resp, "VERSION:1.1\n");
}

#[test]
fn format_response_multiple_parts_uses_field_sep() {
    let resp = format_response("STATUS", &["OK", "/home/user/file.txt"]);
    assert_eq!(resp, "STATUS:OK:/home/user/file.txt\n");
}

#[test]
fn format_response_no_parts() {
    let resp = format_response("PING", &[]);
    assert_eq!(resp, "PING\n");
}

#[test]
fn format_response_get_strings_uses_field_sep() {
    let resp = format_response(
        "GET_STRINGS",
        &["SHARE_MENU_TITLE", "Share", "OPEN_PRIVATE_LINK", "Open in browser"],
    );
    assert_eq!(
        resp,
        "GET_STRINGS:SHARE_MENU_TITLE:Share:OPEN_PRIVATE_LINK:Open in browser\n"
    );
}

#[test]
fn format_response_ends_with_newline() {
    let resp = format_response("CMD", &["a", "b"]);
    assert!(resp.ends_with('\n'), "response must end with newline");
}
```

- [ ] Run (expect failure):

```bash
cargo test -p socket-api --test protocol_tests 2>&1 | head -20
# Expected: error[E0432]: unresolved imports
```

- [ ] Replace `crates/socket-api/src/protocol.rs` with:

```rust
//! Wire protocol parsing and formatting for the ownCloud socket API.
//!
//! **Wire format**
//! - Framing: newline-delimited (`\n`)
//! - Field separator within a message: `\x1e` (ASCII record separator 30)
//! - Client→server: `COMMAND:argument\n`  (or `COMMAND\n` for no-arg commands)
//! - Server→client: `COMMAND:result:path\n`
//! - V2 commands: `V2/COMMAND_NAME\n` (body JSON on next line, handled by server)

use crate::error::{Result, SocketApiError};

/// Field separator character used within socket API messages.
pub const FIELD_SEP: char = '\x1e';

/// A parsed client-to-server command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `VERSION` — request the server protocol version.
    Version,
    /// `GET_STRINGS` — request localised UI strings.
    GetStrings,
    /// `GET_MENU_ITEMS:path` — request context menu items for `path`.
    GetMenuItems { path: String },
    /// `RETRIEVE_FILE_STATUS:path` — query sync status of a file.
    RetrieveFileStatus { path: String },
    /// `RETRIEVE_FOLDER_STATUS:path` — query sync status of a folder.
    RetrieveFolderStatus { path: String },
    /// `SHARE:path` — open the share dialog for `path`.
    Share { path: String },
    /// `MAKE_AVAILABLE_LOCALLY:p1\x1ep2…` — hydrate one or more paths.
    MakeAvailableLocally { paths: Vec<String> },
    /// `MAKE_ONLINE_ONLY:p1\x1ep2…` — dehydrate one or more paths.
    MakeOnlineOnly { paths: Vec<String> },
    /// `COPY_PRIVATE_LINK:path` — copy a private link to the clipboard.
    CopyPrivateLink { path: String },
    /// `V2/COMMAND_NAME` — a V2 JSON command; `body` carries the raw JSON.
    V2 { name: String, body: String },
}

/// Parse a single line (without the trailing `\n`) into a [`Command`].
///
/// Returns [`SocketApiError::Protocol`] for any malformed or unrecognised input.
pub fn parse_command(line: &str) -> Result<Command> {
    if line.is_empty() {
        return Err(SocketApiError::Protocol("empty command line".into()));
    }

    // V2 commands start with "V2/"
    if let Some(rest) = line.strip_prefix("V2/") {
        return Ok(Command::V2 {
            name: rest.to_string(),
            body: String::new(),
        });
    }

    // Split on the first ':' to separate the command name from its arguments.
    let (cmd, args) = match line.split_once(':') {
        Some((c, a)) => (c, a),
        None => (line, ""),
    };

    match cmd {
        "VERSION" => Ok(Command::Version),
        "GET_STRINGS" => Ok(Command::GetStrings),

        "GET_MENU_ITEMS" => {
            if args.is_empty() {
                return Err(SocketApiError::Protocol(
                    "GET_MENU_ITEMS requires a non-empty path argument".into(),
                ));
            }
            Ok(Command::GetMenuItems {
                path: args.to_string(),
            })
        }

        "RETRIEVE_FILE_STATUS" => {
            if args.is_empty() {
                return Err(SocketApiError::Protocol(
                    "RETRIEVE_FILE_STATUS requires a path argument".into(),
                ));
            }
            Ok(Command::RetrieveFileStatus {
                path: args.to_string(),
            })
        }

        "RETRIEVE_FOLDER_STATUS" => {
            if args.is_empty() {
                return Err(SocketApiError::Protocol(
                    "RETRIEVE_FOLDER_STATUS requires a path argument".into(),
                ));
            }
            Ok(Command::RetrieveFolderStatus {
                path: args.to_string(),
            })
        }

        "SHARE" => {
            if args.is_empty() {
                return Err(SocketApiError::Protocol(
                    "SHARE requires a path argument".into(),
                ));
            }
            Ok(Command::Share {
                path: args.to_string(),
            })
        }

        "MAKE_AVAILABLE_LOCALLY" => {
            if args.is_empty() {
                return Err(SocketApiError::Protocol(
                    "MAKE_AVAILABLE_LOCALLY requires at least one path argument".into(),
                ));
            }
            let paths = args
                .split(FIELD_SEP)
                .map(str::to_string)
                .collect();
            Ok(Command::MakeAvailableLocally { paths })
        }

        "MAKE_ONLINE_ONLY" => {
            if args.is_empty() {
                return Err(SocketApiError::Protocol(
                    "MAKE_ONLINE_ONLY requires at least one path argument".into(),
                ));
            }
            let paths = args
                .split(FIELD_SEP)
                .map(str::to_string)
                .collect();
            Ok(Command::MakeOnlineOnly { paths })
        }

        "COPY_PRIVATE_LINK" => {
            if args.is_empty() {
                return Err(SocketApiError::Protocol(
                    "COPY_PRIVATE_LINK requires a path argument".into(),
                ));
            }
            Ok(Command::CopyPrivateLink {
                path: args.to_string(),
            })
        }

        other => Err(SocketApiError::Protocol(format!(
            "unknown command: {other:?}"
        ))),
    }
}

/// Format a server-to-client response.
///
/// Parts are joined with `:` (the plain colon is the conventional field
/// separator in the non-V2 response format).  A trailing `\n` is appended.
///
/// ```
/// use socket_api::protocol::format_response;
/// assert_eq!(format_response("VERSION", &["1.1"]), "VERSION:1.1\n");
/// assert_eq!(format_response("STATUS", &["OK", "/foo"]), "STATUS:OK:/foo\n");
/// assert_eq!(format_response("PING", &[]), "PING\n");
/// ```
pub fn format_response(cmd: &str, parts: &[&str]) -> String {
    if parts.is_empty() {
        format!("{cmd}\n")
    } else {
        format!("{cmd}:{}\n", parts.join(":"))
    }
}
```

- [ ] Run (expect pass):

```bash
cargo test -p socket-api --test protocol_tests 2>&1
# Expected: all tests pass
```

- [ ] Commit:

```bash
git add crates/socket-api/src/protocol.rs
git commit -m "feat(socket-api): protocol parsing and formatting with exhaustive tests"
```

---

## Task 3: status_resolver.rs

- [ ] Write the failing test first. Create `crates/socket-api/tests/status_resolver_tests.rs`:

```rust
use camino::Utf8PathBuf;
use socket_api::status_resolver::StatusResolver;
use sync_engine::state::{FileStatus, SyncState};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

fn make_resolver_with_folder(
    root: &str,
    file_path: &str,
    status: FileStatus,
) -> StatusResolver {
    let folder_id = Uuid::new_v4();
    let mut state = SyncState::new(folder_id);
    state.set_file_status(Utf8PathBuf::from(file_path), status);

    let mut states = HashMap::new();
    states.insert(folder_id, state);

    let folder_roots = vec![(Utf8PathBuf::from(root), folder_id)];

    StatusResolver::new(
        Arc::new(RwLock::new(states)),
        Arc::new(RwLock::new(folder_roots)),
    )
}

#[test]
fn path_not_in_any_folder_returns_none() {
    let resolver = StatusResolver::new(
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(RwLock::new(vec![])),
    );
    assert_eq!(resolver.resolve_file("/not/synced/file.txt"), "NONE");
}

#[test]
fn file_with_ok_status_returns_ok() {
    let resolver = make_resolver_with_folder(
        "/sync/root",
        "/sync/root/file.txt",
        FileStatus::Ok,
    );
    assert_eq!(resolver.resolve_file("/sync/root/file.txt"), "OK");
}

#[test]
fn file_with_syncing_status_returns_sync() {
    let resolver = make_resolver_with_folder(
        "/sync/root",
        "/sync/root/uploading.txt",
        FileStatus::Syncing,
    );
    assert_eq!(resolver.resolve_file("/sync/root/uploading.txt"), "SYNC");
}

#[test]
fn file_with_error_status_returns_error() {
    let resolver = make_resolver_with_folder(
        "/sync/root",
        "/sync/root/broken.txt",
        FileStatus::Error("checksum mismatch".into()),
    );
    assert_eq!(resolver.resolve_file("/sync/root/broken.txt"), "ERROR");
}

#[test]
fn file_with_excluded_status_returns_excluded() {
    let resolver = make_resolver_with_folder(
        "/sync/root",
        "/sync/root/.hidden",
        FileStatus::Excluded,
    );
    assert_eq!(resolver.resolve_file("/sync/root/.hidden"), "EXCLUDED");
}

#[test]
fn file_in_folder_but_no_status_entry_returns_ok() {
    // File is inside a sync folder but has no explicit status — assume OK.
    let folder_id = Uuid::new_v4();
    let state = SyncState::new(folder_id);
    let mut states = HashMap::new();
    states.insert(folder_id, state);
    let folder_roots = vec![(Utf8PathBuf::from("/sync/root"), folder_id)];
    let resolver = StatusResolver::new(
        Arc::new(RwLock::new(states)),
        Arc::new(RwLock::new(folder_roots)),
    );
    // File is under /sync/root but has no explicit status entry.
    assert_eq!(resolver.resolve_file("/sync/root/any_file.txt"), "OK");
}

#[test]
fn resolve_folder_path_in_sync_root_returns_ok() {
    let folder_id = Uuid::new_v4();
    let state = SyncState::new(folder_id);
    let mut states = HashMap::new();
    states.insert(folder_id, state);
    let folder_roots = vec![(Utf8PathBuf::from("/sync/root"), folder_id)];
    let resolver = StatusResolver::new(
        Arc::new(RwLock::new(states)),
        Arc::new(RwLock::new(folder_roots)),
    );
    assert_eq!(resolver.resolve_folder("/sync/root/subdir"), "OK");
}

#[test]
fn find_folder_for_path_returns_correct_uuid() {
    let folder_id = Uuid::new_v4();
    let state = SyncState::new(folder_id);
    let mut states = HashMap::new();
    states.insert(folder_id, state);
    let folder_roots = vec![(Utf8PathBuf::from("/sync/root"), folder_id)];
    let resolver = StatusResolver::new(
        Arc::new(RwLock::new(states)),
        Arc::new(RwLock::new(folder_roots)),
    );
    assert_eq!(
        resolver.find_folder_for_path("/sync/root/deep/file.txt"),
        Some(folder_id)
    );
    assert_eq!(resolver.find_folder_for_path("/outside/path"), None);
}
```

- [ ] Run (expect failure):

```bash
cargo test -p socket-api --test status_resolver_tests 2>&1 | head -20
# Expected: error — StatusResolver not yet implemented
```

- [ ] Replace `crates/socket-api/src/status_resolver.rs` with:

```rust
//! Maps filesystem paths to sync status tags understood by shell extensions.
//!
//! The resolver reads from `Arc<RwLock<…>>` state shared with the sync engine.
//! All methods take `&self` and acquire read locks only — no mutation occurs here.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use camino::Utf8PathBuf;
use uuid::Uuid;

use sync_engine::state::{FileStatus, SyncState};

/// Translates a filesystem path into a socket-protocol status tag string.
///
/// Status tag mapping:
/// - [`FileStatus::Ok`]       → `"OK"`
/// - [`FileStatus::Syncing`]  → `"SYNC"`
/// - [`FileStatus::Error`]    → `"ERROR"`
/// - [`FileStatus::Excluded`] → `"EXCLUDED"`
/// - Path not in any sync folder → `"NONE"`
/// - Path in sync folder but no explicit status → `"OK"` (assumed clean)
pub struct StatusResolver {
    sync_states: Arc<RwLock<HashMap<Uuid, SyncState>>>,
    folder_roots: Arc<RwLock<Vec<(Utf8PathBuf, Uuid)>>>,
}

impl StatusResolver {
    /// Create a new resolver backed by the given shared state maps.
    pub fn new(
        sync_states: Arc<RwLock<HashMap<Uuid, SyncState>>>,
        folder_roots: Arc<RwLock<Vec<(Utf8PathBuf, Uuid)>>>,
    ) -> Self {
        Self {
            sync_states,
            folder_roots,
        }
    }

    /// Return the status tag for a **file** path.
    pub fn resolve_file(&self, path: &str) -> &'static str {
        let Some(folder_id) = self.find_folder_for_path(path) else {
            return "NONE";
        };

        let states = self.sync_states.read().unwrap();
        let Some(state) = states.get(&folder_id) else {
            return "NONE";
        };

        let utf8_path = Utf8PathBuf::from(path);
        match state.file_statuses.get(&utf8_path) {
            None => "OK", // file is inside a sync folder but not tracked → OK
            Some(FileStatus::Ok) => "OK",
            Some(FileStatus::Syncing) => "SYNC",
            Some(FileStatus::Error(_)) => "ERROR",
            Some(FileStatus::Excluded) => "EXCLUDED",
        }
    }

    /// Return the status tag for a **folder** path.
    ///
    /// The folder status is the worst status of any file inside it.
    /// If no file has an explicit error or sync status the folder is `"OK"`.
    pub fn resolve_folder(&self, path: &str) -> &'static str {
        let Some(folder_id) = self.find_folder_for_path(path) else {
            return "NONE";
        };

        let states = self.sync_states.read().unwrap();
        let Some(state) = states.get(&folder_id) else {
            return "NONE";
        };

        let prefix = Utf8PathBuf::from(path);
        let mut worst: &'static str = "OK";

        for (file_path, status) in &state.file_statuses {
            if !file_path.starts_with(&prefix) {
                continue;
            }
            let tag = match status {
                FileStatus::Ok => "OK",
                FileStatus::Syncing => "SYNC",
                FileStatus::Error(_) => "ERROR",
                FileStatus::Excluded => "EXCLUDED",
            };
            // Priority order: ERROR > SYNC > EXCLUDED > OK
            worst = worse_status(worst, tag);
        }

        worst
    }

    /// Find the UUID of the sync folder whose local root is a prefix of `path`.
    ///
    /// Returns `None` if `path` is not inside any known sync folder.
    pub fn find_folder_for_path(&self, path: &str) -> Option<Uuid> {
        let roots = self.folder_roots.read().unwrap();
        let path_buf = Utf8PathBuf::from(path);
        // Prefer the most specific (longest) matching root.
        roots
            .iter()
            .filter(|(root, _)| path_buf.starts_with(root))
            .max_by_key(|(root, _)| root.as_str().len())
            .map(|(_, id)| *id)
    }
}

/// Return the "worse" of two status tag strings by priority.
fn worse_status(a: &'static str, b: &'static str) -> &'static str {
    fn priority(s: &str) -> u8 {
        match s {
            "ERROR" => 3,
            "SYNC" => 2,
            "EXCLUDED" => 1,
            _ => 0,
        }
    }
    if priority(b) > priority(a) { b } else { a }
}
```

- [ ] Run (expect pass):

```bash
cargo test -p socket-api --test status_resolver_tests 2>&1
# Expected: all tests pass
```

- [ ] Commit:

```bash
git add crates/socket-api/src/status_resolver.rs
git commit -m "feat(socket-api): StatusResolver mapping paths to status tags"
```

---

## Task 4: transport/unix.rs

- [ ] Write the failing test first. Create `crates/socket-api/tests/unix_transport_tests.rs`:

```rust
#[cfg(unix)]
mod unix_tests {
    use socket_api::transport::unix::UnixTransport;
    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;

    #[tokio::test]
    async fn bind_accept_and_exchange_message() {
        let dir = TempDir::new().unwrap();
        let socket_path = dir.path().join("test.sock");

        let transport = UnixTransport::bind(&socket_path).await.unwrap();

        // Spawn a client task that connects, sends a line, then reads back a reply.
        let path_clone = socket_path.clone();
        let client = tokio::spawn(async move {
            // Give the server a moment to reach accept().
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            let mut stream = UnixStream::connect(&path_clone).await.unwrap();
            stream.write_all(b"hello\n").await.unwrap();
            let mut buf = [0u8; 32];
            let n = stream.read(&mut buf).await.unwrap();
            String::from_utf8_lossy(&buf[..n]).to_string()
        });

        // Server side: accept one connection, echo the data back.
        let mut conn = transport.accept().await.unwrap();
        let mut buf = [0u8; 32];
        let n = conn.read(&mut buf).await.unwrap();
        conn.write_all(&buf[..n]).await.unwrap();

        let received = client.await.unwrap();
        assert_eq!(received, "hello\n");
    }

    #[tokio::test]
    async fn socket_file_removed_on_drop() {
        let dir = TempDir::new().unwrap();
        let socket_path = dir.path().join("drop_test.sock");
        {
            let _transport = UnixTransport::bind(&socket_path).await.unwrap();
            assert!(socket_path.exists(), "socket file should exist while transport is alive");
        }
        assert!(!socket_path.exists(), "socket file should be removed on drop");
    }
}
```

- [ ] Run (expect failure):

```bash
cargo test -p socket-api --test unix_transport_tests 2>&1 | head -20
# Expected: error — UnixTransport not yet implemented
```

- [ ] Replace `crates/socket-api/src/transport/mod.rs` with:

```rust
//! Transport abstraction for the socket API.
//!
//! The [`Transport`] trait provides a platform-independent accept loop.
//! `AsyncReadWrite` is the combined read+write trait object used per connection.

use std::pin::Pin;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::error::Result;

/// A readable and writable connection accepted from the transport.
pub trait AsyncReadWrite: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> AsyncReadWrite for T {}

/// A boxed connection handle.
pub type Connection = Box<dyn AsyncReadWrite>;

/// Platform-agnostic transport.
///
/// Implementations:
/// - [`unix::UnixTransport`] — Unix domain socket (macOS + Linux)
/// - [`windows::WindowsTransport`] — Named pipe (Windows only)
#[async_trait::async_trait]
pub trait Transport: Send + Sync {
    /// Accept the next incoming connection.
    async fn accept(&self) -> Result<Connection>;
}

pub mod unix;

#[cfg(target_os = "windows")]
pub mod windows;
```

- [ ] Create `crates/socket-api/src/transport/unix.rs`:

```rust
//! Unix domain socket transport for macOS and Linux.
//!
//! Platform socket paths:
//! - macOS: `~/Library/Group Containers/$(APP_GROUP_ID)/owncloud.sock`
//! - Linux: `$XDG_RUNTIME_DIR/owncloud/socket`

use std::path::{Path, PathBuf};

use tokio::net::UnixListener;

use crate::error::{Result, SocketApiError};
use crate::transport::{Connection, Transport};

/// A transport backed by a Unix domain socket.
///
/// The socket file is created on [`bind`](UnixTransport::bind) and removed on
/// [`Drop`].
pub struct UnixTransport {
    listener: UnixListener,
    socket_path: PathBuf,
}

impl UnixTransport {
    /// Bind to a Unix domain socket at `path`.
    ///
    /// If a socket file already exists at `path` it is removed first (handles
    /// stale sockets from a previous crash).
    pub async fn bind(path: &Path) -> Result<Self> {
        // Remove a stale socket file if present.
        if path.exists() {
            std::fs::remove_file(path).map_err(|e| {
                SocketApiError::Transport(format!(
                    "failed to remove stale socket {}: {e}",
                    path.display()
                ))
            })?;
        }

        // Ensure the parent directory exists.
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                SocketApiError::Transport(format!(
                    "failed to create socket directory {}: {e}",
                    parent.display()
                ))
            })?;
        }

        let listener = UnixListener::bind(path).map_err(|e| {
            SocketApiError::Transport(format!(
                "failed to bind Unix socket {}: {e}",
                path.display()
            ))
        })?;

        Ok(Self {
            listener,
            socket_path: path.to_owned(),
        })
    }

    /// Return the canonical socket path for the current platform.
    ///
    /// - Linux:  `$XDG_RUNTIME_DIR/owncloud/socket`  (falls back to `/tmp/owncloud/socket`)
    /// - macOS:  `~/Library/Group Containers/owncloud.socketapi/owncloud.sock`
    pub fn default_path() -> PathBuf {
        #[cfg(target_os = "macos")]
        {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home)
                .join("Library/Group Containers/owncloud.socketapi")
                .join("owncloud.sock")
        }
        #[cfg(not(target_os = "macos"))]
        {
            let runtime = std::env::var("XDG_RUNTIME_DIR")
                .unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(runtime).join("owncloud").join("socket")
        }
    }
}

#[async_trait::async_trait]
impl Transport for UnixTransport {
    async fn accept(&self) -> Result<Connection> {
        let (stream, _addr) = self.listener.accept().await.map_err(|e| {
            SocketApiError::Transport(format!("accept failed: {e}"))
        })?;
        Ok(Box::new(stream))
    }
}

impl Drop for UnixTransport {
    fn drop(&mut self) {
        // Best-effort cleanup — ignore errors (e.g. file already removed).
        let _ = std::fs::remove_file(&self.socket_path);
    }
}
```

- [ ] Add `async-trait` to `crates/socket-api/Cargo.toml` dependencies:

```toml
async-trait = { workspace = true }
```

- [ ] Run (expect pass):

```bash
cargo test -p socket-api --test unix_transport_tests 2>&1
# Expected:
# test unix_tests::bind_accept_and_exchange_message ... ok
# test unix_tests::socket_file_removed_on_drop ... ok
```

- [ ] Commit:

```bash
git add crates/socket-api/src/transport/
git commit -m "feat(socket-api): UnixTransport with bind/accept/drop cleanup"
```

---

## Task 5: transport/windows.rs

- [ ] Create `crates/socket-api/src/transport/windows.rs`:

```rust
//! Named-pipe transport for Windows.
//!
//! Pipe name format: `\\.\pipe\ownCloud-{Username}`
//! Username is read from the `USERNAME` environment variable.

#![cfg(target_os = "windows")]

use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

use crate::error::{Result, SocketApiError};
use crate::transport::{Connection, Transport};

/// A transport backed by a Windows named pipe.
pub struct WindowsTransport {
    pipe_name: String,
}

impl WindowsTransport {
    /// Create a named-pipe server for the current Windows user.
    ///
    /// Pipe name: `\\.\pipe\ownCloud-{USERNAME}`.
    pub fn bind() -> Result<Self> {
        let username = std::env::var("USERNAME").unwrap_or_else(|_| "ownCloud".into());
        let pipe_name = format!(r"\\.\pipe\ownCloud-{username}");
        Ok(Self { pipe_name })
    }

    /// Accept a single connection by creating a new pipe server instance.
    async fn accept_inner(&self) -> Result<NamedPipeServer> {
        let server = ServerOptions::new()
            .first_pipe_instance(false)
            .create(&self.pipe_name)
            .map_err(|e| SocketApiError::Transport(format!("named pipe create failed: {e}")))?;

        server
            .connect()
            .await
            .map_err(|e| SocketApiError::Transport(format!("named pipe connect failed: {e}")))?;

        Ok(server)
    }
}

#[async_trait::async_trait]
impl Transport for WindowsTransport {
    async fn accept(&self) -> Result<Connection> {
        let pipe = self.accept_inner().await?;
        Ok(Box::new(pipe))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires Windows"]
    fn windows_transport_binds() {
        // Run manually on a Windows host:
        //   cargo test -p socket-api -- windows_tests --ignored
        //
        // Expected: WindowsTransport::bind() returns Ok, and a client
        // connecting with CreateFile to \\.\pipe\ownCloud-<user> succeeds.
    }
}
```

- [ ] Verify the crate still compiles on non-Windows (windows.rs is cfg-gated):

```bash
cargo build -p socket-api 2>&1
# Expected: compiles without errors on Linux/macOS
```

- [ ] Commit:

```bash
git add crates/socket-api/src/transport/windows.rs
git commit -m "feat(socket-api): Windows named-pipe transport (cfg-gated, test ignored)"
```

---

## Task 6: broadcast.rs

- [ ] Write the failing test first. Create `crates/socket-api/tests/broadcast_tests.rs`:

```rust
use socket_api::broadcast::BroadcastSender;
use tokio::sync::mpsc;

#[tokio::test]
async fn two_connections_both_receive_status_changed() {
    let broadcaster = BroadcastSender::new();

    let (tx1, mut rx1) = mpsc::channel::<String>(8);
    let (tx2, mut rx2) = mpsc::channel::<String>(8);

    let _id1 = broadcaster.add_connection(tx1);
    let _id2 = broadcaster.add_connection(tx2);

    broadcaster.status_changed("OK", "/foo/bar.txt").await;

    let msg1 = rx1.recv().await.expect("connection 1 should receive message");
    let msg2 = rx2.recv().await.expect("connection 2 should receive message");

    assert_eq!(msg1, "STATUS:OK:/foo/bar.txt\n");
    assert_eq!(msg2, "STATUS:OK:/foo/bar.txt\n");
}

#[tokio::test]
async fn removed_connection_does_not_receive() {
    let broadcaster = BroadcastSender::new();

    let (tx1, mut rx1) = mpsc::channel::<String>(8);
    let (tx2, mut rx2) = mpsc::channel::<String>(8);

    let id1 = broadcaster.add_connection(tx1);
    let _id2 = broadcaster.add_connection(tx2);

    // Remove connection 1 before broadcasting.
    broadcaster.remove_connection(id1);

    broadcaster.status_changed("SYNC", "/a/b.txt").await;

    // Connection 2 should receive.
    let msg2 = rx2.recv().await.expect("connection 2 should receive");
    assert_eq!(msg2, "STATUS:SYNC:/a/b.txt\n");

    // Connection 1 should NOT receive (channel closed or nothing queued).
    // Since we removed it, its tx was dropped; recv should return None or nothing.
    assert!(rx1.try_recv().is_err(), "removed connection must not receive");
}

#[tokio::test]
async fn register_path_broadcasts_register_message() {
    let broadcaster = BroadcastSender::new();
    let (tx, mut rx) = mpsc::channel::<String>(8);
    broadcaster.add_connection(tx);

    broadcaster.register_path("/sync/root").await;

    let msg = rx.recv().await.unwrap();
    assert_eq!(msg, "REGISTER_PATH:/sync/root\n");
}

#[tokio::test]
async fn unregister_path_broadcasts_unregister_message() {
    let broadcaster = BroadcastSender::new();
    let (tx, mut rx) = mpsc::channel::<String>(8);
    broadcaster.add_connection(tx);

    broadcaster.unregister_path("/sync/root").await;

    let msg = rx.recv().await.unwrap();
    assert_eq!(msg, "UNREGISTER_PATH:/sync/root\n");
}

#[tokio::test]
async fn update_view_broadcasts_update_message() {
    let broadcaster = BroadcastSender::new();
    let (tx, mut rx) = mpsc::channel::<String>(8);
    broadcaster.add_connection(tx);

    broadcaster.update_view("/sync/root/subdir").await;

    let msg = rx.recv().await.unwrap();
    assert_eq!(msg, "UPDATE_VIEW:/sync/root/subdir\n");
}
```

- [ ] Run (expect failure):

```bash
cargo test -p socket-api --test broadcast_tests 2>&1 | head -20
# Expected: error — BroadcastSender not yet implemented
```

- [ ] Replace `crates/socket-api/src/broadcast.rs` with:

```rust
//! Broadcast sender — delivers server-initiated messages to all connected clients.
//!
//! The server calls methods like [`BroadcastSender::status_changed`] whenever
//! sync state changes.  Each connected client has a [`ConnectionHandle`] with
//! an `mpsc::Sender<String>`.  The broadcast clones and sends the message to
//! every open channel; stale channels (dropped receiver) are pruned lazily.

use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;
use uuid::Uuid;

/// Handle to one connected client, held inside `BroadcastSender`.
pub struct ConnectionHandle {
    /// Channel to the per-connection writer task.
    pub tx: mpsc::Sender<String>,
    /// Unique identifier, used for removal.
    pub id: Uuid,
}

/// Sends broadcast messages to all currently connected clients.
#[derive(Clone)]
pub struct BroadcastSender {
    connections: Arc<Mutex<Vec<ConnectionHandle>>>,
}

impl BroadcastSender {
    /// Create an empty sender with no registered connections.
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Register a new client connection.
    ///
    /// Returns the [`Uuid`] that can be passed to [`remove_connection`] on
    /// disconnect.
    pub fn add_connection(&self, tx: mpsc::Sender<String>) -> Uuid {
        let id = Uuid::new_v4();
        let handle = ConnectionHandle { tx, id };
        self.connections.lock().unwrap().push(handle);
        id
    }

    /// Remove a client connection by id.
    pub fn remove_connection(&self, id: Uuid) {
        let mut conns = self.connections.lock().unwrap();
        conns.retain(|h| h.id != id);
    }

    /// Broadcast `REGISTER_PATH:path\n` to all clients.
    pub async fn register_path(&self, path: &str) {
        self.broadcast(format!("REGISTER_PATH:{path}\n")).await;
    }

    /// Broadcast `UNREGISTER_PATH:path\n` to all clients.
    pub async fn unregister_path(&self, path: &str) {
        self.broadcast(format!("UNREGISTER_PATH:{path}\n")).await;
    }

    /// Broadcast `STATUS:tag:path\n` to all clients.
    pub async fn status_changed(&self, tag: &str, path: &str) {
        self.broadcast(format!("STATUS:{tag}:{path}\n")).await;
    }

    /// Broadcast `UPDATE_VIEW:path\n` to all clients.
    pub async fn update_view(&self, path: &str) {
        self.broadcast(format!("UPDATE_VIEW:{path}\n")).await;
    }

    /// Send `message` to every registered connection.
    ///
    /// Connections whose receiver has been dropped are pruned from the list.
    async fn broadcast(&self, message: String) {
        // Collect senders without holding the lock across awaits.
        let senders: Vec<(Uuid, mpsc::Sender<String>)> = {
            let conns = self.connections.lock().unwrap();
            conns.iter().map(|h| (h.id, h.tx.clone())).collect()
        };

        let mut dead_ids: Vec<Uuid> = Vec::new();

        for (id, tx) in senders {
            if tx.send(message.clone()).await.is_err() {
                // Receiver dropped — mark for removal.
                dead_ids.push(id);
            }
        }

        if !dead_ids.is_empty() {
            let mut conns = self.connections.lock().unwrap();
            conns.retain(|h| !dead_ids.contains(&h.id));
        }
    }
}

impl Default for BroadcastSender {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] Run (expect pass):

```bash
cargo test -p socket-api --test broadcast_tests 2>&1
# Expected: all 5 tests pass
```

- [ ] Commit:

```bash
git add crates/socket-api/src/broadcast.rs
git commit -m "feat(socket-api): BroadcastSender with add/remove/broadcast methods"
```

---

## Task 7: commands/status.rs

- [ ] Write the failing test first. Create `crates/socket-api/tests/command_status_tests.rs`:

```rust
use camino::Utf8PathBuf;
use socket_api::commands::status::{
    handle_retrieve_file_status, handle_retrieve_folder_status,
};
use socket_api::status_resolver::StatusResolver;
use sync_engine::state::{FileStatus, SyncState};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

fn make_resolver(root: &str, file_path: &str, status: FileStatus) -> StatusResolver {
    let folder_id = Uuid::new_v4();
    let mut state = SyncState::new(folder_id);
    state.set_file_status(Utf8PathBuf::from(file_path), status);
    let mut states = HashMap::new();
    states.insert(folder_id, state);
    let folder_roots = vec![(Utf8PathBuf::from(root), folder_id)];
    StatusResolver::new(
        Arc::new(RwLock::new(states)),
        Arc::new(RwLock::new(folder_roots)),
    )
}

fn make_empty_resolver() -> StatusResolver {
    StatusResolver::new(
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(RwLock::new(vec![])),
    )
}

// ── RETRIEVE_FILE_STATUS ─────────────────────────────────────────────────────

#[test]
fn file_status_none_for_untracked_path() {
    let resolver = make_empty_resolver();
    let resp = handle_retrieve_file_status("/not/synced/file.txt", &resolver);
    assert_eq!(resp, "STATUS:NONE:/not/synced/file.txt\n");
}

#[test]
fn file_status_ok() {
    let resolver = make_resolver("/sync", "/sync/ok.txt", FileStatus::Ok);
    let resp = handle_retrieve_file_status("/sync/ok.txt", &resolver);
    assert_eq!(resp, "STATUS:OK:/sync/ok.txt\n");
}

#[test]
fn file_status_sync() {
    let resolver = make_resolver("/sync", "/sync/uploading.txt", FileStatus::Syncing);
    let resp = handle_retrieve_file_status("/sync/uploading.txt", &resolver);
    assert_eq!(resp, "STATUS:SYNC:/sync/uploading.txt\n");
}

#[test]
fn file_status_error() {
    let resolver = make_resolver(
        "/sync",
        "/sync/broken.txt",
        FileStatus::Error("network error".into()),
    );
    let resp = handle_retrieve_file_status("/sync/broken.txt", &resolver);
    assert_eq!(resp, "STATUS:ERROR:/sync/broken.txt\n");
}

#[test]
fn file_status_excluded() {
    let resolver = make_resolver("/sync", "/sync/.hidden", FileStatus::Excluded);
    let resp = handle_retrieve_file_status("/sync/.hidden", &resolver);
    assert_eq!(resp, "STATUS:EXCLUDED:/sync/.hidden\n");
}

// ── RETRIEVE_FOLDER_STATUS ───────────────────────────────────────────────────

#[test]
fn folder_status_none_for_untracked_path() {
    let resolver = make_empty_resolver();
    let resp = handle_retrieve_folder_status("/not/synced/", &resolver);
    assert_eq!(resp, "STATUS:NONE:/not/synced/\n");
}

#[test]
fn folder_status_ok_for_clean_folder() {
    let folder_id = Uuid::new_v4();
    let state = SyncState::new(folder_id);
    let mut states = HashMap::new();
    states.insert(folder_id, state);
    let folder_roots = vec![(Utf8PathBuf::from("/sync"), folder_id)];
    let resolver = StatusResolver::new(
        Arc::new(RwLock::new(states)),
        Arc::new(RwLock::new(folder_roots)),
    );
    let resp = handle_retrieve_folder_status("/sync/subdir", &resolver);
    assert_eq!(resp, "STATUS:OK:/sync/subdir\n");
}
```

- [ ] Run (expect failure):

```bash
cargo test -p socket-api --test command_status_tests 2>&1 | head -20
# Expected: error — commands::status module not yet implemented
```

- [ ] Replace `crates/socket-api/src/commands/mod.rs` with:

```rust
//! Command handler modules — one module per logical command group.

pub mod menu;
pub mod share;
pub mod status;
pub mod v2;
pub mod vfs_cmds;
```

- [ ] Create stub files so the crate compiles:

`crates/socket-api/src/commands/menu.rs` — `// TODO`  
`crates/socket-api/src/commands/share.rs` — `// TODO`  
`crates/socket-api/src/commands/v2.rs` — `// TODO`  
`crates/socket-api/src/commands/vfs_cmds.rs` — `// TODO`  

- [ ] Create `crates/socket-api/src/commands/status.rs`:

```rust
//! Handlers for `RETRIEVE_FILE_STATUS` and `RETRIEVE_FOLDER_STATUS`.

use crate::protocol::format_response;
use crate::status_resolver::StatusResolver;

/// Handle a `RETRIEVE_FILE_STATUS:path` command.
///
/// Returns `STATUS:tag:path\n`.
pub fn handle_retrieve_file_status(path: &str, resolver: &StatusResolver) -> String {
    let tag = resolver.resolve_file(path);
    format_response("STATUS", &[tag, path])
}

/// Handle a `RETRIEVE_FOLDER_STATUS:path` command.
///
/// Returns `STATUS:tag:path\n`.
pub fn handle_retrieve_folder_status(path: &str, resolver: &StatusResolver) -> String {
    let tag = resolver.resolve_folder(path);
    format_response("STATUS", &[tag, path])
}
```

- [ ] Run (expect pass):

```bash
cargo test -p socket-api --test command_status_tests 2>&1
# Expected: all tests pass
```

- [ ] Commit:

```bash
git add crates/socket-api/src/commands/
git commit -m "feat(socket-api): status command handlers with full tag coverage"
```

---

## Task 8: commands/menu.rs

- [ ] Write the failing test first. Create `crates/socket-api/tests/command_menu_tests.rs`:

```rust
use camino::Utf8PathBuf;
use socket_api::commands::menu::{handle_get_strings, handle_get_menu_items};
use socket_api::status_resolver::StatusResolver;
use sync_engine::state::{FileStatus, SyncState};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

fn make_resolver_with_file(root: &str, file_path: &str, status: FileStatus) -> StatusResolver {
    let folder_id = Uuid::new_v4();
    let mut state = SyncState::new(folder_id);
    state.set_file_status(Utf8PathBuf::from(file_path), status);
    let mut states = HashMap::new();
    states.insert(folder_id, state);
    let folder_roots = vec![(Utf8PathBuf::from(root), folder_id)];
    StatusResolver::new(
        Arc::new(RwLock::new(states)),
        Arc::new(RwLock::new(folder_roots)),
    )
}

fn make_empty_resolver() -> StatusResolver {
    StatusResolver::new(
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(RwLock::new(vec![])),
    )
}

// ── GET_STRINGS ───────────────────────────────────────────────────────────────

#[test]
fn get_strings_contains_share_menu_title() {
    let resp = handle_get_strings();
    assert!(
        resp.contains("SHARE_MENU_TITLE"),
        "GET_STRINGS should contain SHARE_MENU_TITLE"
    );
}

#[test]
fn get_strings_starts_with_get_strings_prefix() {
    let resp = handle_get_strings();
    assert!(resp.starts_with("GET_STRINGS:"), "should start with GET_STRINGS:");
}

#[test]
fn get_strings_ends_with_newline() {
    let resp = handle_get_strings();
    assert!(resp.ends_with('\n'), "GET_STRINGS response must end with newline");
}

#[test]
fn get_strings_contains_make_available() {
    let resp = handle_get_strings();
    assert!(resp.contains("MAKE_AVAILABLE"), "should contain MAKE_AVAILABLE key");
}

#[test]
fn get_strings_contains_make_online_only() {
    let resp = handle_get_strings();
    assert!(resp.contains("MAKE_ONLINE_ONLY"), "should contain MAKE_ONLINE_ONLY key");
}

// ── GET_MENU_ITEMS ────────────────────────────────────────────────────────────

#[test]
fn get_menu_items_path_not_in_sync_folder_returns_empty() {
    let resolver = make_empty_resolver();
    let resp = handle_get_menu_items("/not/synced/file.txt", &resolver);
    // Path is outside any sync folder: response contains only the path, no items.
    assert!(
        resp.starts_with("GET_MENU_ITEMS:"),
        "should start with GET_MENU_ITEMS:"
    );
    assert!(resp.ends_with('\n'));
}

#[test]
fn get_menu_items_synced_file_includes_share_item() {
    let resolver = make_resolver_with_file("/sync", "/sync/doc.pdf", FileStatus::Ok);
    let resp = handle_get_menu_items("/sync/doc.pdf", &resolver);
    assert!(
        resp.contains("SHARE"),
        "menu for a synced file should include SHARE item"
    );
}

#[test]
fn get_menu_items_synced_file_includes_copy_link_item() {
    let resolver = make_resolver_with_file("/sync", "/sync/doc.pdf", FileStatus::Ok);
    let resp = handle_get_menu_items("/sync/doc.pdf", &resolver);
    assert!(
        resp.contains("COPY_PRIVATE_LINK"),
        "menu for a synced file should include COPY_PRIVATE_LINK item"
    );
}

#[test]
fn get_menu_items_format_has_path_first() {
    let resolver = make_resolver_with_file("/sync", "/sync/a.txt", FileStatus::Ok);
    let resp = handle_get_menu_items("/sync/a.txt", &resolver);
    // Format: GET_MENU_ITEMS:path\x1ename:command:state\x1e…\n
    assert!(
        resp.starts_with("GET_MENU_ITEMS:/sync/a.txt"),
        "path must be first field after GET_MENU_ITEMS:"
    );
}
```

- [ ] Run (expect failure):

```bash
cargo test -p socket-api --test command_menu_tests 2>&1 | head -20
# Expected: error — handle_get_strings / handle_get_menu_items not yet implemented
```

- [ ] Replace `crates/socket-api/src/commands/menu.rs` with:

```rust
//! Handlers for `GET_STRINGS` and `GET_MENU_ITEMS`.

use crate::protocol::FIELD_SEP;
use crate::status_resolver::StatusResolver;

/// Handle a `GET_STRINGS` command.
///
/// Returns a fixed set of localised key→value pairs joined by `\x1e`.
/// Format: `GET_STRINGS:KEY1:value1\x1eKEY2:value2\n`
pub fn handle_get_strings() -> String {
    let parts: Vec<(&str, &str)> = vec![
        ("SHARE_MENU_TITLE", "Share"),
        ("COPY_LINK", "Copy link"),
        ("MAKE_AVAILABLE", "Make available locally"),
        ("MAKE_ONLINE_ONLY", "Make online only"),
        ("OPEN_PRIVATE_LINK", "Open in browser"),
    ];

    let mut out = String::from("GET_STRINGS:");
    let pairs: Vec<String> = parts
        .iter()
        .map(|(k, v)| format!("{k}:{v}"))
        .collect();
    out.push_str(&pairs.join(&FIELD_SEP.to_string()));
    out.push('\n');
    out
}

/// Handle a `GET_MENU_ITEMS:path` command.
///
/// Format:
/// ```text
/// GET_MENU_ITEMS:path\x1ename1:command1:state1\x1ename2:command2:state2\n
/// ```
///
/// If `path` is not inside any sync folder, only the path echo is returned
/// (no items).
pub fn handle_get_menu_items(path: &str, resolver: &StatusResolver) -> String {
    let sep = FIELD_SEP.to_string();

    if resolver.find_folder_for_path(path).is_none() {
        // Not in any sync folder — return empty item list.
        return format!("GET_MENU_ITEMS:{path}\n");
    }

    // Build context menu items.
    // Format per item: "display_name:COMMAND:enabled"
    let mut items: Vec<String> = Vec::new();

    items.push(format!("Share:SHARE:enabled"));
    items.push(format!("Copy link:COPY_PRIVATE_LINK:enabled"));

    // In a full implementation we'd check VfsStatus here.
    // For now we expose both hydrate/dehydrate actions.
    items.push(format!("Make available locally:MAKE_AVAILABLE_LOCALLY:enabled"));
    items.push(format!("Make online only:MAKE_ONLINE_ONLY:enabled"));

    let mut out = format!("GET_MENU_ITEMS:{path}");
    for item in &items {
        out.push(FIELD_SEP);
        out.push_str(item);
    }
    out.push('\n');
    out
}
```

- [ ] Run (expect pass):

```bash
cargo test -p socket-api --test command_menu_tests 2>&1
# Expected: all tests pass
```

- [ ] Commit:

```bash
git add crates/socket-api/src/commands/menu.rs
git commit -m "feat(socket-api): GET_STRINGS and GET_MENU_ITEMS command handlers"
```

---

## Task 9: commands/vfs_cmds.rs

- [ ] Write the failing test first. Create `crates/socket-api/tests/command_vfs_tests.rs`:

```rust
use std::sync::Arc;
use socket_api::broadcast::BroadcastSender;
use socket_api::commands::vfs_cmds::{
    handle_make_available_locally, handle_make_online_only,
};
use tokio::sync::mpsc;
use vfs_off::VfsOff;

#[tokio::test]
async fn make_available_locally_returns_ok() {
    let vfs: Arc<dyn vfs_core::Vfs> = Arc::new(VfsOff::new());
    let broadcaster = BroadcastSender::new();
    let paths = vec!["/sync/root/file.txt".to_string()];

    let resp = handle_make_available_locally(paths, vfs, &broadcaster).await;
    assert_eq!(resp, "MAKE_AVAILABLE_LOCALLY:OK\n");
}

#[tokio::test]
async fn make_online_only_returns_ok() {
    let vfs: Arc<dyn vfs_core::Vfs> = Arc::new(VfsOff::new());
    let broadcaster = BroadcastSender::new();
    let paths = vec!["/sync/root/file.txt".to_string()];

    let resp = handle_make_online_only(paths, vfs, &broadcaster).await;
    assert_eq!(resp, "MAKE_ONLINE_ONLY:OK\n");
}

#[tokio::test]
async fn make_available_locally_broadcasts_status_for_each_path() {
    let vfs: Arc<dyn vfs_core::Vfs> = Arc::new(VfsOff::new());
    let broadcaster = BroadcastSender::new();

    let (tx, mut rx) = mpsc::channel::<String>(8);
    broadcaster.add_connection(tx);

    let paths = vec![
        "/sync/root/a.txt".to_string(),
        "/sync/root/b.txt".to_string(),
    ];

    handle_make_available_locally(paths, vfs, &broadcaster).await;

    // Two STATUS broadcasts should arrive (one per path).
    let msg1 = rx.recv().await.expect("first broadcast missing");
    let msg2 = rx.recv().await.expect("second broadcast missing");

    assert!(msg1.starts_with("STATUS:"), "expected STATUS broadcast, got: {msg1}");
    assert!(msg2.starts_with("STATUS:"), "expected STATUS broadcast, got: {msg2}");
}

#[tokio::test]
async fn make_online_only_broadcasts_status_for_each_path() {
    let vfs: Arc<dyn vfs_core::Vfs> = Arc::new(VfsOff::new());
    let broadcaster = BroadcastSender::new();

    let (tx, mut rx) = mpsc::channel::<String>(8);
    broadcaster.add_connection(tx);

    let paths = vec!["/sync/root/c.txt".to_string()];

    handle_make_online_only(paths, vfs, &broadcaster).await;

    let msg = rx.recv().await.expect("broadcast missing");
    assert!(msg.starts_with("STATUS:"), "expected STATUS broadcast, got: {msg}");
}
```

- [ ] Run (expect failure):

```bash
cargo test -p socket-api --test command_vfs_tests 2>&1 | head -20
# Expected: error — vfs_cmds module not yet implemented
```

- [ ] Replace `crates/socket-api/src/commands/vfs_cmds.rs` with:

```rust
//! Handlers for `MAKE_AVAILABLE_LOCALLY` and `MAKE_ONLINE_ONLY`.
//!
//! These commands ask the VFS layer to hydrate or dehydrate files, then
//! broadcast a STATUS update to all shell extension clients.

use std::sync::Arc;

use camino::Utf8Path;

use crate::broadcast::BroadcastSender;
use vfs_core::Vfs;

/// Handle a `MAKE_AVAILABLE_LOCALLY:p1\x1ep2…` command.
///
/// Calls [`Vfs::hydrate`] for each path, then broadcasts a STATUS update.
/// Returns `"MAKE_AVAILABLE_LOCALLY:OK\n"` on success, or an error line if any
/// hydration fails (errors are logged but do not abort processing of remaining
/// paths).
pub async fn handle_make_available_locally(
    paths: Vec<String>,
    vfs: Arc<dyn Vfs>,
    broadcast: &BroadcastSender,
) -> String {
    let mut had_error = false;

    for path in &paths {
        let utf8 = Utf8Path::new(path.as_str());
        match vfs.hydrate(utf8).await {
            Ok(()) => {
                broadcast.status_changed("OK", path).await;
            }
            Err(e) => {
                tracing::warn!("hydrate failed for {path}: {e}");
                broadcast.status_changed("ERROR", path).await;
                had_error = true;
            }
        }
    }

    if had_error {
        "MAKE_AVAILABLE_LOCALLY:ERROR\n".to_string()
    } else {
        "MAKE_AVAILABLE_LOCALLY:OK\n".to_string()
    }
}

/// Handle a `MAKE_ONLINE_ONLY:p1\x1ep2…` command.
///
/// Calls [`Vfs::dehydrate`] for each path, then broadcasts a STATUS update.
/// Returns `"MAKE_ONLINE_ONLY:OK\n"` on success.
pub async fn handle_make_online_only(
    paths: Vec<String>,
    vfs: Arc<dyn Vfs>,
    broadcast: &BroadcastSender,
) -> String {
    let mut had_error = false;

    for path in &paths {
        let utf8 = Utf8Path::new(path.as_str());
        match vfs.dehydrate(utf8).await {
            Ok(()) => {
                broadcast.status_changed("OK", path).await;
            }
            Err(e) => {
                tracing::warn!("dehydrate failed for {path}: {e}");
                broadcast.status_changed("ERROR", path).await;
                had_error = true;
            }
        }
    }

    if had_error {
        "MAKE_ONLINE_ONLY:ERROR\n".to_string()
    } else {
        "MAKE_ONLINE_ONLY:OK\n".to_string()
    }
}
```

- [ ] Run (expect pass):

```bash
cargo test -p socket-api --test command_vfs_tests 2>&1
# Expected: all 4 tests pass
```

- [ ] Commit:

```bash
git add crates/socket-api/src/commands/vfs_cmds.rs
git commit -m "feat(socket-api): MAKE_AVAILABLE_LOCALLY and MAKE_ONLINE_ONLY handlers"
```

---

## Task 10: commands/share.rs + v2.rs

- [ ] Write the failing test first. Create `crates/socket-api/tests/command_share_v2_tests.rs`:

```rust
use socket_api::commands::share::{handle_share, handle_copy_private_link};
use socket_api::commands::v2::handle_v2_get_client_icon;

// ── SHARE ────────────────────────────────────────────────────────────────────

#[test]
fn share_returns_ok_response() {
    let resp = handle_share("/sync/root/doc.pdf");
    assert_eq!(resp, "SHARE:OK:/sync/root/doc.pdf\n");
}

#[test]
fn share_response_format() {
    let resp = handle_share("/any/path/here");
    assert!(resp.starts_with("SHARE:OK:"), "should start with SHARE:OK:");
    assert!(resp.ends_with('\n'), "should end with newline");
}

// ── COPY_PRIVATE_LINK ────────────────────────────────────────────────────────

#[test]
fn copy_private_link_returns_ok_response() {
    let resp = handle_copy_private_link("/sync/root/image.png");
    assert_eq!(resp, "COPY_PRIVATE_LINK:OK:/sync/root/image.png\n");
}

#[test]
fn copy_private_link_response_format() {
    let resp = handle_copy_private_link("/any/path");
    assert!(
        resp.starts_with("COPY_PRIVATE_LINK:OK:"),
        "should start with COPY_PRIVATE_LINK:OK:"
    );
    assert!(resp.ends_with('\n'));
}

// ── V2/GET_CLIENT_ICON ────────────────────────────────────────────────────────

#[test]
fn v2_get_client_icon_parses_id_and_returns_result() {
    let body = r#"{"id":"42","arguments":{}}"#;
    let resp = handle_v2_get_client_icon(body);
    assert!(
        resp.contains(r#""id":"42""#),
        "response should echo the request id"
    );
    assert!(
        resp.contains(r#""icon":"#),
        "response should contain icon field"
    );
}

#[test]
fn v2_get_client_icon_starts_with_v2_prefix() {
    let body = r#"{"id":"1","arguments":{}}"#;
    let resp = handle_v2_get_client_icon(body);
    assert!(
        resp.starts_with("V2/GET_CLIENT_ICON\n"),
        "V2 response should start with V2/GET_CLIENT_ICON\\n"
    );
}

#[test]
fn v2_get_client_icon_ends_with_newline() {
    let body = r#"{"id":"7","arguments":{}}"#;
    let resp = handle_v2_get_client_icon(body);
    assert!(resp.ends_with('\n'), "V2 response must end with newline");
}

#[test]
fn v2_get_client_icon_malformed_body_uses_unknown_id() {
    // Graceful degradation when body is not valid JSON.
    let resp = handle_v2_get_client_icon("not json at all");
    assert!(
        resp.contains(r#""id":"unknown""#),
        "malformed body should produce id=unknown, got: {resp}"
    );
}
```

- [ ] Run (expect failure):

```bash
cargo test -p socket-api --test command_share_v2_tests 2>&1 | head -20
# Expected: error — handle_share, handle_copy_private_link, handle_v2_get_client_icon not yet implemented
```

- [ ] Replace `crates/socket-api/src/commands/share.rs` with:

```rust
//! Handlers for `SHARE` and `COPY_PRIVATE_LINK`.
//!
//! The socket API acknowledges these commands immediately with an `OK` response.
//! The actual work (opening a share dialog, copying a URL) is delegated to the
//! daemon process via an event; the socket server only acts as a relay.

/// Handle a `SHARE:path` command.
///
/// Returns `"SHARE:OK:path\n"`.
pub fn handle_share(path: &str) -> String {
    format!("SHARE:OK:{path}\n")
}

/// Handle a `COPY_PRIVATE_LINK:path` command.
///
/// Returns `"COPY_PRIVATE_LINK:OK:path\n"`.
pub fn handle_copy_private_link(path: &str) -> String {
    format!("COPY_PRIVATE_LINK:OK:{path}\n")
}
```

- [ ] Replace `crates/socket-api/src/commands/v2.rs` with:

```rust
//! Handler for `V2/GET_CLIENT_ICON`.
//!
//! V2 protocol wire format (server→client):
//! ```text
//! V2/GET_CLIENT_ICON\n
//! {"id":"<id>","result":{"icon":"<base64-png>"}}\n
//! ```

use serde_json::Value;

/// A minimal 1×1 transparent PNG encoded as base64, used as the placeholder
/// client icon when a real icon asset is not yet available.
///
/// In production this would be replaced with the actual 16×16 ownCloud icon.
const PLACEHOLDER_ICON_BASE64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

/// Handle a `V2/GET_CLIENT_ICON` command.
///
/// `body` is the raw JSON from the second line of the V2 request.
///
/// Returns the two-line V2 response:
/// ```text
/// V2/GET_CLIENT_ICON\n{"id":"<id>","result":{"icon":"<base64>"}}\n
/// ```
pub fn handle_v2_get_client_icon(body: &str) -> String {
    // Parse the request id; fall back to "unknown" on malformed JSON.
    let id = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("id").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string());

    let result_json = serde_json::json!({
        "id": id,
        "result": {
            "icon": PLACEHOLDER_ICON_BASE64
        }
    });

    format!("V2/GET_CLIENT_ICON\n{result_json}\n")
}
```

- [ ] Run (expect pass):

```bash
cargo test -p socket-api --test command_share_v2_tests 2>&1
# Expected: all tests pass
```

- [ ] Commit:

```bash
git add crates/socket-api/src/commands/share.rs crates/socket-api/src/commands/v2.rs
git commit -m "feat(socket-api): SHARE, COPY_PRIVATE_LINK, V2/GET_CLIENT_ICON command handlers"
```

---

## Task 11: server.rs — SocketApiServer

- [ ] Write the failing test first. Create `crates/socket-api/tests/server_unit_tests.rs`:

```rust
//! Unit tests for SocketApiServer construction and the per-connection
//! command dispatch logic (without a live transport).

use camino::Utf8PathBuf;
use socket_api::server::SocketApiServer;
use sync_engine::state::SyncState;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;
use vfs_off::VfsOff;

fn make_server() -> Arc<SocketApiServer> {
    let folder_id = Uuid::new_v4();
    let state = SyncState::new(folder_id);
    let mut states = HashMap::new();
    states.insert(folder_id, state);
    let sync_states = Arc::new(RwLock::new(states));
    let folder_roots = Arc::new(RwLock::new(vec![
        (Utf8PathBuf::from("/sync/root"), folder_id),
    ]));
    let vfs: Arc<dyn vfs_core::Vfs> = Arc::new(VfsOff::new());

    Arc::new(SocketApiServer::new(sync_states, folder_roots, vfs))
}

#[test]
fn server_constructs_without_panic() {
    let _server = make_server();
}

#[tokio::test]
async fn dispatch_version_command_returns_version_string() {
    let server = make_server();
    let resp = server.dispatch_line("VERSION").await;
    assert_eq!(resp, Some("VERSION:1.1\n".to_string()));
}

#[tokio::test]
async fn dispatch_get_strings_returns_string_map() {
    let server = make_server();
    let resp = server.dispatch_line("GET_STRINGS").await;
    let resp = resp.expect("dispatch returned None");
    assert!(resp.starts_with("GET_STRINGS:"));
    assert!(resp.contains("SHARE_MENU_TITLE"));
}

#[tokio::test]
async fn dispatch_retrieve_file_status_untracked() {
    let server = make_server();
    let resp = server
        .dispatch_line("RETRIEVE_FILE_STATUS:/outside/sync.txt")
        .await;
    assert_eq!(resp, Some("STATUS:NONE:/outside/sync.txt\n".to_string()));
}

#[tokio::test]
async fn dispatch_share_returns_ok() {
    let server = make_server();
    let resp = server.dispatch_line("SHARE:/sync/root/doc.pdf").await;
    assert_eq!(resp, Some("SHARE:OK:/sync/root/doc.pdf\n".to_string()));
}

#[tokio::test]
async fn dispatch_copy_private_link_returns_ok() {
    let server = make_server();
    let resp = server
        .dispatch_line("COPY_PRIVATE_LINK:/sync/root/file.txt")
        .await;
    assert_eq!(
        resp,
        Some("COPY_PRIVATE_LINK:OK:/sync/root/file.txt\n".to_string())
    );
}

#[tokio::test]
async fn dispatch_make_available_locally_returns_ok() {
    let server = make_server();
    let resp = server
        .dispatch_line("MAKE_AVAILABLE_LOCALLY:/sync/root/file.txt")
        .await;
    assert_eq!(resp, Some("MAKE_AVAILABLE_LOCALLY:OK\n".to_string()));
}

#[tokio::test]
async fn dispatch_unknown_command_returns_none() {
    let server = make_server();
    let resp = server.dispatch_line("UNKNOWN_COMMAND:arg").await;
    assert!(resp.is_none(), "unknown command should return None (silently ignored)");
}
```

- [ ] Run (expect failure):

```bash
cargo test -p socket-api --test server_unit_tests 2>&1 | head -20
# Expected: error — SocketApiServer not yet implemented
```

- [ ] Replace `crates/socket-api/src/server.rs` with:

```rust
//! `SocketApiServer` — accept loop and per-connection command dispatcher.
//!
//! # Architecture
//!
//! ```text
//! SocketApiServer::run(transport)
//!   └── loop: transport.accept()
//!         └── spawn connection_task(stream, server)
//!               ├── reader: lines → dispatch_line() → write response
//!               └── writer: BroadcastSender mpsc::Receiver → write broadcasts
//! ```

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use camino::Utf8PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::broadcast::BroadcastSender;
use crate::commands::menu::{handle_get_menu_items, handle_get_strings};
use crate::commands::share::{handle_copy_private_link, handle_share};
use crate::commands::status::{handle_retrieve_file_status, handle_retrieve_folder_status};
use crate::commands::v2::handle_v2_get_client_icon;
use crate::commands::vfs_cmds::{handle_make_available_locally, handle_make_online_only};
use crate::error::{Result, SocketApiError};
use crate::protocol::{parse_command, Command};
use crate::status_resolver::StatusResolver;
use crate::transport::{Connection, Transport};
use sync_engine::state::SyncState;
use vfs_core::Vfs;

/// The IPC server for shell extension integrations.
pub struct SocketApiServer {
    resolver: Arc<StatusResolver>,
    broadcast: Arc<BroadcastSender>,
    vfs: Arc<dyn Vfs>,
    folder_roots: Arc<RwLock<Vec<(Utf8PathBuf, Uuid)>>>,
}

impl SocketApiServer {
    /// Construct a new server.
    pub fn new(
        sync_states: Arc<RwLock<HashMap<Uuid, SyncState>>>,
        folder_roots: Arc<RwLock<Vec<(Utf8PathBuf, Uuid)>>>,
        vfs: Arc<dyn Vfs>,
    ) -> Self {
        let resolver = Arc::new(StatusResolver::new(
            sync_states,
            folder_roots.clone(),
        ));
        let broadcast = Arc::new(BroadcastSender::new());
        Self {
            resolver,
            broadcast,
            vfs,
            folder_roots,
        }
    }

    /// Return a clone of the broadcast sender so the sync engine can push
    /// status updates to shell extensions.
    pub fn broadcast(&self) -> Arc<BroadcastSender> {
        self.broadcast.clone()
    }

    /// Run the accept loop, spawning a task for every new connection.
    ///
    /// This method runs until the transport errors or the returned future is
    /// cancelled (e.g. on shutdown).
    pub async fn run(self: Arc<Self>, transport: Box<dyn Transport>) -> Result<()> {
        loop {
            let conn = transport.accept().await.map_err(|e| {
                SocketApiError::Transport(format!("accept error: {e}"))
            })?;

            let server = self.clone();
            tokio::spawn(async move {
                if let Err(e) = server.handle_connection(conn).await {
                    tracing::warn!("connection error: {e}");
                }
            });
        }
    }

    /// Handle a single client connection from accept to EOF.
    async fn handle_connection(self: Arc<Self>, conn: Connection) -> Result<()> {
        // Register a per-connection broadcast channel.
        let (tx, mut broadcast_rx) = mpsc::channel::<String>(64);
        let conn_id = self.broadcast.add_connection(tx);

        // Split the connection into a BufReader (for lines) and a write half.
        // We use an `Arc<Mutex<>>` around the connection so both halves can
        // share ownership.  For simplicity we serialize writes behind a mutex.
        let conn = Arc::new(tokio::sync::Mutex::new(conn));

        // Send REGISTER_PATH for all active folders on connect.
        {
            let roots = self.folder_roots.read().unwrap();
            for (root, _) in roots.iter() {
                let msg = format!("REGISTER_PATH:{root}\n");
                let mut guard = conn.lock().await;
                if let Err(e) = guard.write_all(msg.as_bytes()).await {
                    tracing::warn!("failed to send REGISTER_PATH: {e}");
                    break;
                }
            }
        }

        // Spawn a writer task that drains the broadcast channel into the socket.
        let conn_writer = conn.clone();
        let write_task = tokio::spawn(async move {
            while let Some(msg) = broadcast_rx.recv().await {
                let mut guard = conn_writer.lock().await;
                if guard.write_all(msg.as_bytes()).await.is_err() {
                    break;
                }
            }
        });

        // Read loop: read lines, dispatch, write responses.
        {
            // We need an owned read half; take the connection via the mutex and
            // read directly from it inside the lock.  For command traffic this
            // is fine since commands are infrequent.
            let mut line = String::new();
            loop {
                line.clear();
                let n = {
                    let mut guard = conn.lock().await;
                    let mut reader = BufReader::new(&mut **guard);
                    match reader.read_line(&mut line).await {
                        Ok(n) => n,
                        Err(e) => {
                            tracing::debug!("read error: {e}");
                            0
                        }
                    }
                };

                if n == 0 {
                    // EOF — client disconnected.
                    break;
                }

                let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
                if trimmed.is_empty() {
                    continue;
                }

                if let Some(response) = self.dispatch_line(trimmed).await {
                    let mut guard = conn.lock().await;
                    if guard.write_all(response.as_bytes()).await.is_err() {
                        break;
                    }
                }
            }
        }

        // Clean up: remove the connection from the broadcaster.
        self.broadcast.remove_connection(conn_id);
        write_task.abort();

        Ok(())
    }

    /// Dispatch a single (trimmed) command line and return the response string,
    /// or `None` for unknown/ignored commands.
    ///
    /// This method is `pub` for unit-testing the dispatch table without a live
    /// transport.
    pub async fn dispatch_line(&self, line: &str) -> Option<String> {
        let cmd = match parse_command(line) {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!("parse error for {:?}: {e}", line);
                return None;
            }
        };

        let response = match cmd {
            Command::Version => "VERSION:1.1\n".to_string(),

            Command::GetStrings => handle_get_strings(),

            Command::GetMenuItems { path } => {
                handle_get_menu_items(&path, &self.resolver)
            }

            Command::RetrieveFileStatus { path } => {
                handle_retrieve_file_status(&path, &self.resolver)
            }

            Command::RetrieveFolderStatus { path } => {
                handle_retrieve_folder_status(&path, &self.resolver)
            }

            Command::Share { path } => handle_share(&path),

            Command::CopyPrivateLink { path } => handle_copy_private_link(&path),

            Command::MakeAvailableLocally { paths } => {
                handle_make_available_locally(
                    paths,
                    self.vfs.clone(),
                    &self.broadcast,
                )
                .await
            }

            Command::MakeOnlineOnly { paths } => {
                handle_make_online_only(
                    paths,
                    self.vfs.clone(),
                    &self.broadcast,
                )
                .await
            }

            Command::V2 { name, body } if name == "GET_CLIENT_ICON" => {
                handle_v2_get_client_icon(&body)
            }

            Command::V2 { name, .. } => {
                tracing::debug!("unhandled V2 command: {name}");
                return None;
            }
        };

        Some(response)
    }
}
```

- [ ] Run (expect pass):

```bash
cargo test -p socket-api --test server_unit_tests 2>&1
# Expected: all tests pass
```

- [ ] Commit:

```bash
git add crates/socket-api/src/server.rs
git commit -m "feat(socket-api): SocketApiServer accept loop and command dispatcher"
```

---

## Task 12: Integration tests

- [ ] Create `crates/socket-api/tests/server_tests.rs`:

```rust
//! Integration tests: full round-trip through UnixTransport → SocketApiServer.
//!
//! Each test starts a real server on a temp socket path and connects a client.

#[cfg(unix)]
mod integration {
    use camino::Utf8PathBuf;
    use socket_api::broadcast::BroadcastSender;
    use socket_api::server::SocketApiServer;
    use socket_api::transport::unix::UnixTransport;
    use socket_api::transport::Transport;
    use sync_engine::state::SyncState;
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};
    use std::time::Duration;
    use tempfile::TempDir;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;
    use uuid::Uuid;
    use vfs_off::VfsOff;

    /// Build a server with an empty sync state and start it on a temp socket.
    /// Returns `(server_arc, broadcast_arc, socket_path, _temp_dir)`.
    async fn start_server(
        dir: &TempDir,
    ) -> (
        Arc<SocketApiServer>,
        Arc<BroadcastSender>,
        std::path::PathBuf,
    ) {
        let socket_path = dir.path().join("test.sock");

        let sync_states: Arc<RwLock<HashMap<Uuid, SyncState>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let folder_roots: Arc<RwLock<Vec<(Utf8PathBuf, Uuid)>>> =
            Arc::new(RwLock::new(vec![]));
        let vfs: Arc<dyn vfs_core::Vfs> = Arc::new(VfsOff::new());

        let server = Arc::new(SocketApiServer::new(
            sync_states,
            folder_roots,
            vfs,
        ));
        let broadcast = server.broadcast();

        let transport = UnixTransport::bind(&socket_path).await.unwrap();
        let server_clone = server.clone();
        tokio::spawn(async move {
            let _ = server_clone
                .run(Box::new(transport) as Box<dyn Transport>)
                .await;
        });

        // Give the server a moment to reach accept().
        tokio::time::sleep(Duration::from_millis(20)).await;

        (server, broadcast, socket_path)
    }

    /// Connect a client, send one command, read and return one response line.
    async fn send_command(socket_path: &std::path::Path, cmd: &str) -> String {
        let mut stream = UnixStream::connect(socket_path).await.unwrap();
        stream
            .write_all(format!("{cmd}\n").as_bytes())
            .await
            .unwrap();

        let mut reader = BufReader::new(&mut stream);
        let mut response = String::new();
        reader.read_line(&mut response).await.unwrap();
        response
    }

    #[tokio::test]
    async fn version_command_returns_version_1_1() {
        let dir = TempDir::new().unwrap();
        let (_, _, socket_path) = start_server(&dir).await;

        let resp = send_command(&socket_path, "VERSION").await;
        assert_eq!(resp, "VERSION:1.1\n");
    }

    #[tokio::test]
    async fn get_strings_response_contains_share_menu_title() {
        let dir = TempDir::new().unwrap();
        let (_, _, socket_path) = start_server(&dir).await;

        let resp = send_command(&socket_path, "GET_STRINGS").await;
        assert!(
            resp.contains("SHARE_MENU_TITLE"),
            "GET_STRINGS should contain SHARE_MENU_TITLE, got: {resp}"
        );
    }

    #[tokio::test]
    async fn retrieve_file_status_untracked_returns_none() {
        let dir = TempDir::new().unwrap();
        let (_, _, socket_path) = start_server(&dir).await;

        let resp = send_command(
            &socket_path,
            "RETRIEVE_FILE_STATUS:/tmp/not-synced-at-all.txt",
        )
        .await;
        assert_eq!(resp, "STATUS:NONE:/tmp/not-synced-at-all.txt\n");
    }

    #[tokio::test]
    async fn make_available_locally_returns_ok() {
        let dir = TempDir::new().unwrap();
        let (_, _, socket_path) = start_server(&dir).await;

        let resp = send_command(&socket_path, "MAKE_AVAILABLE_LOCALLY:/some/path/file.txt").await;
        assert_eq!(resp, "MAKE_AVAILABLE_LOCALLY:OK\n");
    }

    #[tokio::test]
    async fn broadcast_status_changed_reaches_connected_client() {
        let dir = TempDir::new().unwrap();
        let (_, broadcast, socket_path) = start_server(&dir).await;

        // Connect a client.
        let mut stream = UnixStream::connect(&socket_path).await.unwrap();
        let mut reader = BufReader::new(&mut stream);

        // Drain the REGISTER_PATH messages sent on connect (there are none in
        // the empty-state server, but we wait briefly to be safe).
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Trigger an external broadcast.
        broadcast.status_changed("OK", "/foo/bar.txt").await;

        // The broadcast message should arrive on the connected stream.
        let mut line = String::new();
        tokio::time::timeout(Duration::from_millis(200), reader.read_line(&mut line))
            .await
            .expect("timed out waiting for broadcast")
            .unwrap();

        assert_eq!(line, "STATUS:OK:/foo/bar.txt\n");
    }

    #[tokio::test]
    async fn share_command_returns_ok() {
        let dir = TempDir::new().unwrap();
        let (_, _, socket_path) = start_server(&dir).await;

        let resp = send_command(&socket_path, "SHARE:/sync/root/document.pdf").await;
        assert_eq!(resp, "SHARE:OK:/sync/root/document.pdf\n");
    }

    #[tokio::test]
    async fn multiple_sequential_commands_on_same_connection() {
        let dir = TempDir::new().unwrap();
        let (_, _, socket_path) = start_server(&dir).await;

        let mut stream = UnixStream::connect(&socket_path).await.unwrap();
        let mut reader = BufReader::new(&mut stream);

        // Send VERSION
        stream.write_all(b"VERSION\n").await.unwrap();
        let mut line1 = String::new();
        reader.read_line(&mut line1).await.unwrap();
        assert_eq!(line1, "VERSION:1.1\n");

        // Send GET_STRINGS
        stream.write_all(b"GET_STRINGS\n").await.unwrap();
        let mut line2 = String::new();
        reader.read_line(&mut line2).await.unwrap();
        assert!(line2.starts_with("GET_STRINGS:"), "got: {line2}");
    }
}
```

- [ ] Run (expect pass):

```bash
cargo test -p socket-api --test server_tests 2>&1
# Expected: all integration tests pass
```

- [ ] Run the full socket-api test suite:

```bash
cargo test -p socket-api 2>&1
# Expected: all tests pass across all test files
```

- [ ] Run the full workspace test suite:

```bash
cargo test --workspace 2>&1
# Expected: no regressions across vfs-core, vfs-off, sync-engine, socket-api
```

- [ ] Commit:

```bash
git add crates/socket-api/tests/server_tests.rs
git commit -m "feat(socket-api): integration tests for full command/response and broadcast round-trips"
```

---

## Completion checklist

- [ ] `cargo test -p socket-api --test error_variants` — 3 tests pass
- [ ] `cargo test -p socket-api --test protocol_tests` — all parse + format tests pass
- [ ] `cargo test -p socket-api --test status_resolver_tests` — all 8 tests pass
- [ ] `cargo test -p socket-api --test unix_transport_tests` — 2 tests pass
- [ ] `cargo test -p socket-api --test broadcast_tests` — 5 tests pass
- [ ] `cargo test -p socket-api --test command_status_tests` — all tests pass
- [ ] `cargo test -p socket-api --test command_menu_tests` — all tests pass
- [ ] `cargo test -p socket-api --test command_vfs_tests` — 4 tests pass
- [ ] `cargo test -p socket-api --test command_share_v2_tests` — all tests pass
- [ ] `cargo test -p socket-api --test server_unit_tests` — all tests pass
- [ ] `cargo test -p socket-api --test server_tests` — all integration tests pass
- [ ] `cargo test --workspace` — no regressions
- [ ] All 12 tasks committed individually with descriptive messages
- [ ] No `unwrap()` in library code paths (only in tests)
- [ ] No `todo!()` / `unimplemented!()` in committed code
- [ ] Windows transport file is `#[cfg(target_os = "windows")]` gated and compiles cleanly on Linux/macOS
