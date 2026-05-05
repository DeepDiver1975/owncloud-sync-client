# GUI Visual Design — ownCloud Sync Client (iced)

**Date:** 2026-05-05
**Status:** Approved for implementation

---

## Overview

A full visual redesign of the iced-based GUI (`crates/gui`). All existing views are restyled using a shared theme module. No new features are introduced; this is purely a visual layer change. The redesign supports both light and dark mode, uses the official ownCloud brand blue (`#0082C9`) as the primary accent, and follows the "ownCloud Blueprint" direction selected during design review.

---

## Design Direction: ownCloud Blueprint

**Palette — dark mode**

| Role | Hex | Usage |
|---|---|---|
| BG base | `#1a1e23` | Window background |
| BG surface | `#111418` | Title bar, sidebar |
| BG card | `#1c2128` | Account/folder cards |
| BG hover | `#21262d` | Hover states |
| Border subtle | `#2e3338` | Card borders, dividers |
| Border default | `#444c56` | Input borders |
| Text primary | `#c9d1d9` | Headings, folder names |
| Text secondary | `#8b949e` | Nav items, captions |
| Text muted | `#6e7681` | Paths, hints |
| Accent | `#0082C9` | Active nav, primary buttons, focus rings |
| Status OK | `#3fb950` | Synced LED + badge |
| Status Syncing | `#58a6d4` | Syncing LED + badge |
| Status Error | `#f85149` | Error LED + badge |
| Status Paused | `#d29922` | Paused LED + badge |

**Palette — light mode**

| Role | Hex | Usage |
|---|---|---|
| BG base | `#f3f6f9` | Window background |
| BG surface | `#ffffff` | Title bar, sidebar |
| BG card | `#ffffff` | Cards (with subtle shadow) |
| Border subtle | `#dde3ea` | Card borders, sidebar divider |
| Border default | `#d0d7de` | Input borders |
| Text primary | `#1f2328` | Headings, folder names |
| Text secondary | `#57606a` | Nav items, captions |
| Text muted | `#8c959f` | Paths, hints |
| Accent | `#0082C9` | Active nav, primary buttons, focus rings |
| Status OK | `#15803d` / `#dcfce7` bg | Synced |
| Status Syncing | `#1d65b5` / `#dbeafe` bg | Syncing |
| Status Error | `#b91c1c` / `#fee2e2` bg | Error |
| Status Paused | `#854d0e` / `#fef9c3` bg | Paused |

---

## Window Structure

```
┌─ Title bar ────────────────────────────────────┐
│  [ownCloud icon]  ownCloud Sync                 │
├─ Sidebar ──┬─ Content area ────────────────────┤
│            │                                    │
│ nav items  │  view-specific content             │
│            │                                    │
└────────────┴────────────────────────────────────┘
```

**Title bar:** ownCloud official icon (the interlocking-circles mark) at 22×22px, followed by "ownCloud Sync" in 12px semibold. No controls in the title bar. The SVG is fetched from `https://raw.githubusercontent.com/owncloud/client/refs/heads/master/src/resources/theme/universal/owncloud-icon.svg` and embedded as a static byte literal in `theme.rs` (or referenced via `iced::widget::svg::Handle::from_memory`). It must be vendored into `crates/gui/assets/owncloud-icon.svg` and included with `include_bytes!`.

**Sidebar width:** 156px, fixed.

**Sidebar items (top to bottom):**
1. ☁ Sync Status
2. ＋ Add Account
3. ⚙ Settings

No Quit item in the sidebar. Quit lives exclusively in the system tray context menu.

**Active nav item style:** `rgba(0,130,201, 0.13)` background, accent-coloured text (dark) / `rgba(0,130,201, 0.09)` background, `#0082C9` text (light).

---

## Shared Theme Module (`crates/gui/src/theme.rs`)

A new `theme.rs` module exposes:

- **Colour constants** for both palettes (used by all view style functions)
- **`app_theme() -> iced::Theme`** — returns `iced::Theme::custom(...)` using the palette; called once in `main.rs`
- **Container style functions:** `sidebar_style`, `content_style`, `card_style`, `section_header_style`, `error_banner_style`, `status_indicator_style(color)`
- **Button style functions:** `primary_button_style`, `ghost_button_style`, `nav_button_style`, `nav_active_style`, `icon_button_style`, `danger_button_style`
- **Text input style function:** `text_input_style`
- **Text style helper:** `colored_text(color)` — returns a `text::Style` closure
- **Status helpers:** `status_color(status) -> Color`, `status_label(status) -> &str`

All style functions follow the `fn(theme: &iced::Theme, status: widget::Status) -> widget::Style` signature required by iced 0.13.

---

## View Specifications

### View 1 — Sync Status

**Empty state:** Centred column with a large cloud glyph (muted), "No accounts configured" heading, subtitle, and a primary "＋ Add account" button.

**With accounts:** Scrollable column of account sections. Each section:

- **Section header** (`section_header_style` container): LED dot (status colour) + account display name + server URL hint + ⚙ icon button (→ AccountSettings)
- **Folder rows** (`card_style` container, one per folder):
  - Left: folder display name (primary text) + local path (muted, truncated to 44 chars with `…` mid-truncation)
  - Right (margin-left: auto): progress bar (3px height, shown only when syncing) + ↗ icon button (opens local folder) + action badge

**Action badge** is the primary interactive element per folder:

| Folder status | Badge label | On press |
|---|---|---|
| Idle | `↻ Sync Now` | `ForceSyncFolder` |
| Syncing | `⏸ Pause` | `PauseFolder` |
| Paused | `▶ Resume` | `ResumeFolder` |
| Error | `⚠ N errors` | (no action, informational) |

Badge background is a translucent tint of the status colour; text is the status colour. Border is `rgba(status_color, 0.3)`.

---

### View 2 — Add Account

Centred content column (max-width 420px):

1. Heading: "Add Account" (20px bold)
2. Caption: "Enter your ownCloud server address. Sign-in will open in your browser."
3. Label + text input (`text_input_style`): placeholder `https://your.server.com`, focus ring in accent blue
4. Helper text: "Include the full address including https://"
5. Error banner (`error_banner_style`): shown when `error.is_some()`, red tint with border
6. Button row: primary "Connect →" + ghost "Cancel"

---

### View 3 — Add Account Waiting

Centred column:

1. Spinner glyph `⟳` (22px, accent blue)
2. Heading: "Waiting for sign-in…" (14px semibold)
3. Caption: "Complete authentication in the browser window that just opened."
4. Muted hint: server URL
5. Ghost "Cancel" button (→ SyncStatus)

---

### View 4 — Pick Local Folder

Centred content column:

1. Heading: "Choose a local folder" (15px bold)
2. Contextual caption naming the space and account: "Where should **{display_name}** from **{url}** sync to?"
3. Folder path field: styled container showing the current path value with a "Browse…" link (accent colour) on the right. On press → native folder picker (not yet implemented; field is editable via text input for now)
4. Error banner if `error.is_some()`
5. Button row: primary "Start Syncing" + ghost "Cancel"

---

### View 5 — Account Settings

Content area (no max-width):

1. Header row: account display name (14px semibold) + server URL (muted, 10px) on the left; "Remove Account" danger button on the right
2. "SYNCED FOLDERS" section label
3. Card listing folders: display name → local path (each row separated by a subtle border)
4. Ghost "← Back" link (→ SyncStatus)

"Remove Account" uses `danger_button_style`: red-tinted background with red border and text.

---

### View 6 — General Settings

Content area:

1. "General Settings" heading (14px semibold)
2. Toggle rows (label + sublabel + toggle switch on the right):
   - Launch at login
   - Show in system tray
   - Desktop notifications
3. Toggle switch: 30×17px pill, accent blue when on, border colour when off; white 13px circle thumb

> Note: toggle state and settings persistence are out of scope for this design task. The view renders the rows; wiring them to actual settings is a separate task.

---

## Iced Theme Integration

`main.rs` passes the custom theme to `iced::application(...)`:

```rust
iced::application("ownCloud Sync", IcedApp::update, IcedApp::view)
    .theme(|_| theme::app_theme())
    .subscription(IcedApp::subscription)
    .run_with(IcedApp::init)
```

All views receive the theme via iced's standard widget styling API (`.style(fn)` on each widget). No global CSS or stylesheet — all styles are Rust functions in `theme.rs`.

---

## Light / Dark Mode

iced 0.13 does not automatically follow OS dark/light preference. The theme is set once at startup. A follow-up task can add a `Message::ThemeChanged(Theme)` and OS watcher subscription to toggle between `theme::dark()` and `theme::light()` at runtime — but that is **out of scope** for this design implementation.

For now, the app ships with the dark theme active by default. The light palette is fully specified here so it can be wired up immediately when that follow-up task is tackled.

---

## Files Affected

| File | Change |
|---|---|
| `crates/gui/src/theme.rs` | **New** — all colour constants and style functions |
| `crates/gui/src/main.rs` | Add `.theme(...)` call; import `theme` module |
| `crates/gui/src/lib.rs` | Export `pub mod theme` |
| `crates/gui/src/views/sync_status.rs` | Full restyle |
| `crates/gui/src/views/add_account.rs` | Full restyle |
| `crates/gui/src/views/add_account_waiting.rs` | Full restyle |
| `crates/gui/src/views/pick_local_folder.rs` | Full restyle |
| `crates/gui/src/views/account_settings.rs` | Full restyle |
| `crates/gui/src/views/general_settings.rs` | Full restyle |

No changes to `app.rs`, `model.rs`, `daemon_conn.rs`, `subscription.rs`, `spawn.rs`, or `tray.rs`.

---

## Out of Scope

- Light/dark mode OS auto-switching (separate task)
- Toggle persistence in General Settings (separate task)
- Native folder picker for Pick Local Folder (separate task)
- Any new messages, model fields, or daemon protocol changes
- Animation / transitions (iced 0.13 has limited animation support)
