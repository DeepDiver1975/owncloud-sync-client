// crates/ocis-client/src/auth/keychain.rs
use keyring::Entry;

use crate::auth::oidc::TokenSet;
use crate::error::{OcisError, Result};

const SERVICE_NAME: &str = "owncloud-sync";

/// Thin wrapper around the OS keychain for persisting [`TokenSet`] values.
pub struct KeychainStore;

impl KeychainStore {
    /// Serialize `token_set` as JSON and store it under `account_id`.
    pub fn save(account_id: &str, token_set: &TokenSet) -> Result<()> {
        let json = serde_json::to_string(token_set).map_err(OcisError::Json)?;
        let entry =
            Entry::new(SERVICE_NAME, account_id).map_err(|e| OcisError::Keychain(e.to_string()))?;
        entry
            .set_password(&json)
            .map_err(|e| OcisError::Keychain(e.to_string()))
    }

    /// Load and deserialize the [`TokenSet`] for `account_id`, or `None` if absent.
    pub fn load(account_id: &str) -> Result<Option<TokenSet>> {
        let entry =
            Entry::new(SERVICE_NAME, account_id).map_err(|e| OcisError::Keychain(e.to_string()))?;

        match entry.get_password() {
            Ok(json) => {
                let token_set: TokenSet = serde_json::from_str(&json).map_err(OcisError::Json)?;
                Ok(Some(token_set))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(OcisError::Keychain(e.to_string())),
        }
    }

    /// Delete the stored entry for `account_id`. Returns `Ok(())` if absent.
    pub fn delete(account_id: &str) -> Result<()> {
        let entry =
            Entry::new(SERVICE_NAME, account_id).map_err(|e| OcisError::Keychain(e.to_string()))?;

        match entry.delete_password() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(OcisError::Keychain(e.to_string())),
        }
    }
}
