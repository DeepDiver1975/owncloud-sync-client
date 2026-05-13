# Folder Picker — Design Spec

**Date:** 2026-05-12  
**Branch:** fix/folder-selection  
**Status:** Approved

## Problem

The `PickLocalFolder` view requires users to type a local path into a text field. This is poor UX and inaccessible for users who cannot easily type filesystem paths.

## Decision

Replace the free-text path input with a native OS folder picker button backed by `rfd::AsyncFileDialog`. The OS picker handles folder creation on all supported platforms (macOS, Windows, Linux GTK/KDE). No free-text input is retained — the picker is the only way to set the path.

Daemon-side path validation (`SetAccountFolder` rejects non-existent paths) is unchanged — it guards the IPC contract for all callers, not just the GUI.

## Accessibility

- Single focusable button with a clear label ("Choose folder…" / "Change folder…")
- Chosen path displayed as static text, announced by screen readers when the region is focused
- "Start Syncing" is disabled (no `on_press`) until a folder is chosen — screen readers announce it as unavailable
- Native OS picker is fully accessible (VoiceOver, NVDA, JAWS tested by OS vendors)
- Tab order: heading → caption → folder well → choose/change button → Start Syncing → Cancel

## Dependency

Add to `crates/gui/Cargo.toml`:

```toml
rfd = "0.15"
```

No workspace-level change needed — this is GUI-only.

## Model (`crates/gui/src/model.rs`)

`View::PickLocalFolder` field change:

```
- local_path_input: String      // free-text, always present
+ local_path: Option<String>    // None = nothing chosen, Some(path) = picker returned a path
```

`error: Option<String>` is kept unchanged — it surfaces daemon-side errors (e.g. `AccountSetFolderFailed`) as a banner. The client-side "please enter a path" validation is removed; the submit guard (`local_path.is_some()`) makes it unreachable anyway.

## Messages (`crates/gui/src/app.rs`)

| Old | New | Reason |
|-----|-----|--------|
| `PickLocalFolderPathChanged(String)` | removed | no text input |
| _(new)_ | `PickLocalFolderBrowse` | user pressed "Choose/Change folder…" button |
| _(new)_ | `PickLocalFolderPicked(Option<String>)` | async picker returned; `None` = dismissed |

`PickLocalFolderSubmit` guard: `local_path.is_some()` instead of `!local_path_input.is_empty()`.

## Update logic (`crates/gui/src/app.rs` — `update()`)

**`PickLocalFolderBrowse`:**
```rust
iced::Task::perform(
    async {
        rfd::AsyncFileDialog::new()
            .pick_folder()
            .await
            .map(|h| h.path().to_string_lossy().into_owned())
    },
    Message::PickLocalFolderPicked,
)
```

**`PickLocalFolderPicked(Option<String>)`:**
- If `Some(path)`: set `local_path = Some(path)`, clear `error`
- If `None`: no-op (user dismissed the picker)

**`PickLocalFolderSubmit`:**
- Guard: `if local_path.is_none() { return Task::none(); }`
- Send `DaemonCommand::SetAccountFolder { account_id, local_path: path }`

## View (`crates/gui/src/views/pick_local_folder.rs`)

Signature change:

```rust
// Old
pub fn pick_local_folder_view<'a>(
    display_name: &'a str,
    url: &'a str,
    local_path_input: &'a str,
    error: Option<&'a str>,
) -> Element<'a, Message>

// New
pub fn pick_local_folder_view<'a>(
    display_name: &'a str,
    url: &'a str,
    local_path: Option<&'a str>,
    error: Option<&'a str>,
) -> Element<'a, Message>
```

Layout (top to bottom):

1. Heading: "Choose a local folder"
2. Caption: "Where should {display_name} from {url} sync to?"
3. Label: "Local folder"
4. **Folder well** (container):
   - `None`: dashed border, dimmed text "No folder selected"
   - `Some(path)`: folder icon + path text
5. **Choose/Change button**: `"Choose folder…"` when `None`, `"Change folder…"` when `Some(_)`  
   — fires `Message::PickLocalFolderBrowse`
6. Error banner (if `error.is_some()`)
7. Actions row: "Start Syncing" (`.on_press` only when `local_path.is_some()`) + "Cancel"

## Call site (`crates/gui/src/main.rs`)

Update `View::PickLocalFolder` destructure and `pick_local_folder_view` call to pass `local_path.as_deref()` instead of `local_path_input`.

## Daemon (`crates/daemon/src/gui_ipc/handler.rs`)

No changes. Path-existence validation stays.

## Tests

- Unit test: `PickLocalFolderPicked(None)` does not change `local_path`
- Unit test: `PickLocalFolderPicked(Some(path))` sets `local_path`
- Unit test: `PickLocalFolderSubmit` when `local_path = None` → no daemon command sent
- Existing acceptance test `test_account_setup` continues to work unchanged (it uses IPC directly, not the GUI picker)
