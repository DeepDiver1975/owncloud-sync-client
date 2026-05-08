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
