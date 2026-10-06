use crate::error::Error;
use crate::xbox::models::{PersonResponse, Profile};

const PROFILE_URL: &str = "https://peoplehub.xboxlive.com/users/me/people/xuids({xuid})/decoration/detail,preferredColor,presenceDetail,multiplayerSummary";

/// Load the signed-in user's people-hub profile.
///
/// `authorization` is the caller's Xbox Live authorization header value.
pub async fn fetch_profile(authorization: &str, xuid: &str) -> Result<PersonResponse, Error> {
    let url = PROFILE_URL.replace("{xuid}", xuid);
    let body = reqwest::Client::new()
        .get(url)
        .header("Authorization", authorization)
        .header("x-xbl-contract-version", "5")
        .header("Accept", "application/json")
        .header("Accept-Language", "en-US")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    person_from_body(&body)
}

fn person_from_body(body: &str) -> Result<PersonResponse, Error> {
    let profile: Profile = serde_json::from_str(body)?;
    profile.people.into_iter().next().ok_or(Error::EmptyProfile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_first_person() {
        let person = person_from_body(
            r#"{"people":[{"xuid":"1","gamertag":"Example","gamerScore":"250","displayPicRaw":"https://example/pic"}]}"#,
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
