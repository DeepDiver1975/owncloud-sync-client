// crates/ocis-client/src/auth/token_manager.rs
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::auth::keychain::KeychainStore;
use crate::auth::oidc::{OidcAuth, TokenSet};
use crate::error::{OcisError, Result};

#[derive(Debug)]
pub struct TokenManager {
    oidc: OidcAuth,
    token: Arc<RwLock<TokenSet>>,
    account_id: String,
}

impl TokenManager {
    pub fn new(oidc: OidcAuth, token: TokenSet, account_id: impl Into<String>) -> Self {
        Self {
            oidc,
            token: Arc::new(RwLock::new(token)),
            account_id: account_id.into(),
        }
    }

    /// Shared arc — pass to `WebDavClient` / `GraphClient`.
    pub fn token_arc(&self) -> Arc<RwLock<TokenSet>> {
        Arc::clone(&self.token)
    }

    /// Returns a valid access token, refreshing via OIDC if the current one is expired.
    pub async fn get_valid_token(&self) -> Result<String> {
        // Fast path: token still valid.
        {
            let t = self.token.read().await;
            if !t.is_expired() {
                return Ok(t.access_token.clone());
            }
        }

        // Acquire write lock and recheck — a concurrent caller may have already refreshed.
        let refresh_token = {
            let t = self.token.write().await;
            if !t.is_expired() {
                return Ok(t.access_token.clone());
            }
            t.refresh_token.clone()
        };

        let refresh_token = refresh_token
            .ok_or_else(|| OcisError::Auth("token expired, no refresh token available".into()))?;

        // Network call outside any lock.
        let new_token = self.oidc.refresh(&refresh_token).await?;

        let access_token = new_token.access_token.clone();
        {
            let mut t = self.token.write().await;
            *t = new_token.clone();
        }

        // Best-effort keychain persist — failure is non-fatal.
        let id = self.account_id.clone();
        tokio::task::spawn_blocking(move || {
            if let Err(e) = KeychainStore::save(&id, &new_token) {
                tracing::warn!("failed to save refreshed token to keychain: {e}");
            }
        }); // JoinHandle intentionally dropped — fire-and-forget

        Ok(access_token)
    }
}
