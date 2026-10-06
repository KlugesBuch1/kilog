use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct AppConfig {
    pub check_for_game_updates: bool,
    pub check_for_app_updates: bool,
    pub unlock_all: bool,
    pub auto_spoofer: bool,
    pub force_region: bool,
    pub autostart_xbox_app: bool,
    pub start_xbox_app_hidden: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            check_for_game_updates: true,
            check_for_app_updates: true,
            unlock_all: false,
            auto_spoofer: false,
            force_region: false,
            autostart_xbox_app: false,
            start_xbox_app_hidden: false,
        }
    }
}

impl AppConfig {
    pub fn load() -> Result<Self, Error> {
        let path = config_path()?;
        if !path.exists() {
            let config = Self::default();
            config.save()?;
            return Ok(config);
        }

        let text = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn save(&self) -> Result<(), Error> {
        let path = config_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}

pub fn kilog_dir() -> Result<PathBuf, Error> {
    let dirs = directories::UserDirs::new().ok_or(Error::NoDocuments)?;
    let docs = dirs.document_dir().ok_or(Error::NoDocuments)?;
    Ok(docs.join("Kilog"))
}

pub fn config_path() -> Result<PathBuf, Error> {
    Ok(kilog_dir()?.join("config.json"))
}
