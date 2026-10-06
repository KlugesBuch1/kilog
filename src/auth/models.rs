use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MicrosoftOAuthResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub id_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MicrosoftOAuthError {
    pub error: String,
    #[serde(default)]
    pub error_description: Option<String>,
    #[serde(default)]
    pub error_codes: Vec<i64>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_token_response() {
        let body = r#"{
            "token_type": "Bearer",
            "expires_in": 3600,
            "scope": "openid",
            "access_token": "access",
            "refresh_token": "refresh",
            "id_token": "id"
        }"#;

        let token = parse_oauth_response(body).unwrap();
        assert_eq!(token.access_token, "access");
        assert_eq!(token.token_type, "Bearer");
        assert_eq!(token.expires_in, 3600);
        assert_eq!(token.scope.as_deref(), Some("openid"));
        assert_eq!(token.refresh_token.as_deref(), Some("refresh"));
        assert_eq!(token.id_token.as_deref(), Some("id"));
    }

    #[test]
    fn parses_error_response() {
        let body = r#"{
            "error": "invalid_grant",
            "error_description": "The code is expired.",
            "error_codes": [70000]
        }"#;

        let err = parse_oauth_response(body).unwrap_err();
        let OAuthParseError::OAuth(oauth) = err else {
            panic!("expected an oauth error");
        };
        assert_eq!(oauth.error, "invalid_grant");
        assert_eq!(
            oauth.error_description.as_deref(),
            Some("The code is expired.")
        );
        assert_eq!(oauth.error_codes, vec![70000]);
    }
}
