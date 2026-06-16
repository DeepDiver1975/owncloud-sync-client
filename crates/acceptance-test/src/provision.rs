// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

//! Admin-authenticated oCIS user provisioning via the Graph API.
//!
//! Multi-account acceptance tests need more than the bootstrap `admin` user.
//! oCIS lets an admin create local users through `POST /graph/v1.0/users`; this
//! module wraps that (and the matching delete) with the same insecure
//! basic-auth client style as [`crate::ocis_client::OcisClient`]. oCIS
//! auto-creates each user's personal space on their first login, so no further
//! server-side setup is required after [`UserProvisioner::create_user`].

use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use url::Url;

/// Creates and deletes oCIS users as the bootstrap `admin` account.
pub struct UserProvisioner {
    client: Client,
    base_url: Url,
    admin_user: String,
    admin_pass: String,
}

/// A user created on the oCIS server for the lifetime of a test.
#[derive(Debug, Clone)]
pub struct ProvisionedUser {
    /// Graph user id — needed for deletion.
    pub id: String,
    /// Login name (`onPremisesSamAccountName`).
    pub username: String,
    pub password: String,
}

#[derive(Deserialize)]
struct CreatedUser {
    id: String,
}

impl UserProvisioner {
    /// Construct against the running oCIS using the bootstrap `admin`/`admin`
    /// credentials and an insecure TLS client (the test oCIS uses a self-signed
    /// cert).
    pub async fn new(base_url: Url) -> Result<Self> {
        let client = Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .context("failed to build provisioning HTTP client")?;
        Ok(Self {
            client,
            base_url,
            admin_user: "admin".to_owned(),
            admin_pass: "admin".to_owned(),
        })
    }

    /// Create a local user via `POST /graph/v1.0/users`. The password profile
    /// disables the forced-change-on-next-login flow so the user can log in
    /// immediately through OIDC.
    pub async fn create_user(
        &self,
        username: &str,
        password: &str,
        display_name: &str,
    ) -> Result<ProvisionedUser> {
        let url = self
            .base_url
            .join("/graph/v1.0/users")
            .context("invalid users URL")?;
        let body = json!({
            "onPremisesSamAccountName": username,
            "displayName": display_name,
            "mail": format!("{username}@example.com"),
            "passwordProfile": {
                "password": password,
                "forceChangePasswordNextSignIn": false
            }
        });
        let created: CreatedUser = self
            .client
            .post(url)
            .basic_auth(&self.admin_user, Some(&self.admin_pass))
            .json(&body)
            .send()
            .await
            .context("create_user request failed")?
            .error_for_status()
            .context("create_user returned an error status")?
            .json()
            .await
            .context("create_user response was not valid JSON")?;
        Ok(ProvisionedUser {
            id: created.id,
            username: username.to_owned(),
            password: password.to_owned(),
        })
    }

    /// Best-effort delete via `DELETE /graph/v1.0/users/{id}`. Because
    /// provisioned usernames are unique per run, a failed delete never breaks a
    /// later test — this is hygiene, not correctness.
    pub async fn delete_user(&self, id: &str) -> Result<()> {
        let url = self
            .base_url
            .join(&format!("/graph/v1.0/users/{id}"))
            .context("invalid user-delete URL")?;
        self.client
            .delete(url)
            .basic_auth(&self.admin_user, Some(&self.admin_pass))
            .send()
            .await
            .context("delete_user request failed")?
            .error_for_status()
            .context("delete_user returned an error status")?;
        Ok(())
    }
}
