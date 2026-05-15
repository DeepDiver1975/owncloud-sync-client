use std::path::Path;
use serde::{Deserialize, Serialize};

use crate::model::Language;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GuiConfig {
    #[serde(default)]
    pub language: Option<Language>,
}

impl GuiConfig {
    pub fn load_or_default(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}
