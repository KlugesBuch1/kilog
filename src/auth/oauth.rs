use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::auth::models::{MicrosoftOAuthError, OAuthParseError, parse_oauth_response};
use crate::error::Error;

pub const DEFAULT_CLIENT_ID: &str = "000000004424da1f";

const DEVICE_CODE_URL: &str = "https://login.live.com/oauth20_connect.srf";
const TOKEN_URL: &str = "https://login.live.com/oauth20_token.srf";
const SCOPE: &str = "service::user.auth.xboxlive.com::MBI_SSL";
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceCode {
    pub user_code: String,
    pub device_code: String,
    #[serde(alias = "verification_url")]
    pub verification_uri: String,
    #[serde(default = "default_interval")]
    pub interval: u64,
}

fn default_interval() -> u64 {
    5
}

pub async fn request_device_code(client_id: &str) -> Result<DeviceCode, Error> {
    let client_id = resolve_client_id(client_id);
    let response = reqwest::Client::new()
        .post(DEVICE_CODE_URL)
        .form(&[
            ("client_id", client_id.as_str()),
            ("scope", SCOPE),
            ("response_type", "device_code"),
        ])
        .send()
        .await?
        .text()
        .await?;
    match serde_json::from_str::<DeviceCode>(&response) {
        Ok(code) => Ok(code),
        Err(err) => Err(oauth_or_json(&response, err)),
    }
}

pub async fn poll_device_token(
    client_id: &str,
    device_code: &str,
    mut interval: u64,
) -> Result<crate::auth::MicrosoftOAuthResponse, Error> {
    if interval == 0 {
        interval = default_interval();
    }
    let client_id = resolve_client_id(client_id);

    let client = reqwest::Client::new();
    let started = Instant::now();
    loop {
        tokio::time::sleep(Duration::from_secs(interval)).await;

        let response = client
            .post(TOKEN_URL)
            .form(&[
                ("client_id", client_id.as_str()),
                ("device_code", device_code),
                ("grant_type", DEVICE_GRANT),
            ])
            .send()
            .await?
            .text()
            .await?;

        match interpret_token_body(&response, interval)? {
            PollStep::Token(token) => return Ok(token),
            PollStep::Pending(next_interval) => interval = next_interval,
            PollStep::Expired => {
                return Err(Error::OAuth("device code expired before approval".into()));
            }
        }

        if started.elapsed() > Duration::from_secs(60 * 30) {
            return Err(Error::OAuth("device code expired before approval".into()));
        }
    }
}

pub async fn refresh_token_grant(
    refresh_token: &str,
) -> Result<crate::auth::MicrosoftOAuthResponse, Error> {
    if refresh_token.is_empty() {
        return Err(Error::OAuth("refresh token is empty".into()));
    }
    let client_id = resolve_client_id("");
    let response = reqwest::Client::new()
        .post(TOKEN_URL)
        .form(&[
            ("client_id", client_id.as_str()),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
            ("scope", SCOPE),
        ])
        .send()
        .await?
        .text()
        .await?;
    parse_oauth_response(&response).map_err(|err| match err {
        OAuthParseError::OAuth(oauth) => {
            Error::OAuth(oauth.error_description.unwrap_or(oauth.error))
        }
        OAuthParseError::Json(err) => err.into(),
    })
}

enum PollStep {
    Token(crate::auth::MicrosoftOAuthResponse),
    Pending(u64),
    Expired,
}

fn interpret_token_body(body: &str, interval: u64) -> Result<PollStep, Error> {
    match parse_oauth_response(body) {
        Ok(token) => Ok(PollStep::Token(token)),
        Err(OAuthParseError::OAuth(err)) => match err.error.as_str() {
            "authorization_pending" => Ok(PollStep::Pending(interval)),
            "slow_down" => Ok(PollStep::Pending(interval.saturating_add(5))),
            "expired_token" => Ok(PollStep::Expired),
            _ => Err(Error::OAuth(err.error_description.unwrap_or(err.error))),
        },
        Err(OAuthParseError::Json(err)) => Err(err.into()),
    }
}

fn oauth_or_json(body: &str, json_err: serde_json::Error) -> Error {
    match serde_json::from_str::<MicrosoftOAuthError>(body) {
        Ok(err) => Error::OAuth(err.error_description.unwrap_or(err.error)),
        Err(_) => json_err.into(),
    }
}

fn resolve_client_id(explicit: &str) -> String {
    if let Ok(from_env) = std::env::var("KILOG_OAUTH_CLIENT_ID") {
        let from_env = from_env.trim();
        if !from_env.is_empty() {
            return from_env.to_owned();
        }
    }
    let explicit = explicit.trim();
    if !explicit.is_empty() {
        return explicit.to_owned();
    }
    DEFAULT_CLIENT_ID.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_client_id_uses_the_xbox_app_default() {
        let id = resolve_client_id("");
        if std::env::var("KILOG_OAUTH_CLIENT_ID")
            .ok()
            .is_some_and(|value| !value.trim().is_empty())
        {
            assert_ne!(id, "");
        } else {
            assert_eq!(id, DEFAULT_CLIENT_ID);
        }
    }

    #[test]
    fn pending_response_keeps_the_interval() {
        let body = r#"{"error":"authorization_pending","error_description":"still waiting"}"#;
        let PollStep::Pending(interval) = interpret_token_body(body, 5).unwrap() else {
            panic!("expected a pending poll");
        };
        assert_eq!(interval, 5);
    }

    #[test]
    fn token_response_finishes_the_poll() {
        let body = r#"{"token_type":"Bearer","expires_in":3600,"access_token":"access"}"#;
        let PollStep::Token(token) = interpret_token_body(body, 5).unwrap() else {
            panic!("expected a token");
        };
        assert_eq!(token.access_token, "access");
    }
}
