---
title: Branch Integration — feat/gui-visual-redesign onto origin/main
date: 2026-05-06
status: approved
---

## Context

`feat/gui-visual-redesign` diverged from `main` at `64b005bf`. Since then, `main` received one squash-merge commit (`c5e5b3ba`, PR #11) that added real account-setup business logic across 23 files including several GUI files the feature branch also touched.

## Approach

Rebase `feat/gui-visual-redesign` onto `origin/main` for a linear history, then force-push. Four files require conflict resolution.

## Conflict Resolutions

### `crates/gui/src/model.rs` and `crates/gui/src/views/mod.rs`

Both branches made identical additions (`PickLocalFolder` variant; `pub mod pick_local_folder`). Accept either side — the result is the same.

### `crates/gui/src/main.rs`

Two non-overlapping logical changes must be combined:

- **Keep from main:** daemon reconnect logic added to `IcedApp::update()` (the `DaemonDisconnected` handler block and the subscription nil-guard).
- **Keep from feature branch:** the complete sidebar layout rebuild of `IcedApp::view()`, which already routes `PickLocalFolder` through the nav correctly.
- **Discard from main:** the `PickLocalFolder { .. }` dispatch arm that was added to main's old flat `view()` — superseded by the feature's rebuilt view.

### `crates/gui/src/views/pick_local_folder.rs`

Use the feature branch's styled version as the base. Replace stub navigation calls with real business-logic messages:

| Stub (feature branch) | Real message (from main) |
|-----------------------|--------------------------|
| `on_input` closure using `Message::NavigateTo(View::PickLocalFolder { .. })` | `Message::PickLocalFolderPathChanged(v)` |
| "Start Syncing" `.on_press(Message::NavigateTo(View::SyncStatus))` | `Message::PickLocalFolderSubmit` |
| "Cancel" `.on_press(Message::NavigateTo(View::SyncStatus))` | `Message::PickLocalFolderCancel` |

Drop the `account_id: Uuid` parameter from the view function signature — the app state owns it and it is not needed by the view. Align the signature to `(display_name, url, local_path_input, error)` matching how `main.rs` calls it.

## Verification

After rebase and manual resolutions: `cargo build` must succeed with no errors or new warnings before force-pushing.

## Out of Scope

- No functional changes to non-GUI crates.
- No restyling of the views added by PR #11 (acceptance tests, daemon IPC, ocis-client) — those are backend and out of scope for this visual redesign branch.
