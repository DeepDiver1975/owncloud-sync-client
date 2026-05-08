# Design: Fix account setup + align acceptance tests with GUI user flow

**Date:** 2026-05-07
**Status:** Approved

## Problem

When a user manually sets up an account through the GUI, the account never appears in
the accounts view after the OIDC callback completes. The acceptance tests pass because
they bypass the GUI entirely — they send `AddAccount` and `SetAccountFolder` commands
directly to the daemon via IPC, never interacting with the Iced GUI. This means the
tests exercise a path that no human user ever takes, and the actual GUI path is untested
and broken.

## Root Cause

The acceptance tests use `DaemonIpcClient` to send commands directly to the daemon
socket, completely skipping the GUI's input widgets and state machine. The daemon IPC
path works reliably; the GUI path through AT-SPI2 → Iced → IPC has never been
exercised by tests. Any bug in the GUI flow (missed event, wrong view transition, widget
not responding) is invisible to the test suite.

The daemon's `broadcast::channel` delivers events only to subscribers that are already
connected when the event fires. This is correct for the normal case (GUI connects at
startup, subscribes, stays connected). However, if the GUI is ever in an unexpected
state when `AccountAddCompleted` arrives — for example because the AT-SPI subscriber
interaction races with the Iced event loop — the account never appears in the view.
The real fix is to make the acceptance tests exercise the GUI path so any such failure
is caught.

## Design

Two sequenced parts: (1) fix GUI accessible names so AT-SPI2 can find widgets
reliably, (2) rewrite the acceptance test to drive the GUI through AT-SPI2 while
using the daemon IPC stream only as a completion signal.

---

### Part 1 — Fix GUI accessible names

Iced 0.13 surfaces button accessible names from their inner widget's text content via
AccessKit. A `button(text("Connect →"))` gets accessible name `"Connect →"`. A
`button(row![text("☁"), text("Sync Status")])` may produce an unpredictable
concatenated name or an empty name depending on AccessKit's tree-flattening behavior.

**Change:** Replace multi-child nav buttons in `main.rs` with single-text-node content.
Drop the decorative icon text nodes from the "Sync Status" and "Add Account" nav
buttons. The result is a stable, predictable accessible name for every interactive
widget.

**Widget accessibility map after fix:**

| View | Widget | AT-SPI Role | Accessible name |
|---|---|---|---|
| sidebar | Sync Status nav button | `Button` | `"Sync Status"` |
| sidebar | Add Account nav button | `Button` | `"Add Account"` |
| AddAccount | Server URL field | `Entry` | `"https://your.server.com"` (placeholder) |
| AddAccount | submit button | `Button` | `"Connect →"` |
| AddAccountWaiting | cancel button | `Button` | `"Cancel"` |
| PickLocalFolder | folder path field | `Entry` | `"~/ownCloud"` (placeholder) |
| PickLocalFolder | submit button | `Button` | `"Start Syncing"` |
| SyncStatus | account name text | `Label` or `Static` | account display name string |

The exact `Role` emitted by Iced's AccessKit bridge for `text_input` (likely `Entry`)
must be confirmed by inspecting the live accessible tree. `AtSpiClient` gains a
`dump_tree()` helper that `wait_for_widget` calls on timeout, logging every
`(role, name, path)` triple. This makes role mismatches debuggable without needing
an external AT-SPI inspector.

---

### Part 2 — Add `AtSpiClient` to `TestEnvironment`

`TestEnvironment` gains a public `atspi: AtSpiClient` field. It is instantiated after
the GUI spawns and the `ScreenReaderEnabled` D-Bus property is flipped to `true`
(already handled by the existing fixture code). No changes to daemon spawning,
Docker oCIS setup, or Playwright invocation.

`AtSpiClient` gains one new method:

```
dump_tree(&self) -> String
```

Walks the BFS accessible tree from the registry root and returns a multi-line string
of `(role, name, path)` for every reachable node. Called automatically by
`wait_for_widget` when it times out, written to the test output via `eprintln!`.

---

### Part 3 — Rewrite `add_account()` in `fixture.rs`

The new `add_account()` is dual-channel: AT-SPI2 drives all user input; daemon IPC
events are used only as completion signals (they are more reliable than polling the
accessible tree for view transitions).

```
Step  Driver       Action
────  ───────────  ──────────────────────────────────────────────────────────
1     AT-SPI       wait_for_widget(Button, "Add Account", 10s) → click
2     AT-SPI       wait_for_widget(Entry, "https://your.server.com", 5s)
                     → set_text(ocis_url)
3     AT-SPI       wait_for_widget(Button, "Connect →", 5s) → click
4     daemon IPC   wait_for(AccountAddStarted, 15s)
5     daemon IPC   wait_for_oidc_url() from daemon stdout
6     Playwright   complete_oidc_login (unchanged)
7     daemon IPC   wait_for(AccountAddCompleted, 30s)
8     AT-SPI       wait_for_widget(Entry, "~/ownCloud", 10s)
                     → set_text(sync_dir)
9     AT-SPI       wait_for_widget(Button, "Start Syncing", 5s) → click
10    daemon IPC   wait_for(AccountFolderAdded, 30s)
11    AT-SPI       wait_for_widget(Label/Static, display_name, 10s)
                     ← proves account visible in SyncStatus view
```

Step 11 is the critical new assertion: it confirms the account appears in the GUI,
not just in the daemon config file. This is the test that was previously impossible
and that catches the real user-visible failure.

`account_setup.rs` keeps the existing config-file assertions (account count,
`user_id`, folder count) as a backend cross-check alongside the new AT-SPI assertion.

---

### Part 4 — Rewrite `duplicate_account` test

The second account attempt also goes through the GUI:

```
Step  Driver       Action
────  ───────────  ──────────────────────────────────────────────────────────
1–10  (same as add_account for the first account)
11    AT-SPI       wait_for_widget(Button, "Add Account", 10s) → click
12    AT-SPI       wait_for_widget(Entry, placeholder, 5s) → set_text(url)
13    AT-SPI       wait_for_widget(Button, "Connect →", 5s) → click
14    daemon IPC   wait_for(AccountAddStarted, 15s)
15    daemon IPC   wait_for_oidc_url() from daemon stdout
16    Playwright   complete_oidc_login (same credentials)
17    daemon IPC   wait_for(AccountAddFailed | AccountAddCompleted, 30s)
                     → assert AccountAddFailed
18    AT-SPI       wait_for_widget(Entry, placeholder, 10s)
                     ← proves GUI returned to AddAccount view with error
```

Step 18 confirms the GUI surfaced the failure and returned the user to the AddAccount
form. The daemon IPC assert in step 17 is a cross-check.

---

### Part 5 — `AtSpiClient`: no structural changes

`find_widget` and `wait_for_widget` signatures are unchanged. The only addition is
`dump_tree()`. The BFS traversal, `click()`, and `set_text()` methods are unchanged.

---

## Out of scope

- Daemon protocol changes (no `AccountsSnapshot` replay-on-subscribe)
- AT-SPI automation for sync, pause, resume, or remove-account flows
- Changes to `test_sync`
- Changes to CI workflow (the acceptance job already builds with `test-accessibility`
  and runs with `OCIS_ACCEPTANCE=1`)

## Files changed

| File | Change |
|---|---|
| `crates/gui/src/main.rs` | Remove icon text nodes from nav buttons |
| `crates/acceptance-test/src/atspi_client.rs` | Add `dump_tree()` method |
| `crates/acceptance-test/src/fixture.rs` | Add `atspi` field; rewrite `add_account()` |
| `crates/acceptance-test/tests/account_setup.rs` | Add AT-SPI assertion for display name |
| `crates/acceptance-test/tests/duplicate_account.rs` | Rewrite to drive GUI |
