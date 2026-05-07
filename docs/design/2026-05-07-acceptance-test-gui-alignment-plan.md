# Acceptance Test GUI Alignment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rewrite the acceptance tests to drive the Iced GUI through AT-SPI2 (as a real user would), while using daemon IPC events only as completion signals, and fix the GUI's accessible names so AT-SPI2 can find widgets reliably.

**Architecture:** Two sequenced parts — (1) collapse multi-child nav buttons in `main.rs` to single text nodes so AccessKit emits stable accessible names, (2) rewrite `fixture.rs::add_account()` to click through the GUI via `AtSpiClient` rather than sending commands directly to the daemon. Daemon IPC stays as an observation channel, not an input channel.

**Tech Stack:** Rust, Iced 0.13, atspi 0.29, atspi-common 0.13, AccessKit (via Iced's `test-accessibility` feature), Playwright (Node.js), tokio

---

## File Map

| File | What changes |
|---|---|
| `crates/gui/src/main.rs` | Collapse multi-child nav buttons to single text nodes |
| `crates/acceptance-test/src/atspi_client.rs` | Add `dump_tree()` method; update `wait_for_widget` to log tree on timeout |
| `crates/acceptance-test/src/fixture.rs` | Add `atspi: AtSpiClient` field; rewrite `add_account()` to drive GUI |
| `crates/acceptance-test/tests/account_setup.rs` | Add AT-SPI display-name assertion after account setup |
| `crates/acceptance-test/tests/duplicate_account.rs` | Drive second OIDC attempt through GUI; assert GUI returns to AddAccount view |

---

## Task 1: Fix nav button accessible names in the GUI

**Files:**
- Modify: `crates/gui/src/main.rs`

Iced's AccessKit bridge derives a button's accessible name from its inner widget content. A `button(row![text("☁"), text("Sync Status")])` may produce an unpredictable concatenated name or an empty name. Collapsing each nav button to a single `text(...)` child gives it a deterministic accessible name.

- [ ] **Step 1: Replace the three nav buttons in `main.rs`**

Replace the `nav_sync` definition (the `let nav_sync = iced::widget::button(row![...])` block):

```rust
let nav_sync = iced::widget::button(
    text("☁ Sync Status")
        .size(12)
        .style(theme::colored_text(if is_sync {
            theme::ACCENT
        } else {
            theme::TEXT_SECONDARY
        })),
)
.on_press(Message::NavigateTo(View::SyncStatus))
.width(Length::Fill)
.padding([7, 9])
.style(if is_sync {
    theme::nav_active_style
} else {
    theme::nav_button_style
});
```

Replace the `nav_add` definition:

```rust
let nav_add = iced::widget::button(
    text("+ Add Account")
        .size(12)
        .style(theme::colored_text(if is_add {
            theme::ACCENT
        } else {
            theme::TEXT_SECONDARY
        })),
)
.on_press(Message::NavigateTo(View::AddAccount {
    url_input: String::new(),
    error: None,
}))
.width(Length::Fill)
.padding([7, 9])
.style(if is_add {
    theme::nav_active_style
} else {
    theme::nav_button_style
});
```

Replace the `nav_settings` definition:

```rust
let nav_settings = iced::widget::button(
    text("⚙ Settings")
        .size(12)
        .style(theme::colored_text(if is_settings {
            theme::ACCENT
        } else {
            theme::TEXT_SECONDARY
        })),
)
.on_press(Message::NavigateTo(View::GeneralSettings))
.width(Length::Fill)
.padding([7, 9])
.style(if is_settings {
    theme::nav_active_style
} else {
    theme::nav_button_style
});
```

- [ ] **Step 2: Verify it compiles**

```bash
cargo build -p gui
```

Expected: no errors.

- [ ] **Step 3: Run cargo fmt and commit**

```bash
cargo fmt -p gui
git add crates/gui/src/main.rs
git commit -s -m "fix(gui): use single text node per nav button for stable AT-SPI accessible names"
```

---

## Task 2: Add `dump_tree()` to `AtSpiClient` and update `wait_for_widget`

**Files:**
- Modify: `crates/acceptance-test/src/atspi_client.rs`

`dump_tree()` walks the entire AT-SPI2 accessible tree from the registry root and returns a formatted string of every `(role, name)` pair. `wait_for_widget` calls it on timeout so role/name mismatches are debuggable without an external inspector.

- [ ] **Step 1: Add `dump_tree()` method to `AtSpiClient`**

Append this method inside the `impl AtSpiClient` block (after `set_text`):

```rust
/// Walk the full accessible tree and return a formatted dump of every node.
/// Called automatically by [`wait_for_widget`] on timeout.
pub async fn dump_tree(&self) -> String {
    let zconn = self.conn.connection();
    let mut output = String::new();

    let registry_root = match self.conn.root_accessible_on_registry().await {
        Ok(r) => r,
        Err(e) => return format!("dump_tree: registry root error: {e}"),
    };

    let app_refs = match registry_root.get_children().await {
        Ok(children) => children,
        Err(e) => return format!("dump_tree: children error: {e}"),
    };

    let mut queue: VecDeque<(ObjectRefOwned, usize)> =
        app_refs.into_iter().map(|r| (r, 0)).collect();
    let mut visited: HashSet<(String, String)> = HashSet::new();

    while let Some((obj_ref, depth)) = queue.pop_front() {
        if obj_ref.is_null() {
            continue;
        }

        let key = (
            obj_ref.name_as_str().unwrap_or("").to_string(),
            obj_ref.path_as_str().to_string(),
        );
        if !visited.insert(key) {
            continue;
        }

        let proxy: AccessibleProxy<'_> = match obj_ref.as_accessible_proxy(zconn).await {
            Ok(p) => p,
            Err(_) => continue,
        };

        let role = proxy.get_role().await.unwrap_or(Role::Invalid);
        let name = proxy.name().await.unwrap_or_default();
        let indent = "  ".repeat(depth);
        output.push_str(&format!("{indent}[{role:?}] {name:?}\n"));

        if let Ok(children) = proxy.get_children().await {
            for child in children {
                if !child.is_null() {
                    queue.push_back((child, depth + 1));
                }
            }
        }
    }

    output
}
```

- [ ] **Step 2: Update `wait_for_widget` to dump the tree on timeout**

Replace the timeout arm in `wait_for_widget`:

```rust
pub async fn wait_for_widget(
    &self,
    role: Role,
    name: &str,
    timeout: Duration,
) -> Result<ObjectRefOwned> {
    let deadline = Instant::now() + timeout;
    loop {
        match self.find_widget(role, name).await {
            Ok(widget) => return Ok(widget),
            Err(_) => {
                let now = Instant::now();
                if now >= deadline {
                    let tree = self.dump_tree().await;
                    return Err(anyhow!(
                        "timed out waiting for widget with role {:?} and name {:?}\nAccessible tree:\n{}",
                        role,
                        name,
                        tree
                    ));
                }
                let remaining = deadline - now;
                tokio::time::sleep(remaining.min(Duration::from_millis(250))).await;
            }
        }
    }
}
```

- [ ] **Step 3: Verify it compiles**

```bash
cargo build -p acceptance-test
```

Expected: no errors.

- [ ] **Step 4: Run cargo fmt and commit**

```bash
cargo fmt -p acceptance-test
git add crates/acceptance-test/src/atspi_client.rs
git commit -s -m "feat(acceptance): add dump_tree() to AtSpiClient; log accessible tree on wait_for_widget timeout"
```

---

## Task 3: Wire `AtSpiClient` into `TestEnvironment`

**Files:**
- Modify: `crates/acceptance-test/src/fixture.rs`

`TestEnvironment` gains a `pub atspi: AtSpiClient` field so tests can drive the GUI. It is constructed after the GUI is spawned and `ScreenReaderEnabled` is set to `true`.

- [ ] **Step 1: Add imports to `fixture.rs`**

Add these two lines to the existing `use` block at the top of `fixture.rs`:

```rust
use atspi::Role;
use crate::atspi_client::AtSpiClient;
```

- [ ] **Step 2: Add `atspi` field to `TestEnvironment`**

Replace the struct definition:

```rust
pub struct TestEnvironment {
    pub ocis_url: Url,
    pub sync_dir: TempDir,
    pub config_dir: TempDir,
    pub daemon_ipc: DaemonIpcClient,
    pub atspi: AtSpiClient,
    pub ocis_client: OcisClient,
    pub daemon_stdout: Lines<BufReader<tokio::process::ChildStdout>>,
    daemon: Child,
    gui: Child,
    atspi_bus: Child,
}
```

- [ ] **Step 3: Instantiate `AtSpiClient` in `TestEnvironment::start()`**

In `start()`, the last two lines before the `Ok(Self { ... })` return currently are:

```rust
tokio::time::sleep(Duration::from_secs(2)).await;
set_screen_reader(true);
tokio::time::sleep(Duration::from_millis(500)).await;
```

Add `AtSpiClient::connect()` after the final sleep:

```rust
tokio::time::sleep(Duration::from_secs(2)).await;
set_screen_reader(true);
tokio::time::sleep(Duration::from_millis(500)).await;

let atspi = AtSpiClient::connect()
    .await
    .context("failed to connect to AT-SPI2 accessibility bus")?;
```

- [ ] **Step 4: Add `atspi` to the `Ok(Self { ... })` return**

```rust
Ok(Self {
    ocis_url: Url::parse(OCIS_URL)?,
    sync_dir,
    config_dir,
    daemon_ipc,
    atspi,
    ocis_client,
    daemon_stdout,
    daemon,
    gui,
    atspi_bus,
})
```

- [ ] **Step 5: Verify it compiles**

```bash
cargo build -p acceptance-test
```

Expected: no errors.

- [ ] **Step 6: Run cargo fmt and commit**

```bash
cargo fmt -p acceptance-test
git add crates/acceptance-test/src/fixture.rs
git commit -s -m "feat(acceptance): add AtSpiClient field to TestEnvironment"
```

---

## Task 4: Rewrite `add_account()` to drive the GUI via AT-SPI2

**Files:**
- Modify: `crates/acceptance-test/src/fixture.rs`

The new `add_account()` is dual-channel: AT-SPI2 drives all user input (clicking buttons, typing into text fields), daemon IPC events are used only to wait for async operations to complete (OIDC started, OIDC completed, folder registered).

The role used for `text_input` in Iced's AccessKit bridge is likely `Role::Entry`. If the acceptance test fails to find the Entry widget, the `dump_tree()` output in the error message will show the actual role — adjust the constant accordingly.

- [ ] **Step 1: Replace `add_account()` in `fixture.rs`**

Replace the entire `add_account` method body:

```rust
/// Runs the full account-setup flow by driving the GUI through AT-SPI2.
/// Daemon IPC events are used only as completion signals.
pub async fn add_account(&mut self) -> Result<()> {
    // 1. Click "Add Account" in the nav sidebar.
    let add_btn = self
        .atspi
        .wait_for_widget(Role::Button, "Add Account", Duration::from_secs(10))
        .await
        .context("Add Account nav button not found")?;
    self.atspi
        .click(&add_btn)
        .await
        .context("failed to click Add Account")?;

    // 2. Type the server URL into the text field (found by its placeholder text).
    let url_field = self
        .atspi
        .wait_for_widget(Role::Entry, "https://your.server.com", Duration::from_secs(5))
        .await
        .context("server URL text input not found")?;
    self.atspi
        .set_text(&url_field, self.ocis_url.as_str())
        .await
        .context("failed to set server URL")?;

    // 3. Click "Connect →".
    let connect_btn = self
        .atspi
        .wait_for_widget(Role::Button, "Connect →", Duration::from_secs(5))
        .await
        .context("Connect button not found")?;
    self.atspi
        .click(&connect_btn)
        .await
        .context("failed to click Connect")?;

    // 4. Wait for daemon to confirm OIDC flow started.
    self.daemon_ipc
        .wait_for(
            |e| matches!(e, DaemonEvent::AccountAddStarted { .. }),
            Duration::from_secs(15),
        )
        .await
        .ok_or_else(|| anyhow!("AccountAddStarted not received"))?;

    // 5. Read the OIDC authorization URL from daemon stdout.
    let auth_url = self.wait_for_oidc_url().await?;

    let callback_port = auth_url
        .query_pairs()
        .find_map(|(k, v)| {
            if k == "redirect_uri" {
                url::Url::parse(&v).ok().and_then(|u| u.port())
            } else {
                None
            }
        })
        .ok_or_else(|| anyhow!("could not extract callback port from redirect_uri"))?;

    // 6. Complete OIDC login in headless browser.
    complete_oidc_login(&auth_url, callback_port, "admin", "admin")
        .await
        .context("Playwright OIDC login failed")?;

    // 7. Wait for daemon to confirm OIDC completed and account saved.
    self.daemon_ipc
        .wait_for(
            |e| matches!(e, DaemonEvent::AccountAddCompleted { .. }),
            Duration::from_secs(30),
        )
        .await
        .ok_or_else(|| anyhow!("AccountAddCompleted not received"))?;

    // 8. Type the local sync folder path.
    let sync_path = self.sync_dir.path().to_string_lossy().into_owned();
    let folder_field = self
        .atspi
        .wait_for_widget(Role::Entry, "~/ownCloud", Duration::from_secs(10))
        .await
        .context("folder path text input not found")?;
    self.atspi
        .set_text(&folder_field, &sync_path)
        .await
        .context("failed to set folder path")?;

    // 9. Click "Start Syncing".
    let sync_btn = self
        .atspi
        .wait_for_widget(Role::Button, "Start Syncing", Duration::from_secs(5))
        .await
        .context("Start Syncing button not found")?;
    self.atspi
        .click(&sync_btn)
        .await
        .context("failed to click Start Syncing")?;

    // 10. Wait for daemon to confirm folder registered.
    self.daemon_ipc
        .wait_for(
            |e| matches!(e, DaemonEvent::AccountFolderAdded { .. }),
            Duration::from_secs(30),
        )
        .await
        .ok_or_else(|| anyhow!("AccountFolderAdded not received"))?;

    Ok(())
}
```

- [ ] **Step 2: Verify it compiles**

```bash
cargo build -p acceptance-test
```

Expected: no errors.

- [ ] **Step 3: Run cargo fmt and commit**

```bash
cargo fmt -p acceptance-test
git add crates/acceptance-test/src/fixture.rs
git commit -s -m "feat(acceptance): rewrite add_account() to drive GUI via AT-SPI2"
```

---

## Task 5: Extend `account_setup.rs` with AT-SPI display-name assertion

**Files:**
- Modify: `crates/acceptance-test/tests/account_setup.rs`

After `add_account()` the GUI should be on the SyncStatus view showing the account's display name. This test adds an AT-SPI assertion that the display name is actually visible in the GUI — this is the assertion that was impossible before and that catches the original bug.

The `text()` widget role in Iced's AT-SPI bridge is most likely `Role::Label`. If the test fails with "timed out", check the `dump_tree()` output in the failure message and adjust the role constant.

- [ ] **Step 1: Replace `account_setup.rs` with the extended version**

```rust
use acceptance_test::fixture::TestEnvironment;
use atspi::Role;
use daemon::config::AppConfig;
use std::time::Duration;

#[tokio::test]
async fn test_account_setup() {
    if std::env::var("OCIS_ACCEPTANCE").is_err() {
        eprintln!("Skipping test_account_setup: OCIS_ACCEPTANCE not set");
        return;
    }

    let mut env = TestEnvironment::start()
        .await
        .expect("failed to start TestEnvironment");

    env.add_account()
        .await
        .expect("account setup via OIDC failed");

    // Assert: config file has exactly 1 account with non-empty user_id and 1 folder.
    let config_path = env.config_dir.path().join("owncloud").join("owncloud.toml");
    let cfg = AppConfig::load(&config_path).expect("failed to load config after add_account");

    assert_eq!(cfg.account.len(), 1, "expected exactly 1 account in config");
    let account = &cfg.account[0];
    assert!(
        !account.user_id.is_empty(),
        "expected user_id to be non-empty"
    );
    assert_eq!(
        account.folder.len(),
        1,
        "expected exactly 1 folder in account"
    );

    // Assert: the account display name is visible in the GUI SyncStatus view.
    // This is the key assertion: it confirms the GUI actually updated, not just the config.
    // If this fails with "timed out", check the dump_tree() output in the error message
    // and update Role::Label to the role the AT-SPI bridge emits for Iced text() widgets.
    env.atspi
        .wait_for_widget(Role::Label, &account.display_name, Duration::from_secs(10))
        .await
        .expect("account display name not visible in SyncStatus view");
}
```

- [ ] **Step 2: Verify it compiles**

```bash
cargo build -p acceptance-test
```

Expected: no errors.

- [ ] **Step 3: Run cargo fmt and commit**

```bash
cargo fmt -p acceptance-test
git add crates/acceptance-test/tests/account_setup.rs
git commit -s -m "feat(acceptance): assert account display name visible in GUI after account setup"
```

---

## Task 6: Rewrite `duplicate_account.rs` to drive second attempt via GUI

**Files:**
- Modify: `crates/acceptance-test/tests/duplicate_account.rs`

The second account attempt is now driven through the GUI: click "Add Account", enter URL, click "Connect →", complete OIDC. After the daemon rejects the duplicate, the GUI should return to the AddAccount view. The final assertion checks that the URL input field is visible again (confirming the GUI surfaced the error and reverted).

- [ ] **Step 1: Replace `duplicate_account.rs` with the GUI-driven version**

```rust
use acceptance_test::fixture::TestEnvironment;
use acceptance_test::playwright::complete_oidc_login;
use atspi::Role;
use daemon::gui_ipc::protocol::DaemonEvent;
use std::time::Duration;

#[tokio::test]
async fn test_duplicate_account_rejected() {
    if std::env::var("OCIS_ACCEPTANCE").is_err() {
        eprintln!("Skipping test_duplicate_account_rejected: OCIS_ACCEPTANCE not set");
        return;
    }

    let mut env = TestEnvironment::start()
        .await
        .expect("failed to start TestEnvironment");

    // First account setup via GUI — must succeed.
    env.add_account()
        .await
        .expect("first account setup via OIDC failed");

    // Second attempt: drive through the GUI exactly as a user would.

    // Click "Add Account" in the nav sidebar.
    let add_btn = env
        .atspi
        .wait_for_widget(Role::Button, "Add Account", Duration::from_secs(10))
        .await
        .expect("Add Account nav button not found for second attempt");
    env.atspi
        .click(&add_btn)
        .await
        .expect("failed to click Add Account for second attempt");

    // Type the same server URL.
    let url_field = env
        .atspi
        .wait_for_widget(Role::Entry, "https://your.server.com", Duration::from_secs(5))
        .await
        .expect("URL text input not found for second attempt");
    env.atspi
        .set_text(&url_field, env.ocis_url.as_str())
        .await
        .expect("failed to set server URL for second attempt");

    // Click "Connect →".
    let connect_btn = env
        .atspi
        .wait_for_widget(Role::Button, "Connect →", Duration::from_secs(5))
        .await
        .expect("Connect button not found for second attempt");
    env.atspi
        .click(&connect_btn)
        .await
        .expect("failed to click Connect for second attempt");

    // Wait for daemon to confirm a new OIDC flow started.
    env.daemon_ipc
        .wait_for(
            |e| matches!(e, DaemonEvent::AccountAddStarted { .. }),
            Duration::from_secs(15),
        )
        .await
        .expect("AccountAddStarted not received for second attempt");

    // Complete OIDC login with the same credentials (same user = duplicate).
    let auth_url = env
        .wait_for_oidc_url()
        .await
        .expect("OIDC_AUTH_URL not emitted for second attempt");

    let callback_port = auth_url
        .query_pairs()
        .find_map(|(k, v)| {
            if k == "redirect_uri" {
                url::Url::parse(&v).ok().and_then(|u| u.port())
            } else {
                None
            }
        })
        .expect("could not extract callback port from redirect_uri");

    complete_oidc_login(&auth_url, callback_port, "admin", "admin")
        .await
        .expect("Playwright OIDC login failed for second attempt");

    // Daemon must reject the duplicate.
    let event = env
        .daemon_ipc
        .wait_for(
            |e| {
                matches!(
                    e,
                    DaemonEvent::AccountAddFailed { .. } | DaemonEvent::AccountAddCompleted { .. }
                )
            },
            Duration::from_secs(30),
        )
        .await
        .expect("neither AccountAddFailed nor AccountAddCompleted received");

    assert!(
        matches!(event, DaemonEvent::AccountAddFailed { .. }),
        "expected AccountAddFailed for duplicate account, got: {event:?}"
    );

    // GUI must have returned to the AddAccount view (URL input visible again).
    // The URL field is found by its placeholder text even when it has content —
    // AT-SPI accessible names for text inputs come from the placeholder, not the value.
    env.atspi
        .wait_for_widget(
            Role::Entry,
            "https://your.server.com",
            Duration::from_secs(10),
        )
        .await
        .expect("URL input not visible after duplicate rejection — GUI did not return to AddAccount view");
}
```

- [ ] **Step 2: Verify it compiles**

```bash
cargo build -p acceptance-test
```

Expected: no errors.

- [ ] **Step 3: Run cargo fmt and commit**

```bash
cargo fmt -p acceptance-test
git add crates/acceptance-test/tests/duplicate_account.rs
git commit -s -m "feat(acceptance): drive duplicate account rejection through GUI via AT-SPI2"
```

---

## Task 7: Run the full acceptance suite

This requires a live oCIS instance and Playwright. It validates the full stack end to end.

- [ ] **Step 1: Ensure oCIS is running**

```bash
docker compose -f tests/docker/compose.yml up -d
```

Expected: oCIS health endpoint returns 200 within ~60 seconds.

- [ ] **Step 2: Build the test-accessibility GUI binary**

```bash
cargo build -p gui --features test-accessibility
```

Expected: produces `target/debug/ocsync` with AccessKit enabled.

- [ ] **Step 3: Run the acceptance suite**

```bash
OCIS_ACCEPTANCE=1 cargo test -p acceptance-test -- --nocapture --test-threads=1
```

Expected: `test_account_setup` and `test_duplicate_account_rejected` pass.

**If `wait_for_widget` times out:** read the `dump_tree()` output in the error. The output lists every accessible node as `[Role] "name"`. Find the widget you're looking for and update the `Role` constant in the relevant step. Common adjustments:
- Iced `text_input` may emit `Role::Entry` or `Role::Text` — update to whichever appears
- Iced `text()` (static label) may emit `Role::Label`, `Role::Text`, or `Role::Static` — update `account_setup.rs` assertion accordingly

- [ ] **Step 4: Commit any role adjustments found in step 3**

If you had to change any `Role::` constants after reading `dump_tree()` output:

```bash
cargo fmt -p acceptance-test
git add crates/acceptance-test/
git commit -s -m "fix(acceptance): correct AT-SPI roles for Iced widget types"
```

---

## Self-Review

**Spec coverage check:**

| Spec requirement | Covered by |
|---|---|
| Fix GUI nav button accessible names | Task 1 |
| Add `dump_tree()` to `AtSpiClient` | Task 2 |
| Log tree on `wait_for_widget` timeout | Task 2 |
| Add `atspi: AtSpiClient` to `TestEnvironment` | Task 3 |
| Rewrite `add_account()` to use AT-SPI for input | Task 4 |
| Keep daemon IPC as completion signal | Task 4 (steps 4, 7, 10) |
| Assert account display name visible in GUI | Task 5 |
| Rewrite duplicate test to drive GUI | Task 6 |
| Assert GUI returns to AddAccount on duplicate | Task 6 (final assert) |
| Guidance for role mismatches | Task 7 |

**Placeholder scan:** No TBDs or incomplete sections. All code steps are complete. ✓

**Type consistency:**
- `AtSpiClient::dump_tree(&self) -> String` — async, returns String. Called in `wait_for_widget` as `self.dump_tree().await`. ✓
- `AtSpiClient::wait_for_widget(role: Role, name: &str, timeout: Duration)` — signature unchanged from Task 2. ✓
- `TestEnvironment::atspi: AtSpiClient` — field added in Task 3, used in Tasks 4, 5, 6. ✓
- `Role::Button`, `Role::Entry`, `Role::Label` — all exist in `atspi-common-0.13.0::Role` enum. ✓
- `add_account(&mut self) -> Result<()>` — same signature as before. ✓
