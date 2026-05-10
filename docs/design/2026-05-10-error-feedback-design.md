# Error Feedback — Spec

**Issue:** [#16](https://github.com/DeepDiver1975/owncloud-sync-client/issues/16)
**Date:** 2026-05-10
**Branch:** fix/error-feedback

## Problem

When a folder sync fails, the sync status view shows a badge labelled "⚠ N error(s)". The badge is non-interactive — users have no way to see the actual error messages. The error strings are already held in `FolderView.errors: Vec<String>` (populated via `DaemonEvent::SyncFinished`), but are never surfaced.

## Goal

Make the error badge a clickable button that navigates to a dedicated **folder error detail view** where the user can read the full error list.

## Out of scope

- Dismissing / clearing errors
- Retry button
- Per-error actions
- Changes to the daemon or IPC protocol

---

## Design

### 1. New `View` variant — `model.rs`

```rust
View::FolderErrors {
    account_id: Uuid,
    folder_id: Uuid,
}
```

Added to the existing `View` enum. The view resolves both identifiers at render time from `app.accounts`; no additional state is stored in the variant.

### 2. New view — `crates/gui/src/views/folder_errors.rs`

Public function signature:
```rust
pub fn folder_errors_view(account: &AccountView, folder_id: Uuid) -> Element<'_, Message>
```

Layout (top to bottom):

| Element | Detail |
|---------|--------|
| Header | Folder `display_name` (13 px, `TEXT_PRIMARY`) + `local_path` (11 px, `TEXT_MUTED`) |
| Error list | Scrollable `Column` of `card_style` containers, one per error string. Error text in `STATUS_ERROR` colour, 12 px. |
| Empty state | If `errors` is empty: "No errors recorded" in `TEXT_MUTED`, centred. |
| Back button | `← Back` → `NavigateTo(View::SyncStatus)`, `ghost_button_style` |

Padding and spacing follow existing views (`[16, 20]` outer, `spacing(12)` between sections).

### 3. Wire the error badge — `sync_status.rs`

`folder_row` currently receives `&FolderView`. It is extended to also receive `account_id: Uuid`.

The `badge_msg` match arm for `FolderStatus::Error` changes from `None` to:

```rust
FolderStatus::Error => Some(Message::NavigateTo(View::FolderErrors {
    account_id,
    folder_id: folder.id,
})),
```

The call site `account_section` passes `account.id` when calling `folder_row`.

### 4. Register the new view module — `views/mod.rs`

```rust
pub mod folder_errors;
```

### 5. Dispatch the view — `main.rs`

In the `view()` method's `match &self.app.active_view` block, add a match arm:

```rust
View::FolderErrors { account_id, folder_id } => {
    if let Some(account) = self.app.accounts.iter().find(|a| a.id == *account_id) {
        gui::views::folder_errors::folder_errors_view(account, *folder_id)
    } else {
        gui::views::sync_status::sync_status_view(&self.app.accounts)
    }
}
```

The fallback to `SyncStatus` handles the edge case where the account was removed while the view was open.

Also update the two `matches!` guards above the content block (which determine nav-bar button highlight state) to include `View::FolderErrors { .. }` in the same group as `View::AccountSettings`.

---

## File change summary

| File | Change |
|------|--------|
| `crates/gui/src/model.rs` | Add `FolderErrors` variant to `View` |
| `crates/gui/src/views/folder_errors.rs` | New file — error detail view |
| `crates/gui/src/views/mod.rs` | `pub mod folder_errors;` |
| `crates/gui/src/views/sync_status.rs` | Pass `account_id` to `folder_row`; make error badge clickable |
| `crates/gui/src/main.rs` | Dispatch `View::FolderErrors` in `view()`; update nav-bar `matches!` guards |

No changes to daemon, IPC protocol, or acceptance tests required for this scope.

---

## Testing

- Unit test in `folder_errors.rs`: render with a `FolderView` containing two error strings; assert the view does not panic (compile-level coverage).
- Unit test: render with empty `errors`; assert empty-state path is exercised.
- Existing `model_tests.rs` and `update_tests.rs` will need the new `View` variant added to any exhaustive match patterns.
