// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

use anyhow::{anyhow, Result};
use std::io::Write;
use tokio::process::Command;
use url::Url;

/// CSS selectors for a server's web login form. They differ between oCIS (its
/// built-in IdP) and ownCloud Classic (oc10), so the Playwright script is
/// parameterized rather than hard-coding one server's markup.
#[derive(Debug, Clone, Copy)]
pub struct LoginSelectors {
    pub username: &'static str,
    pub password: &'static str,
    pub submit: &'static str,
    /// Selector for an explicit authorization-grant button shown on a separate
    /// page *after* login (oc10's OAuth2 app), or `None` when the server has no
    /// such page (oCIS, whose optional consent screen is handled inline).
    pub authorize: Option<&'static str>,
}

impl LoginSelectors {
    /// oCIS login form (`services/idp/assets/identifier/index.html`).
    pub const OCIS: LoginSelectors = LoginSelectors {
        username: "#oc-login-username",
        password: "#oc-login-password",
        submit: "button[type=\"submit\"]",
        authorize: None,
    };

    /// ownCloud Classic (oc10) login form (`core/templates/login.php`),
    /// stable across oc10 10.x. The submit element's tag changed (input →
    /// button) over releases, but `id="submit"` is constant. After login, oc10
    /// shows a grant page with an "Authorize" button (alongside a "Switch
    /// users" button, so match on the button text).
    pub const OC10: LoginSelectors = LoginSelectors {
        username: "#user",
        password: "#password",
        submit: "#submit",
        authorize: Some("button:has-text(\"Authorize\")"),
    };
}

/// Completes an OIDC/OAuth2 PKCE login using a headless Playwright/Chromium
/// browser, driving the login form identified by `selectors`. Returns the page
/// title shown on the callback page after login completes.
pub async fn complete_oidc_login(
    auth_url: &Url,
    callback_port: u16,
    username: &str,
    password: &str,
    selectors: LoginSelectors,
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
  await page.waitForSelector({sel_username}, {{ timeout: 15000 }});
  await page.fill({sel_username}, {username});
  await page.fill({sel_password}, {password});
  await page.click({sel_submit});
  // The callback host differs by backend (oCIS uses 127.0.0.1, oc10 uses
  // localhost), so match either on the OS-assigned port.
  const callbackPattern = new RegExp('^http://(127\\.0\\.0\\.1|localhost):{callback_port}/');
  const authorizeSel = {authorize};
  if (authorizeSel) {{
    // oc10 shows a separate authorization-grant page after login. Click its
    // "Authorize" button, which redirects to the callback.
    await page.waitForSelector(authorizeSel, {{ timeout: 15000 }});
    await page.click(authorizeSel);
    await page.waitForURL(callbackPattern, {{ timeout: 15000 }});
  }} else {{
    // oCIS may show a consent page after login — race between consent and callback.
    await Promise.race([
      page.waitForURL('**consent**', {{ timeout: 15000 }}),
      page.waitForURL(callbackPattern, {{ timeout: 15000 }}),
    ]);
    if (page.url().includes('consent')) {{
      await page.click({sel_submit});
      await page.waitForURL(callbackPattern, {{ timeout: 15000 }});
    }}
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
        sel_username = serde_json::to_string(selectors.username)?,
        sel_password = serde_json::to_string(selectors.password)?,
        sel_submit = serde_json::to_string(selectors.submit)?,
        // `Some("sel")` -> JS string literal; `None` -> JS `null`.
        authorize = serde_json::to_string(&selectors.authorize)?,
        callback_port = callback_port,
    );

    let mut tmp = tempfile::NamedTempFile::with_suffix(".js")?;
    tmp.write_all(script.as_bytes())?;
    tmp.flush()?;

    let output = Command::new("node").arg(tmp.path()).output().await?;

    if !output.status.success() {
        return Err(anyhow!(
            "Playwright script exited with status: {}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr),
        ));
    }

    let title = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(title)
}
