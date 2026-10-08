use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct AppConfig {
    pub force_region: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            force_region: false,
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
