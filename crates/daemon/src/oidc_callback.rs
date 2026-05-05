use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::config::{AccountConfig, AppConfig};
use crate::gui_ipc::protocol::DaemonEvent;
use crate::gui_ipc::GuiIpcServer;
use ocis_client::auth::{KeychainStore, OidcAuth, PkceVerifier};
use ocis_client::GraphClient;

const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);

const SUCCESS_HTML: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 67\r\nConnection: close\r\n\r\n<html><body>Sign-in complete. You can close this tab.</body></html>";

#[allow(clippy::too_many_arguments)]
pub async fn run_callback(
    listener: TcpListener,
    oidc: OidcAuth,
    verifier: PkceVerifier,
    account_id: Uuid,
    url: String,
    ipc: Arc<GuiIpcServer>,
    config: Arc<tokio::sync::Mutex<AppConfig>>,
    config_path: std::path::PathBuf,
) {
    let result = tokio::time::timeout(
        SIGN_IN_TIMEOUT,
        handle_callback(
            listener,
            oidc,
            verifier,
            account_id,
            url,
            Arc::clone(&ipc),
            config,
            config_path,
        ),
    )
    .await;

    match result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            tracing::warn!("OIDC callback error: {e}");
            ipc.broadcast(DaemonEvent::AccountAddFailed {
                account_id,
                reason: e.to_string(),
            });
        }
        Err(_) => {
            tracing::warn!("OIDC callback timed out for account {account_id}");
            ipc.broadcast(DaemonEvent::AccountAddFailed {
                account_id,
                reason: "sign-in timed out".to_string(),
            });
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_callback(
    listener: TcpListener,
    oidc: OidcAuth,
    verifier: PkceVerifier,
    account_id: Uuid,
    url: String,
    ipc: Arc<GuiIpcServer>,
    config: Arc<tokio::sync::Mutex<AppConfig>>,
    config_path: std::path::PathBuf,
) -> anyhow::Result<()> {
    let (mut stream, _peer) = listener.accept().await?;

    let mut buf = Vec::with_capacity(512);
    let mut tmp = [0u8; 512];
    loop {
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            anyhow::bail!("connection closed before request received");
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.windows(2).any(|w| w == b"\r\n") {
            break;
        }
        if buf.len() > 8192 {
            anyhow::bail!("request too large");
        }
    }

    let request = std::str::from_utf8(&buf)?;
    let code = extract_query_param(request, "code")
        .ok_or_else(|| anyhow::anyhow!("no 'code' parameter in callback"))?;

    let tokens = oidc
        .exchange_code(&code, verifier)
        .await
        .map_err(|e| anyhow::anyhow!("token exchange failed: {e}"))?;

    // 1. Save tokens to keychain.
    let account_id_str = account_id.to_string();
    {
        let account_id_str = account_id_str.clone();
        let tokens_to_save = tokens.clone();
        tokio::task::spawn_blocking(move || KeychainStore::save(&account_id_str, &tokens_to_save))
            .await
            .map_err(|e| anyhow::anyhow!("keychain task panicked: {e}"))?
            .map_err(|e| anyhow::anyhow!("keychain save failed: {e}"))?;
    }

    // Helper closure to delete keychain entry on failure.
    let delete_keychain = |id: &str| {
        let id = id.to_string();
        tokio::task::spawn_blocking(move || {
            if let Err(e) = KeychainStore::delete(&id) {
                tracing::warn!("failed to delete keychain entry for {id}: {e}");
            }
        })
    };

    // 2. Call GET /graph/v1.0/me to get user identity.
    let base_url = match url::Url::parse(&url) {
        Ok(u) => u,
        Err(e) => {
            delete_keychain(&account_id_str).await.ok();
            anyhow::bail!("invalid server URL: {e}");
        }
    };
    let token_arc = Arc::new(RwLock::new(tokens));
    let graph = GraphClient::new(base_url, token_arc);
    let user_info = match graph.get_me().await {
        Ok(info) => info,
        Err(e) => {
            delete_keychain(&account_id_str).await.ok();
            anyhow::bail!("GET /me failed: {e}");
        }
    };

    // 3. Check for duplicate account (same url + user_id) and save — atomically
    //    under a single lock acquisition to prevent TOCTOU races.
    {
        let mut cfg = config.lock().await;
        if cfg
            .account
            .iter()
            .any(|a| a.url == url && a.user_id == user_info.id)
        {
            drop(cfg);
            delete_keychain(&account_id_str).await.ok();
            anyhow::bail!(
                "account already exists for user '{}' on {url}",
                user_info.id
            );
        }
        cfg.account.push(AccountConfig {
            id: account_id,
            url: url.clone(),
            user_id: user_info.id.clone(),
            username: String::new(),
            display_name: user_info.display_name.clone(),
            folder: vec![],
        });
        if let Err(e) = cfg.save(&config_path) {
            drop(cfg);
            delete_keychain(&account_id_str).await.ok();
            anyhow::bail!("failed to save config: {e}");
        }
    }

    // 5. Broadcast AccountAddCompleted.
    ipc.broadcast(DaemonEvent::AccountAddCompleted {
        account_id,
        user_id: user_info.id,
        display_name: user_info.display_name,
        url,
    });

    stream.write_all(SUCCESS_HTML).await?;
    Ok(())
}

fn extract_query_param(request: &str, param: &str) -> Option<String> {
    let first_line = request.lines().next()?;
    // e.g. "GET /callback?code=abc&state=xyz HTTP/1.1"
    let path = first_line.split_whitespace().nth(1)?;
    let query = path.split_once('?').map(|(_, q)| q)?;
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == param {
                return Some(decode_query_value(v));
            }
        }
    }
    None
}

fn decode_query_value(s: &str) -> String {
    // Minimal decoding for query values: replace '+' with space.
    // Auth codes in practice are base64url and contain no encoded chars.
    s.replace('+', " ")
}

#[cfg(test)]
mod tests {
    use super::extract_query_param;

    #[test]
    fn extracts_code_from_get_request() {
        let req = "GET /callback?code=abc123&state=xyz HTTP/1.1\r\nHost: localhost\r\n\r\n";
        assert_eq!(extract_query_param(req, "code"), Some("abc123".to_string()));
        assert_eq!(extract_query_param(req, "state"), Some("xyz".to_string()));
    }

    #[test]
    fn returns_none_for_missing_param() {
        let req = "GET /callback?state=xyz HTTP/1.1\r\n\r\n";
        assert_eq!(extract_query_param(req, "code"), None);
    }

    #[test]
    fn returns_none_for_no_query_string() {
        let req = "GET /callback HTTP/1.1\r\n\r\n";
        assert_eq!(extract_query_param(req, "code"), None);
    }
}
