use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use uuid::Uuid;

use crate::config::{AccountConfig, AppConfig};
use crate::gui_ipc::protocol::DaemonEvent;
use crate::gui_ipc::GuiIpcServer;
use ocis_client::auth::{KeychainStore, OidcAuth, PkceVerifier};

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

    let account_id_str = account_id.to_string();
    tokio::task::spawn_blocking(move || KeychainStore::save(&account_id_str, &tokens))
        .await
        .map_err(|e| anyhow::anyhow!("keychain task panicked: {e}"))?
        .map_err(|e| anyhow::anyhow!("keychain save failed: {e}"))?;

    {
        let mut cfg = config.lock().await;
        cfg.account.push(AccountConfig {
            id: account_id,
            url,
            username: String::new(),
            display_name: String::new(),
            folder: vec![],
        });
        cfg.save(&config_path)?;
    }

    ipc.broadcast(DaemonEvent::AccountStateChanged {
        account_id,
        state: "added".to_string(),
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
