use crate::error::Error;

#[derive(Debug, Clone)]
pub struct PersonResponse {
    pub xuid: Option<String>,
    pub gamertag: Option<String>,
    pub gamer_score: Option<String>,
    pub display_pic_raw: Option<String>,
    pub modern_gamertag: Option<String>,
    pub account_tier: Option<String>,
    pub bio: Option<String>,
    pub location: Option<String>,
    pub reputation: Option<String>,
}

impl PersonResponse {
    pub fn developer_preview() -> Self {
        Self {
            xuid: Some("dev".into()),
            gamertag: Some("Developer".into()),
            gamer_score: Some("0".into()),
            display_pic_raw: None,
            modern_gamertag: None,
            account_tier: Some("Developer".into()),
            bio: None,
            location: None,
            reputation: None,
        }
    }
}

pub async fn xuid_for_gamertag(authorization: &str, gamertag: &str) -> Result<String, Error> {
    let gamertag = gamertag.trim();
    if gamertag.is_empty() {
        return Err(Error::Xbox("gamertag is empty".into()));
    }
    if authorization.trim().is_empty() {
        return Err(Error::Xbox("authorization is empty".into()));
    }
    let url = format!(
        "https://profile.xboxlive.com/users/gt({})/profile/settings?settings=Gamertag",
        encode_gamertag(gamertag)
    );
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?
        .get(url)
        .header("Authorization", authorization)
        .header("x-xbl-contract-version", "2")
        .header("Accept", "application/json")
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        let snippet: String = body.chars().take(180).collect();
        return Err(Error::Xbox(format!(
            "gamertag lookup returned {}: {snippet}",
            status.as_u16()
        )));
    }
    let person = person_from_settings(&body)?;
    person
        .xuid
        .filter(|xuid| !xuid.trim().is_empty())
        .ok_or_else(|| Error::Xbox(format!("no xuid for {gamertag}")))
}

fn encode_gamertag(gamertag: &str) -> String {
    let mut encoded = String::new();
    for byte in gamertag.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

pub async fn fetch_me(authorization: &str) -> Result<PersonResponse, Error> {
    let body = reqwest::Client::new()
        .get("https://profile.xboxlive.com/users/me/profile/settings?settings=Gamertag,ModernGamertag,Gamerscore,GameDisplayPicRaw,AccountTier,Bio,Location,XboxOneRep")
        .header("Authorization", authorization)
        .header("x-xbl-contract-version", "2")
        .header("Accept", "application/json")
        .header("Accept-Language", "en-US")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    person_from_settings(&body)
}

fn person_from_settings(body: &str) -> Result<PersonResponse, Error> {
    #[derive(serde::Deserialize)]
    struct Payload {
        #[serde(rename = "profileUsers")]
        profile_users: Vec<User>,
    }
    #[derive(serde::Deserialize)]
    struct User {
        id: Option<String>,
        #[serde(default)]
        settings: Vec<Setting>,
    }
    #[derive(serde::Deserialize)]
    struct Setting {
        id: String,
        value: String,
    }

    let payload: Payload = serde_json::from_str(body)?;
    let user = payload
        .profile_users
        .into_iter()
        .next()
        .ok_or(Error::EmptyProfile)?;
    let value = |name: &str| {
        user.settings
            .iter()
            .find(|setting| setting.id.eq_ignore_ascii_case(name))
            .map(|setting| setting.value.clone())
    };
    Ok(PersonResponse {
        xuid: user.id,
        gamertag: value("Gamertag"),
        gamer_score: value("Gamerscore"),
        display_pic_raw: value("GameDisplayPicRaw"),
        modern_gamertag: value("ModernGamertag"),
        account_tier: value("AccountTier"),
        bio: value("Bio"),
        location: value("Location"),
        reputation: value("XboxOneRep"),
    })
}
