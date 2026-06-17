# GUI Acceptance Test Migration — Tier 2: Space + Role Provisioning Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add admin-side Graph provisioning of project spaces + role assignment to the acceptance harness, a fixture path to sync a named project space, and a role-matrix acceptance test migrating the editable `tst_spaces` scenarios.

**Architecture:** A new `SpaceProvisioner` (admin basic-auth Graph wrapper) creates a `project` drive and assigns a role to a user, resolving the role definition **by display name** so an unavailable role (e.g. Secure Viewer) degrades to a skip rather than a failure. `OcisClient` gains a constructor that targets a project space by id for server-side assertions. The fixture's shared `add_account_inner` is generalized with a `SpaceChoice` selector (`Personal` | `Named`) so a new public `add_account_on_space()` can sync the shared project space while `add_account` / `add_account_as` keep selecting personal unchanged.

**Tech Stack:** Rust, tokio, `crates/acceptance-test` harness (`TestEnvironment`, `DaemonIpcClient`, `OcisClient`, `UserProvisioner`, `poll_until`), oCIS over Docker Compose, Graph API (`/graph/v1.0/drives`, `/roleManagement/permissions/roleDefinitions`, `/drives/{id}/root/invite`), WebDAV.

**Spec:** `docs/design/2026-06-16-space-role-provisioning-design.md`

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

Without `OCIS_ACCEPTANCE` set, every test early-returns (`skip_if_no_acceptance`), so a plain `cargo test` compiles and "passes" by skipping. The compile gate is the honest red/green signal available without the full environment; the full acceptance command is the real validation when Docker + a display are present. This mirrors the Tier 1 and Tier 2-multiuser plans exactly.

**Stale binaries:** acceptance tests spawn `ocsyncd`/`ocsync` from `target/debug`. Before a local acceptance run, build the binaries first — `cargo test` does **not** rebuild the spawned binaries:

```bash
cargo build --workspace --bins
```

**Commit signing:** every commit uses `git commit -s` (DCO) and is PGP-signed (configured globally). The signing key is blocked inside the command sandbox, so signing commits must run with the sandbox disabled. After committing, verify the signature is present with `git cat-file commit HEAD | grep gpgsig`. Run `cargo fmt` before each commit and include the formatting.

**Branch:** all work is on `test/space-role-provisioning` (already created; the design doc and this plan are committed there). Never push to `main`; open a PR at the end.

**Shared test-path convention** (paths in synced files): non-ASCII (Chinese) **and** HTTP-encoding chars (space, `?`, `<`/`>`), e.g. `"项目 up?file.txt"`.

**Provisioning constants** (used by the new tests):

- Password for provisioned users: `"Secret123!"` (satisfies oCIS default password policy).
- Usernames **and** project-space names are suffixed with a short uuid so concurrent test binaries never collide.
- Role display names: `"Space Viewer"`, `"Space Editor"`, `"Space Manager"`, `"Secure Viewer"`.

---

## File structure

| File | Responsibility | Action |
|---|---|---|
| `crates/acceptance-test/src/provision.rs` | Add `SpaceProvisioner`, `ProvisionedSpace`, `RoleAssignment` — admin Graph space-create + role-resolve + role-assign | Modify |
| `crates/acceptance-test/src/ocis_client.rs` | Add `OcisClient::from_credentials_on_space` — target a project space by id for server-side assertions | Modify |
| `crates/acceptance-test/src/fixture.rs` | Add `SpaceChoice` enum; generalize `add_account_inner`; add public `add_account_on_space` | Modify |
| `crates/acceptance-test/tests/space_roles.rs` | Role-matrix scenarios (editor, manager, viewer, secure viewer) | Create |
| `crates/acceptance-test/Cargo.toml` | Register the `space_roles` test | Modify |

---

## Task 1: Add `SpaceProvisioner` to `provision.rs`

Adds the admin-authenticated Graph wrapper that creates a `project` space, resolves a role definition by display name, and assigns it to a user via the space invite endpoint. Constructed exactly like the existing `UserProvisioner` (insecure reqwest client + `admin`/`admin` basic auth).

**Files:**
- Modify: `crates/acceptance-test/src/provision.rs`

- [ ] **Step 1: Add imports**

The existing `provision.rs` already imports `anyhow::{Context, Result}`, `reqwest::Client`, `serde::Deserialize`, `serde_json::json`, and `url::Url`. No new imports are required for this task — confirm those five lines are present at the top of the file before continuing.

- [ ] **Step 2: Append the `SpaceProvisioner` types and impl**

Add the following to the **end** of `crates/acceptance-test/src/provision.rs` (after the existing `UserProvisioner` impl):

```rust
/// A project space created on the oCIS server for the lifetime of a test.
#[derive(Debug, Clone)]
pub struct ProvisionedSpace {
    /// Drive id — used as the space id by `ListSpaces` and in WebDAV URLs.
    pub id: String,
    /// The (unique) space name as created.
    pub name: String,
}

/// Outcome of [`SpaceProvisioner::assign_role`].
///
/// A requested role may simply not exist in this oCIS configuration (the most
/// likely case is `Secure Viewer`). That is an environment property, not a test
/// failure, so it is reported as a value the caller can skip-and-log on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoleAssignment {
    /// The role was found by display name and assigned to the user.
    Assigned,
    /// No role definition with the requested display name exists on this server.
    Unavailable,
}

/// Creates project spaces and assigns roles as the bootstrap `admin` account.
pub struct SpaceProvisioner {
    client: Client,
    base_url: Url,
    admin_user: String,
    admin_pass: String,
}

#[derive(Deserialize)]
struct CreatedDrive {
    id: String,
    name: String,
}

#[derive(Deserialize)]
struct RoleDefinition {
    id: String,
    #[serde(rename = "displayName", default)]
    display_name: String,
}

impl SpaceProvisioner {
    /// Construct against the running oCIS using the bootstrap `admin`/`admin`
    /// credentials and an insecure TLS client (the test oCIS uses a self-signed
    /// cert). Same style as [`UserProvisioner::new`].
    pub async fn new(base_url: Url) -> Result<Self> {
        let client = Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .context("failed to build space-provisioning HTTP client")?;
        Ok(Self {
            client,
            base_url,
            admin_user: "admin".to_owned(),
            admin_pass: "admin".to_owned(),
        })
    }

    /// Create a `project` space via `POST /graph/v1.0/drives`. Returns the new
    /// drive id (the space id surfaced later by `ListSpaces` and used in WebDAV
    /// URLs).
    pub async fn create_project_space(&self, name: &str) -> Result<ProvisionedSpace> {
        let url = self
            .base_url
            .join("/graph/v1.0/drives")
            .context("invalid drives URL")?;
        let body = json!({
            "name": name,
            "driveType": "project"
        });
        let created: CreatedDrive = self
            .client
            .post(url)
            .basic_auth(&self.admin_user, Some(&self.admin_pass))
            .json(&body)
            .send()
            .await
            .context("create_project_space request failed")?
            .error_for_status()
            .context("create_project_space returned an error status")?
            .json()
            .await
            .context("create_project_space response was not valid JSON")?;
        Ok(ProvisionedSpace {
            id: created.id,
            name: created.name,
        })
    }

    /// Resolve a `unifiedRoleDefinition` id by its `displayName` via
    /// `GET /graph/v1.0/roleManagement/permissions/roleDefinitions`. Returns
    /// `None` when no role with that display name exists on this server.
    pub async fn resolve_role_id(&self, display_name: &str) -> Result<Option<String>> {
        let url = self
            .base_url
            .join("/graph/v1.0/roleManagement/permissions/roleDefinitions")
            .context("invalid roleDefinitions URL")?;
        // oCIS returns a bare JSON array of role definitions here.
        let defs: Vec<RoleDefinition> = self
            .client
            .get(url)
            .basic_auth(&self.admin_user, Some(&self.admin_pass))
            .send()
            .await
            .context("roleDefinitions request failed")?
            .error_for_status()
            .context("roleDefinitions returned an error status")?
            .json()
            .await
            .context("roleDefinitions response was not valid JSON")?;
        Ok(defs
            .into_iter()
            .find(|d| d.display_name == display_name)
            .map(|d| d.id))
    }

    /// Assign the role named `role_display_name` to user `user_id` on the space
    /// `space_id`, via `POST /graph/v1.0/drives/{space_id}/root/invite`.
    ///
    /// Returns [`RoleAssignment::Unavailable`] (without erroring) when the role
    /// display name does not resolve, so the caller can skip-and-log.
    pub async fn assign_role(
        &self,
        space_id: &str,
        user_id: &str,
        role_display_name: &str,
    ) -> Result<RoleAssignment> {
        let Some(role_id) = self.resolve_role_id(role_display_name).await? else {
            return Ok(RoleAssignment::Unavailable);
        };
        let url = self
            .base_url
            .join(&format!("/graph/v1.0/drives/{space_id}/root/invite"))
            .context("invalid invite URL")?;
        let body = json!({
            "recipients": [
                { "objectId": user_id, "@libre.graph.recipient.type": "user" }
            ],
            "roles": [role_id]
        });
        self.client
            .post(url)
            .basic_auth(&self.admin_user, Some(&self.admin_pass))
            .json(&body)
            .send()
            .await
            .context("assign_role invite request failed")?
            .error_for_status()
            .context("assign_role invite returned an error status")?;
        Ok(RoleAssignment::Assigned)
    }
}
```

> **Executor note (response shapes):** the role-definitions endpoint is expected to return a bare JSON array. If `resolve_role_id` fails to deserialize at runtime against this oCIS version, the response is likely wrapped as `{ "value": [ ... ] }` — in that case introduce a `#[derive(Deserialize)] struct RoleDefsResponse { value: Vec<RoleDefinition> }` and deserialize into that instead. Likewise, if `POST /graph/v1.0/drives` returns 403, the test oCIS forbids admin space creation — **stop and report** (it blocks the whole Tier 2 space effort), as with the user-creation note in the multi-user plan.

- [ ] **Step 3: Format**

Run: `cargo fmt`
Expected: reformatted if needed, no errors.

- [ ] **Step 4: Verify the crate library compiles**

Run: `cargo build -p acceptance-test`
Expected: builds cleanly. Warnings about unused `SpaceProvisioner` / `ProvisionedSpace` / `RoleAssignment` are acceptable — they are exercised by the test added in Task 4.

- [ ] **Step 5: Commit** (sandbox disabled — signing key needed)

```bash
cargo fmt
git add crates/acceptance-test/src/provision.rs
git commit -s -m "test(acceptance): add SpaceProvisioner for Graph space + role provisioning"
git cat-file commit HEAD | grep gpgsig   # verify signature present
```

---

## Task 2: Add a project-space constructor to `OcisClient`

The existing `OcisClient::from_credentials` always selects the user's **personal** drive. Server-side assertions against a shared project space need a client scoped to that space's id. Add a sibling constructor that takes the space id directly.

**Files:**
- Modify: `crates/acceptance-test/src/ocis_client.rs`

- [ ] **Step 1: Add the `from_credentials_on_space` constructor**

In `crates/acceptance-test/src/ocis_client.rs`, add this method inside `impl OcisClient`, immediately **after** the existing `from_credentials` method (before `webdav_url`):

```rust
    /// Like [`Self::from_credentials`], but targets a specific space by id
    /// instead of auto-selecting the personal drive. Used for server-side
    /// assertions against a shared project space. Does not call the Graph API —
    /// the caller already knows the space id (e.g. from `SpaceProvisioner`).
    pub async fn from_credentials_on_space(
        base_url: Url,
        username: &str,
        password: &str,
        space_id: &str,
    ) -> Result<Self> {
        let client = Client::builder()
            .danger_accept_invalid_certs(true)
            .build()?;
        Ok(Self {
            client,
            base_url,
            space_id: space_id.to_owned(),
            username: username.to_owned(),
            password: password.to_owned(),
        })
    }
```

- [ ] **Step 2: Format**

Run: `cargo fmt`
Expected: reformatted if needed, no errors.

- [ ] **Step 3: Verify the crate library compiles**

Run: `cargo build -p acceptance-test`
Expected: builds cleanly (a dead-code warning on the new method is acceptable until Task 4 uses it).

- [ ] **Step 4: Commit** (sandbox disabled — signing key needed)

```bash
cargo fmt
git add crates/acceptance-test/src/ocis_client.rs
git commit -s -m "test(acceptance): add OcisClient::from_credentials_on_space for project spaces"
git cat-file commit HEAD | grep gpgsig   # verify signature present
```

---

## Task 3: Generalize the fixture for project-space account setup

Add a `SpaceChoice` selector to the shared `add_account_inner`, then add a public `add_account_on_space()` that selects a named project space. `add_account` and `add_account_as` keep selecting personal, so every existing test compiles and behaves unchanged.

**Files:**
- Modify: `crates/acceptance-test/src/fixture.rs`

- [ ] **Step 1: Add the `SpaceChoice` enum**

In `crates/acceptance-test/src/fixture.rs`, add this enum immediately **after** the `AccountHandle` struct (before `const OCIS_URL`):

```rust
/// Which space `add_account_inner` selects for `SetAccountFolders`.
enum SpaceChoice {
    /// Select the user's personal space (`drive_type == "personal"`).
    Personal,
    /// Select a project space by its (unique) name as it appears in
    /// `SpacesListed`.
    Named(String),
}
```

- [ ] **Step 2: Change `add_account_inner` to take a `SpaceChoice`**

Replace the **signature** of `add_account_inner` (currently ending `root_path: PathBuf,\n    ) -> Result<(AccountHandle, String)> {`) so it accepts the selector. Change:

```rust
    async fn add_account_inner(
        &mut self,
        username: &str,
        password: &str,
        root_path: PathBuf,
    ) -> Result<(AccountHandle, String)> {
```

to:

```rust
    async fn add_account_inner(
        &mut self,
        username: &str,
        password: &str,
        root_path: PathBuf,
        space: SpaceChoice,
    ) -> Result<(AccountHandle, String)> {
```

- [ ] **Step 3: Replace the space-selection block (step 6 inside the method)**

Inside `add_account_inner`, find the block that currently selects the personal space:

```rust
        let personal = spaces
            .iter()
            .find(|s| s.drive_type == "personal")
            .ok_or_else(|| anyhow!("no personal space in SpacesListed"))?;
        let personal_space_name = personal.name.clone();
```

Replace it with a selector that honours `SpaceChoice` (rename the binding to `selected` since it is no longer always personal):

```rust
        let selected = match &space {
            SpaceChoice::Personal => spaces
                .iter()
                .find(|s| s.drive_type == "personal")
                .ok_or_else(|| anyhow!("no personal space in SpacesListed"))?,
            SpaceChoice::Named(name) => spaces
                .iter()
                .find(|s| &s.name == name)
                .ok_or_else(|| anyhow!("space {name:?} not in SpacesListed: {spaces:?}"))?,
        };
        let selected_space_name = selected.name.clone();
```

- [ ] **Step 4: Update the `SetAccountFolders` send and the `AccountHandle` to use `selected`**

Still inside `add_account_inner`, update the `SetAccountFolders` send to reference `selected` instead of `personal`. Change:

```rust
                spaces: vec![SpaceSelection {
                    space_id: personal.id.clone(),
                    display_name: personal.name.clone(),
                }],
```

to:

```rust
                spaces: vec![SpaceSelection {
                    space_id: selected.id.clone(),
                    display_name: selected.name.clone(),
                }],
```

Then update the returned `AccountHandle` at the end of the method. Change:

```rust
        let handle = AccountHandle {
            account_id,
            personal_folder_id,
            personal_sync_dir: root_path.join(&personal_space_name),
            personal_space_name,
        };
```

to:

```rust
        let handle = AccountHandle {
            account_id,
            personal_folder_id,
            personal_sync_dir: root_path.join(&selected_space_name),
            personal_space_name: selected_space_name,
        };
```

(The `AccountHandle` field names stay `personal_*` for API compatibility; they now denote "this account's selected-space" root and name. The `personal_folder_id` binding earlier in the method is unchanged — `AccountFolderAdded` fires for the one selected space.)

- [ ] **Step 5: Update the two existing callers to pass `SpaceChoice::Personal`**

In `add_account`, change:

```rust
        let (handle, title) = self.add_account_inner("admin", "admin", root).await?;
```

to:

```rust
        let (handle, title) = self
            .add_account_inner("admin", "admin", root, SpaceChoice::Personal)
            .await?;
```

In `add_account_as`, change:

```rust
        self.add_account_inner(username, password, root).await
```

to:

```rust
        self.add_account_inner(username, password, root, SpaceChoice::Personal)
            .await
```

- [ ] **Step 6: Add the public `add_account_on_space` method**

Immediately **after** `add_account_as` (before `bare_url`), add:

```rust
    /// Runs the full account-setup flow for an arbitrary user, selecting a named
    /// **project space** (instead of personal) for sync. Roots the account at
    /// `sync_dir/<username>/` so accounts never share a local path; the returned
    /// [`AccountHandle`]'s `personal_sync_dir` points at the project space's
    /// local root (`sync_dir/<username>/<space_name>`). The space must already
    /// be shared with the user (e.g. via `SpaceProvisioner::assign_role`) or it
    /// will not appear in `SpacesListed` and this errors.
    pub async fn add_account_on_space(
        &mut self,
        username: &str,
        password: &str,
        space_name: &str,
    ) -> Result<(AccountHandle, String)> {
        let root = self.sync_dir.path().join(username);
        self.add_account_inner(
            username,
            password,
            root,
            SpaceChoice::Named(space_name.to_owned()),
        )
        .await
    }
```

- [ ] **Step 7: Format**

Run: `cargo fmt`
Expected: reformatted if needed, no errors.

- [ ] **Step 8: Verify the whole acceptance crate still compiles (incl. existing tests)**

Run: `cargo test -p acceptance-test --no-run`
Expected: builds cleanly. All existing test binaries (`account_setup`, `sync`, `delete`, `edit`, `move_rename`, `pause_resume`, `remove_account`, `duplicate_account`, `tray`, `multi_account`, `remove_one_of_two`) still compile against the unchanged `add_account` / `add_account_as` / `personal_sync_dir()` API.

- [ ] **Step 9: Commit** (sandbox disabled — signing key needed)

```bash
cargo fmt
git add crates/acceptance-test/src/fixture.rs
git commit -s -m "test(acceptance): add add_account_on_space via SpaceChoice selector"
git cat-file commit HEAD | grep gpgsig   # verify signature present
```

---

## Task 4: Write the role-matrix test `tests/space_roles.rs`

Four scenarios. Each provisions its own uniquely-named user + project space (independent, concurrency-safe), assigns a role, sets up the role-holder's account on that space, and asserts sync behaviour bidirectionally. Editor/Manager assert a full round-trip; Viewer/Secure-Viewer assert down-sync works but a local edit never reaches the server. Secure Viewer skips-and-logs if the role is unavailable.

**Files:**
- Create: `crates/acceptance-test/tests/space_roles.rs`
- Modify: `crates/acceptance-test/Cargo.toml`

- [ ] **Step 1: Register the test in `Cargo.toml`**

In `crates/acceptance-test/Cargo.toml`, add this entry after the `remove_one_of_two` `[[test]]` block:

```toml
[[test]]
name = "space_roles"
path = "tests/space_roles.rs"
```

- [ ] **Step 2: Write the test file**

Create `crates/acceptance-test/tests/space_roles.rs`:

```rust
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

//! Migrated from owncloud/client test/gui/tst_spaces (editable scenarios).
//!
//! Requires Tier 2 space + role provisioning: the admin creates a project space
//! and assigns a role (Space Editor / Manager / Viewer / Secure Viewer) to a
//! freshly-provisioned user via the Graph API, then that user adds the space as
//! an account and syncs it.
//!
//! The sync client performs NO client-side role enforcement (confirmed in the
//! design doc): a viewer's local edit is uploaded and rejected by the server.
//! So read-only roles are verified by a NEGATIVE server-side assertion (the
//! locally-written file never appears in the space), not a client-side block.
//! Client-side read-only enforcement is recorded as a product gap.

use std::time::Duration;

use acceptance_test::fixture::TestEnvironment;
use acceptance_test::ocis_client::OcisClient;
use acceptance_test::poll::poll_until;
use acceptance_test::provision::{RoleAssignment, SpaceProvisioner, UserProvisioner};
use acceptance_test::testutil::skip_if_no_acceptance;

/// Shared setup: provision a uniquely-named user + project space, assign
/// `role_display_name`, and (if the role exists) add the user's account on that
/// space. Returns `None` when the role is unavailable on this oCIS, so the
/// caller can skip-and-log.
///
/// On success returns the live `TestEnvironment`, the provisioner (for cleanup),
/// the provisioned user, an admin `OcisClient` scoped to the project space, and
/// the user's `AccountHandle`.
struct RoleScenario {
    env: TestEnvironment,
    user_provisioner: UserProvisioner,
    user_id: String,
    space_admin_ocis: OcisClient,
    user_sync_dir: std::path::PathBuf,
}

async fn setup_role_scenario(
    role_label: &str,
    role_display_name: &str,
) -> Option<RoleScenario> {
    let mut env = TestEnvironment::start()
        .await
        .expect("TestEnvironment::start");

    let user_provisioner = UserProvisioner::new(env.ocis_url.clone())
        .await
        .expect("UserProvisioner::new");
    let space_provisioner = SpaceProvisioner::new(env.ocis_url.clone())
        .await
        .expect("SpaceProvisioner::new");

    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let short = &suffix[..8];
    let user_name = format!("{role_label}-{short}");
    let space_name = format!("项目 {role_label} {short}"); // non-ASCII space name

    let user = user_provisioner
        .create_user(&user_name, "Secret123!", role_label)
        .await
        .expect("create role user");
    let space = space_provisioner
        .create_project_space(&space_name)
        .await
        .expect("create project space");

    // Assign the role; bail out (skip) if this oCIS lacks the role definition.
    let assignment = space_provisioner
        .assign_role(&space.id, &user.id, role_display_name)
        .await
        .expect("assign_role");
    if assignment == RoleAssignment::Unavailable {
        eprintln!(
            "SKIP: role {role_display_name:?} is unavailable in this oCIS config; \
             scenario {role_label} not run"
        );
        let _ = user_provisioner.delete_user(&user.id).await;
        return None;
    }

    // The role-holder adds the project space as an account and syncs it.
    let (handle, _) = env
        .add_account_on_space(&user.username, &user.password, &space.name)
        .await
        .expect("add_account_on_space");

    // Admin client scoped to the project space for server-side assertions.
    let space_admin_ocis =
        OcisClient::from_credentials_on_space(env.ocis_url.clone(), "admin", "admin", &space.id)
            .await
            .expect("space admin OcisClient");

    Some(RoleScenario {
        env,
        user_provisioner,
        user_id: user.id,
        space_admin_ocis,
        user_sync_dir: handle.personal_sync_dir,
    })
}

/// Editors can write: full bidirectional round-trip in the shared space.
#[tokio::test]
async fn test_space_editor_round_trip() {
    if skip_if_no_acceptance() {
        return;
    }
    let scenario = setup_role_scenario("editor", "Space Editor")
        .await
        .expect("Space Editor role must exist in a stock oCIS");
    assert_round_trip(&scenario).await;
    let _ = scenario
        .user_provisioner
        .delete_user(&scenario.user_id)
        .await;
}

/// Managers can write too: same round-trip, confirming manager assignment.
#[tokio::test]
async fn test_space_manager_round_trip() {
    if skip_if_no_acceptance() {
        return;
    }
    let scenario = setup_role_scenario("manager", "Space Manager")
        .await
        .expect("Space Manager role must exist in a stock oCIS");
    assert_round_trip(&scenario).await;
    let _ = scenario
        .user_provisioner
        .delete_user(&scenario.user_id)
        .await;
}

/// Viewers are read-only: down-sync works, a local edit never reaches the server.
#[tokio::test]
async fn test_space_viewer_read_only() {
    if skip_if_no_acceptance() {
        return;
    }
    let scenario = setup_role_scenario("viewer", "Space Viewer")
        .await
        .expect("Space Viewer role must exist in a stock oCIS");
    assert_read_only(&scenario).await;
    let _ = scenario
        .user_provisioner
        .delete_user(&scenario.user_id)
        .await;
}

/// Secure Viewers are read-only as well; skip-and-log if the role is unavailable.
#[tokio::test]
async fn test_secure_viewer_read_only() {
    if skip_if_no_acceptance() {
        return;
    }
    let Some(scenario) = setup_role_scenario("secureviewer", "Secure Viewer").await else {
        return; // role unavailable on this oCIS — logged in setup_role_scenario
    };
    assert_read_only(&scenario).await;
    let _ = scenario
        .user_provisioner
        .delete_user(&scenario.user_id)
        .await;
}

/// Bidirectional round-trip for write-capable roles: a server-created file
/// down-syncs locally, and a locally-created file up-syncs to the space.
async fn assert_round_trip(s: &RoleScenario) {
    // Down-sync: admin PUTs into the space → appears in the user's local dir.
    let down_file = "项目 down?file.txt";
    s.space_admin_ocis
        .put(down_file, b"server body")
        .await
        .expect("admin put into project space");
    poll_until(
        || {
            let p = s.user_sync_dir.join(down_file);
            async move { p.exists() }
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("server file did not down-sync to the user's local space dir");

    // Up-sync: user writes locally → appears in the space server-side.
    let up_file = "项目 up<file>.txt";
    std::fs::write(s.user_sync_dir.join(up_file), b"local body").expect("write local up file");
    poll_until(
        || async { s.space_admin_ocis.exists(up_file).await.unwrap_or(false) },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("local file did not up-sync to the project space");
}

/// Read-only behaviour: a server-created file still down-syncs (proving the
/// account is genuinely connected to the space), but a locally-created file is
/// NEVER accepted by the server.
async fn assert_read_only(s: &RoleScenario) {
    // Down-sync still works for read-only roles.
    let down_file = "项目 ro down?file.txt";
    s.space_admin_ocis
        .put(down_file, b"server body")
        .await
        .expect("admin put into project space");
    poll_until(
        || {
            let p = s.user_sync_dir.join(down_file);
            async move { p.exists() }
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("server file did not down-sync for read-only role");

    // Up-sync must be rejected: write locally, wait, assert it never appears.
    let up_file = "项目 ro up<file>.txt";
    std::fs::write(s.user_sync_dir.join(up_file), b"forbidden body")
        .expect("write local up file");
    // Give the daemon ample time to attempt (and the server to reject) the upload.
    tokio::time::sleep(Duration::from_secs(15)).await;
    assert!(
        !s.space_admin_ocis.exists(up_file).await.unwrap_or(true),
        "a read-only role's local file must NOT appear in the project space"
    );
}
```

> **Executor note (down-sync is the connectivity proof):** the read-only test asserts down-sync positively *before* the negative up-sync check so a failure cleanly distinguishes "account never connected to the space" (down-sync fails) from "write was correctly rejected" (up-sync absent). Keep that ordering.

- [ ] **Step 3: Compile gate**

Run: `cargo test -p acceptance-test --test space_roles --no-run`
Expected: compiles; produces a test binary.

- [ ] **Step 4: Run against the acceptance environment (if available)**

```bash
cargo build --workspace --bins
OCIS_ACCEPTANCE=1 cargo test -p acceptance-test --test space_roles -- --nocapture
```

Expected: `test_space_editor_round_trip`, `test_space_manager_round_trip`, `test_space_viewer_read_only` pass; `test_secure_viewer_read_only` passes or logs the unavailable-role skip. (If Docker/display are unavailable, this step is deferred to CI; the compile gate stands in.)

> **Executor note (real-behaviour fallback):** if the editor/manager round-trip fails on the **up-sync** half, the project space may not be writable through the same WebDAV path shape used for personal spaces, or role propagation may lag — first observe the daemon's `SyncFinished` errors and the server state, then adjust (e.g. increase the poll timeout) rather than weakening the assertion. If the read-only **negative** assertion fails (the file *did* appear), that is a genuine finding about server enforcement — report it; do not loosen the test to hide it.

- [ ] **Step 5: Format and commit** (sandbox disabled — signing key needed)

```bash
cargo fmt
git add crates/acceptance-test/tests/space_roles.rs crates/acceptance-test/Cargo.toml
git commit -s -m "test(acceptance): migrate tst_spaces editable role-matrix scenarios"
git cat-file commit HEAD | grep gpgsig   # verify signature present
```

---

## Task 5: Update the gap registry and final verification

- [ ] **Step 1: Record the confirmed read-only-enforcement gap**

In `docs/design/2026-06-16-gui-acceptance-test-migration-design.md`, find the product-feature-gaps table row:

```
| Read-only / viewer permission enforcement on the client | `tst_spaces` (viewer/downloader cannot edit) | Needs confirmation whether enforced client-side. |
```

Replace its Notes cell so the row reads:

```
| Read-only / viewer permission enforcement on the client | `tst_spaces` (viewer/downloader cannot edit) | Confirmed NOT enforced client-side: the client attempts the upload and the server rejects it. `tst_spaces` editable scenarios migrate the server-observable behaviour (see 2026-06-16-space-role-provisioning-design.md). |
```

- [ ] **Step 2: Whole-crate compile of all tests**

Run: `cargo test -p acceptance-test --no-run`
Expected: every test binary builds, including the new `space_roles` and all pre-existing ones.

- [ ] **Step 3: Skip-path sanity (no Docker needed)**

Run: `cargo test -p acceptance-test`
Expected: every test runs and is skipped (prints "Skipping: OCIS_ACCEPTANCE not set"), exit code 0.

- [ ] **Step 4: Full acceptance run (if Docker + display available)**

```bash
cargo build --workspace --bins
OCIS_ACCEPTANCE=1 cargo test -p acceptance-test -- --nocapture
```

Expected: all tests pass, including the four new role-matrix tests (Secure Viewer may log a skip).

- [ ] **Step 5: Confirm `cargo fmt` is clean**

Run: `cargo fmt --check`
Expected: no diff.

- [ ] **Step 6: Commit the gap-registry update** (sandbox disabled — signing key needed)

```bash
git add docs/design/2026-06-16-gui-acceptance-test-migration-design.md
git commit -s -m "docs(acceptance): record confirmed client read-only enforcement gap"
git cat-file commit HEAD | grep gpgsig   # verify signature present
```

- [ ] **Step 7: Push the branch and open a PR**

```bash
git push -u origin test/space-role-provisioning
gh pr create --fill --base main
```

Reference the design doc in the PR body. The PR closes the **space + role provisioning** portion of Tier 2; logout/login lifecycle and space-selection GUI driving remain deferred (note this in the PR description).

---

## What this plan does NOT cover (deferred)

- **Client-side role/permission enforcement** — a separate product feature (analysis recorded for future work); this plan migrates only the server-observable behaviour.
- **Logout / login lifecycle** (`tst_loginLogout`) — separate deferred Tier 2 item.
- **Space-selection GUI driving / selective-sync UI** — cross-refs `2026-05-13-space-selection-design.md`.
- **Project-space deletion/cleanup** beyond best-effort user deletion (unique space names prevent collisions across runs).
```
