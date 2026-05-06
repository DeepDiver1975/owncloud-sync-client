use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct AppConfig {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub account: Vec<AccountConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct GeneralConfig {
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_true")]
    pub notification_enabled: bool,
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u64,
    /// Accept invalid/self-signed TLS certificates. For testing only.
    #[serde(default)]
    pub insecure: bool,
}

fn default_log_level() -> String {
    "info".to_string()
}
fn default_true() -> bool {
    true
}
fn default_poll_interval() -> u64 {
    30
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            log_level: default_log_level(),
            notification_enabled: default_true(),
            poll_interval_secs: default_poll_interval(),
            insecure: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct AccountConfig {
    pub id: Uuid,
    pub url: String,
    #[serde(default)]
    pub user_id: String,
    pub username: String,
    pub display_name: String,
    #[serde(default)]
    pub folder: Vec<FolderConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct FolderConfig {
    pub id: Uuid,
    pub local_path: String,
    pub space_id: String,
    pub display_name: String,
    #[serde(default)]
    pub selective_sync_excluded: Vec<String>,
    #[serde(default = "default_vfs_mode")]
    pub vfs_mode: String,
    #[serde(default)]
    pub paused: bool,
}

fn default_vfs_mode() -> String {
    "off".to_string()
}

impl AppConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: AppConfig = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn load_or_default(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(AppConfig {
                general: GeneralConfig::default(),
                account: vec![],
            });
        }
        Self::load(path)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    const EXAMPLE_TOML: &str = r#"
[general]
log_level = "info"
notification_enabled = true

[[account]]
id = "11111111-1111-1111-1111-111111111111"
url = "https://ocis.example.com"
username = "alice"
display_name = "Alice"

[[account.folder]]
id = "22222222-2222-2222-2222-222222222222"
local_path = "/home/alice/ownCloud"
space_id = "drive-id"
display_name = "Personal"
selective_sync_excluded = ["large-videos/"]
vfs_mode = "off"
paused = false
"#;

    #[test]
    fn parse_example_toml() {
        let cfg: AppConfig = toml::from_str(EXAMPLE_TOML).unwrap();
        assert_eq!(cfg.general.log_level, "info");
        assert!(cfg.general.notification_enabled);
        assert_eq!(cfg.account.len(), 1);
        let acc = &cfg.account[0];
        assert_eq!(acc.username, "alice");
        assert_eq!(acc.folder.len(), 1);
        let folder = &acc.folder[0];
        assert_eq!(folder.local_path, "/home/alice/ownCloud");
        assert_eq!(folder.selective_sync_excluded, vec!["large-videos/"]);
        assert_eq!(folder.vfs_mode, "off");
        assert!(!folder.paused);
    }

    #[test]
    fn round_trip_save_and_load() {
        let cfg: AppConfig = toml::from_str(EXAMPLE_TOML).unwrap();
        let file = NamedTempFile::new().unwrap();
        cfg.save(file.path()).unwrap();
        let loaded = AppConfig::load(file.path()).unwrap();
        assert_eq!(cfg, loaded);
    }

    #[test]
    fn load_or_default_returns_default_when_absent() {
        let path = std::path::Path::new("/tmp/this-file-does-not-exist-ocsyncd-test.toml");
        let cfg = AppConfig::load_or_default(path).unwrap();
        assert!(cfg.account.is_empty());
        assert_eq!(cfg.general.log_level, "info");
    }
}
