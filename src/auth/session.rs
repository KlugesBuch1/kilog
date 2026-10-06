use serde::{Deserialize, Serialize};

use crate::config::kilog_dir;
use crate::error::Error;

#[derive(Default, Serialize, Deserialize)]
struct SessionFile {
    refresh_token: Option<String>,
}

fn session_path() -> Result<std::path::PathBuf, Error> {
    Ok(kilog_dir()?.join("session.json"))
}

pub fn load_refresh_token() -> Result<Option<String>, Error> {
    let path = session_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path)?;
    let session: SessionFile = serde_json::from_str(&text)?;
    Ok(session.refresh_token.filter(|token| !token.is_empty()))
}

pub fn save_refresh_token(refresh_token: &str) -> Result<(), Error> {
    let path = session_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let session = SessionFile {
        refresh_token: Some(refresh_token.to_owned()),
    };
    std::fs::write(path, serde_json::to_string(&session)?)?;
    Ok(())
}

pub fn clear_refresh_token() -> Result<(), Error> {
    let path = session_path()?;
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}
