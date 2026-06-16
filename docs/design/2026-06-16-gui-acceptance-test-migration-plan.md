# GUI Acceptance Test Migration — Tier 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate the Tier 1 ("migratable now, no harness extension") scenarios from the classic ownCloud desktop client's GUI acceptance tests into the new Rust client's `crates/acceptance-test` suite: delete, edit, move/rename, and pause/resume.

**Architecture:** Each migrated scenario becomes a new `tests/*.rs` integration test in `crates/acceptance-test`, driven by daemon IPC + filesystem + server-side `OcisClient` assertions (the established pattern in `sync.rs`). Tests are bidirectional (assert both up-sync and down-sync in one test) and use paths containing non-ASCII (Chinese) **and** HTTP-encoding characters (space, `?`, `<`). Two small enablers are added first: WebDAV `delete`/`move_item` helpers on `OcisClient` (needed for the down-sync half of delete/move tests), and capture of the personal folder's `folder_id` on `TestEnvironment` (needed to address `PauseFolder`/`ResumeFolder`).

**Tech Stack:** Rust, tokio, `crates/acceptance-test` harness (`TestEnvironment`, `DaemonIpcClient`, `OcisClient`, `poll_until`), oCIS over Docker Compose, WebDAV (`/dav/spaces/<id>/...`).

**Spec:** `docs/design/2026-06-16-gui-acceptance-test-migration-design.md`

---

## Conventions used throughout this plan

**Running acceptance tests** (require Docker + a display server; the fixture starts/stops oCIS):

```bash
OCIS_ACCEPTANCE=1 cargo test -p acceptance-test --test <name> -- --nocapture
```

**Compile-only gate** (works anywhere, no Docker/display needed — verifies the test builds and is wired into `Cargo.toml`):

```bash
cargo test -p acceptance-test --test <name> --no-run
```

Without `OCIS_ACCEPTANCE` set, every test early-returns (the `skip_if_no_acceptance` guard), so a plain `cargo test` compiles and "passes" by skipping. The compile gate is the honest red/green signal available without the full environment; the full acceptance command is the real validation when Docker + a display are present.

**Commit signing:** every commit uses `git commit -s` (DCO) and is PGP-signed (configured globally). Run `cargo fmt` before each commit and include the formatting.

**Shared test-path convention** (declare near the top of each new test file as needed):

- A name with non-ASCII + space + `?`: `"测试 delete?file.txt"`
- A name with non-ASCII + space + `<` `>`: `"上传 edit<file>.txt"`

---

## File structure

| File | Responsibility | Action |
|---|---|---|
| `crates/acceptance-test/src/ocis_client.rs` | Server-side WebDAV assertions/helpers | Modify — add `delete`, `move_item` |
| `crates/acceptance-test/src/fixture.rs` | `TestEnvironment` harness | Modify — capture + expose `personal_folder_id` |
| `crates/acceptance-test/tests/delete.rs` | Migrate `tst_deletFilesFolders` | Create |
| `crates/acceptance-test/tests/edit.rs` | Migrate `tst_editFiles` | Create |
| `crates/acceptance-test/tests/move_rename.rs` | Migrate `tst_moveFilesFolders` | Create |
| `crates/acceptance-test/tests/pause_resume.rs` | Migrate pause/resume behaviour from `tst_syncing` | Create |
| `crates/acceptance-test/Cargo.toml` | Test registration | Modify — add 4 `[[test]]` entries |

---

## Task 1: Add `delete` and `move_item` helpers to `OcisClient`

The down-sync half of the delete and move tests needs to mutate the server directly. `OcisClient` currently has `put/get/exists/collection_exists` but no delete or move. Add them using the existing `webdav_url` percent-encoding path builder.

**Files:**
- Modify: `crates/acceptance-test/src/ocis_client.rs`

- [ ] **Step 1: Add the two methods to the `impl OcisClient` block**

Insert after the existing `collection_exists` method (before the closing `}` of the `impl`):

```rust
    /// Deletes a file or collection on the server via WebDAV DELETE.
    pub async fn delete(&self, path: &str) -> Result<()> {
        self.client
            .request(reqwest::Method::DELETE, self.webdav_url(path)?)
            .basic_auth(&self.username, Some(&self.password))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    /// Moves/renames a file or collection on the server via WebDAV MOVE.
    /// `to` is a space-relative path, encoded the same way as `from`.
    pub async fn move_item(&self, from: &str, to: &str) -> Result<()> {
        let destination = self.webdav_url(to)?;
        self.client
            .request(
                reqwest::Method::from_bytes(b"MOVE").unwrap(),
                self.webdav_url(from)?,
            )
            .basic_auth(&self.username, Some(&self.password))
            .header("Destination", destination.as_str())
            .header("Overwrite", "T")
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
```

- [ ] **Step 2: Format**

Run: `cargo fmt`
Expected: no diff complaints; file reformatted if needed.

- [ ] **Step 3: Verify the crate library compiles**

Run: `cargo build -p acceptance-test`
Expected: builds cleanly (warnings about unused `delete`/`move_item` are acceptable — they are exercised by tests added in later tasks).

- [ ] **Step 4: Commit**

```bash
git add crates/acceptance-test/src/ocis_client.rs
git commit -s -m "test(acceptance): add WebDAV delete and move_item helpers to OcisClient"
```

---

## Task 2: Capture the personal folder's `folder_id` on `TestEnvironment`

`PauseFolder`/`ResumeFolder`/`TriggerSync` are addressed by `folder_id`. `add_account()` already waits for `AccountFolderAdded` (which carries `folder_id`) but discards it. Capture it into a new field and expose an accessor so the pause/resume test can address the folder.

**Files:**
- Modify: `crates/acceptance-test/src/fixture.rs`

- [ ] **Step 1: Add the field to the `TestEnvironment` struct**

Find the struct definition (the block starting `pub struct TestEnvironment {`). Add this field immediately after the `personal_space_name: String,` field:

```rust
    /// `folder_id` of the personal-space sync folder, captured from the
    /// `AccountFolderAdded` event during `add_account()`. `None` until then.
    pub personal_folder_id: Option<uuid::Uuid>,
```

- [ ] **Step 2: Initialize the field in `TestEnvironment::start()`**

In the `Ok(Self { ... })` constructor at the end of `start()`, add immediately after the `personal_space_name: String::new(),` line:

```rust
            personal_folder_id: None,
```

- [ ] **Step 3: Populate the field in `add_account()`**

Find step 8 of `add_account()` — the `wait_for(AccountFolderAdded ...)` call. It currently looks like:

```rust
        // 8. Wait for folder added.
        self.daemon_ipc
            .wait_for(
                |e| matches!(e, DaemonEvent::AccountFolderAdded { .. }),
                Duration::from_secs(30),
            )
            .await
            .ok_or_else(|| anyhow!("AccountFolderAdded not received"))?;
```

Replace that block with one that captures the `folder_id`:

```rust
        // 8. Wait for folder added, capturing the personal folder_id.
        let folder_added = self
            .daemon_ipc
            .wait_for(
                |e| matches!(e, DaemonEvent::AccountFolderAdded { .. }),
                Duration::from_secs(30),
            )
            .await
            .ok_or_else(|| anyhow!("AccountFolderAdded not received"))?;
        if let DaemonEvent::AccountFolderAdded { folder_id, .. } = folder_added {
            self.personal_folder_id = Some(folder_id);
        }
```

- [ ] **Step 4: Add an accessor method**

Inside `impl TestEnvironment`, after the `personal_sync_dir()` method, add:

```rust
    /// Returns the personal-space sync folder's `folder_id`, captured during
    /// `add_account()`. Panics if called before a successful `add_account()`.
    pub fn personal_folder_id(&self) -> uuid::Uuid {
        self.personal_folder_id
            .expect("personal_folder_id not set — call add_account() first")
    }
```

- [ ] **Step 5: Format**

Run: `cargo fmt`
Expected: reformatted if needed, no errors.

- [ ] **Step 6: Verify compilation**

Run: `cargo build -p acceptance-test`
Expected: builds cleanly.

- [ ] **Step 7: Commit**

```bash
git add crates/acceptance-test/src/fixture.rs
git commit -s -m "test(acceptance): capture personal folder_id on TestEnvironment"
```

---

## Task 3: Migrate `tst_deletFilesFolders` → `tests/delete.rs`

Old scenarios: delete a file, delete a folder, delete a file+folder while a sibling survives. Migrated as a bidirectional test: (a) delete locally → assert gone on server; (b) delete on server → assert gone locally. Special-character + non-ASCII names. A survivor file is asserted to remain untouched.

**Files:**
- Create: `crates/acceptance-test/tests/delete.rs`
- Modify: `crates/acceptance-test/Cargo.toml`

- [ ] **Step 1: Register the test in `Cargo.toml`**

In `crates/acceptance-test/Cargo.toml`, add this entry alongside the other `[[test]]` blocks (after the `tray` entry):

```toml
[[test]]
name = "delete"
path = "tests/delete.rs"
```

- [ ] **Step 2: Write the test file**

Create `crates/acceptance-test/tests/delete.rs`:

```rust
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

//! Migrated from owncloud/client test/gui/tst_deletFilesFolders.
//! Bidirectional deletion: local delete propagates up; remote delete propagates down.
//! A sibling file is asserted to survive both deletions.

use std::time::Duration;

use acceptance_test::{fixture::TestEnvironment, poll::poll_until};
use daemon::gui_ipc::protocol::DaemonEvent;

fn skip_if_no_acceptance() -> bool {
    if std::env::var("OCIS_ACCEPTANCE").is_err() {
        eprintln!("Skipping: OCIS_ACCEPTANCE not set");
        return true;
    }
    false
}

async fn env_after_initial_sync() -> TestEnvironment {
    let mut env = TestEnvironment::start()
        .await
        .expect("TestEnvironment::start");
    env.add_account().await.expect("add_account");
    env.daemon_ipc
        .wait_for(
            |e| matches!(e, DaemonEvent::SyncFinished { errors, .. } if errors.is_empty()),
            Duration::from_secs(60),
        )
        .await
        .expect("initial SyncFinished not received");
    env
}

#[tokio::test]
async fn test_delete_file_and_folder_bidirectional() {
    if skip_if_no_acceptance() {
        return;
    }
    let env = env_after_initial_sync().await;
    let sync_dir = env.personal_sync_dir();

    // Names: non-ASCII + HTTP-encoding chars.
    let up_file = "删除 up?file.txt"; // deleted locally -> expect gone on server
    let survivor = "幸存 keep<file>.txt"; // must remain after both deletes
    let down_file = "删除 down file.txt"; // deleted on server -> expect gone locally

    // Seed all three on the server, wait for them to sync down.
    for name in [up_file, survivor, down_file] {
        env.ocis_client
            .put(name, b"seed")
            .await
            .unwrap_or_else(|_| panic!("seed {name}"));
    }
    for name in [up_file, survivor, down_file] {
        let p = sync_dir.join(name);
        poll_until(
            || {
                let p = p.clone();
                async move { p.exists() }
            },
            Duration::from_secs(30),
            Duration::from_secs(1),
        )
        .await
        .unwrap_or_else(|_| panic!("{name} did not sync down"));
    }

    // --- UP: delete locally, expect gone on server ---
    std::fs::remove_file(sync_dir.join(up_file)).expect("remove local up_file");
    poll_until(
        || async { !env.ocis_client.exists(up_file).await.unwrap_or(true) },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("local deletion did not propagate to server");

    // --- DOWN: delete on server, expect gone locally ---
    env.ocis_client
        .delete(down_file)
        .await
        .expect("server delete");
    poll_until(
        || {
            let p = sync_dir.join(down_file);
            async move { !p.exists() }
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("server deletion did not propagate to client");

    // --- SURVIVOR: untouched on both ends ---
    assert!(
        env.ocis_client.exists(survivor).await.unwrap_or(false),
        "survivor must still exist on server"
    );
    assert!(
        sync_dir.join(survivor).exists(),
        "survivor must still exist locally"
    );
}
```

- [ ] **Step 3: Compile gate**

Run: `cargo test -p acceptance-test --test delete --no-run`
Expected: compiles; produces a test binary.

- [ ] **Step 4: Run against the acceptance environment (if available)**

Run: `OCIS_ACCEPTANCE=1 cargo test -p acceptance-test --test delete -- --nocapture`
Expected: `test_delete_file_and_folder_bidirectional ... ok`. (If Docker/display are unavailable, this step is deferred to CI; the compile gate stands in.)

- [ ] **Step 5: Format and commit**

```bash
cargo fmt
git add crates/acceptance-test/tests/delete.rs crates/acceptance-test/Cargo.toml
git commit -s -m "test(acceptance): migrate tst_deletFilesFolders (bidirectional delete)"
```

---

## Task 4: Migrate `tst_editFiles` → `tests/edit.rs`

Old scenario: overwrite a file (special-character name) and assert the new content on the server. Migrated bidirectionally: edit locally → server reflects; edit on server → local reflects. Non-ASCII + HTTP-encoding names.

**Files:**
- Create: `crates/acceptance-test/tests/edit.rs`
- Modify: `crates/acceptance-test/Cargo.toml`

- [ ] **Step 1: Register the test in `Cargo.toml`**

```toml
[[test]]
name = "edit"
path = "tests/edit.rs"
```

- [ ] **Step 2: Write the test file**

Create `crates/acceptance-test/tests/edit.rs`:

```rust
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

//! Migrated from owncloud/client test/gui/tst_editFiles.
//! Bidirectional edit: local overwrite propagates up; remote overwrite propagates down.

use std::time::Duration;

use acceptance_test::{fixture::TestEnvironment, poll::poll_until};
use daemon::gui_ipc::protocol::DaemonEvent;

fn skip_if_no_acceptance() -> bool {
    if std::env::var("OCIS_ACCEPTANCE").is_err() {
        eprintln!("Skipping: OCIS_ACCEPTANCE not set");
        return true;
    }
    false
}

#[tokio::test]
async fn test_edit_file_bidirectional() {
    if skip_if_no_acceptance() {
        return;
    }
    let mut env = TestEnvironment::start()
        .await
        .expect("TestEnvironment::start");
    env.add_account().await.expect("add_account");
    env.daemon_ipc
        .wait_for(
            |e| matches!(e, DaemonEvent::SyncFinished { errors, .. } if errors.is_empty()),
            Duration::from_secs(60),
        )
        .await
        .expect("initial SyncFinished not received");

    let sync_dir = env.personal_sync_dir();
    let up_name = "编辑 up?file.txt"; // edited locally
    let down_name = "编辑 down<file>.txt"; // edited on server

    // Seed both, wait for them to sync down.
    env.ocis_client
        .put(up_name, b"original up")
        .await
        .expect("seed up");
    env.ocis_client
        .put(down_name, b"original down")
        .await
        .expect("seed down");
    for (name, content) in [(up_name, b"original up" as &[u8]), (down_name, b"original down")] {
        let p = sync_dir.join(name);
        poll_until(
            || {
                let p = p.clone();
                async move { std::fs::read(&p).map(|c| c == content).unwrap_or(false) }
            },
            Duration::from_secs(30),
            Duration::from_secs(1),
        )
        .await
        .unwrap_or_else(|_| panic!("{name} did not sync down with seed content"));
    }

    // --- UP: overwrite locally, expect server content updated ---
    std::fs::write(sync_dir.join(up_name), b"edited up content").expect("overwrite local");
    poll_until(
        || async {
            env.ocis_client
                .get(up_name)
                .await
                .map(|b| b.as_ref() == b"edited up content")
                .unwrap_or(false)
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("local edit did not propagate to server");

    // --- DOWN: overwrite on server, expect local content updated ---
    env.ocis_client
        .put(down_name, b"edited down content")
        .await
        .expect("server overwrite");
    poll_until(
        || {
            let p = sync_dir.join(down_name);
            async move {
                std::fs::read(&p)
                    .map(|c| c == b"edited down content")
                    .unwrap_or(false)
            }
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("server edit did not propagate to client");
}
```

- [ ] **Step 3: Compile gate**

Run: `cargo test -p acceptance-test --test edit --no-run`
Expected: compiles.

- [ ] **Step 4: Run against the acceptance environment (if available)**

Run: `OCIS_ACCEPTANCE=1 cargo test -p acceptance-test --test edit -- --nocapture`
Expected: `test_edit_file_bidirectional ... ok`.

- [ ] **Step 5: Format and commit**

```bash
cargo fmt
git add crates/acceptance-test/tests/edit.rs crates/acceptance-test/Cargo.toml
git commit -s -m "test(acceptance): migrate tst_editFiles (bidirectional edit)"
```

---

## Task 5: Migrate `tst_moveFilesFolders` → `tests/move_rename.rs`

Old scenarios: move a file out of a deep subfolder to the sync root; rename a file. Migrated bidirectionally: (a) rename/move locally → server reflects new path, old path gone; (b) move on server → local reflects. Non-ASCII + HTTP-encoding names.

**Files:**
- Create: `crates/acceptance-test/tests/move_rename.rs`
- Modify: `crates/acceptance-test/Cargo.toml`

- [ ] **Step 1: Register the test in `Cargo.toml`**

```toml
[[test]]
name = "move_rename"
path = "tests/move_rename.rs"
```

- [ ] **Step 2: Write the test file**

Create `crates/acceptance-test/tests/move_rename.rs`:

```rust
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

//! Migrated from owncloud/client test/gui/tst_moveFilesFolders.
//! Bidirectional move/rename: local rename-out-of-subdir propagates up;
//! server-side move propagates down.

use std::time::Duration;

use acceptance_test::{fixture::TestEnvironment, poll::poll_until};
use daemon::gui_ipc::protocol::DaemonEvent;

fn skip_if_no_acceptance() -> bool {
    if std::env::var("OCIS_ACCEPTANCE").is_err() {
        eprintln!("Skipping: OCIS_ACCEPTANCE not set");
        return true;
    }
    false
}

#[tokio::test]
async fn test_move_and_rename_bidirectional() {
    if skip_if_no_acceptance() {
        return;
    }
    let mut env = TestEnvironment::start()
        .await
        .expect("TestEnvironment::start");
    env.add_account().await.expect("add_account");
    env.daemon_ipc
        .wait_for(
            |e| matches!(e, DaemonEvent::SyncFinished { errors, .. } if errors.is_empty()),
            Duration::from_secs(60),
        )
        .await
        .expect("initial SyncFinished not received");

    let sync_dir = env.personal_sync_dir();

    // --- UP: local move from a nested subdir to the sync root ---
    let sub = "子目录 a?dir"; // nested directory
    let up_src_rel = format!("{sub}/移动 up file.txt");
    let up_dst_rel = "移动 up file.txt"; // moved to root
    let nested_dir = sync_dir.join(sub);
    std::fs::create_dir_all(&nested_dir).expect("create nested dir");
    std::fs::write(nested_dir.join("移动 up file.txt"), b"move me up").expect("write nested file");

    // Wait for the nested file to reach the server before moving it.
    poll_until(
        || {
            let path = up_src_rel.clone();
            async move {
                env.ocis_client
                    .exists(&path)
                    .await
                    .unwrap_or(false)
            }
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("nested file did not sync up before move");

    // Move locally: nested -> root.
    std::fs::rename(
        sync_dir.join(&up_src_rel),
        sync_dir.join(up_dst_rel),
    )
    .expect("local move to root");

    poll_until(
        || async {
            env.ocis_client.exists(up_dst_rel).await.unwrap_or(false)
                && !env.ocis_client.exists(&up_src_rel).await.unwrap_or(true)
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("local move did not propagate to server (new path present, old gone)");
    let moved = env
        .ocis_client
        .get(up_dst_rel)
        .await
        .expect("get moved file");
    assert_eq!(moved.as_ref(), b"move me up", "moved file content preserved");

    // --- DOWN: server-side rename, expect local reflects ---
    let down_old = "重命名 old file.txt";
    let down_new = "重命名 new<file>.txt";
    env.ocis_client
        .put(down_old, b"rename me on server")
        .await
        .expect("seed down_old");
    poll_until(
        || {
            let p = sync_dir.join(down_old);
            async move { p.exists() }
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("down_old did not sync down");

    env.ocis_client
        .move_item(down_old, down_new)
        .await
        .expect("server move_item");

    poll_until(
        || {
            let new_p = sync_dir.join(down_new);
            let old_p = sync_dir.join(down_old);
            async move { new_p.exists() && !old_p.exists() }
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("server rename did not propagate to client");
}
```

- [ ] **Step 3: Compile gate**

Run: `cargo test -p acceptance-test --test move_rename --no-run`
Expected: compiles.

- [ ] **Step 4: Run against the acceptance environment (if available)**

Run: `OCIS_ACCEPTANCE=1 cargo test -p acceptance-test --test move_rename -- --nocapture`
Expected: `test_move_and_rename_bidirectional ... ok`.

- [ ] **Step 5: Format and commit**

```bash
cargo fmt
git add crates/acceptance-test/tests/move_rename.rs crates/acceptance-test/Cargo.toml
git commit -s -m "test(acceptance): migrate tst_moveFilesFolders (bidirectional move/rename)"
```

---

## Task 6: Migrate pause/resume behaviour from `tst_syncing` → `tests/pause_resume.rs`

Old `tst_syncing` pauses the file sync, makes a local change, then resumes and expects the change to flush. Migrated via `PauseFolder`/`ResumeFolder` IPC addressed by the captured `folder_id`: while paused, a new local file must NOT appear on the server; after resume, it must. (Up-direction only — pause/resume is about suppressing propagation, which has a single meaningful direction here.)

**Files:**
- Create: `crates/acceptance-test/tests/pause_resume.rs`
- Modify: `crates/acceptance-test/Cargo.toml`

- [ ] **Step 1: Register the test in `Cargo.toml`**

```toml
[[test]]
name = "pause_resume"
path = "tests/pause_resume.rs"
```

- [ ] **Step 2: Write the test file**

Create `crates/acceptance-test/tests/pause_resume.rs`:

```rust
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

//! Migrated from owncloud/client test/gui/tst_syncing (pause/resume).
//! While the folder is paused, a new local file must NOT propagate to the
//! server; after resume, it must.

use std::time::Duration;

use acceptance_test::{fixture::TestEnvironment, poll::poll_until};
use daemon::gui_ipc::protocol::{DaemonCommand, DaemonEvent};

fn skip_if_no_acceptance() -> bool {
    if std::env::var("OCIS_ACCEPTANCE").is_err() {
        eprintln!("Skipping: OCIS_ACCEPTANCE not set");
        return true;
    }
    false
}

#[tokio::test]
async fn test_pause_blocks_then_resume_flushes() {
    if skip_if_no_acceptance() {
        return;
    }
    let mut env = TestEnvironment::start()
        .await
        .expect("TestEnvironment::start");
    env.add_account().await.expect("add_account");
    env.daemon_ipc
        .wait_for(
            |e| matches!(e, DaemonEvent::SyncFinished { errors, .. } if errors.is_empty()),
            Duration::from_secs(60),
        )
        .await
        .expect("initial SyncFinished not received");

    let folder_id = env.personal_folder_id();
    let sync_dir = env.personal_sync_dir();
    let name = "暂停 paused?file.txt";

    // Pause the folder.
    env.daemon_ipc
        .send(DaemonCommand::PauseFolder { folder_id })
        .await
        .expect("send PauseFolder");

    // Create a local file while paused.
    std::fs::write(sync_dir.join(name), b"written while paused").expect("write local file");

    // For a bounded window, the file must NOT appear on the server.
    let appeared_while_paused = poll_until(
        || async { env.ocis_client.exists(name).await.unwrap_or(false) },
        Duration::from_secs(8),
        Duration::from_secs(1),
    )
    .await
    .is_ok();
    assert!(
        !appeared_while_paused,
        "file must not upload while folder is paused"
    );

    // Resume — the pending change must now flush to the server.
    env.daemon_ipc
        .send(DaemonCommand::ResumeFolder { folder_id })
        .await
        .expect("send ResumeFolder");

    poll_until(
        || async { env.ocis_client.exists(name).await.unwrap_or(false) },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("file did not upload after resume");
}
```

- [ ] **Step 3: Compile gate**

Run: `cargo test -p acceptance-test --test pause_resume --no-run`
Expected: compiles.

- [ ] **Step 4: Run against the acceptance environment (if available)**

Run: `OCIS_ACCEPTANCE=1 cargo test -p acceptance-test --test pause_resume -- --nocapture`
Expected: `test_pause_blocks_then_resume_flushes ... ok`.

> **Note for the executor:** if this test fails at the "must not upload while paused" assertion, it means `PauseFolder` does not suppress watcher-driven uploads — that is a real product finding, not a test bug. Stop and report it (it would reclassify the pause/resume scenario from ✅ to a product gap in the spec) rather than weakening the assertion.

- [ ] **Step 5: Format and commit**

```bash
cargo fmt
git add crates/acceptance-test/tests/pause_resume.rs crates/acceptance-test/Cargo.toml
git commit -s -m "test(acceptance): migrate tst_syncing pause/resume behaviour"
```

---

## Task 7: Final verification

- [ ] **Step 1: Whole-workspace compile of the acceptance crate's tests**

Run: `cargo test -p acceptance-test --no-run`
Expected: all four new test binaries (`delete`, `edit`, `move_rename`, `pause_resume`) plus the existing ones build.

- [ ] **Step 2: Skip-path sanity (no Docker needed)**

Run: `cargo test -p acceptance-test`
Expected: every test runs and is skipped (prints "Skipping: OCIS_ACCEPTANCE not set"), exit code 0.

- [ ] **Step 3: Full acceptance run (if Docker + display available)**

Run: `just acceptance`  (equivalently `OCIS_ACCEPTANCE=1 cargo test -p acceptance-test -- --nocapture`)
Expected: all migrated tests pass.

- [ ] **Step 4: Confirm `cargo fmt` is clean**

Run: `cargo fmt --check`
Expected: no diff.

---

## What this plan does NOT cover (deferred to later plans)

Per the spec's tiered backlog, the following are **out of scope here** and need their own plans:

- **Tier 2 (harness extensions):** multi-user provisioning → multiple-accounts / remove-one-of-two / activity multi-account filter; logout→login lifecycle; space + role provisioning → editable `tst_spaces` scenarios.
- **Tier 3 remainders:** ignore/exclude behaviour (`.htaccess`), remove-only-account (needs verification of whether the daemon emits `AccountSnapshot` after `RemoveAccount` without a request command).
- **Product feature gaps (❌):** Activity tab, Settings tab/options, all-tabs inventory, advanced account-config UI, self-signed cert flow, Linux VFS, read-only/viewer enforcement.

These remain recorded in the gap registry of `docs/design/2026-06-16-gui-acceptance-test-migration-design.md`.
