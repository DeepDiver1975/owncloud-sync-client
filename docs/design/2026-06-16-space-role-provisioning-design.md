# GUI Acceptance Test Migration — Tier 2: Space + Role Provisioning Design

**Date:** 2026-06-16
**Status:** Approved
**Branch:** `test/space-role-provisioning`

## Overview

This closes the third Tier 2 harness gap from the migration backlog: **space +
role provisioning**, which unblocks the editable `tst_spaces` scenarios from the
old `owncloud/client` Squish suite.

Today the acceptance harness can provision additional users (`UserProvisioner`,
PR #71) but cannot create a **project space** or assign a **role** to a user on
that space, and the fixture only ever syncs a user's **personal** space. This
design adds:

1. A `SpaceProvisioner` (admin Graph-API wrapper) that creates a project space,
   resolves role definitions by name, and assigns a role to a user.
2. A fixture method `add_account_on_space()` that runs the account-setup flow for
   a user and selects a **named project space** (instead of personal) for sync.
3. A role-matrix acceptance test (`tests/space_roles.rs`) covering Space Viewer,
   Space Editor, Space Manager, and Secure Viewer.

This is **harness + tests only**. No daemon or product feature is added.

## Key finding driving scope

The sync client performs **no client-side role/permission enforcement**. Grep of
`crates/daemon/src` and `crates/ocis-client/src` finds no read-only, role, or
permission handling. A viewer who edits a local file does not get blocked by the
client — the client attempts the upload and the **server rejects it**.

Consequence: the "viewer cannot edit" scenarios are migrated as **negative
server-side assertions** (the locally-written file never appears in the space),
not as client-side enforcement checks. Client-side read-only enforcement is
recorded as a product gap.

## Architecture

Per-scenario data flow (all admin-driven setup, then the role-holder syncs):

```
UserProvisioner.create_user(role_holder)          // existing (PR #71)
SpaceProvisioner.create_project_space(name)        // new
SpaceProvisioner.assign_role(space, user, role)    // new (resolves role by name)
TestEnvironment.add_account_on_space(user, space)  // new fixture path
  -> assert sync behaviour for that role (bidirectional)
cleanup: delete user (best-effort); space left best-effort
```

| Component | File | Action | Responsibility |
|---|---|---|---|
| `SpaceProvisioner` + `RoleAssignment` outcome | `crates/acceptance-test/src/provision.rs` | Modify (extend) | Admin Graph wrapper: create project space, resolve role definition by display name, assign role to a user on a space. Same insecure-basic-auth style as `UserProvisioner`. |
| `add_account_on_space()` | `crates/acceptance-test/src/fixture.rs` | Modify | Sibling of `add_account_as`; selects a named project space for `SetAccountFolders`. `add_account` / `add_account_as` unchanged. |
| `tests/space_roles.rs` | `crates/acceptance-test/tests/space_roles.rs` | Create | One `#[tokio::test]` per role. |
| `Cargo.toml` | `crates/acceptance-test/Cargo.toml` | Modify | Register the `space_roles` test. |

**Tech stack:** Rust, tokio, `crates/acceptance-test` harness (`TestEnvironment`,
`DaemonIpcClient`, `OcisClient`, `UserProvisioner`, `poll_until`), oCIS over
Docker Compose, Graph API, WebDAV.

## Graph API specifics (`SpaceProvisioner`)

Constructed exactly like `UserProvisioner`: insecure reqwest client + bootstrap
`admin`/`admin` basic auth against the test oCIS base URL.

- **Create project space** — `POST /graph/v1.0/drives` with
  `{"name": "<unique-name>", "driveType": "project"}`. Returns the drive `id`,
  which is the space id surfaced later by `ListSpaces` and used in WebDAV URLs.
  Space names are suffixed with a short uuid so concurrent test binaries never
  collide (same convention as provisioned usernames).

- **Resolve role definition by name** —
  `GET /graph/v1.0/roleManagement/permissions/roleDefinitions`, find the
  `unifiedRoleDefinition` whose `displayName` matches the requested role
  (`Space Viewer`, `Space Editor`, `Space Manager`, `Secure Viewer`).
  Resolving **by display name** (not a hardcoded UUID) is what lets an
  unavailable role degrade gracefully.
  - If the role is **not found**, `assign_role` returns a typed
    `RoleAssignment::Unavailable` outcome rather than erroring, so the calling
    scenario can **skip-and-log** ("role X unavailable in this oCIS config")
    while the rest of the matrix still runs.

- **Assign role on the space** —
  `POST /graph/v1.0/drives/{driveId}/root/invite` with
  `{"recipients": [{"objectId": "<userId>"}], "roles": ["<roleDefinitionId>"]}`.

> **Executor note:** if `POST /graph/v1.0/drives` returns 403/permission-denied,
> the test oCIS config may not allow admin space creation. That is a
> harness/environment finding — stop and report it (it blocks the whole effort),
> as with the user-creation note in the multi-user plan.

## Fixture: `add_account_on_space()`

A sibling to `add_account_as`. It reuses the same account-setup IPC flow but, at
the `ListSpaces` → `SetAccountFolders` step, selects a space **by name** (the
project space the admin shared) instead of the `personal` space.

- Signature (illustrative):
  `async fn add_account_on_space(&mut self, username: &str, password: &str, space_name: &str) -> Result<(AccountHandle, String)>`
- Roots the account at `sync_dir/<username>/` (same as `add_account_as`); the
  returned `AccountHandle.personal_sync_dir` points at the project space's local
  root (`sync_dir/<username>/<space_name>`). (The field name stays
  `personal_sync_dir` for API compatibility; it denotes "this account's synced
  space root".)
- The shared core may be a small generalization of `add_account_inner` that
  takes a space selector (`personal` vs a named space); `add_account` and
  `add_account_as` continue to default to personal, so every existing test
  compiles and behaves unchanged.
- If the named space is absent from `SpacesListed` (sharing/role assignment did
  not propagate in time), the method errors clearly so the scenario fails with a
  diagnostic rather than hanging.

## Scenario matrix (`tests/space_roles.rs`)

One `#[tokio::test]` per role. Each test provisions its **own** uniquely-named
user **and** project space, so tests are independent and concurrency-safe. Every
test is **bidirectional**. All synced paths use non-ASCII (Chinese) **and**
HTTP-encoding characters (space, `?`, `<`/`>`) per the project rule.

Per-user server-side assertions use `OcisClient::from_credentials` for the
role-holder; admin-side writes (to seed the down-sync half) use an admin
`OcisClient` scoped to the project space.

| Test | Role | Down-sync (server → local) | Up-sync (local → server) |
|---|---|---|---|
| `test_space_editor_round_trip` | Space Editor | admin PUTs a file into the space → assert it appears in the editor's local space dir | editor writes a local file → assert it appears in the space server-side |
| `test_space_manager_round_trip` | Space Manager | same as editor | same as editor (confirms manager-role assignment + sync) |
| `test_space_viewer_read_only` | Space Viewer | admin PUTs a file → assert it appears locally | viewer writes a local file → **bounded-poll assert it NEVER appears in the space server-side** |
| `test_secure_viewer_read_only` | Secure Viewer | (if role available) admin PUTs → appears locally | same negative assertion; **skip + log if role unavailable** |

**Negative assertion semantics:** the viewer up-sync writes a local file, waits a
bounded interval, and asserts the file is absent from the project space
server-side. This is behaviour-agnostic — it does not depend on the daemon
emitting any specific sync-error event. Down-sync is verified positively so the
test still proves the account is genuinely connected to the space.

**Secure Viewer availability:** `assign_role` returns `RoleAssignment::Unavailable`
when the `Secure Viewer` role definition is absent; the test logs a clear skip
message and returns success, so a stock oCIS without that role does not fail the
suite.

**Cleanup:** each test deletes its provisioned user best-effort (as in the
multi-user tests). Project spaces are left in place (best-effort hygiene; unique
names prevent collisions). oCIS project-space deletion requires a disable +
purge dance that is out of scope for test hygiene.

## Conventions (consistent with the Tier 2 multi-user plan)

- **Compile gate (no Docker/display):**
  `cargo test -p acceptance-test --test space_roles --no-run`
- **Full acceptance run:**
  `OCIS_ACCEPTANCE=1 cargo test -p acceptance-test --test space_roles -- --nocapture`
  (without `OCIS_ACCEPTANCE`, `skip_if_no_acceptance` early-returns every test).
- **Commit signing:** every commit uses `git commit -s` (DCO) and is PGP-signed.
  The signing key is blocked inside the command sandbox, so signing commits run
  with the sandbox disabled; verify with `git cat-file commit HEAD | grep gpgsig`.
  Run `cargo fmt` before each commit.
- **Provisioning constants:** role-holder password `"Secret123!"`; usernames and
  space names suffixed with a short uuid.
- **Branch:** `test/space-role-provisioning`. Never push to `main`; open a PR.

## Gap registry updates (migration design doc)

- **Client-side read-only / viewer enforcement** — confirmed **not** implemented.
  The client attempts the upload; the server rejects it. Recorded as a product
  gap (was previously "needs confirmation"). The editable `tst_spaces` scenarios
  migrate the *server-observable* behaviour, not client enforcement.
- **Activity-view "viewer blacklist" display** — remains a GUI product gap.

## Out of scope / non-goals

- Logout / login lifecycle (`tst_loginLogout`) — separate deferred Tier 2 item.
- Space-selection GUI driving and selective-sync UI (cross-refs the deferred
  `2026-05-13-space-selection-design.md`).
- Any new daemon or product feature (read-only enforcement, Activity view).
- Project-space deletion/cleanup beyond best-effort.

## References

- Multi-user provisioning (predecessor): `docs/design/2026-06-16-gui-acceptance-test-tier2-multiuser-design.md` and its plan.
- Migration backlog + gap registry: `docs/design/2026-06-16-gui-acceptance-test-migration-design.md`.
- Existing provisioner: `crates/acceptance-test/src/provision.rs`.
- Graph client (read-only spaces): `crates/ocis-client/src/graph/mod.rs`.
- Protocol: `crates/daemon/src/gui_ipc/protocol.rs` (`ListSpaces`, `SetAccountFolders`, `SpacesListed`).
- Deferred space-selection: `docs/superpowers/specs/2026-05-13-space-selection-design.md`.
- Old GUI tests: https://github.com/owncloud/client/tree/master/test/gui (`tst_spaces`).
