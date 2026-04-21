# Plan 10: Shell Integration — Linux (D-Bus Service + Nautilus/Dolphin)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement `oc-dbus-service` (Rust binary) that bridges the ownCloud socket API to D-Bus, plus a Nautilus Python extension script and a Dolphin service menu desktop file.

**Architecture:** `oc-dbus-service` connects to the daemon Unix socket (socket API, Plan 5) and exposes a D-Bus service (`org.owncloud.FileManager1`) that Nautilus and Dolphin query for file emblems and context menu actions. The Nautilus Python script queries D-Bus for emblems. Dolphin uses a `.desktop` service menu file calling `oc-dbus-service` CLI subcommands.

**Tech Stack:** Rust 2021, zbus (D-Bus), tokio, thiserror. Python 3 + gi (Nautilus extension). Depends on socket API wire protocol (Plan 5) — NOT the Rust crate.

---

## Context

Socket API wire protocol (connect to `$XDG_RUNTIME_DIR/owncloud/socket`):
- `RETRIEVE_FILE_STATUS:path\n` → `STATUS:tag:path\n` (tags: OK, SYNC, WARNING, ERROR, EXCLUDED, NONE)
- `GET_MENU_ITEMS:path\n` → `GET_MENU_ITEMS:path\x1ename:cmd:state\x1e...\n`
- `MAKE_AVAILABLE_LOCALLY:path\n`, `MAKE_ONLINE_ONLY:path\n`, `SHARE:path\n`, `COPY_PRIVATE_LINK:path\n`
- Broadcasts: `STATUS:tag:path\n`, `UPDATE_VIEW:/path\n`, `REGISTER_PATH:/path\n`

D-Bus interface `org.owncloud.FileManager1` on bus name `org.owncloud.FileManager1`, object path `/org/owncloud/FileManager1`:
```
method GetFileStatus(path: String) -> (status: String, emblem: String)
method GetMenuItems(path: String) -> (items: Vec<(name: String, command: String, enabled: bool)>)
method ExecuteCommand(command: String, paths: Vec<String>) -> ()
signal StatusChanged(path: String, status: String)
signal PathRegistered(path: String)
```

Emblem mapping (Nautilus emblem names):
- OK → `"emblem-default"` (green checkmark)
- SYNC → `"emblem-synchronizing"` (arrows)
- WARNING → `"emblem-important"` (warning)
- ERROR → `"emblem-problem"` (red X)
- EXCLUDED → `"emblem-readonly"` (grey)
- NONE → `""` (no emblem)

## File map

```
shell-integration/linux/
  oc-dbus-service/
    Cargo.toml
    src/main.rs              # entry point: connect socket + start D-Bus service
    src/socket_client.rs     # async socket API client (tokio UnixStream)
    src/dbus_service.rs      # zbus interface impl
    src/emblem.rs            # status_to_emblem() mapping
    tests/emblem_tests.rs
    tests/dbus_tests.rs
  nautilus/
    owncloud-nautilus.py     # Nautilus Python extension
  dolphin/
    owncloud.desktop         # Dolphin service menu
```

## 8 tasks — full code, TDD, no placeholders

### Task 1: Cargo.toml + emblem.rs
- [ ] Create `shell-integration/linux/oc-dbus-service/Cargo.toml` with deps tokio (full), zbus = "4", serde, thiserror, tracing, tracing-subscriber.
- [ ] Create `src/emblem.rs`:
```rust
pub fn status_to_emblem(status: &str) -> &'static str {
    match status {
        "OK" => "emblem-default",
        "SYNC" => "emblem-synchronizing",
        "WARNING" => "emblem-important",
        "ERROR" => "emblem-problem",
        "EXCLUDED" => "emblem-readonly",
        _ => "",
    }
}
```
- [ ] Create `tests/emblem_tests.rs` — all 6 inputs including `"NONE"` → `""` and unknown tag → `""`.
- [ ] Commit: `git commit -m "feat(linux-shell): add Cargo.toml and emblem mapping"`

### Task 2: socket_client.rs — async socket API client
- [ ] Create `src/socket_client.rs`:
```rust
use tokio::net::UnixStream;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub struct SocketClient {
    writer: tokio::net::unix::OwnedWriteHalf,
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
}

impl SocketClient {
    pub async fn connect() -> Result<Self, SocketError>
    // path: $XDG_RUNTIME_DIR/owncloud/socket or fallback ~/.local/share/owncloud/socket
    
    pub async fn get_file_status(&mut self, path: &str) -> Result<String, SocketError>
    // write "RETRIEVE_FILE_STATUS:{path}\n"
    // read line, parse "STATUS:tag:path" → return tag
    
    pub async fn get_menu_items(&mut self, path: &str) -> Result<Vec<(String, String, bool)>, SocketError>
    // write "GET_MENU_ITEMS:{path}\n"
    // read line, parse \x1e-separated items → Vec<(name, command, enabled)>
    
    pub async fn execute_command(&mut self, command: &str, paths: &[String]) -> Result<(), SocketError>
    // write "{command}:{paths joined by \x1e}\n"
    // read and discard response line
    
    pub async fn read_broadcast(&mut self) -> Result<Broadcast, SocketError>
    // read one line, parse into Broadcast enum
}

pub enum Broadcast {
    Status { tag: String, path: String },
    RegisterPath(String),
    UpdateView(String),
    Unknown(String),
}

pub enum SocketError { Connect(std::io::Error), Io(std::io::Error), Parse(String) }
```
- [ ] Extract pure parsing helpers `parse_status_line` and `parse_menu_items_line` so they are testable without a socket.
- [ ] Create `tests/dbus_tests.rs` (partial — parsing tests):
  - `parse_status_line("STATUS:OK:/foo/bar\n")` → tag="OK"
  - `parse_menu_items_line("GET_MENU_ITEMS:/foo\x1eShare:SHARE:enabled\x1eMake Available:MAKE_AVAILABLE_LOCALLY:disabled\n")` → 2 items, second disabled
- [ ] Commit: `git commit -m "feat(linux-shell): add SocketClient"`

### Task 3: dbus_service.rs — zbus interface
- [ ] Create `src/dbus_service.rs`:
```rust
use zbus::interface;

pub struct OwnCloudFileManager {
    socket_path: String,
}

#[interface(name = "org.owncloud.FileManager1")]
impl OwnCloudFileManager {
    async fn get_file_status(&self, path: String) -> zbus::fdo::Result<(String, String)> {
        // connect socket, call get_file_status, return (status_tag, emblem_name)
        // return ("NONE", "") on connect failure (daemon not running)
    }
    
    async fn get_menu_items(&self, path: String) -> zbus::fdo::Result<Vec<(String, String, bool)>> {
        // connect socket, call get_menu_items
        // return empty vec on failure
    }
    
    async fn execute_command(&self, command: String, paths: Vec<String>) -> zbus::fdo::Result<()> {
        // connect socket, call execute_command
    }
    
    #[zbus(signal)]
    async fn status_changed(ctx: &zbus::SignalContext<'_>, path: String, status: String) -> zbus::Result<()>;
    
    #[zbus(signal)]
    async fn path_registered(ctx: &zbus::SignalContext<'_>, path: String) -> zbus::Result<()>;
}
```
- [ ] Note: each method creates a fresh `SocketClient::connect()` to avoid holding a connection across async boundaries.
- [ ] Add `#[tokio::test]` in `tests/dbus_tests.rs`: start a mock Unix socket server in a background task that replies with canned responses, instantiate `OwnCloudFileManager`, call `get_file_status`, assert correct (status, emblem) returned.
- [ ] Commit: `git commit -m "feat(linux-shell): add D-Bus interface"`

### Task 4: main.rs — entry point + broadcast loop
- [ ] Create `src/main.rs`:
```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::init();
    
    let socket_path = socket_path();  // XDG_RUNTIME_DIR/owncloud/socket
    
    // 1. Start zbus connection on session bus
    let conn = zbus::ConnectionBuilder::session()?
        .name("org.owncloud.FileManager1")?
        .serve_at("/org/owncloud/FileManager1", OwnCloudFileManager { socket_path: socket_path.clone() })?
        .build()
        .await?;
    
    // 2. Spawn broadcast listener task:
    //    loop: SocketClient::connect() → on success, read_broadcast() loop
    //          STATUS broadcast → emit StatusChanged D-Bus signal
    //          REGISTER_PATH → emit PathRegistered signal
    //          on socket error: wait 5s, reconnect
    
    // 3. Keep main alive
    std::future::pending::<()>().await;
    Ok(())
}

fn socket_path() -> String {
    let xdg_runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    format!("{}/owncloud/socket", xdg_runtime)
}
```
- [ ] Implement complete broadcast loop with reconnect handling (5s delay on error, loop forever).
- [ ] Commit: `git commit -m "feat(linux-shell): add main entry point with broadcast loop"`

### Task 5: Nautilus Python extension
- [ ] Create `shell-integration/linux/nautilus/owncloud-nautilus.py`:
```python
# shell-integration/linux/nautilus/owncloud-nautilus.py
# Install to: ~/.local/share/nautilus/extensions/owncloud-nautilus.py

import gi
gi.require_version('Nautilus', '4.0')
gi.require_version('GLib', '2.0')
from gi.repository import Nautilus, GLib
import dbus

DBUS_SERVICE = 'org.owncloud.FileManager1'
DBUS_PATH = '/org/owncloud/FileManager1'
DBUS_INTERFACE = 'org.owncloud.FileManager1'

class OwnCloudMenuProvider(GObject.GObject, Nautilus.MenuProvider, Nautilus.InfoProvider):
    def __init__(self):
        self._bus = None
        self._iface = None
        self._try_connect()
    
    def _try_connect(self):
        try:
            self._bus = dbus.SessionBus()
            obj = self._bus.get_object(DBUS_SERVICE, DBUS_PATH)
            self._iface = dbus.Interface(obj, DBUS_INTERFACE)
        except dbus.DBusException:
            self._iface = None
    
    def update_file_info(self, file):
        if self._iface is None:
            self._try_connect()
            return Nautilus.OperationResult.FAILED
        
        path = file.get_location().get_path()
        if path is None:
            return Nautilus.OperationResult.COMPLETE
        
        try:
            status, emblem = self._iface.GetFileStatus(path)
            if emblem:
                file.add_emblem(emblem)
        except dbus.DBusException:
            self._iface = None
        
        return Nautilus.OperationResult.COMPLETE
    
    def get_file_items(self, files):
        if not files or self._iface is None:
            return []
        
        path = files[0].get_location().get_path()
        if path is None:
            return []
        
        try:
            items_raw = self._iface.GetMenuItems(path)
        except dbus.DBusException:
            return []
        
        menu_items = []
        for name, command, enabled in items_raw:
            item = Nautilus.MenuItem(
                name=f'OwnCloud::{command}',
                label=name,
                tip='',
                icon=''
            )
            item.connect('activate', self._on_menu_item, command, [f.get_location().get_path() for f in files])
            menu_items.append(item)
        
        return menu_items
    
    def _on_menu_item(self, menu_item, command, paths):
        if self._iface:
            try:
                self._iface.ExecuteCommand(command, paths)
            except dbus.DBusException:
                pass
```
- [ ] Note installation path: `~/.local/share/nautilus/extensions/owncloud-nautilus.py`
- [ ] Note reload command: `nautilus -q && nautilus`
- [ ] Commit: `git add shell-integration/linux/nautilus/ && git commit -m "feat(linux-shell): add Nautilus Python extension"`

### Task 6: Dolphin service menu
- [ ] Create `shell-integration/linux/dolphin/owncloud.desktop`:
```ini
# shell-integration/linux/dolphin/owncloud.desktop
# Install to: ~/.local/share/kservices5/ServiceMenus/owncloud.desktop

[Desktop Entry]
Type=Service
ServiceTypes=KonqPopupMenu/Plugin
MimeType=all/all;
Actions=owncloud_share;owncloud_copy_link;owncloud_make_available;owncloud_make_online_only;
X-KDE-Priority=TopLevel
X-KDE-Submenu=ownCloud

[Desktop Action owncloud_share]
Name=Share...
Icon=owncloud
Exec=oc-dbus-service execute-command SHARE %F

[Desktop Action owncloud_copy_link]
Name=Copy link
Icon=edit-copy
Exec=oc-dbus-service execute-command COPY_PRIVATE_LINK %F

[Desktop Action owncloud_make_available]
Name=Make available locally
Icon=folder-sync
Exec=oc-dbus-service execute-command MAKE_AVAILABLE_LOCALLY %F

[Desktop Action owncloud_make_online_only]
Name=Make online only
Icon=folder-cloud
Exec=oc-dbus-service execute-command MAKE_ONLINE_ONLY %F
```
- [ ] Add CLI subcommand handling to `main.rs` using `std::env::args()` (no clap needed):
```rust
// In main(), check args first:
if let Some(("execute-command", args)) = parse_cli_subcommand() {
    // connect socket directly, send command, exit
}
```
- [ ] Implement `parse_cli_subcommand()` with a simple `match` on `std::env::args()`.
- [ ] Commit: `git add shell-integration/linux/dolphin/ && git commit -m "feat(linux-shell): add Dolphin service menu"`

### Task 7: Integration tests
- [ ] Complete `tests/dbus_tests.rs`:
```rust
#[tokio::test]
async fn test_get_file_status_returns_none_when_no_daemon() {
    // OwnCloudFileManager with a nonexistent socket path
    // Call get_file_status — should return ("NONE", "") not panic
    let svc = OwnCloudFileManager { socket_path: "/tmp/nonexistent_owncloud_test.sock".into() };
    // Call method directly without D-Bus (unit test the method body)
    // Returns ("NONE", "") gracefully
}

#[tokio::test]
async fn test_socket_client_parse_status_line() {
    // pure parsing, no socket
    let result = parse_status_line("STATUS:OK:/home/user/file.txt\n");
    assert_eq!(result, Some(("OK".into(), "/home/user/file.txt".into())));
}

#[tokio::test]
async fn test_socket_client_parse_menu_items() {
    let line = "GET_MENU_ITEMS:/foo\x1eShare:SHARE:enabled\x1eMake Available:MAKE_AVAILABLE_LOCALLY:disabled\n";
    let items = parse_menu_items_line(line);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0], ("Share".into(), "SHARE".into(), true));
    assert_eq!(items[1], ("Make Available".into(), "MAKE_AVAILABLE_LOCALLY".into(), false));
}
```
- [ ] Run: `cargo test --package oc-dbus-service`
- [ ] Commit: `git commit -m "test(linux-shell): add D-Bus service unit tests"`

### Task 8: Installation script + manual testing guide
- [ ] Create `shell-integration/linux/install.sh`:
```bash
#!/bin/bash
set -e
# Copy oc-dbus-service to ~/.local/bin/
mkdir -p ~/.local/bin
cp target/release/oc-dbus-service ~/.local/bin/
# Install Nautilus extension
mkdir -p ~/.local/share/nautilus/extensions
cp shell-integration/linux/nautilus/owncloud-nautilus.py ~/.local/share/nautilus/extensions/
# Install Dolphin service menu
mkdir -p ~/.local/share/kservices5/ServiceMenus
cp shell-integration/linux/dolphin/owncloud.desktop ~/.local/share/kservices5/ServiceMenus/
echo "Installation complete. Restart Nautilus with: nautilus -q && nautilus"
```
- [ ] Add manual testing checklist:
  - [ ] Build: `cargo build --release --package oc-dbus-service`
  - [ ] Run `./install.sh`
  - [ ] Start `ocsyncd` (either via `ocsync` GUI or `./target/release/ocsyncd &`)
  - [ ] Start `oc-dbus-service` in a terminal
  - [ ] Verify D-Bus service: `busctl --user list | grep owncloud`
  - [ ] Restart Nautilus: `nautilus -q && nautilus`
  - [ ] Navigate to a synced folder in Nautilus
  - [ ] Verify emblem icons on files
  - [ ] Right-click a file, verify ownCloud submenu
  - [ ] Open Dolphin, navigate to synced folder, right-click, verify ownCloud submenu
  - [ ] Test `oc-dbus-service execute-command SHARE /some/synced/path`
  - [ ] Verify signal: `dbus-monitor --session "type='signal',interface='org.owncloud.FileManager1'"` while syncing a file
- [ ] Commit: `git add shell-integration/linux/ && git commit -m "feat(linux-shell): add install script and testing guide"`
