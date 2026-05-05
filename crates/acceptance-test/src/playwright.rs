use anyhow::{anyhow, Result};
use std::io::Write;
use tokio::process::Command;
use url::Url;

/// Completes an OIDC PKCE login using a headless Playwright/Chromium browser.
///
/// `auth_url` — the authorization URL emitted by the daemon on stdout (OIDC_AUTH_URL=...)
/// `callback_port` — the local port the daemon's callback server is listening on
///
/// Writes a JS script to a temp file and runs it via `node`. Playwright must be
/// installed (`npm install playwright && npx playwright install chromium`).
pub async fn complete_oidc_login(
    auth_url: &Url,
    callback_port: u16,
    username: &str,
    password: &str,
) -> Result<()> {
    // node resolves require() relative to the script file, not the process CWD.
    // Write the script into the workspace root (next to node_modules) so that
    // `require('playwright')` resolves to the local installation.
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
  // oCIS may show a consent page after login — race between consent and callback
  const callbackPattern = 'http://127.0.0.1:{callback_port}/**';
  await Promise.race([
    page.waitForURL('**consent**', {{ timeout: 15000 }}),
    page.waitForURL(callbackPattern, {{ timeout: 15000 }}),
  ]);
  if (page.url().includes('consent')) {{
    await page.click('button[type="submit"]');
    await page.waitForURL(callbackPattern, {{ timeout: 15000 }});
  }}
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

    let status = Command::new("node").arg(tmp.path()).status().await?;

    if !status.success() {
        return Err(anyhow!("Playwright script exited with status: {status}"));
    }
    Ok(())
}
