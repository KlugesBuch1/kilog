use serde::Deserialize;

use crate::error::Error;

const ACHIEVEMENTS_URL: &str =
    "https://achievements.xboxlive.com/users/xuid({xuid})/achievements?titleId={title_id}&maxItems=1000";
const ZERO_REQUIREMENT: &str = "00000000000000000000000000000000";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnlockStyle {
    Title,
    Event,
}

impl UnlockStyle {
    pub fn label(self) -> &'static str {
        match self {
            Self::Title => "Title based",
            Self::Event => "Event based",
        }
    }
}

pub async fn fetch_unlock_style(
    authorization: &str,
    xuid: &str,
    title_id: u64,
) -> Result<UnlockStyle, Error> {
    if xuid.trim().is_empty() {
        return Err(Error::Xbox("xuid is empty".into()));
    }
    if authorization.trim().is_empty() {
        return Err(Error::Xbox("authorization is empty".into()));
    }
    let url = ACHIEVEMENTS_URL
        .replace("{xuid}", xuid.trim())
        .replace("{title_id}", &title_id.to_string());
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?
        .get(url)
        .header("Authorization", authorization)
        .header("x-xbl-contract-version", "4")
        .header("Accept", "application/json")
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        let snippet: String = body.chars().take(180).collect();
        return Err(Error::Xbox(format!(
            "achievements returned {}: {snippet}",
            status.as_u16()
        )));
    }
    unlock_style(&body)
}

fn unlock_style(body: &str) -> Result<UnlockStyle, Error> {
    let body: AchievementsBody = serde_json::from_str(body)?;
    let event = body.achievements.iter().any(|achievement| {
        achievement.progression.as_ref().is_some_and(|progression| {
            progression.requirements.iter().any(|requirement| {
                requirement
                    .id
                    .as_deref()
                    .is_some_and(|id| !id.trim().is_empty() && !is_zero_requirement(id))
            })
        })
    });
    Ok(if event {
        UnlockStyle::Event
    } else {
        UnlockStyle::Title
    })
}

fn is_zero_requirement(id: &str) -> bool {
    let compact: String = id.chars().filter(|ch| *ch != '-').collect();
    compact.eq_ignore_ascii_case(ZERO_REQUIREMENT)
}

#[derive(Deserialize)]
struct AchievementsBody {
    #[serde(default)]
    achievements: Vec<Achievement>,
}

#[derive(Deserialize)]
struct Achievement {
    progression: Option<Progression>,
}

#[derive(Deserialize)]
struct Progression {
    #[serde(default)]
    requirements: Vec<Requirement>,
}

#[derive(Deserialize)]
struct Requirement {
    id: Option<String>,
}
