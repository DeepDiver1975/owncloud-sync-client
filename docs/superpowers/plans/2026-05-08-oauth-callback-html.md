# OAuth Callback HTML Page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace bare inline OIDC callback HTTP responses with styled HTML pages (success + error) from upstream ownCloud, and assert the callback page title in the acceptance test.

**Architecture:** `success.html` and `error.html` live in `crates/daemon/resources/oauth/` and are embedded at compile time via `include_str!`. A `render()` helper fills `@{TITLE}` / `@{MESSAGE}` placeholders at runtime. The Playwright acceptance helper is extended to return the callback page title, which `test_account_setup` asserts equals `"Successfully signed in"`.

**Tech Stack:** Rust (tokio, `include_str!`), HTML/CSS with dark-mode support, Node.js Playwright (headless Chromium)

---

### Task 1: Add HTML resource files

**Files:**
- Create: `crates/daemon/resources/oauth/success.html`
- Create: `crates/daemon/resources/oauth/error.html`

- [ ] **Step 1: Create the resources directory**

```bash
mkdir -p crates/daemon/resources/oauth
```

- [ ] **Step 2: Create `success.html`**

Create `crates/daemon/resources/oauth/success.html` with this exact content (verbatim from upstream ownCloud client):

```html
<!DOCTYPE html>

<html lang="en">

<head>
<title>@{TITLE}</title>
<style>
html, body {
    height: 100%;
    width: 100%;
    margin: 0;
}

body {
    background-color: #ffffff;
    color: #222222;
    font-family: "Noto Sans", OpenSans, Verdana, Helvetica, Arial, sans-serif;
    display: flex;
    flex-direction: column;
    align-items: center;
}

@media (prefers-color-scheme: dark) {
    body {
        background-color: #444444;
        color: #ffffff;
    }
}

.row {
    display: flex;
    flex-direction: row;
    align-items: center;
    height: 100%;
}

.content {
    text-align: center;
}
</style>
</head>

<body>
<div class="row">
    <div class="content">
        <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" style="width:96px;height:96px;fill:#49851C;"><!--!Font Awesome Free 6.7.2 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2025 Fonticons, Inc.--><path d="M256 48a208 208 0 1 1 0 416 208 208 0 1 1 0-416zm0 464A256 256 0 1 0 256 0a256 256 0 1 0 0 512zM369 209c9.4-9.4 9.4-24.6 0-33.9s-24.6-9.4-33.9 0l-111 111-47-47c-9.4-9.4-24.6-9.4-33.9 0s-9.4 24.6 0 33.9l64 64c9.4 9.4 24.6 9.4 33.9 0L369 209z"/></svg>
        <h1>@{TITLE}</h1>
        <h2>@{MESSAGE}</h2>
    </div>
</div>
</body>
</html>
```

- [ ] **Step 3: Create `error.html`**

Create `crates/daemon/resources/oauth/error.html` with this exact content:

```html
<!DOCTYPE html>

<html lang="en">

<head>
<title>@{TITLE}</title>
<style>
html, body {
    height: 100%;
    width: 100%;
    margin: 0;
}

body {
    background-color: #ffffff;
    color: #222222;
    font-family: "Noto Sans", OpenSans, Verdana, Helvetica, Arial, sans-serif;
    display: flex;
    flex-direction: column;
    align-items: center;
}

@media (prefers-color-scheme: dark) {
    body {
        background-color: #444444;
        color: #ffffff;
    }
}

.row {
    display: flex;
    flex-direction: row;
    align-items: center;
    height: 100%;
}

.content {
    text-align: center;
}
</style>
</head>

<body>
<div class="row">
    <div class="content">
        <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640" style="width:96px;height:96px;fill:#E50101;"><!--!Font Awesome Free 7.0.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2025 Fonticons, Inc.--><path d="M320 112C434.9 112 528 205.1 528 320C528 434.9 434.9 528 320 528C205.1 528 112 434.9 112 320C112 205.1 205.1 112 320 112zM320 576C461.4 576 576 461.4 576 320C576 178.6 461.4 64 320 64C178.6 64 64 178.6 64 320C64 461.4 178.6 576 320 576zM231 231C221.6 240.4 221.6 255.6 231 264.9L286 319.9L231 374.9C221.6 384.3 221.6 399.5 231 408.8C240.4 418.1 255.6 418.2 264.9 408.8L319.9 353.8L374.9 408.8C384.3 418.2 399.5 418.2 408.8 408.8C418.1 399.4 418.2 384.2 408.8 374.9L353.8 319.9L408.8 264.9C418.2 255.5 418.2 240.3 408.8 231C399.4 221.7 384.2 221.6 374.9 231L319.9 286L264.9 231C255.5 221.6 240.3 221.6 231 231z"/></svg>
        <h1>@{TITLE}</h1>
        <h2>@{MESSAGE}</h2>
    </div>
</div>
</body>
</html>
```

- [ ] **Step 4: Commit**

```bash
git add crates/daemon/resources/
git commit -s -m "feat(daemon): add OAuth callback HTML resource templates"
```

---

### Task 2: Add `render()` helper with unit test

**Files:**
- Modify: `crates/daemon/src/oidc_callback.rs`

`@{TITLE}` and `@{MESSAGE}` each appear twice in the templates (in `<title>` and `<h1>`/`<h2>`). `str::replace` replaces all occurrences, which is correct. `String::len()` returns byte count — the correct value for HTTP `Content-Length`.

- [ ] **Step 1: Write the failing unit tests**

In `crates/daemon/src/oidc_callback.rs`, add to the existing `#[cfg(test)]` block at the bottom (after the existing tests):

```rust
    #[test]
    fn render_fills_title_and_message() {
        let template = "<title>@{TITLE}</title><p>@{MESSAGE}</p>";
        assert_eq!(
            render(template, "Hello", "World"),
            "<title>Hello</title><p>World</p>"
        );
    }

    #[test]
    fn render_replaces_all_occurrences() {
        let template = "<title>@{TITLE}</title><h1>@{TITLE}</h1><h2>@{MESSAGE}</h2>";
        assert_eq!(
            render(template, "T", "M"),
            "<title>T</title><h1>T</h1><h2>M</h2>"
        );
    }
```

- [ ] **Step 2: Run to confirm failure**

```bash
cargo test -p daemon render_fills_title 2>&1 | tail -5
```
Expected: `error[E0425]: cannot find function 'render' in this scope`

- [ ] **Step 3: Implement `render()`**

In `crates/daemon/src/oidc_callback.rs`, add this function after the `use` imports and before `const SIGN_IN_TIMEOUT`:

```rust
fn render(template: &str, title: &str, message: &str) -> String {
    template
        .replace("@{TITLE}", title)
        .replace("@{MESSAGE}", message)
}
```

- [ ] **Step 4: Run to confirm passing**

```bash
cargo test -p daemon render 2>&1 | tail -6
```
Expected:
```
test oidc_callback::tests::render_fills_title_and_message ... ok
test oidc_callback::tests::render_replaces_all_occurrences ... ok
test result: ok. 2 passed; 0 failed
```

- [ ] **Step 5: Commit**

```bash
git add crates/daemon/src/oidc_callback.rs
git commit -s -m "feat(daemon): add render() helper for HTML template placeholder substitution"
```

---

### Task 3: Wire HTML templates into `oidc_callback.rs`

**Files:**
- Modify: `crates/daemon/src/oidc_callback.rs`

- [ ] **Step 1: Replace inline constants and `send_error_page`**

In `crates/daemon/src/oidc_callback.rs`, remove these two items:

```rust
const SUCCESS_HTML: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 67\r\nConnection: close\r\n\r\n<html><body>Sign-in complete. You can close this tab.</body></html>";

/// Writes a plain-text 200 error response to `stream` so the browser can complete its
/// navigation before we clean up. Without this the browser gets ERR_CONNECTION_RESET.
async fn send_error_page(stream: &mut tokio::net::TcpStream, reason: &str) {
    let body = format!("Sign-in failed: {reason}");
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(resp.as_bytes()).await;
}
```

Add these in their place (after `render()`, before `const SIGN_IN_TIMEOUT`):

```rust
const SUCCESS_HTML_TEMPLATE: &str = include_str!("../resources/oauth/success.html");
const ERROR_HTML_TEMPLATE: &str = include_str!("../resources/oauth/error.html");

async fn send_html_response(stream: &mut tokio::net::TcpStream, status: &str, html: String) {
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        html.len(),
        html
    );
    let _ = stream.write_all(resp.as_bytes()).await;
}
```

- [ ] **Step 2: Replace the three error call sites**

There are three `send_error_page` calls in `handle_callback`. Each follows this pattern — replace them all:

**Before (all three look like this):**
```rust
send_error_page(&mut stream, &msg).await;
```

**After (all three become this):**
```rust
send_html_response(
    &mut stream,
    "400 Bad Request",
    render(ERROR_HTML_TEMPLATE, "Sign-in failed", &msg),
).await;
```

The three locations are:
1. After `let msg = format!("invalid server URL: {e}");`
2. After `let msg = format!("GET /me failed: {e}");`
3. Inside `if let Err(ref e) = save_result` — here `msg` is `e.to_string()`, so use:
   ```rust
   send_html_response(
       &mut stream,
       "400 Bad Request",
       render(ERROR_HTML_TEMPLATE, "Sign-in failed", &e.to_string()),
   ).await;
   ```

- [ ] **Step 3: Replace the success response**

Near the end of `handle_callback`, replace:

```rust
stream.write_all(SUCCESS_HTML).await?;
```

with:

```rust
send_html_response(
    &mut stream,
    "200 OK",
    render(
        SUCCESS_HTML_TEMPLATE,
        "Successfully signed in",
        "Now, explore ownCloud on desktop.",
    ),
).await;
```

The trailing `Ok(())` on the next line stays unchanged.

- [ ] **Step 4: Build to confirm clean compile**

```bash
cargo build -p daemon 2>&1 | grep '^error' | head -10
```
Expected: no output.

- [ ] **Step 5: Run all daemon unit tests**

```bash
cargo test -p daemon 2>&1 | tail -10
```
Expected: all pass — `render_fills_title_and_message`, `render_replaces_all_occurrences`, `extracts_code_from_get_request`, `returns_none_for_missing_param`, `returns_none_for_no_query_string`.

- [ ] **Step 6: Commit**

```bash
git add crates/daemon/src/oidc_callback.rs
git commit -s -m "feat(daemon): serve styled HTML callback pages for OIDC success and error"
```

---

### Task 4: Update `complete_oidc_login` to return the page title

**Files:**
- Modify: `crates/acceptance-test/src/playwright.rs`

The JS script prints the callback page's `document.title` to stdout before closing. The Rust wrapper captures stdout via `.output()` instead of `.status()` and returns the trimmed title line as `Result<String>`.

- [ ] **Step 1: Replace `crates/acceptance-test/src/playwright.rs` entirely**

```rust
use anyhow::{anyhow, Result};
use std::io::Write;
use tokio::process::Command;
use url::Url;

/// Completes an OIDC PKCE login using a headless Playwright/Chromium browser.
/// Returns the page title shown on the callback page after login completes.
pub async fn complete_oidc_login(
    auth_url: &Url,
    callback_port: u16,
    username: &str,
    password: &str,
) -> Result<String> {
    let workspace_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let playwright_path = workspace_root.join("node_modules").join("playwright");

    let script = format!(
        r#"const {{ chromium }} = require({playwright_path});
(async () => {{
  const browser = await chromium.launch({{ headless: true }});
  const context = await browser.newContext({{ ignoreHTTPSErrors: true }});
  const page = await context.newPage();
  await page.goto({auth_url});
  await page.waitForSelector('#oc-login-username', {{ timeout: 15000 }});
  await page.fill('#oc-login-username', {username});
  await page.fill('#oc-login-password', {password});
  await page.click('button[type="submit"]');
  const callbackPattern = 'http://127.0.0.1:{callback_port}/**';
  await Promise.race([
    page.waitForURL('**consent**', {{ timeout: 15000 }}),
    page.waitForURL(callbackPattern, {{ timeout: 15000 }}),
  ]);
  if (page.url().includes('consent')) {{
    await page.click('button[type="submit"]');
    await page.waitForURL(callbackPattern, {{ timeout: 15000 }});
  }}
  const title = await page.title();
  process.stdout.write(title + '\n');
  await browser.close();
}})();
"#,
        playwright_path = serde_json::to_string(&playwright_path.to_string_lossy().as_ref())?,
        auth_url = serde_json::to_string(auth_url.as_str())?,
        username = serde_json::to_string(username)?,
        password = serde_json::to_string(password)?,
        callback_port = callback_port,
    );

    let mut tmp = tempfile::NamedTempFile::with_suffix(".js")?;
    tmp.write_all(script.as_bytes())?;
    tmp.flush()?;

    let output = Command::new("node").arg(tmp.path()).output().await?;

    if !output.status.success() {
        return Err(anyhow!(
            "Playwright script exited with status: {}",
            output.status
        ));
    }

    let title = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(title)
}
```

- [ ] **Step 2: Build to confirm `playwright.rs` itself compiles**

```bash
cargo build -p acceptance-test 2>&1 | grep '^error' | head -10
```
Expected: no errors (the caller in `fixture.rs` uses `?` which silently drops the `String` — no type error, at most an `unused_must_use` warning which is addressed in Task 5).

- [ ] **Step 3: Commit**

```bash
git add crates/acceptance-test/src/playwright.rs
git commit -s -m "feat(acceptance): return callback page title from complete_oidc_login"
```

---

### Task 5: Thread the title through `add_account` in `fixture.rs`

**Files:**
- Modify: `crates/acceptance-test/src/fixture.rs`

- [ ] **Step 1: Change `add_account` signature to `Result<String>`**

In `crates/acceptance-test/src/fixture.rs`, change the method signature from:

```rust
pub async fn add_account(&mut self) -> Result<()> {
```

to:

```rust
pub async fn add_account(&mut self) -> Result<String> {
```

- [ ] **Step 2: Capture the title from `complete_oidc_login`**

Find:

```rust
complete_oidc_login(&auth_url, callback_port, "admin", "admin")
    .await
    .context("Playwright OIDC login failed")?;
```

Replace with:

```rust
let callback_title = complete_oidc_login(&auth_url, callback_port, "admin", "admin")
    .await
    .context("Playwright OIDC login failed")?;
```

- [ ] **Step 3: Return the title**

Replace the final `Ok(())` at the end of `add_account` with:

```rust
Ok(callback_title)
```

- [ ] **Step 4: Build the workspace**

```bash
cargo build --workspace 2>&1 | grep '^error' | head -10
```
Expected: no output. (`duplicate_account.rs` calls `env.add_account().await.expect(...)` and discards the `String` — valid Rust, no error.)

- [ ] **Step 5: Commit**

```bash
git add crates/acceptance-test/src/fixture.rs
git commit -s -m "feat(acceptance): propagate callback page title through add_account"
```

---

### Task 6: Assert page title in `test_account_setup`

**Files:**
- Modify: `crates/acceptance-test/tests/account_setup.rs`

- [ ] **Step 1: Capture and assert the title**

In `crates/acceptance-test/tests/account_setup.rs`, replace:

```rust
env.add_account()
    .await
    .expect("account setup via OIDC failed");
```

with:

```rust
let callback_title = env
    .add_account()
    .await
    .expect("account setup via OIDC failed");

assert_eq!(
    callback_title,
    "Successfully signed in",
    "expected success page title after OIDC login"
);
```

- [ ] **Step 2: Build the full workspace**

```bash
cargo build --workspace 2>&1 | grep '^error' | head -10
```
Expected: no output.

- [ ] **Step 3: Run all unit tests**

```bash
cargo test --workspace --lib 2>&1 | tail -10
```
Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add crates/acceptance-test/tests/account_setup.rs
git commit -s -m "test(acceptance): assert callback page title is 'Successfully signed in'"
```
