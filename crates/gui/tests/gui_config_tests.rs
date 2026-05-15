use gui::gui_config::GuiConfig;
use gui::model::Language;

#[test]
fn gui_config_round_trips_language() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gui-config.toml");

    let cfg = GuiConfig { language: Some(Language::De) };
    cfg.save(&path).unwrap();

    let loaded = GuiConfig::load_or_default(&path);
    assert_eq!(loaded.language, Some(Language::De));
}

#[test]
fn gui_config_missing_file_returns_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gui-config.toml");

    let cfg = GuiConfig::load_or_default(&path);
    assert_eq!(cfg.language, None);
}

#[test]
fn gui_config_save_creates_parent_directories() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("deep").join("gui-config.toml");

    let cfg = GuiConfig { language: Some(Language::Fr) };
    cfg.save(&path).unwrap(); // should not fail even though parents don't exist

    let loaded = GuiConfig::load_or_default(&path);
    assert_eq!(loaded.language, Some(Language::Fr));
}
