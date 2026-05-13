use daemon::config::AppConfig;
use std::io::Write;
use tempfile::NamedTempFile;

const MULTI_ACCOUNT_TOML: &str = r#"
[general]
log_level = "debug"
notification_enabled = false

[[account]]
id = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"
url = "https://cloud.example.com"
username = "bob"
display_name = "Bob"

[[account.folder]]
id = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"
local_path = "/home/bob/ownCloud"
space_id = "space-1"
display_name = "Home"
vfs_mode = "off"
paused = false

[[account]]
id = "cccccccc-cccc-cccc-cccc-cccccccccccc"
url = "https://corp.example.com"
username = "bob.corp"
display_name = "Bob (Work)"
"#;

#[test]
fn load_multi_account_from_disk() {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", MULTI_ACCOUNT_TOML).unwrap();
    let cfg = AppConfig::load(file.path()).unwrap();
    assert_eq!(cfg.general.log_level, "debug");
    assert!(!cfg.general.notification_enabled);
    assert_eq!(cfg.account.len(), 2);
    assert_eq!(cfg.account[0].username, "bob");
    assert_eq!(cfg.account[0].folder.len(), 1);
    assert_eq!(cfg.account[1].username, "bob.corp");
    assert!(cfg.account[1].folder.is_empty());
}

#[test]
fn round_trip_multi_account() {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", MULTI_ACCOUNT_TOML).unwrap();
    let cfg = AppConfig::load(file.path()).unwrap();
    let out = NamedTempFile::new().unwrap();
    cfg.save(out.path()).unwrap();
    let cfg2 = AppConfig::load(out.path()).unwrap();
    assert_eq!(cfg, cfg2);
}

#[test]
fn account_config_user_id_defaults_empty_when_absent() {
    let toml = r#"
[[account]]
id = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"
url = "https://cloud.example.com"
username = "bob"
display_name = "Bob"
"#;
    let cfg: daemon::config::AppConfig = toml::from_str(toml).unwrap();
    assert_eq!(cfg.account[0].user_id, "");
}

#[test]
fn account_config_user_id_round_trips() {
    let toml = r#"
[[account]]
id = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"
url = "https://cloud.example.com"
user_id = "4c510ada-c86b-4815-8820-42cdf82c3d51"
username = "alice"
display_name = "Alice"
"#;
    let cfg: daemon::config::AppConfig = toml::from_str(toml).unwrap();
    assert_eq!(
        cfg.account[0].user_id,
        "4c510ada-c86b-4815-8820-42cdf82c3d51"
    );
}

#[test]
fn find_account_by_url_and_user_id_returns_match() {
    use daemon::config::{AccountConfig, AppConfig, GeneralConfig};
    use uuid::Uuid;
    let cfg = AppConfig {
        general: GeneralConfig::default(),
        account: vec![AccountConfig {
            id: Uuid::new_v4(),
            url: "https://cloud.example.com".to_string(),
            user_id: "uid-alice".to_string(),
            username: "alice".to_string(),
            display_name: "Alice".to_string(),
            folder: vec![],
            dismissed_spaces: vec![],
        }],
    };
    assert!(cfg
        .account
        .iter()
        .any(|a| a.url == "https://cloud.example.com" && a.user_id == "uid-alice"));
}

#[test]
fn find_account_by_url_and_user_id_returns_none_when_absent() {
    use daemon::config::{AccountConfig, AppConfig, GeneralConfig};
    use uuid::Uuid;
    let cfg = AppConfig {
        general: GeneralConfig::default(),
        account: vec![AccountConfig {
            id: Uuid::new_v4(),
            url: "https://cloud.example.com".to_string(),
            user_id: "uid-alice".to_string(),
            username: "alice".to_string(),
            display_name: "Alice".to_string(),
            folder: vec![],
            dismissed_spaces: vec![],
        }],
    };
    assert!(!cfg
        .account
        .iter()
        .any(|a| a.url == "https://cloud.example.com" && a.user_id == "uid-bob"));
}
