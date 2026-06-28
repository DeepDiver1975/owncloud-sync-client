// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

// crates/ocis-client/src/lib.rs
pub mod auth;
pub mod capabilities;
pub mod error;
pub mod graph;
pub mod tus;
pub mod webdav;
pub mod webfinger;

pub use capabilities::{detect_server_type, ocs_user, probe_owncloud_status, ServerType};
pub use webfinger::resolve_server_instance;
pub use error::{OcisError, Result};
pub use graph::{webdav_root, webdav_url_for_space, GraphClient, Space, SpaceQuota, UserInfo};
pub use tus::{TusClient, TusUpload};

/// Build a `reqwest::Client`, accepting invalid TLS certs when `OCIS_INSECURE=1` is set.
pub fn build_http_client() -> reqwest::Client {
    let insecure = std::env::var("OCIS_INSECURE")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false);
    reqwest::Client::builder()
        .danger_accept_invalid_certs(insecure)
        .build()
        .expect("build reqwest client")
}
