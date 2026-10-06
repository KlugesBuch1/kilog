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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_signed_in_settings() {
        let person = person_from_settings(
            r#"{"profileUsers":[{"id":"9","settings":[{"id":"Gamertag","value":"Example"},{"id":"Gamerscore","value":"250"},{"id":"GameDisplayPicRaw","value":"https://example/pic"}]}]}"#,
        )
        .unwrap();
        assert_eq!(person.gamertag.as_deref(), Some("Example"));
        assert_eq!(person.gamer_score.as_deref(), Some("250"));
        assert_eq!(
            person.display_pic_raw.as_deref(),
            Some("https://example/pic")
        );
    }
}
