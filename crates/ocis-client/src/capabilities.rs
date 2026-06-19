// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

// crates/ocis-client/src/capabilities.rs
//
// Server-type detection and identity for ownCloud backends via the legacy OCS API.
//
// oCIS exposes the Graph API and Spaces; ownCloud Classic ("oc10", owncloud/core)
// does not — it has a single WebDAV root per user. Both products serve the OCS
// capabilities endpoint, and oCIS additionally advertises `spaces.enabled = true`
// there. We use the presence/absence of that flag to tell the two apart.

use std::sync::Arc;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use url::Url;

use crate::auth::oidc::TokenSet;
use crate::error::{OcisError, Result};

/// Which ownCloud backend an account talks to.
///
/// Defaults to [`ServerType::Ocis`] so existing configs (written before this
/// field existed) keep their original behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerType {
    #[default]
    Ocis,
    Classic,
}

#[derive(Debug, Deserialize)]
struct StatusPhp {
    #[serde(default)]
    installed: bool,
    #[serde(default)]
    productname: String,
}

#[derive(Debug, Deserialize)]
struct OcsEnvelope<T> {
    ocs: OcsBody<T>,
}

#[derive(Debug, Deserialize)]
struct OcsBody<T> {
    data: T,
}

#[derive(Debug, Deserialize)]
struct CapabilitiesData {
    #[serde(default)]
    capabilities: Capabilities,
}

#[derive(Debug, Default, Deserialize)]
struct Capabilities {
    #[serde(default)]
    spaces: Option<SpacesCapability>,
}

#[derive(Debug, Deserialize)]
struct SpacesCapability {
    #[serde(default)]
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct OcsUserData {
    id: String,
    #[serde(rename = "display-name", default)]
    display_name: String,
}

async fn get_ocs_json<T: serde::de::DeserializeOwned>(
    client: &Client,
    base_url: &Url,
    token: &Arc<RwLock<TokenSet>>,
    path: &str,
) -> Result<T> {
    let url = base_url.join(path).map_err(OcisError::Url)?;
    let access_token = token.read().await.access_token.clone();

    let resp = client
        .get(url)
        .bearer_auth(&access_token)
        // OCS defaults to XML; force JSON. Some setups honor the query param,
        // others the Accept header — send both.
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(OcisError::Http)?
        .error_for_status()
        .map_err(OcisError::Http)?;

    resp.json::<T>().await.map_err(OcisError::Http)
}

/// Probe `{base_url}/status.php` (served without authentication by both oCIS and
/// oc10) to confirm the host is a reachable ownCloud server.
///
/// Used at account-add time, before any token exists, to decide whether to fall
/// back to the static oc10 OAuth2 flow. Returns `Ok(())` only for an installed
/// ownCloud-family server.
pub async fn probe_owncloud_status(base_url: &Url, insecure: bool) -> Result<()> {
    let url = base_url.join("status.php").map_err(OcisError::Url)?;
    let env_insecure = std::env::var("OCIS_INSECURE")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false);
    let client = Client::builder()
        .danger_accept_invalid_certs(insecure || env_insecure)
        .build()
        .map_err(OcisError::Http)?;

    let status: StatusPhp = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(OcisError::Http)?
        .error_for_status()
        .map_err(OcisError::Http)?
        .json()
        .await
        .map_err(OcisError::Http)?;

    if status.installed {
        Ok(())
    } else {
        Err(OcisError::Auth(format!(
            "server at {base_url} is not a usable ownCloud instance (productname={:?})",
            status.productname
        )))
    }
}

/// Detect whether `base_url` is an oCIS or a Classic (oc10) server.
///
/// Queries `ocs/v1.php/cloud/capabilities` and inspects `spaces.enabled`.
/// A server that advertises spaces is oCIS; anything else that answers the
/// OCS endpoint is treated as Classic.
pub async fn detect_server_type(
    base_url: &Url,
    token: &Arc<RwLock<TokenSet>>,
) -> Result<ServerType> {
    let client = crate::build_http_client();
    let data: OcsEnvelope<CapabilitiesData> = get_ocs_json(
        &client,
        base_url,
        token,
        "ocs/v1.php/cloud/capabilities?format=json",
    )
    .await?;

    let spaces_enabled = data
        .ocs
        .data
        .capabilities
        .spaces
        .map(|s| s.enabled)
        .unwrap_or(false);

    Ok(if spaces_enabled {
        ServerType::Ocis
    } else {
        ServerType::Classic
    })
}

/// Identity of the authenticated user on a Classic server.
///
/// Returns `(user_id, display_name)`. The `user_id` is the value that forms the
/// legacy WebDAV path `/remote.php/dav/files/{user_id}/`, so it must be the OCS
/// `id` and not a display name.
pub async fn ocs_user(base_url: &Url, token: &Arc<RwLock<TokenSet>>) -> Result<(String, String)> {
    let client = crate::build_http_client();
    let data: OcsEnvelope<OcsUserData> = get_ocs_json(
        &client,
        base_url,
        token,
        "ocs/v1.php/cloud/user?format=json",
    )
    .await?;
    let display_name = if data.ocs.data.display_name.is_empty() {
        data.ocs.data.id.clone()
    } else {
        data.ocs.data.display_name
    };
    Ok((data.ocs.data.id, display_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_caps(json: &str) -> ServerType {
        let data: OcsEnvelope<CapabilitiesData> = serde_json::from_str(json).unwrap();
        let spaces_enabled = data
            .ocs
            .data
            .capabilities
            .spaces
            .map(|s| s.enabled)
            .unwrap_or(false);
        if spaces_enabled {
            ServerType::Ocis
        } else {
            ServerType::Classic
        }
    }

    #[test]
    fn spaces_enabled_means_ocis() {
        let json = r#"{"ocs":{"data":{"capabilities":{"spaces":{"enabled":true}}}}}"#;
        assert_eq!(parse_caps(json), ServerType::Ocis);
    }

    #[test]
    fn spaces_disabled_means_classic() {
        let json = r#"{"ocs":{"data":{"capabilities":{"spaces":{"enabled":false}}}}}"#;
        assert_eq!(parse_caps(json), ServerType::Classic);
    }

    #[test]
    fn no_spaces_capability_means_classic() {
        let json = r#"{"ocs":{"data":{"capabilities":{"core":{"pollinterval":60}}}}}"#;
        assert_eq!(parse_caps(json), ServerType::Classic);
    }

    #[test]
    fn parses_ocs_user_id_and_display_name() {
        let json = r#"{"ocs":{"data":{"id":"alice","display-name":"Alice Liddell"}}}"#;
        let data: OcsEnvelope<OcsUserData> = serde_json::from_str(json).unwrap();
        assert_eq!(data.ocs.data.id, "alice");
        assert_eq!(data.ocs.data.display_name, "Alice Liddell");
    }

    #[test]
    fn server_type_serde_roundtrip() {
        assert_eq!(
            serde_json::to_string(&ServerType::Classic).unwrap(),
            "\"classic\""
        );
        assert_eq!(
            serde_json::from_str::<ServerType>("\"ocis\"").unwrap(),
            ServerType::Ocis
        );
    }
}
