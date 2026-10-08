use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::auth::models::{MicrosoftOAuthError, OAuthParseError, parse_oauth_response};
use crate::error::Error;

pub const DEFAULT_CLIENT_ID: &str = "000000004424da1f";

const DEVICE_CODE_URL: &str = "https://login.live.com/oauth20_connect.srf";
const AUTHORIZE_URL: &str = "https://login.live.com/oauth20_authorize.srf";
const TOKEN_URL: &str = "https://login.live.com/oauth20_token.srf";
const DESKTOP_REDIRECT: &str = "https://login.live.com/oauth20_desktop.srf";
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
            PollStep::Token(mut token) => {
                token.client_id = client_id.clone();
                return Ok(token);
            }
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

pub fn authorization_url(client_id: &str) -> String {
    let client_id = resolve_client_id(client_id);
    let mut url = reqwest::Url::parse(AUTHORIZE_URL).expect("authorize url");
    url.query_pairs_mut()
        .append_pair("client_id", &client_id)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", DESKTOP_REDIRECT)
        .append_pair("response_mode", "query")
        .append_pair("scope", SCOPE)
        .append_pair("prompt", "select_account");
    url.into()
}

pub fn authorization_code_from_redirect(url: &str) -> Option<Result<String, Error>> {
    let url = reqwest::Url::parse(url).ok()?;
    let mut code = None;
    let mut error = None;
    let mut description = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            "error_description" => description = Some(value.into_owned()),
            _ => {}
        }
    }
    if code.as_ref().is_none_or(|value| value.is_empty())
        && error.as_ref().is_none_or(|value| value.is_empty())
        && description.as_ref().is_none_or(|value| value.is_empty())
    {
        return None;
    }
    if let Some(code) = code.filter(|value| !value.is_empty())
        && error.as_ref().is_none_or(|value| value.is_empty())
    {
        return Some(Ok(code));
    }
    Some(Err(Error::OAuth(
        description
            .filter(|value| !value.is_empty())
            .or(error)
            .unwrap_or_else(|| "sign-in failed".into()),
    )))
}

pub async fn exchange_authorization_code(
    client_id: &str,
    code: &str,
) -> Result<crate::auth::MicrosoftOAuthResponse, Error> {
    if code.is_empty() {
        return Err(Error::OAuth("authorization code is empty".into()));
    }
    let client_id = resolve_client_id(client_id);
    let response = reqwest::Client::new()
        .post(TOKEN_URL)
        .form(&[
            ("client_id", client_id.as_str()),
            ("redirect_uri", DESKTOP_REDIRECT),
            ("grant_type", "authorization_code"),
            ("code", code),
            ("scope", SCOPE),
        ])
        .send()
        .await?
        .text()
        .await?;
    let mut token = parse_oauth_response(&response).map_err(|err| match err {
        OAuthParseError::OAuth(oauth) => oauth_failure(oauth),
        OAuthParseError::Json(err) => err.into(),
    })?;
    token.client_id = client_id;
    Ok(token)
}

pub async fn interactive_sign_in() -> Result<crate::auth::MicrosoftOAuthResponse, Error> {
    let client_id = resolve_client_id("");
    let url = authorization_url(&client_id);
    let code = tokio::task::spawn_blocking(move || crate::auth::browser::capture_code(&url))
        .await
        .map_err(|err| Error::OAuth(format!("sign-in window stopped: {err}")))??;
    exchange_authorization_code(&client_id, &code).await
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
    let mut token = parse_oauth_response(&response).map_err(|err| match err {
        OAuthParseError::OAuth(oauth) => oauth_failure(oauth),
        OAuthParseError::Json(err) => err.into(),
    })?;
    token.client_id = client_id;
    Ok(token)
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

pub(crate) fn oauth_failure(err: crate::auth::models::MicrosoftOAuthError) -> Error {
    if err.error == "invalid_grant" {
        Error::InvalidGrant
    } else {
        Error::OAuth(err.error_description.unwrap_or(err.error))
    }
}

pub(crate) fn resolve_client_id(explicit: &str) -> String {
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
