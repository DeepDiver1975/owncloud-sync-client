// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

// crates/ocis-client/src/graph/mod.rs
use std::sync::Arc;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use url::Url;

use crate::auth::oidc::TokenSet;
use crate::error::{OcisError, Result};

/// Quota information for a Space.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceQuota {
    pub total: i64,
    pub used: i64,
    pub remaining: i64,
}

/// Identity information for the authenticated user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
}

/// An oCIS Space (Drive).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Space {
    pub id: String,
    pub name: String,
    #[serde(rename = "driveType")]
    pub drive_type: String,
    #[serde(rename = "webUrl")]
    pub web_url: String,
    pub quota: Option<SpaceQuota>,
}

#[derive(Debug, Deserialize)]
struct DriveListResponse {
    value: Vec<DriveJson>,
}

#[derive(Debug, Deserialize)]
struct DriveJson {
    id: String,
    name: String,
    #[serde(rename = "driveType", default)]
    drive_type: String,
    #[serde(rename = "webUrl", default)]
    web_url: String,
    quota: Option<QuotaJson>,
}

#[derive(Debug, Deserialize)]
struct QuotaJson {
    total: Option<i64>,
    used: Option<i64>,
    remaining: Option<i64>,
}

impl From<DriveJson> for Space {
    fn from(d: DriveJson) -> Self {
        Space {
            id: d.id,
            name: d.name,
            drive_type: d.drive_type,
            web_url: d.web_url,
            quota: d.quota.map(|q| SpaceQuota {
                total: q.total.unwrap_or(0),
                used: q.used.unwrap_or(0),
                remaining: q.remaining.unwrap_or(0),
            }),
        }
    }
}

/// HTTP client for the oCIS Graph API.
#[derive(Debug, Clone)]
pub struct GraphClient {
    pub base_url: Url,
    client: Client,
    token: Arc<RwLock<TokenSet>>,
}

impl GraphClient {
    /// Create a new client. `base_url` should be the oCIS server root.
    pub fn new(base_url: Url, token: Arc<RwLock<TokenSet>>) -> Self {
        let client = crate::build_http_client();
        Self {
            base_url,
            client,
            token,
        }
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = self.base_url.join(path).map_err(OcisError::Url)?;
        let token = self.token.read().await.access_token.clone();

        let resp = self
            .client
            .get(url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(OcisError::Http)?
            .error_for_status()
            .map_err(OcisError::Http)?;

        resp.json::<T>().await.map_err(OcisError::Http)
    }

    /// List all Spaces (Drives) accessible to the current user.
    ///
    /// Calls `GET /graph/v1.0/me/drives`.
    pub async fn list_spaces(&self) -> Result<Vec<Space>> {
        let resp: DriveListResponse = self.get_json("graph/v1.0/me/drives").await?;
        Ok(resp.value.into_iter().map(Space::from).collect())
    }

    /// Fetch a single Space by its Drive ID.
    ///
    /// Calls `GET /graph/v1.0/drives/{driveId}`.
    pub async fn get_space(&self, drive_id: &str) -> Result<Space> {
        let path = format!("graph/v1.0/drives/{drive_id}");
        let drive: DriveJson = self.get_json(&path).await?;
        Ok(Space::from(drive))
    }

    /// Fetch the identity of the currently authenticated user.
    ///
    /// Calls `GET /graph/v1.0/me`.
    pub async fn get_me(&self) -> Result<UserInfo> {
        self.get_json("graph/v1.0/me").await
    }
}

/// Return the WebDAV base URL for the given `space_id` on `server_url`.
pub fn webdav_url_for_space(server_url: &Url, space_id: &str) -> Result<Url> {
    let path = format!("dav/spaces/{space_id}/");
    server_url.join(&path).map_err(OcisError::Url)
}

/// Build the WebDAV root URL for a sync folder, branching on the server type.
///
/// - oCIS uses a per-Space root: `{server_url}/dav/spaces/{space_id}/`.
/// - Classic (oc10) has a single per-user root: `{server_url}/remote.php/dav/files/{user_id}/`
///   (`space_id` is ignored — Classic accounts use a synthetic sentinel space).
///
/// `server_url` is expected without a trailing slash; both forms are tolerated.
pub fn webdav_root(
    server_url: &Url,
    server_type: crate::ServerType,
    space_id: &str,
    user_id: &str,
) -> Result<Url> {
    // Ensure the base path ends with '/' so `join` appends rather than replacing
    // the last segment — this preserves sub-path installs like `https://host/owncloud`.
    let mut base = server_url.clone();
    if !base.path().ends_with('/') {
        let with_slash = format!("{}/", base.path());
        base.set_path(&with_slash);
    }
    match server_type {
        crate::ServerType::Ocis => base
            .join(&format!("dav/spaces/{space_id}/"))
            .map_err(OcisError::Url),
        crate::ServerType::Classic => base
            .join(&format!("remote.php/dav/files/{user_id}/"))
            .map_err(OcisError::Url),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ServerType;

    #[test]
    fn ocis_root_uses_spaces_path() {
        let base = Url::parse("https://ocis.example.com/").unwrap();
        let root = webdav_root(&base, ServerType::Ocis, "drive-123", "alice").unwrap();
        assert_eq!(
            root.as_str(),
            "https://ocis.example.com/dav/spaces/drive-123/"
        );
    }

    #[test]
    fn classic_root_uses_legacy_files_path() {
        let base = Url::parse("https://oc10.example.com/").unwrap();
        let root = webdav_root(&base, ServerType::Classic, "ignored-sentinel", "alice").unwrap();
        assert_eq!(
            root.as_str(),
            "https://oc10.example.com/remote.php/dav/files/alice/"
        );
    }
}
