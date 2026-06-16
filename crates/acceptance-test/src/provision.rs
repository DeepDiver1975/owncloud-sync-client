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

/// A project space created on the oCIS server for the lifetime of a test.
#[derive(Debug, Clone)]
pub struct ProvisionedSpace {
    /// Drive id — used as the space id by `ListSpaces` and in WebDAV URLs.
    pub id: String,
    /// The (unique) space name as created.
    pub name: String,
}

/// Built-in oCIS `unifiedRoleDefinition` ids for project-space sharing.
///
/// Roles are matched by these **stable** ids, not by `displayName`: oCIS display
/// names collide (`SpaceViewer` and `Viewer` are both "Can view"; several editor
/// variants are "Can edit") and are localized, so a name match is ambiguous. The
/// ids are defined in `owncloud/ocis` `services/graph/pkg/unifiedrole/roles.go`.
pub mod role_ids {
    /// Space Viewer — read-only on a project space.
    pub const SPACE_VIEWER: &str = "a8d5fe5e-96e3-418d-825b-534dbdf22b99";
    /// Space Editor — read + write on a project space.
    pub const SPACE_EDITOR: &str = "58c63c02-1d89-4572-916a-870abc5a1b7d";
    /// Manager — read + write + manage members.
    pub const MANAGER: &str = "312c0871-5ef7-4b3a-85b6-0e4074c64049";
    /// Secure Viewer — download-disabled viewer; often disabled in stock oCIS.
    pub const SECURE_VIEWER: &str = "aa97fe03-7980-45ac-9e50-b325749fd7e6";
}

/// Outcome of [`SpaceProvisioner::assign_role`].
///
/// A requested role may simply not be enabled in this oCIS configuration (the
/// most likely case is Secure Viewer). That is an environment property, not a
/// test failure, so it is reported as a value the caller can skip-and-log on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoleAssignment {
    /// The role id was present in the server's role definitions and assigned.
    Assigned,
    /// No role definition with the requested id is enabled on this server.
    Unavailable,
}

/// Creates project spaces and assigns roles as the bootstrap `admin` account.
pub struct SpaceProvisioner {
    client: Client,
    base_url: Url,
    admin_user: String,
    admin_pass: String,
}

#[derive(Deserialize)]
struct CreatedDrive {
    id: String,
    name: String,
}

/// The roleDefinitions endpoint wraps the array in a `value` field.
#[derive(Deserialize)]
struct RoleDefinitionsResponse {
    #[serde(default)]
    value: Vec<RoleDefinition>,
}

#[derive(Deserialize)]
struct RoleDefinition {
    id: String,
}

impl SpaceProvisioner {
    /// Construct against the running oCIS using the bootstrap `admin`/`admin`
    /// credentials and an insecure TLS client (the test oCIS uses a self-signed
    /// cert). Same style as [`UserProvisioner::new`].
    pub async fn new(base_url: Url) -> Result<Self> {
        let client = Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .context("failed to build space-provisioning HTTP client")?;
        Ok(Self {
            client,
            base_url,
            admin_user: "admin".to_owned(),
            admin_pass: "admin".to_owned(),
        })
    }

    /// Create a `project` space via `POST /graph/v1.0/drives`. Returns the new
    /// drive id (the space id surfaced later by `ListSpaces` and used in WebDAV
    /// URLs).
    pub async fn create_project_space(&self, name: &str) -> Result<ProvisionedSpace> {
        let url = self
            .base_url
            .join("/graph/v1.0/drives")
            .context("invalid drives URL")?;
        let body = json!({
            "name": name,
            "driveType": "project"
        });
        let created: CreatedDrive = self
            .client
            .post(url)
            .basic_auth(&self.admin_user, Some(&self.admin_pass))
            .json(&body)
            .send()
            .await
            .context("create_project_space request failed")?
            .error_for_status()
            .context("create_project_space returned an error status")?
            .json()
            .await
            .context("create_project_space response was not valid JSON")?;
        Ok(ProvisionedSpace {
            id: created.id,
            name: created.name,
        })
    }

    /// Fetch the set of `unifiedRoleDefinition` ids the server currently exposes,
    /// via `GET /graph/v1beta1/roleManagement/permissions/roleDefinitions`. The
    /// role endpoints live under the Graph mount's `v1beta1` namespace (not
    /// `v1.0`), and the response wraps the array in a `value` field.
    async fn available_role_ids(&self) -> Result<Vec<String>> {
        let url = self
            .base_url
            .join("/graph/v1beta1/roleManagement/permissions/roleDefinitions")
            .context("invalid roleDefinitions URL")?;
        let resp: RoleDefinitionsResponse = self
            .client
            .get(url)
            .basic_auth(&self.admin_user, Some(&self.admin_pass))
            .send()
            .await
            .context("roleDefinitions request failed")?
            .error_for_status()
            .context("roleDefinitions returned an error status")?
            .json()
            .await
            .context("roleDefinitions response was not valid JSON")?;
        Ok(resp.value.into_iter().map(|d| d.id).collect())
    }

    /// Assign the built-in role `role_id` (see [`role_ids`]) to user `user_id` on
    /// the space `space_id`, via
    /// `POST /graph/v1beta1/drives/{space_id}/root/invite`.
    ///
    /// The role id is first checked against the server's enabled role
    /// definitions; if it is not present, returns [`RoleAssignment::Unavailable`]
    /// (without erroring) so the caller can skip-and-log.
    pub async fn assign_role(
        &self,
        space_id: &str,
        user_id: &str,
        role_id: &str,
    ) -> Result<RoleAssignment> {
        if !self
            .available_role_ids()
            .await?
            .iter()
            .any(|id| id == role_id)
        {
            return Ok(RoleAssignment::Unavailable);
        }
        let url = self
            .base_url
            .join(&format!("/graph/v1beta1/drives/{space_id}/root/invite"))
            .context("invalid invite URL")?;
        let body = json!({
            "recipients": [
                { "objectId": user_id, "@libre.graph.recipient.type": "user" }
            ],
            "roles": [role_id]
        });
        self.client
            .post(url)
            .basic_auth(&self.admin_user, Some(&self.admin_pass))
            .json(&body)
            .send()
            .await
            .context("assign_role invite request failed")?
            .error_for_status()
            .context("assign_role invite returned an error status")?;
        Ok(RoleAssignment::Assigned)
    }
}
