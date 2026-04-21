# ownCloud Sync Client — Rust Reimplementation Design

**Date:** 2026-04-20
**Status:** Approved

---

## Overview

A ground-up Rust reimplementation of the ownCloud desktop sync client targeting oCIS (ownCloud Infinite Scale) only. The system is split into a headless sync daemon (`ocsyncd`) and a GUI process (`ocsync` using iced), both running in user space. Shell integrations (file manager overlays, context menus) connect to the daemon via a text-based socket API.

**Targets:** Windows, macOS, Linux
**Server backend:** oCIS only (Spaces, Graph API, OIDC mandatory)
**GUI framework:** iced (pure Rust, cross-platform, Elm architecture)
**VFS:** Windows CloudFiles API, macOS FileProvider (via Swift + XPC), Linux VFS-off (full download)
**Shell integration:** Rust (Windows + Linux), Swift (macOS Finder + FileProvider extensions)

---

## 1. Workspace & Crate Structure

```
owncloud-sync/                        (Cargo workspace)
├── crates/
│   ├── ocis-client/                  # oCIS HTTP API client (WebDAV, Graph, OIDC)
│   ├── sync-engine/                  # Core sync algorithm (discovery, reconcile, propagate)
│   ├── sync-db/                      # SQLite journal (per-folder state, metadata, blacklist)
│   ├── vfs/
│   │   ├── vfs-core/                 # Vfs trait definitions
│   │   ├── vfs-windows/              # Windows CloudFiles API (windows-rs)
│   │   ├── vfs-macos/                # macOS FileProvider XPC bridge (Rust side)
│   │   └── vfs-off/                  # No-op — full download, no virtual files (Linux + fallback)
│   ├── socket-api/                   # Socket API server (shell integration IPC)
│   ├── daemon/                       # ocsyncd binary
│   └── gui/                          # ocsync binary (iced)
├── shell-integration/
│   ├── windows/                      # Rust COM DLLs (oc-overlay, oc-contextmenu, oc-ipc)
│   ├── macos/                        # Swift: FinderSync extension + FileProvider extension
│   └── linux/                        # Rust D-Bus service + Nautilus/Dolphin scripts
└── Cargo.toml
```

**Dependency rules (hard boundaries):**
- `sync-engine` depends on `ocis-client`, `sync-db`, `vfs-core` — no GUI, no socket, no platform VFS
- `vfs-core` defines traits only — platform crates implement them; `sync-engine` sees only `vfs-core`
- `socket-api` reads sync engine state via `Arc<RwLock<SyncState>>` — never triggers sync operations
- `daemon` is the only crate that assembles all others
- `gui` depends only on the daemon IPC protocol types, not on `sync-engine` directly

---

## 2. Daemon (`ocsyncd`) Architecture & Lifecycle

**Runtime:** Single `tokio` async runtime. All sync, network I/O, and socket API handling are async tasks. Blocking OS calls (VFS, file I/O) dispatched via `tokio::task::spawn_blocking`.

**Startup sequence:**
1. Acquire user-scoped lock file — prevents duplicate instances
   - Windows: `%LOCALAPPDATA%\ownCloud\ocsyncd.lock`
   - macOS: `~/Library/Application Support/ownCloud/ocsyncd.lock`
   - Linux: `$XDG_RUNTIME_DIR/owncloud/ocsyncd.lock`
2. Load config (accounts + folders) from platform config dir
3. Initialize `SyncJournalDb` for each folder
4. Initialize VFS backend for each folder
5. Start socket API server (shell integration socket — paths in Section 4)
6. Start daemon IPC server (GUI socket)
   - Windows: `\\.\pipe\ownCloud-GUI-{Username}`
   - macOS: `~/Library/Application Support/ownCloud/daemon-gui.sock`
   - Linux: `$XDG_RUNTIME_DIR/owncloud/daemon-gui.sock`
7. Start sync scheduler loop
8. Emit `READY` on daemon IPC socket

**User-space only — no system service:**
- `ocsyncd` is a plain user process, not registered with systemd/launchd/Windows SCM
- GUI (`ocsync`) spawns daemon on startup if lock file absent or IPC ping fails
- Daemon runs independently of GUI — GUI exit does not kill daemon
- Daemon self-exits only on explicit `Quit` command or all accounts removed
- Autostart via OS login items only (Windows startup registry `HKCU`, macOS Login Items, Linux `~/.config/autostart/`)

**Sync scheduling:**
- File watcher (`notify` crate — wraps ReadDirectoryChangesW / FSEvents / inotify) triggers debounced sync
- Remote poll interval (default 30s) — PROPFIND ETag check on Space root
- Manual trigger via GUI `TriggerSync` command
- One sync job per folder, serialized — concurrent syncs across different folders are allowed

---

## 3. Sync Engine (`sync-engine` crate)

Three-phase pipeline — pure async Rust, no platform or GUI dependencies.

### Phase 1 — Discovery

- **Remote:** Async breadth-first PROPFIND against oCIS WebDAV Space root. Produces `RemoteEntry { path, etag, mtime, size, file_id, permissions }`.
- **Local:** Parallel directory walk via `tokio::task::spawn_blocking` + `rayon`. Produces `LocalEntry { path, mtime, size, inode, is_virtual }`.
- **Database:** `SyncJournalDb` provides last-known state as the three-way baseline.
- **Output:** `(local, remote, journal)` triple per path.

### Phase 2 — Reconciliation

Pure function — no side effects, fully unit-testable:

```rust
fn reconcile(local: Option<LocalEntry>, remote: Option<RemoteEntry>, journal: Option<JournalEntry>) -> SyncInstruction
```

`SyncInstruction` enum: `Upload | Download | DeleteLocal | DeleteRemote | RenameLocal | RenameRemote | Conflict | UpdateMetadata | Ignore`

- Conflict resolution: `ConflictStrategy` — `KeepBoth` (default), `KeepRemote`, `KeepLocal`
- Rename detection: inode matching (local) + `file_id` matching (remote) — avoids re-uploading moved files

### Phase 3 — Propagation

- Executes instructions with configurable parallelism (default: 3 concurrent transfers)
- Upload: TUS chunked protocol for files ≥ 5 MB, plain PUT otherwise
- Download: streaming GET → temp file → atomic rename on completion
- Each completed instruction immediately updates `SyncJournalDb`
- Errors recorded in `error_blacklist` — exponential backoff before retry
- Emits `SyncProgress` / `FileStatusChanged` events to daemon event bus

**Core type:**
```rust
struct SyncFileItem {
    path: Utf8PathBuf,
    instruction: SyncInstruction,
    direction: Direction,       // Up | Down | None
    etag: Option<String>,
    size: u64,
    mtime: SystemTime,
    file_id: Option<String>,    // oCIS file ID for rename detection
    checksum: Option<Checksum>,
    error: Option<SyncError>,
}
```

---

## 4. Socket API Server (`socket-api` crate)

The IPC interface that shell extensions connect to. Implements the same wire protocol as the original C++ client — existing protocol knowledge is preserved.

### Transport

| Platform | Transport |
|---|---|
| Windows | Named pipe `\\.\pipe\ownCloud-{Username}` via `tokio::net::windows::named_pipe` |
| macOS | Unix socket `~/Library/Group Containers/$(APP_GROUP_ID)/owncloud.sock` — App Group ID set at build time, shared between daemon and Finder extension |
| Linux | Unix socket `$XDG_RUNTIME_DIR/owncloud/socket` |

### Wire Protocol

- Text-based, newline-delimited (`\n`)
- Field separator: `\x1e` (ASCII record separator)
- Client → server: `COMMAND:argument`
- Server → client: `COMMAND:result:path`
- V2 commands: `V2/COMMAND_NAME\n{"id":"1","arguments":{...}}\n`

### Commands

| Command | Description |
|---|---|
| `VERSION` | Protocol handshake |
| `GET_STRINGS` | Localized context menu strings |
| `GET_MENU_ITEMS:path` | Available actions for path |
| `RETRIEVE_FILE_STATUS:path` | Sync status for a file |
| `RETRIEVE_FOLDER_STATUS:path` | Sync status for a folder |
| `SHARE:path` | Open share dialog |
| `MAKE_AVAILABLE_LOCALLY:p1\x1ep2` | Hydrate virtual files |
| `MAKE_ONLINE_ONLY:p1\x1ep2` | Dehydrate to virtual |
| `COPY_PRIVATE_LINK:path` | Copy link to clipboard |
| `V2/GET_CLIENT_ICON` | Base64 PNG icon |

### Server-initiated broadcasts

- `REGISTER_PATH:/sync/root` — on startup, for each active sync folder
- `UNREGISTER_PATH:/sync/root` — when folder removed
- `STATUS:tag:path` — file sync status changed
- `UPDATE_VIEW:/path` — shell should refresh directory display

**Status tags:** `SYNC | OK | WARNING | ERROR | EXCLUDED | NONE` — with optional `+SHARED` suffix.

### Internal structure

```
SocketApiServer
  ├── Listener                  # per-platform transport accept loop
  ├── ConnectionSet             # tracks all active shell extension connections
  ├── CommandDispatcher         # routes incoming text commands to handlers
  ├── StatusResolver            # reads Arc<RwLock<SyncState>> — read-only, never triggers sync
  └── BroadcastSender           # sends STATUS/UPDATE_VIEW to relevant connections
```

---

## 5. Virtual File System (`vfs-*` crates)

### Core trait (`vfs-core`)

```rust
trait Vfs: Send + Sync {
    fn create_placeholder(&self, item: &SyncFileItem) -> Result<()>;
    fn update_placeholder(&self, item: &SyncFileItem) -> Result<()>;
    fn hydrate(&self, path: &Utf8Path) -> Result<()>;
    fn dehydrate(&self, path: &Utf8Path) -> Result<()>;
    fn is_virtual(&self, path: &Utf8Path) -> Result<bool>;
    fn status(&self, path: &Utf8Path) -> Result<VfsStatus>;
    fn set_pinned(&self, path: &Utf8Path, pinned: bool) -> Result<()>;
}

enum VfsStatus { Placeholder, Hydrated, Hydrating, Dehydrating }
```

`sync-engine` holds `Arc<dyn Vfs>` — platform details invisible.

### Windows — CloudFiles API (`vfs-windows`)

- `windows-rs` bindings to `CfCreatePlaceholders`, `CfHydratePlaceholder`, `CfDehydratePlaceholder`, `CfUpdatePlaceholder`, `CfSetPinState`
- Registers sync root via `CfRegisterSyncRoot` on folder setup
- On-demand hydration: CloudFiles OS callback → `spawn_blocking` → download → `CfHydratePlaceholder`
- Pin state stored as file attributes (`FILE_ATTRIBUTE_PINNED/UNPINNED`) — no DB needed

### macOS — FileProvider (`vfs-macos` + Swift extension)

- Apple mandates a FileProvider App Extension (Swift/Obj-C only — sandboxed, no pure Rust possible)
- Rust daemon ↔ Swift extension: XPC over App Group shared container
- `vfs-macos` crate: Rust side of XPC bridge — serializes commands, deserializes hydration callbacks
- Swift `FileProvider/` target: `NSFileProviderReplicatedExtension` implementation
- Hydration flow: FileProvider request → XPC → daemon downloads → XPC callback → FileProvider signals OS completion
- Lives in `shell-integration/macos/FileProvider/`

### Linux — VFS Off (`vfs-off`)

- No virtual files on Linux — all files fully downloaded
- `create_placeholder`, `update_placeholder`, `set_pinned` are no-ops returning `Ok(())`
- `is_virtual` always returns `false`
- Also serves as the fallback implementation if a platform VFS fails to initialize

### Socket API integration

- `MAKE_AVAILABLE_LOCALLY` → `vfs.hydrate(path)` per file
- `MAKE_ONLINE_ONLY` → `vfs.dehydrate(path)` per file
- Status queries check `vfs.is_virtual(path)` to select correct status tag

---

## 6. oCIS Client (`ocis-client` crate)

Pure network layer — no sync logic.

**HTTP client:** `reqwest` + `rustls` (no OpenSSL dependency). Connection pool per account.

**Authentication — OIDC only:**
- PKCE authorization code flow via `/.well-known/openid-configuration` discovery
- System browser opened for login; local redirect URI captures callback
- Token storage: OS keychain via `keyring` crate (Windows Credential Manager, macOS Keychain, Linux Secret Service / kwallet)
- `reqwest` middleware intercepts 401s, refreshes token, retries transparently

**WebDAV operations:**
```rust
trait WebDavClient {
    async fn propfind(&self, path: &str, depth: Depth) -> Result<Vec<DavEntry>>;
    async fn get(&self, path: &str) -> Result<impl AsyncRead>;
    async fn put(&self, path: &str, data: impl AsyncRead, size: u64) -> Result<()>;
    async fn delete(&self, path: &str) -> Result<()>;
    async fn mkcol(&self, path: &str) -> Result<()>;
    async fn move_(&self, from: &str, to: &str, overwrite: bool) -> Result<()>;
}
```

**TUS resumable upload:**
- Files ≥ 5 MB (configurable) use chunked TUS protocol
- Upload state (upload ID, byte offset) persisted in `sync-db` — survives daemon restart
- Implemented inline (no suitable existing Rust crate)

**Graph API (oCIS Spaces):**
- `GET /graph/v1.0/me/drives` — list Spaces for account setup
- `GET /graph/v1.0/drives/{driveId}` — Space metadata and quota
- WebDAV root per Space: `/dav/spaces/{spaceId}/`

---

## 7. Configuration & Persistence

### Config file

| Platform | Path |
|---|---|
| Windows | `%APPDATA%\ownCloud\owncloud.toml` |
| macOS | `~/Library/Application Support/ownCloud/owncloud.toml` |
| Linux | `$XDG_CONFIG_HOME/owncloud/owncloud.toml` |

Format: TOML, managed via `config` crate + `serde`. Supports multiple accounts natively via TOML array-of-tables.

```toml
[general]
log_level = "info"
notification_enabled = true

[[account]]
id = "uuid-v4"
url = "https://ocis.example.com"
username = "alice"
display_name = "Alice"
# credentials in keychain, keyed by account.id

[[account.folder]]
id = "uuid-v4"
local_path = "/home/alice/ownCloud"
space_id = "drive-id-from-graph-api"
display_name = "Personal"
selective_sync_excluded = ["large-videos/"]
vfs_mode = "off"           # "off" | "windows_cf" | "macos_fp"
paused = false
```

### Sync journal database (`sync-db` crate)

- SQLite via `sqlx` with compile-time checked queries
- Location: `{local_path}/.sync_{folder_id_hash}.db`
- Migrations in `sync-db/migrations/` — run automatically on open

| Table | Purpose |
|---|---|
| `metadata` | path, etag, mtime, size, inode, file_id, checksum, is_virtual |
| `upload_info` | TUS upload ID, byte offset — for resumable uploads |
| `error_blacklist` | path, error_count, last_error, retry_after |
| `selective_sync` | excluded paths |
| `schema_version` | migration tracking |

### Daemon ↔ GUI IPC protocol

Length-prefixed JSON over Unix socket / named pipe. Types defined in a shared sub-crate (`daemon/src/protocol/`).

```rust
enum DaemonCommand {
    Subscribe,
    TriggerSync { folder_id: Uuid },
    PauseFolder { folder_id: Uuid },
    ResumeFolder { folder_id: Uuid },
    AddAccount { url: String },
    RemoveAccount { account_id: Uuid },
    Quit,
}

enum DaemonEvent {
    SyncStarted { folder_id: Uuid },
    SyncProgress { folder_id: Uuid, done: u64, total: u64 },
    SyncFinished { folder_id: Uuid, errors: Vec<SyncError> },
    FileStatusChanged { path: PathBuf, status: FileStatus },
    AccountStateChanged { account_id: Uuid, state: AccountState },
}
```

---

## 8. GUI (`ocsync` — iced)

**Architecture:** Elm model-update-view. Connects to daemon on startup (spawning it if absent), subscribes to `DaemonEvent` stream via `iced::subscription::unfold`.

```rust
struct App {
    daemon: DaemonConnection,
    accounts: Vec<AccountView>,
    active_view: View,
    tray: TrayHandle,
}

enum View {
    SyncStatus,
    AccountSettings(Uuid),
    AddAccount,
    GeneralSettings,
}
```

**Tray:** `tray-icon` crate — cross-platform. Icon reflects aggregate sync state (idle / syncing / error). Left-click: show/hide window. Right-click menu: per-folder pause/resume, open folder, open web UI, quit.

**Main window — sync status:**
- Account list → folder list (space name, local path, last sync, progress bar, error badge)
- Toolbar: pause all, resume all, force sync

**Add account wizard:**
- Enter server URL → daemon fetches OIDC discovery → system browser for OAuth2 → daemon receives callback → Spaces listed → user selects Spaces + local paths

**Key constraints:**
- No sync logic in GUI — renders daemon state only
- Commands sent via `iced::Command::perform(daemon.send(cmd))`
- Single window, show/hide on tray click (not open/close)
- macOS: `LSUIElement = true` — no Dock icon when window hidden

---

## 9. Shell Integration

### Windows (`shell-integration/windows/` — Rust, `windows-rs`)

Three COM in-process DLLs:

- **`oc-overlay.dll`** — `IShellIconOverlayIdentifier` — 5 overlay icons (Synced, Syncing, Error, Warning, Excluded). Connects to named pipe, calls `RETRIEVE_FILE_STATUS`, maps status tag to overlay index.
- **`oc-contextmenu.dll`** — `IShellExtInit` + `IContextMenu` — right-click submenu (Share, Copy Link, Make Available Locally, Make Online Only). Calls `GET_MENU_ITEMS`, executes user selection via socket command.
- **`oc-ipc.dll`** — shared named pipe connection + message framing helper.

Registration: `regsvr32` to `HKCU` — no elevation required.

### macOS (`shell-integration/macos/` — Swift)

Two App Extensions inside `ocsync.app`:

- **`FinderSync.appex`** — `FINCFinderSync` protocol. File badges (Synced/Syncing/Error) via `FIBadgeIdentifier`. Toolbar items for Share / Make Available / Make Online Only. Connects to Unix socket via Swift `NWConnection`.
- **`FileProvider.appex`** — `NSFileProviderReplicatedExtension`. Placeholder creation, hydration requests, conflict reporting. Communicates with daemon via XPC (App Group shared container). This is the macOS VFS backend.

### Linux (`shell-integration/linux/` — Rust + scripts)

- **`oc-dbus-service`** — Rust binary, implements `org.owncloud.FileManager1` D-Bus service. Translates D-Bus queries (emblems, menu items) to socket API commands against the daemon Unix socket.
- **`owncloud-nautilus.py`** — Python shim for Nautilus: queries D-Bus service, sets file emblems.
- **`dolphin-owncloud.desktop`** — Dolphin service menu: calls `oc-dbus-service` CLI for actions.

---

## Constraints & Key Decisions

| Decision | Rationale |
|---|---|
| oCIS only (no classic ownCloud) | Eliminates Basic Auth, single WebDAV root per Space, clean Graph API |
| User-space only, no system service | Simpler lifecycle, no privilege escalation, standard for desktop apps |
| Daemon + GUI as separate processes | Crash isolation, headless operation, clean testable boundary |
| iced for GUI | Pure Rust, Elm architecture matches sync state model, active community |
| Swift for macOS Finder + FileProvider | Apple mandates App Extension sandbox — pure Rust impossible |
| Rust COM DLLs for Windows shell integration | `windows-rs` makes COM feasible in Rust; avoids C++ build chain |
| SQLite via sqlx | Compile-time query checking, async support, proven in production |
| reqwest + rustls | No OpenSSL dependency — simpler cross-platform build |
| Suffix VFS removed | Legacy feature dropped in C++ client; Linux gets VFS-off (full download) |
| TUS protocol implemented inline | No suitable Rust crate exists for TUS with the required feature set |
