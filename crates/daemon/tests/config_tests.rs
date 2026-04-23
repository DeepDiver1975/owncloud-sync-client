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
