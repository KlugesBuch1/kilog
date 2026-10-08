use serde::Deserialize;

use crate::auth::oauth::resolve_client_id;
use crate::auth::pop::ProofSigner;
use crate::error::Error;

const DEVICE_URL: &str = "https://device.auth.xboxlive.com/device/authenticate";
const SISU_URL: &str = "https://sisu.xboxlive.com/authorize";
const XBOX_LIVE_RELYING_PARTY: &str = "http://xboxlive.com";
const EVENTS_RELYING_PARTY: &str = "http://events.xboxlive.com";
const USER_AGENT: &str = "Mozilla/5.0 (XboxReplay; XboxLiveAuth/3.0) \
AppleWebKit/537.36 (KHTML, like Gecko) \
Chrome/71.0.3578.98 Safari/537.36";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XboxAuthorization {
    pub authorization: String,
    pub events_token: String,
}

impl XboxAuthorization {
    pub fn developer_mock() -> Self {
        Self {
            authorization: "XBL3.0 x=dev;mock".into(),
            events_token: "x:XBL3.0 x=dev;mock".into(),
        }
    }
}

pub async fn authorize_xbox_live(
    access_token: &str,
    client_id: &str,
) -> Result<XboxAuthorization, Error> {
    if access_token.is_empty() {
        return Err(Error::Xbox("microsoft access token is empty".into()));
    }
    let client_id = app_id(client_id);
    let http = reqwest::Client::new();
    let live = authorize_party(
        &http,
        access_token,
        &client_id,
        XBOX_LIVE_RELYING_PARTY,
        false,
    );
    let events = authorize_party(&http, access_token, &client_id, EVENTS_RELYING_PARTY, true);
    let (live_result, events_result) = tokio::join!(live, events);
    let authorization = live_result?;
    let events_token = events_result?;

    Ok(XboxAuthorization {
        authorization,
        events_token,
    })
}

fn app_id(client_id: &str) -> String {
    let client_id = client_id.trim();
    if client_id.is_empty() {
        resolve_client_id("")
    } else {
        client_id.to_owned()
    }
}

async fn authorize_party(
    http: &reqwest::Client,
    access_token: &str,
    client_id: &str,
    relying_party: &str,
    events: bool,
) -> Result<String, Error> {
    let signer = ProofSigner::generate();
    let device_token = request_device_token(http, &signer).await?;
    request_sisu(
        http,
        &signer,
        access_token,
        client_id,
        &device_token,
        relying_party,
        events,
    )
    .await
}

async fn request_device_token(
    http: &reqwest::Client,
    signer: &ProofSigner,
) -> Result<String, Error> {
    let body = device_body(
        signer,
        &uuid::Uuid::new_v4().to_string(),
        &uuid::Uuid::new_v4().to_string(),
    );
    let response = post_signed(http, signer, DEVICE_URL, &body).await?;
    let token: DeviceToken = serde_json::from_str(&response)?;
    if token.token.is_empty() {
        return Err(Error::Xbox("device token was empty".into()));
    }
    Ok(token.token)
}

async fn request_sisu(
    http: &reqwest::Client,
    signer: &ProofSigner,
    access_token: &str,
    client_id: &str,
    device_token: &str,
    relying_party: &str,
    events: bool,
) -> Result<String, Error> {
    let body = sisu_body(signer, access_token, client_id, device_token, relying_party);
    let response = post_signed(http, signer, SISU_URL, &body).await?;
    authorization_header(&response, events)
}

fn device_body(signer: &ProofSigner, id: &str, serial: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "Properties": {
            "AuthMethod": "ProofOfPossession",
            "Id": format!("{{{id}}}"),
            "DeviceType": "Win32",
            "SerialNumber": format!("{{{serial}}}"),
            "Version": "0.0.0",
            "ProofKey": signer.proof_key(),
        },
        "RelyingParty": "http://auth.xboxlive.com",
        "TokenType": "JWT"
    }))
    .expect("device token body")
}

fn sisu_body(
    signer: &ProofSigner,
    access_token: &str,
    client_id: &str,
    device_token: &str,
    relying_party: &str,
) -> String {
    serde_json::to_string(&serde_json::json!({
        "AccessToken": format!("t={access_token}"),
        "AppId": client_id,
        "DeviceToken": device_token,
        "Sandbox": "RETAIL",
        "UseModernGamertag": true,
        "SiteName": "user.auth.xboxlive.com",
        "RelyingParty": relying_party,
        "ProofKey": signer.proof_key(),
    }))
    .expect("sisu body")
}

async fn post_signed(
    http: &reqwest::Client,
    signer: &ProofSigner,
    url: &str,
    body: &str,
) -> Result<String, Error> {
    let signature = signer.sign_request(url, body);
    let response = http
        .post(url)
        .header("Accept", "application/json")
        .header("Accept-Language", "en-US")
        .header("Cache-Control", "no-store, must-revalidate, no-cache")
        .header("User-Agent", USER_AGENT)
        .header("Signature", &signature)
        .header("Content-Type", "application/json; charset=utf-8")
        .body(body.to_owned())
        .send()
        .await?;
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        return Err(Error::Xbox(format!(
            "xbox live returned {status}: {}",
            truncate(&text)
        )));
    }
    Ok(text)
}

fn authorization_header(body: &str, events: bool) -> Result<String, Error> {
    let response: SisuResponse = serde_json::from_str(body)?;
    let token = response.authorization_token.token;
    if token.is_empty() {
        return Err(Error::Xbox("xbox authorization token was empty".into()));
    }
    let user_hash = response
        .authorization_token
        .display_claims
        .and_then(|claims| claims.xui.into_iter().next())
        .map(|claim| claim.uhs)
        .filter(|hash| !hash.is_empty())
        .ok_or_else(|| Error::Xbox("xbox authorization is missing the user hash".into()))?;
    let prefix = if events { "x:XBL3.0 x=" } else { "XBL3.0 x=" };
    Ok(format!("{prefix}{user_hash};{token}"))
}

fn truncate(body: &str) -> String {
    let body = body.trim();
    let end = body
        .char_indices()
        .nth(300)
        .map(|(index, _)| index)
        .unwrap_or(body.len());
    body[..end].to_owned()
}

#[derive(Deserialize)]
struct DeviceToken {
    #[serde(rename = "Token")]
    token: String,
}

#[derive(Deserialize)]
struct SisuResponse {
    #[serde(rename = "AuthorizationToken")]
    authorization_token: AuthToken,
}

#[derive(Deserialize)]
struct AuthToken {
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
