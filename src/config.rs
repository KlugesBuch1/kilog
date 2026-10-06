use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct Settings {
    pub check_for_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            check_for_updates: true,
        }
    }
}

impl Settings {
    pub fn save(&self) -> Result<(), Error> {
        let path = settings_path()?;
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

pub fn settings_path() -> Result<PathBuf, Error> {
    Ok(kilog_dir()?.join("settings.json"))
}

pub fn load_or_create() -> Result<Settings, Error> {
    let path = settings_path()?;
    if !path.exists() {
        std::fs::create_dir_all(kilog_dir()?)?;
        let settings = Settings::default();
        settings.save()?;
        return Ok(settings);
    }

    let text = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&text)?)
}
