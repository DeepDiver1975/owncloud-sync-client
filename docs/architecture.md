# ownCloud Sync Client — Architecture

A Rust reimplementation of the ownCloud desktop sync client targeting oCIS (ownCloud Infinite Scale). The system is a headless sync daemon (`ocsyncd`) and a GUI process (`ocsync`), with shell integrations connecting to the daemon via a text-based socket API.

**Targets:** Windows, macOS, Linux  
**Server backend:** oCIS only (Spaces, Graph API, OIDC mandatory)  
**GUI framework:** iced (pure Rust, cross-platform, Elm architecture)  
**VFS:** Windows CloudFiles API, macOS FileProvider (via Swift + XPC), Linux VFS-off (full download)  
**Shell integration:** Rust (Windows + Linux), Swift (macOS Finder + FileProvider extensions)

---

## Workspace & Crate Structure

```
owncloud-sync/                        (Cargo workspace)
├── crates/
│   ├── ocis-client/                  # oCIS HTTP API client (WebDAV, Graph, OIDC)
│   ├── sync-engine/                  # Core sync algorithm (discovery, reconcile, propagate)
│   ├── sync-db/                      # SQLite journal (per-folder state, metadata, blacklist)
│   ├── vfs-core/                     # Vfs trait definitions
│   ├── vfs-windows/                  # Windows CloudFiles API (windows-rs)
│   ├── vfs-macos/                    # macOS FileProvider XPC bridge (Rust side)
│   ├── vfs-off/                      # No-op — full download, no virtual files (Linux + fallback)
│   ├── socket-api/                   # Socket API server (shell integration IPC)
│   ├── daemon/                       # ocsyncd binary
│   └── gui/                          # ocsync binary (iced)
├── shell-integration/
│   ├── windows/                      # Rust COM DLLs (oc-overlay, oc-contextmenu, oc-ipc)
│   ├── macos/                        # Swift: FinderSync extension + FileProvider extension
│   └── linux/                        # Rust D-Bus service + Nautilus/Dolphin scripts
└── Cargo.toml
```

**Dependency rules:**
- `sync-engine` depends on `ocis-client`, `sync-db`, `vfs-core` — no GUI, no socket, no platform VFS
- `vfs-core` defines traits only; platform crates implement them; `sync-engine` sees only `vfs-core`
- `socket-api` reads sync engine state via `Arc<RwLock<SyncState>>` — never triggers sync
- `daemon` is the only crate that assembles all others
- `gui` depends only on the daemon IPC protocol types, not on `sync-engine` directly

---

## Daemon (`ocsyncd`)

**Runtime:** Single `tokio` async runtime. Blocking OS calls dispatched via `tokio::task::spawn_blocking`.

**Startup sequence:**
1. Acquire user-scoped lock file — prevents duplicate instances
2. Load config (accounts + folders) from platform config dir
3. Initialize `SyncJournalDb` per folder
4. Initialize VFS backend per folder
5. Start socket API server (shell integration socket)
6. Start GUI IPC server
7. Start sync scheduler loop
8. Emit `READY` on GUI IPC socket

**User-space only — no system service.** GUI (`ocsync`) spawns the daemon on startup if needed. Daemon self-exits only on explicit `Quit` command or all accounts removed. Autostart via OS login items only.

**Sync scheduling:**
- File watcher (`notify` crate) triggers debounced sync
- Remote poll interval (default 30s) — PROPFIND ETag check on Space root
- Manual trigger via GUI `TriggerSync` command
- One sync job per folder, serialized; concurrent syncs across different folders allowed

---

## Sync Engine (`sync-engine`)

Three-phase pipeline — pure async Rust, no platform or GUI dependencies.

### Phase 1 — Discovery

- **Remote:** Async breadth-first PROPFIND. Produces `RemoteEntry { path, etag, mtime, size, file_id, permissions }`.
- **Local:** Parallel directory walk via `rayon`. Produces `LocalEntry { path, mtime, size, inode, is_virtual }`.
- **Database:** `SyncJournalDb` provides last-known state as the three-way baseline.

### Phase 2 — Reconciliation

Pure function — no side effects, fully unit-testable:

```rust
fn reconcile(local: Option<LocalEntry>, remote: Option<RemoteEntry>, journal: Option<JournalEntry>) -> SyncInstruction
```

`SyncInstruction`: `Upload | Download | DeleteLocal | DeleteRemote | RenameLocal | RenameRemote | Conflict | UpdateMetadata | Ignore`

Conflict resolution strategy: `ConflictStrategy` — `KeepBoth` (default), `KeepRemote`, `KeepLocal`.

### Phase 3 — Propagation

- Bounded parallelism (default: 3 concurrent transfers)
- Upload: TUS chunked protocol for files ≥ 5 MB, plain PUT otherwise
- Download: streaming GET → temp file → atomic rename on completion
- Each completed instruction immediately updates `SyncJournalDb`
- Errors recorded in `error_blacklist` with exponential backoff before retry

---

## Socket API Server (`socket-api`)

Shell extensions connect to this server using the same wire protocol as the original C++ client.

### Transport

| Platform | Transport |
|---|---|
| Windows | Named pipe `\\.\pipe\ownCloud-{Username}` |
| macOS | Unix socket `~/Library/Group Containers/$(APP_GROUP_ID)/owncloud.sock` |
| Linux | Unix socket `$XDG_RUNTIME_DIR/owncloud/socket` |

### Wire Protocol

Text-based, newline-delimited. Field separator: `\x1e` (ASCII record separator).

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
- `STATUS:tag:path` — file sync status changed (`SYNC | OK | WARNING | ERROR | EXCLUDED | NONE`)
- `UPDATE_VIEW:/path` — shell should refresh directory display

---

## Virtual File System (`vfs-*`)

### Core trait (`vfs-core`)

```rust
trait Vfs: Send + Sync {
    fn create_placeholder(&self, item: &SyncFileItem) -> Result<()>;
    fn hydrate(&self, path: &Utf8Path) -> Result<()>;
    fn dehydrate(&self, path: &Utf8Path) -> Result<()>;
    fn is_virtual(&self, path: &Utf8Path) -> Result<bool>;
    fn status(&self, path: &Utf8Path) -> Result<VfsStatus>;
    fn set_pinned(&self, path: &Utf8Path, pinned: bool) -> Result<()>;
}
```

`sync-engine` holds `Arc<dyn Vfs>` — platform details are invisible to it.

### Windows — CloudFiles API (`vfs-windows`)

`windows-rs` bindings to `CfCreatePlaceholders`, `CfHydratePlaceholder`, `CfDehydratePlaceholder`, `CfUpdatePlaceholder`, `CfSetPinState`. Registers sync root via `CfRegisterSyncRoot` on folder setup.

### macOS — FileProvider (`vfs-macos` + Swift extension)

Apple mandates a FileProvider App Extension (Swift, sandboxed). The Rust `vfs-macos` crate is the Rust side of an XPC bridge. The Swift `FileProvider/` target implements `NSFileProviderReplicatedExtension`.

### Linux — VFS Off (`vfs-off`)

No virtual files on Linux — all files fully downloaded. `create_placeholder`, `set_pinned` are no-ops. Also serves as the fallback if a platform VFS fails to initialize.

---

## oCIS Client (`ocis-client`)

**HTTP client:** `reqwest` + `rustls` (no OpenSSL dependency).

**Authentication:** OIDC PKCE authorization code flow. System browser opened for login; token stored in OS keychain via `keyring` crate (Windows Credential Manager, macOS Keychain, Linux Secret Service).

**TUS resumable upload:** Files ≥ 5 MB use chunked TUS protocol. Upload state persisted in `sync-db` — survives daemon restart.

**Graph API:** `GET /graph/v1.0/me/drives` lists Spaces; WebDAV root per Space is `/dav/spaces/{spaceId}/`.

---

## Configuration

| Platform | Path |
|---|---|
| Windows | `%APPDATA%\ownCloud\owncloud.toml` |
| macOS | `~/Library/Application Support/ownCloud/owncloud.toml` |
| Linux | `$XDG_CONFIG_HOME/owncloud/owncloud.toml` |

```toml
[[account]]
id = "uuid-v4"
url = "https://ocis.example.com"
username = "alice"

[[account.folder]]
id = "uuid-v4"
local_path = "/home/alice/ownCloud"
space_id = "drive-id-from-graph-api"
vfs_mode = "off"   # "off" | "windows_cf" | "macos_fp"
paused = false
```

Sync journal: SQLite at `{local_path}/.sync_{folder_id_hash}.db`.

---

## GUI (`ocsync`)

Elm model-update-view via iced. Connects to daemon on startup (spawning it if absent), subscribes to `DaemonEvent` stream.

No sync logic in the GUI — it only renders daemon state and forwards commands. Single window, show/hide on tray click. macOS: `LSUIElement = true` when window hidden.

---

## Shell Integration

### Windows

Three COM in-process DLLs registered to `HKCU` (no elevation required):
- **`oc-overlay.dll`** — `IShellIconOverlayIdentifier` — 5 overlay icons mapping to sync status tags
- **`oc-contextmenu.dll`** — `IShellExtInit` + `IContextMenu` — right-click submenu
- **`oc-ipc.dll`** — shared named pipe connection helper

### macOS

Two App Extensions inside `ocsync.app`:
- **`FinderSync.appex`** — `FIFinderSync` protocol: file badges, toolbar items, Unix socket via `NWConnection`
- **`FileProvider.appex`** — `NSFileProviderReplicatedExtension`: placeholders, hydration, XPC bridge to daemon

### Linux

- **`oc-dbus-service`** — Rust binary implementing `org.owncloud.FileManager1` D-Bus service
- **`owncloud-nautilus.py`** — Nautilus Python extension: queries D-Bus, sets file emblems
- **`dolphin-owncloud.desktop`** — Dolphin service menu

---

## Key Decisions

| Decision | Rationale |
|---|---|
| oCIS only | Eliminates Basic Auth, clean Graph API, single WebDAV root per Space |
| User-space only, no system service | No privilege escalation, simpler lifecycle |
| Daemon + GUI as separate processes | Crash isolation, headless operation, clean testable boundary |
| iced for GUI | Pure Rust, Elm architecture matches sync state model |
| Swift for macOS extensions | Apple mandates App Extension sandbox — pure Rust impossible |
| Rust COM DLLs for Windows | `windows-rs` makes COM feasible; avoids C++ build chain |
| SQLite via sqlx | Compile-time query checking, async, proven in production |
| reqwest + rustls | No OpenSSL dependency — simpler cross-platform build |
| TUS implemented inline | No suitable Rust crate with the required feature set |
| Linux gets VFS-off only | Suffix VFS dropped; full download is correct default |
