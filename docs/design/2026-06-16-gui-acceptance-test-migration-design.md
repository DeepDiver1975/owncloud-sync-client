# Design: Migrate ownCloud desktop-client GUI acceptance tests to the new Rust client

**Date:** 2026-06-16
**Status:** Draft — pending user review

## Problem

The classic ownCloud desktop client carries a suite of GUI acceptance tests at
[`owncloud/client/test/gui`](https://github.com/owncloud/client/tree/master/test/gui),
written as Squish/Gherkin BDD feature files. The new Rust client
(`owncloud-sync-client`) has its own acceptance-test harness
(`crates/acceptance-test`) driving the real daemon, GUI, and a dockerized oCIS,
but only a handful of behaviours are covered so far.

We want to migrate every old GUI test scenario whose underlying
feature/functionality exists in the new client, and produce an explicit,
categorized list of everything that cannot be migrated yet — separating
**missing product features** from **missing test-harness capabilities**.

This document is the **gap analysis and migration backlog**. It contains no test
code. Writing the actual Rust tests and harness extensions is the job of the
follow-up implementation plan(s).

## Background: the two test systems

### Old client — Squish/Gherkin (`owncloud/client/test/gui`)

11 suites, ~35 scenarios, each a `test.feature` (Gherkin) + `test.py` (step
implementations) driving the Qt GUI through Squish:

| Suite | Scenarios |
|---|---|
| `tst_addAccount` | 6 (normal add, multi-account, advanced-config defaults, self-signed cert, VFS-disabled, manual-space/path-suffix) |
| `tst_syncing` | 4 (upload, sync-all-down, conflict, sync-all default selection) |
| `tst_loginLogout` | 2 (logout, login-after-logout) |
| `tst_removeAccountConnection` | 2 (remove one of two, remove only account) |
| `tst_deletFilesFolders` | 3 (delete file, delete folder, delete file+folder) |
| `tst_editFiles` | 1 (overwrite file with special chars) |
| `tst_moveFilesFolders` | 3 (move up, move down, rename) |
| `tst_activity` | 3 (filter synced, filter not-synced Linux, filter not-synced Windows) |
| `tst_checkAlltabs` | 2 (toolbar tabs, settings options) |
| `tst_spaces` | 9 (per-role: viewer/editor/manager/downloader/uploader/collaborator) |
| `tst_vfs` | 4 (enable/disable, copy-paste, move, quick toggle — all `@skipOnLinux`) |

### New client — Rust acceptance harness (`crates/acceptance-test`)

- `fixture.rs` — `TestEnvironment`: starts oCIS via Docker Compose, spawns
  `ocsyncd` (daemon) + `ocsync` (Iced GUI), connects daemon IPC, drives OIDC
  login via Playwright, and performs `add_account()` (currently single user
  `admin`/`admin`).
- `daemon_ipc.rs` — `DaemonIpcClient`: send `DaemonCommand`, await
  `DaemonEvent` with `wait_for(predicate, timeout)`.
- `ocis_client.rs` — server-side assertions (`put`, `get`, `exists`,
  `collection_exists`).
- `atspi_client.rs` — `AtSpiClient`: drive the real GUI via AT-SPI2
  (find widget by role/name, click, set text).
- `playwright.rs` — headless OIDC login.
- `poll.rs` — `poll_until` helper.

**Three driving channels** are therefore available: daemon IPC events,
filesystem assertions (+ server-side `OcisClient`), and AT-SPI2 (real GUI).

Existing tests: `account_setup.rs` (account setup, multi-space bidirectional),
`duplicate_account.rs`, `sync.rs` (down-sync, upload new, upload changed,
conflict, initial-sync empty/preseeded, new dir, file-in-subdir, watch-driven
upload), `tray.rs` (GUI launches).

Relevant `DaemonCommand`s: `TriggerSync`, `PauseFolder`, `ResumeFolder`,
`AddAccount`, `RemoveAccount`, `ListSpaces`, `SetAccountFolders`,
`AddAccountSpace`, `DismissSpace`, `Quit`.
Relevant `DaemonEvent`s: `SyncStarted/Progress/Finished`, `FileStatusChanged`,
`AccountStateChanged`, `AccountAddStarted/Failed/Completed`,
`AccountFolderAdded`, `SpacesListed`, `AccountSnapshot`, etc.

## Classification scheme

Every old scenario receives exactly one verdict:

| Verdict | Meaning | Migration action |
|---|---|---|
| ✅ **Migratable now** | Behaviour + a driving channel both exist today | Write a Rust acceptance test |
| 🟡 **Partial** | Underlying sync behaviour exists, but the scenario asserts through a GUI surface the new client lacks (Activity tab, Settings tabs, "wizard visible") | Migrate the behaviour via IPC/filesystem; record the GUI surface as a product gap |
| 🔧 **Harness gap** | Product feature exists, but the fixture cannot provision the scenario yet (multi-user, role/share assignment, logout cycle) | List the required fixture extension; migrate after harness work |
| ❌ **Missing feature** | The new client product does not implement this | Record as a product gap; defer |

Each row also records the recommended **driving channel** and any **overlap**
with an existing test.

### Standing test conventions (apply to every migrated sync test)

- **Bidirectional:** assert both up-sync and down-sync in a single test where
  the scenario involves file transfer.
- **Path characters:** test paths must include non-ASCII (Chinese/emoji) **and**
  HTTP-encoding characters (space, `?`, `<`, etc.). The old suites already favor
  special-character names (e.g. `S@mpleFile!With,$pecial&Characters.txt`,
  `` ~`!@#$^&()-_=+{[}];',textfile.txt ``); extend these with non-ASCII.
- **Skip guard:** follow the existing `OCIS_ACCEPTANCE`-not-set early-return
  pattern.

## Per-suite triage

### `tst_addAccount`

| Scenario | Verdict | Notes |
|---|---|---|
| Adding normal account | ✅ | Covered by `account_setup::test_account_setup`. Map explicitly; no new test needed. |
| Adding multiple accounts | 🔧 | Needs multi-user provisioning (Alice + Brian) + multi-account display assertion. |
| Default options in advanced configuration (download-everything / VFS defaults) | ❌ | New setup wizard has no advanced-config panel exposing VFS/download-everything toggles. |
| Adding account with self-signed certificate | ❌ | New harness/daemon runs with `insecure=true`; no certificate-accept UI flow. |
| Adding account with VFS disabled (Windows only) | ❌ | No VFS toggle in account setup. |
| Add space manually / suffix-on-existing-sync-path | 🟡 | Manual space picking ties to the deferred space-selection work; the path-suffix-on-collision behaviour is unverified and, if present, testable via config inspection. |

### `tst_syncing`

| Scenario | Verdict | Notes |
|---|---|---|
| Syncing a file to the server | ✅ | Overlaps `sync::test_upload_new_file`. Re-express as a bidirectional test with special-char paths. |
| Syncing all files/folders from the server | ✅ | Overlaps `sync::test_initial_sync_preseeded_remote`. |
| Sync a file from server + create conflict | 🟡 | Conflict behaviour covered by `sync::test_conflict_resolution`. The "Not Synced tab" / conflict-warnings-table assertion is a GUI gap. |
| Sync-all selected by default / unselect remote folders | 🟡 | Selective-sync UI relates to space-selection (deferred). Pause/resume primitives (`PauseFolder`/`ResumeFolder`) exist and are independently migratable via IPC. |

### `tst_loginLogout`

| Scenario | Verdict | Notes |
|---|---|---|
| Logging out | 🔧 | GUI logout affordance + signed-out state need verification; not a known IPC command today. |
| Login after logging out | 🔧 | Requires a re-login-without-re-adding flow and a GUI logout button. Treat as combined harness + feature gap until the logout/login lifecycle is confirmed. |

### `tst_removeAccountConnection`

| Scenario | Verdict | Notes |
|---|---|---|
| Remove one of two account connections | 🔧 | `RemoveAccount` IPC exists, but the scenario needs a second account → multi-user harness. Assert remaining account via `AccountSnapshot`. |
| Remove the only account → connection wizard visible | 🟡 | Removal behaviour testable via `RemoveAccount` + `AccountSnapshot` (empty). "Wizard visible" is an AT-SPI assertion (GUI surface). |

### `tst_deletFilesFolders` — ✅ Migratable now

Delete a file / delete a folder / delete a file **and** a folder locally, then
assert the deletion propagates to the server (and surviving items remain).
Pure IPC + filesystem + `OcisClient`. Reuse the old special-character filenames
(`textfile0-with-name-more-than-20-characters`,
`` ~`!@#$^&()-_=+{[}];',textfile.txt ``) and add non-ASCII. No existing coverage —
net-new deletion coverage.

### `tst_editFiles` — ✅ Migratable now

Overwrite an existing file (special-character name) with new content, assert the
server reflects the new content. Overlaps `sync::test_upload_changed_file`;
the migration adds the special-character + non-ASCII path dimension.

### `tst_moveFilesFolders` — ✅ Migratable now

Move a file and a folder from a deeply nested (level-5) subfolder to the sync
root and back; rename a file and a folder. Assert the server reflects every
move/rename (content preserved, old paths gone). Pure IPC + filesystem. No
existing move/rename coverage — high-value net-new.

### `tst_activity` — 🟡 Partial (mostly GUI)

The Activity tab with synced / not-synced filtering, blacklist display, and
excluded-file (`.htaccess`) display is a **product GUI gap** in the new client.
The *underlying* ignore/exclude behaviour (e.g. `.htaccess` excluded, certain
names blacklisted) is partially testable via filesystem + (if emitted)
`FileStatusChanged`. Migrate the ignore/exclude **behaviour** where observable;
record the Activity view as a product gap.

### `tst_checkAlltabs` — ❌ Missing feature

Pure GUI inventory: asserts the toolbar tabs (Add Account, Activity, Settings,
Quit) and the contents of the Settings tab (Start on Login, Monochrome icons,
Desktop notifications, Language, Sync hidden files, Edit ignored files, Log
settings, Proxy settings) and the About dialog. The new Iced GUI does not have
this tab/settings structure. Record each surface as a product gap; nothing to
migrate.

### `tst_spaces` — 🔧 Harness gap (+ deferred space-selection)

All 9 scenarios depend on an admin-created project space and a role assigned to
a second user via the Graph API:

| Scenario group | Verdict | Notes |
|---|---|---|
| Editor / Manager / Collaborator can edit/add/delete in space | 🔧 | Needs space provisioning + role assignment harness; behaviour (edit/add/delete sync) otherwise migratable. |
| Viewer/Downloader cannot edit/add (read-only enforced) | 🔧 + ❌ | Needs role harness; read-only permission propagation to the client may itself be a feature gap to confirm. |
| Viewer cannot sync space / role-based space visibility | 🟡 | Ties to space-selection (deferred — see `docs/superpowers/specs/2026-05-13-space-selection-design.md`). |

### `tst_vfs` — ❌ Missing feature (on the Linux harness)

All scenarios are `@skipOnLinux`; VFS placeholders are Windows (CloudFiles) /
macOS (FileProvider). The new client uses full-download fallback on Linux, and
the Docker acceptance harness is Linux. Not testable in the current harness →
product/harness gap on Linux. (VFS may be separately testable on Windows/macOS
runners in the future — out of scope here.)

## Migration backlog (tiered)

### Tier 1 — Migrate now (no new harness)

New `tests/*.rs` files in `crates/acceptance-test`, each following the
`skip_if_no_acceptance` + `TestEnvironment` pattern, IPC + filesystem driven,
bidirectional + special-character/non-ASCII paths:

1. **Delete** (`tst_deletFilesFolders`) — delete file, folder, and file+folder; assert server-side removal and survivor retention.
2. **Edit** (`tst_editFiles`) — overwrite special-char file; assert server content.
3. **Move/Rename** (`tst_moveFilesFolders`) — nested move up/down + rename; assert server reflects moves.
4. **Pause/Resume** (`tst_syncing` selective bits) — `PauseFolder`/`ResumeFolder` via IPC: changes do not propagate while paused, then flush on resume.
5. **Upload / Sync-all consolidation** — fold `tst_syncing` upload + sync-all into the existing bidirectional tests; add explicit mapping rather than duplicate tests.

### Tier 2 — Extend harness, then migrate

Each item names the fixture extension it requires:

1. **Multi-user provisioning** — create additional users (Alice, Brian) and run `add_account()` per user. Unblocks: `tst_addAccount` multi-account, `tst_removeAccountConnection` remove-one-of-two, `tst_activity` multi-account filter.
2. **Logout / login lifecycle** — confirm or add a GUI logout affordance and a re-login-without-re-add flow. Unblocks: `tst_loginLogout`.
3. **Space + role provisioning** — admin creates a project space and assigns a role (viewer/editor/manager/…) to a user via the Graph API. Unblocks: editable `tst_spaces` scenarios.

### Tier 3 — Behaviour-only migration of 🟡 partials

Migrate the underlying behaviour via IPC/filesystem, explicitly dropping the
GUI-surface assertion and recording it in the gap registry:

1. **Conflict** — already covered by `sync::test_conflict_resolution`; map it; the "Not Synced tab" assertion is dropped (logged as gap).
2. **Ignore/exclude** — `.htaccess`-style exclusion behaviour where observable on the filesystem / via `FileStatusChanged`.
3. **Remove-only-account** — `RemoveAccount` + empty `AccountSnapshot`; "wizard visible" dropped (logged as gap).

## Gap registry

### Product feature gaps (❌ / GUI surfaces dropped from 🟡)

| Gap | Old scenario(s) | Notes |
|---|---|---|
| Advanced account-config UI (VFS / download-everything toggle, defaults) | `tst_addAccount` (advanced config, VFS-disabled) | No advanced panel in new wizard. |
| Self-signed certificate accept flow | `tst_addAccount` (self-signed cert) | New client runs insecure in tests; no cert UI. |
| Activity tab / view (synced + not-synced filtering, blacklist/excluded display) | `tst_activity`, `tst_syncing` (conflict tab), `tst_spaces` (viewer blacklist) | No Activity view in new GUI. |
| Settings tab + options (Start on Login, Monochrome icons, Notifications, Language, Sync hidden files, Edit ignored files, Log settings, Proxy) | `tst_checkAlltabs` | No Settings tab structure. |
| Toolbar tab inventory (Add Account / Activity / Settings / Quit) + About dialog | `tst_checkAlltabs` | GUI structure differs. |
| Connection-wizard-visible-after-removal assertion | `tst_removeAccountConnection` (remove only account) | Behaviour migratable; GUI assertion dropped. |
| Linux VFS placeholders | `tst_vfs` (all) | Linux uses full-download fallback. |
| Read-only / viewer permission enforcement on the client | `tst_spaces` (viewer/downloader cannot edit) | Needs confirmation whether enforced client-side. |
| Selective-sync UI / manual space pick / path-suffix-on-collision | `tst_addAccount` (manual space, suffix), `tst_syncing` (unselect folders) | Ties to deferred space-selection. |

### Harness / tooling gaps (🔧)

| Gap | Old scenario(s) | Fixture extension needed |
|---|---|---|
| Multi-user provisioning | `tst_addAccount` (multi), `tst_removeAccountConnection` (remove one), `tst_activity` | Create users via admin Graph API; per-user `add_account()`. |
| GUI logout → login cycle | `tst_loginLogout` | Logout affordance + re-login flow. |
| Space + role assignment | `tst_spaces` (all) | Admin creates space; assign role via Graph API. |
| Space-selection driving | `tst_spaces` (visibility), `tst_syncing` (selective) | Cross-ref `2026-05-13-space-selection-design.md` (deferred). |

## Approximate tally

- ✅ Migratable now: delete (1), edit (1), move/rename (1), pause/resume (1), + map upload/sync-all to existing → **~9 scenarios**
- 🟡 Partial (behaviour migratable, GUI surface dropped): conflict, remove-only-account, ignore/exclude, manual-space-suffix, viewer-visibility → **~7 scenarios**
- 🔧 Harness gap: multi-account, remove-one-of-two, logout/login, editable spaces (editor/manager/collaborator) → **~8 scenarios**
- ❌ Missing feature: advanced config, self-signed cert, VFS-disabled, all-tabs (×2), VFS (×4), read-only enforcement → **~9 scenarios**

(The implementation plan will pin each scenario to an exact verdict and test file.)

## Out of scope (this session)

- Writing Rust acceptance-test code.
- Implementing harness extensions (multi-user, role provisioning, logout flow).
- Implementing missing product features (Activity view, Settings tab, VFS on Linux, etc.).

This document is analysis + backlog only. The follow-up implementation plan turns
the Tier 1 backlog (and as much of Tier 2/3 as desired) into concrete tasks.

## References

- Old GUI tests: https://github.com/owncloud/client/tree/master/test/gui
- New harness: `crates/acceptance-test/` (`fixture.rs`, `daemon_ipc.rs`, `atspi_client.rs`, `playwright.rs`)
- Protocol: `crates/daemon/src/gui_ipc/protocol.rs`
- Acceptance/GUI alignment: `docs/superpowers/specs/2026-05-07-acceptance-test-gui-alignment-design.md`
- Deferred space-selection: `docs/superpowers/specs/2026-05-13-space-selection-design.md`
