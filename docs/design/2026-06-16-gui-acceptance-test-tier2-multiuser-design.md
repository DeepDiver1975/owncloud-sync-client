# Design: GUI Acceptance Test Migration — Tier 2 (Multi-User Provisioning)

**Date:** 2026-06-16
**Status:** Draft — pending user review

## Problem

The GUI acceptance-test migration backlog
(`docs/design/2026-06-16-gui-acceptance-test-migration-design.md`) defines a
**Tier 2** of scenarios that are blocked not by missing product features but by
missing *test-harness* capabilities. Tier 2 lists three independent harness
extensions:

1. **Multi-user provisioning** — create additional oCIS users and run the
   account-setup flow per user.
2. **Logout / login lifecycle** — needs a GUI logout affordance + re-login flow.
3. **Space + role provisioning** — admin creates a project space and assigns a
   role via the Graph API.

This design covers **only multi-user provisioning** (item 1) and the scenario
migrations it unblocks. Items 2 and 3 are explicitly deferred (see
[Deferred work](#deferred-work)).

## Findings from the current codebase

These observations from the existing harness/daemon shaped the design:

- **No logout affordance exists.** A grep across `crates/` for
  `logout`/`sign-out` found nothing in the daemon or GUI, and there is no logout
  `DaemonCommand`. `tst_loginLogout` therefore cannot be migrated without first
  building a product feature — out of scope here.
- **The daemon dedups accounts by `url + user_id`**
  (`crates/daemon/src/oidc_callback.rs:200`), not by URL alone. Two *different*
  users on the *same* oCIS URL is a valid, non-duplicate configuration — exactly
  what multi-account tests need.
- **`OcisClient::from_credentials` is already user-parameterized**
  (`crates/acceptance-test/src/ocis_client.rs:31`). A second user's server-side
  assertions just construct another `OcisClient`.
- **The OIDC login helper is already user-parameterized.**
  `complete_oidc_login(auth_url, port, username, password)`
  (`crates/acceptance-test/src/playwright.rs:11`) accepts arbitrary credentials;
  `add_account()` simply hardcodes `"admin"/"admin"`.
- **`GraphClient` exists but is read-only** (`me/drives`, `drives/{id}`, `me` —
  `crates/ocis-client/src/graph/mod.rs`). User creation will be a new
  admin-basic-auth helper in the acceptance crate, mirroring `OcisClient`'s
  construction style, rather than an extension of `GraphClient` (which is built
  around a per-user OIDC `TokenSet`).
- **The fixture is single-account today.** `TestEnvironment` holds scalar
  `account_id` / `personal_folder_id` / `personal_space_name` fields populated by
  `add_account()`. Existing tests (`account_setup`, `sync`, `delete`, `edit`,
  `move_rename`, `pause_resume`, `remove_account`, `duplicate_account`) depend on
  `add_account()`, `personal_sync_dir()`, `personal_folder_id()`, `account_id()`,
  and `connect_fresh_ipc()`. Backward compatibility is a hard requirement.

## Goal & scope

Extend the acceptance harness to provision additional oCIS users at runtime via
the admin Graph API, and migrate the three multi-account scenarios that this
unblocks:

| Old scenario | Migrated as |
|---|---|
| `tst_addAccount` — adding multiple accounts | `test_add_multiple_accounts` — two accounts present + independent bidirectional sync |
| `tst_removeAccountConnection` — remove one of two | `test_remove_one_of_two_accounts` — remove second account, snapshot shows the first |
| `tst_activity` — multi-account filter | Subsumed as the **behaviour** that two accounts sync independently; the Activity *view* stays a recorded GUI gap |

**Out of scope:** logout/login lifecycle, space/role provisioning, the Activity
GUI view. Recorded in [Deferred work](#deferred-work).

## Architecture

Two harness building blocks plus three test files.

### Block 1 — User provisioning (`crates/acceptance-test/src/provision.rs`, new)

An admin-authenticated wrapper over the oCIS Graph user API, constructed like
`OcisClient` (insecure reqwest client + admin basic-auth).

```rust
/// Admin-authenticated oCIS user provisioning via the Graph API.
pub struct UserProvisioner {
    client: reqwest::Client,
    base_url: Url,
    admin_user: String,
    admin_pass: String,
}

/// A user created on the oCIS server for the duration of a test.
pub struct ProvisionedUser {
    pub id: String,        // Graph user id (for deletion)
    pub username: String,  // onPremisesSamAccountName / login name
    pub password: String,
}

impl UserProvisioner {
    /// Construct with admin/admin against the running oCIS (insecure client).
    pub async fn new(base_url: Url) -> Result<Self>;

    /// POST /graph/v1.0/users with onPremisesSamAccountName, displayName,
    /// and passwordProfile { password, forceChangePasswordNextSignIn: false }.
    pub async fn create_user(
        &self,
        username: &str,
        password: &str,
        display_name: &str,
    ) -> Result<ProvisionedUser>;

    /// DELETE /graph/v1.0/users/{id} — best-effort cleanup.
    pub async fn delete_user(&self, id: &str) -> Result<()>;
}
```

**Unique naming:** callers suffix usernames with a short uuid
(e.g. `alice-<short-uuid>`) so concurrently-running test binaries never collide.
`uuid` is already a crate dependency. Because names are unique, a leaked user
from a crashed run never breaks a later run — `delete_user` is hygiene, not
correctness.

oCIS auto-creates the personal space on the user's first login, so no extra
server-side setup is required after `create_user`.

### Block 2 — Parameterized account add (`crates/acceptance-test/src/fixture.rs`)

`add_account()` is refactored so its OIDC + `ListSpaces` + `SetAccountFolders`
body is parameterized by credentials and returns a per-account handle.

```rust
/// Per-account results from a successful account-setup flow.
pub struct AccountHandle {
    pub account_id: Uuid,
    pub personal_folder_id: Uuid,
    pub personal_space_name: String,
    /// Local root of this account's personal space: sync_dir/<username>/<space_name>.
    pub personal_sync_dir: PathBuf,
}

impl TestEnvironment {
    /// Full OIDC + ListSpaces + SetAccountFolders flow for an arbitrary user.
    /// Roots the account at sync_dir/<username>/ so multiple accounts never
    /// share a local path. Returns the AccountHandle and the OIDC callback-page
    /// title.
    pub async fn add_account_as(
        &mut self,
        username: &str,
        password: &str,
    ) -> Result<(AccountHandle, String)>;

    /// Unchanged signature. Delegates to add_account_as("admin", "admin"),
    /// stores the handle's fields into the existing scalar fields, and returns
    /// the callback-page title — so every existing test compiles unchanged.
    pub async fn add_account(&mut self) -> Result<String>;
}
```

The existing `personal_sync_dir()`, `personal_folder_id()`, `account_id()`
accessors continue to reflect the **admin** account (the one `add_account()`
sets up), preserving all current test behaviour.

### Per-account local layout

Each account roots at `sync_dir/<username>/`, where `<username>` is the
provisioned (unique) login name:

```
sync_dir/
  admin/<SpaceName>/…        # account A (add_account())
  alice-xxxx/<SpaceName>/…   # account B (add_account_as("alice-xxxx", …))
```

This mirrors a real multi-account layout and is robust even if two accounts'
personal spaces share a display name.

## Data flow — remove one of two

1. `env.add_account()` (admin) → account A persisted; scalar fields set.
2. `provisioner.create_user("alice-<uuid>", pw, "Alice")`.
3. `env.add_account_as("alice-<uuid>", pw)` → `AccountHandle` for account B.
4. Probe via `connect_fresh_ipc()` → `AccountSnapshot` lists **2** accounts with
   distinct `account_id`s.
5. `RemoveAccount { account_id: B }`.
6. Fresh probe → `AccountSnapshot` lists **1** account (A, correct id).
7. Assert account B's local files per the daemon's *actual* `RemoveAccount`
   semantics — confirmed during implementation (retained vs. removed) and
   asserted accordingly, not assumed.

## The three migrated tests

All follow the established `skip_if_no_acceptance` early-return +
`TestEnvironment` pattern. Paths use non-ASCII (Chinese) **and** HTTP-encoding
characters (space, `?`, `<`), per the standing convention.

### `tests/multi_account.rs` → `test_add_multiple_accounts`

Provision Alice; `add_account()` (admin) + `add_account_as(alice)`. Assert via
fresh-IPC `AccountSnapshot` that **2** accounts exist with distinct `account_id`s
and URLs. Then a bidirectional independence check: each account up-syncs a file
(special-character name) into *its own* personal space, verified with a per-user
`OcisClient`, confirming the two sync loops do not cross-contaminate. This
subsumes the `tst_activity` multi-account *behaviour*; the Activity GUI view is
recorded as a gap.

### `tests/remove_one_of_two.rs` → `test_remove_one_of_two_accounts`

The [data flow above](#data-flow--remove-one-of-two): two accounts → remove the
second → snapshot shows only the first. Asserts the surviving account's id and
the post-removal local-file state per actual daemon semantics.

### Cleanup

`UserProvisioner::delete_user` runs best-effort in a teardown step of each test.
Because provisioned users are uniquely named, cleanup failure is harmless.

## Testing strategy

- **Compile gate** (no Docker/display): `cargo test -p acceptance-test --test <name> --no-run`
  for each new test — the honest red/green signal in CI-less environments.
- **Skip-path sanity:** plain `cargo test -p acceptance-test` → every test skips
  (prints the `OCIS_ACCEPTANCE not set` line), exit 0.
- **Provisioning smoke test:** a standalone test that `create_user` succeeds and
  the new user can `add_account_as` + sync one file — so a provisioning failure
  is diagnosed independently of the multi-account scenario logic.
- **Full acceptance run** (CI / Docker + display):
  `OCIS_ACCEPTANCE=1 cargo test -p acceptance-test -- --nocapture`.

## Error handling

- `UserProvisioner` surfaces Graph API failures via `error_for_status()` with
  `anyhow` context naming the operation (create/delete user).
- `add_account_as` reuses the existing `wait_for(...).ok_or_else(...)` timeout
  pattern from `add_account()`; no new error model.

## Files

| File | Action |
|---|---|
| `crates/acceptance-test/src/provision.rs` | Create — `UserProvisioner`, `ProvisionedUser` |
| `crates/acceptance-test/src/lib.rs` | Modify — `pub mod provision;` |
| `crates/acceptance-test/src/fixture.rs` | Modify — `AccountHandle`, `add_account_as`, refactor `add_account` to delegate |
| `crates/acceptance-test/tests/multi_account.rs` | Create |
| `crates/acceptance-test/tests/remove_one_of_two.rs` | Create |
| `crates/acceptance-test/Cargo.toml` | Modify — register 2 `[[test]]` entries |

## Conventions

- **Commit signing:** every commit uses `git commit -s` (DCO) and is PGP-signed.
  Run `cargo fmt` before each commit; include the formatting.
- **Branch + PR:** never push to `main`; work on a feature branch and open a PR.
- **Path characters:** non-ASCII + HTTP-encoding chars in every synced path.
- **Bidirectional:** sync assertions cover up- and down-sync where file transfer
  is involved.

## Deferred work

| Item | Reason |
|---|---|
| Logout / login lifecycle (`tst_loginLogout`) | No logout affordance/command exists in the product; would require feature work, not test migration. |
| Space + role provisioning (`tst_spaces` editable scenarios) | Largest harness extension (Graph space-create + role-assign); separate session. Ties to deferred space-selection work. |
| Activity tab / view (synced + not-synced filtering) | Product GUI gap; no Activity view in the new Iced GUI. |

## References

- Migration backlog: `docs/design/2026-06-16-gui-acceptance-test-migration-design.md`
- Tier 1 plan: `docs/design/2026-06-16-gui-acceptance-test-migration-plan.md`
- Harness: `crates/acceptance-test/` (`fixture.rs`, `ocis_client.rs`, `playwright.rs`, `daemon_ipc.rs`)
- Protocol: `crates/daemon/src/gui_ipc/protocol.rs`
- Account dedup key: `crates/daemon/src/oidc_callback.rs:200`
- oCIS Graph users API: `POST /graph/v1.0/users`, `DELETE /graph/v1.0/users/{id}`
