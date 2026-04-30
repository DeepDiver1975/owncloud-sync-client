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
    let script = format!(
        r#"const {{ chromium }} = require('playwright');
(async () => {{
  const browser = await chromium.launch({{ headless: true }});
  const page = await browser.newPage();
  await page.goto({auth_url});
  await page.fill('input[name="login"]', {username});
  await page.fill('input[name="password"]', {password});
  await page.click('button[type="submit"]');
  await page.waitForURL('http://127.0.0.1:{callback_port}/callback**', {{ timeout: 15000 }});
  await browser.close();
}})();
"#,
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
