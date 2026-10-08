use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MicrosoftOAuthResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub client_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MicrosoftOAuthError {
    pub error: String,
    #[serde(default)]
    pub error_description: Option<String>,
}

#[derive(Debug, Error)]
pub enum OAuthParseError {
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("{}", .0.error)]
    OAuth(MicrosoftOAuthError),
}

pub fn parse_oauth_response(body: &str) -> Result<MicrosoftOAuthResponse, OAuthParseError> {
    let value: serde_json::Value = serde_json::from_str(body)?;
    if value.get("error").is_some() {
        let err = serde_json::from_value(value)?;
        return Err(OAuthParseError::OAuth(err));
    }
    Ok(serde_json::from_value(value)?)
}
