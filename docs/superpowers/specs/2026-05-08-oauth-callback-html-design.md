# OAuth Callback HTML Page — Design Spec

**Issue:** [#14 callback html page needs improvement](https://github.com/DeepDiver1975/owncloud-sync-client/issues/14)
**Branch:** `feat/oauth-callback-page`
**Date:** 2026-05-08

---

## Problem

The OIDC callback HTTP response shown in the browser after OIDC login is bare and unstyled:

- **Success path** — a 67-byte inline `<html><body>Sign-in complete. You can close this tab.</body></html>` string.
- **Error path** — plain text `Sign-in failed: <reason>`, always HTTP 200.

The upstream ownCloud desktop client ships polished `success.html` / `error.html` templates
(centered layout, SVG icon, dark-mode support, `{{TITLE}}` / `{{MESSAGE}}` placeholders) that
should be reused here.

---

## Approach

**Embed HTML files at compile time via `include_str!`** (Approach B).

- Store `success.html` and `error.html` under `crates/daemon/resources/oauth/`, mirroring
  the upstream layout (`src/resources/oauth/`).
- Embed with `include_str!` — daemon remains a single self-contained binary.
- Fill placeholders at runtime with two `str::replace` calls.

---

## Components

### 1. `crates/daemon/resources/oauth/success.html`

Verbatim copy of the upstream success template.

Filled at runtime:
- `{{TITLE}}` → `"Successfully signed in"`
- `{{MESSAGE}}` → `"Now, explore ownCloud on desktop."`

### 2. `crates/daemon/resources/oauth/error.html`

Verbatim copy of the upstream error template.

Filled at runtime:
- `{{TITLE}}` → `"Sign-in failed"`
- `{{MESSAGE}}` → the error reason string from the daemon

### 3. `crates/daemon/src/oidc_callback.rs` — changes

- Remove `SUCCESS_HTML` constant.
- Remove `send_error_page` function.
- Add:
  ```rust
  const SUCCESS_HTML_TEMPLATE: &str = include_str!("../resources/oauth/success.html");
  const ERROR_HTML_TEMPLATE: &str = include_str!("../resources/oauth/error.html");

  fn render(template: &str, title: &str, message: &str) -> String {
      template
          .replace("{{TITLE}}", title)
          .replace("{{MESSAGE}}", message)
  }

  async fn send_html_response(stream: &mut TcpStream, status: &str, html: String) {
      let resp = format!(
          "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
          html.len(),
          html
      );
      let _ = stream.write_all(resp.as_bytes()).await;
  }
  ```
- Error responses use **HTTP 400** (was 200 plain-text).
- Success response uses **HTTP 200** with the rendered success HTML.

### 4. `crates/acceptance-test/src/playwright.rs` — changes

Change `complete_oidc_login` signature from `Result<()>` to `Result<String>`, where the
`String` is the page title read after callback navigation completes.

Add to the Playwright JS script (after `waitForURL(callbackPattern)`):
```js
const title = await page.title();
process.stdout.write(title + '\n');
```

The Rust wrapper reads one line of stdout from the node process and returns it as the page title.

### 5. `crates/acceptance-test/tests/account_setup.rs` — changes

After `env.add_account()` (which internally calls `complete_oidc_login`), assert that the
returned page title equals `"Successfully signed in"`.

The `add_account` helper on `TestEnvironment` propagates the title from `complete_oidc_login`
back to the caller.

---

## Data Flow

```
Daemon oidc_callback
  ├── success → render(SUCCESS_HTML_TEMPLATE, "Successfully signed in", "Now, explore…")
  │             HTTP 200, Content-Type: text/html
  └── error   → render(ERROR_HTML_TEMPLATE, "Sign-in failed", reason)
                HTTP 400, Content-Type: text/html

Playwright JS (acceptance test)
  └── after waitForURL(callback) → page.title() → stdout → Rust wrapper → returned String

account_setup AT
  └── assert_eq!(title, "Successfully signed in")
```

---

## Error Handling

- The error HTTP status changes from 200 to **400 Bad Request**. The Playwright script does not
  need to assert the status code directly — the page title on the error page will differ from the
  success title, but the primary acceptance test only covers the happy path.
- The duplicate-account AT (`test_duplicate_account_rejected`) does not need modification — it
  already asserts `AccountAddFailed` via IPC and does not inspect the browser page.

---

## Testing

| Layer | What is asserted |
|-------|-----------------|
| Unit (existing, `oidc_callback.rs`) | `extract_query_param` — no change needed |
| Acceptance (`test_account_setup`) | Page title from Playwright == `"Successfully signed in"` |
| Acceptance (`test_duplicate_account_rejected`) | Unchanged — IPC-level assertion only |

---

## Files Changed

| File | Change |
|------|--------|
| `crates/daemon/resources/oauth/success.html` | New — verbatim upstream template |
| `crates/daemon/resources/oauth/error.html` | New — verbatim upstream template |
| `crates/daemon/src/oidc_callback.rs` | Replace inline responses with template rendering |
| `crates/acceptance-test/src/playwright.rs` | Return page title from `complete_oidc_login` |
| `crates/acceptance-test/src/fixture.rs` | Thread title through `add_account` |
| `crates/acceptance-test/tests/account_setup.rs` | Assert page title |
