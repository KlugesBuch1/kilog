use serde::Deserialize;

use crate::error::Error;

pub async fn request_xbox_live_token(access_token: &str) -> Result<String, Error> {
    let user_token = request_user_token(access_token).await?;
    authorize(&user_token).await
}

async fn request_user_token(access_token: &str) -> Result<String, Error> {
    let body = serde_json::json!({
        "Properties": {
            "AuthMethod": "RPS",
            "SiteName": "user.auth.xboxlive.com",
            "RpsTicket": format!("t={access_token}")
        },
        "RelyingParty": "http://auth.xboxlive.com",
        "TokenType": "JWT"
    });
    let token = post_token("https://user.auth.xboxlive.com/user/authenticate", &body).await?;
    Ok(token.token)
}

async fn authorize(user_token: &str) -> Result<String, Error> {
    let body = serde_json::json!({
        "Properties": {
            "SandboxId": "RETAIL",
            "UserTokens": [user_token]
        },
        "RelyingParty": "http://xboxlive.com",
        "TokenType": "JWT"
    });
    let token = post_token("https://xsts.auth.xboxlive.com/xsts/authorize", &body).await?;
    let user_hash = token
        .display_claims
        .and_then(|claims| claims.xui.into_iter().next())
        .map(|xui| xui.uhs)
        .unwrap_or_default();
    Ok(format!("XBL3.0 x={user_hash};{}", token.token))
}

async fn post_token(url: &str, body: &serde_json::Value) -> Result<TokenResponse, Error> {
    let response = reqwest::Client::new()
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .header("x-xbl-contract-version", "1")
        .json(body)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    Ok(serde_json::from_str(&response)?)
}

#[derive(Deserialize)]
struct TokenResponse {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: Option<DisplayClaims>,
}

#[derive(Deserialize)]
struct DisplayClaims {
    xui: Vec<XuiClaim>,
}

#[derive(Deserialize)]
struct XuiClaim {
    uhs: String,
}
