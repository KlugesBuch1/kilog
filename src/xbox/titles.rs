use serde::Deserialize;

use crate::error::Error;

const TITLE_HISTORY_URL: &str = "https://titlehub.xboxlive.com/users/xuid({xuid})/titles/titleHistory/decoration/Achievement?maxItems=10000";

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TitlesList {
    pub xuid: Option<String>,
    #[serde(default)]
    pub titles: Vec<Title>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Title {
    pub title_id: Option<String>,
    pub pfn: Option<String>,
    pub bing_id: Option<String>,
    pub service_config_id: Option<String>,
    pub windows_phone_product_id: Option<String>,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub devices: Vec<String>,
    pub display_image: Option<String>,
    pub media_item_type: Option<String>,
    pub modern_title_id: Option<String>,
    pub is_bundle: bool,
    pub achievement: Option<AchievementProgress>,
    pub title_history: Option<TitleHistory>,
    pub detail: Option<TitleDetail>,
    pub xbox_live_tier: Option<String>,
}

impl Default for Title {
    fn default() -> Self {
        Self {
            title_id: None,
            pfn: None,
            bing_id: None,
            service_config_id: None,
            windows_phone_product_id: None,
            name: None,
            kind: None,
            devices: Vec::new(),
            display_image: None,
            media_item_type: None,
            modern_title_id: None,
            is_bundle: false,
            achievement: None,
            title_history: None,
            detail: None,
            xbox_live_tier: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AchievementProgress {
    pub current_achievements: f64,
    pub total_achievements: f64,
    pub current_gamerscore: f64,
    pub total_gamerscore: f64,
    pub progress_percentage: f64,
    pub source_version: f64,
}

impl Default for AchievementProgress {
    fn default() -> Self {
        Self {
            current_achievements: 0.0,
            total_achievements: 0.0,
            current_gamerscore: 0.0,
            total_gamerscore: 0.0,
            progress_percentage: 0.0,
            source_version: 0.0,
        }
    }
}

impl AchievementProgress {
    pub fn current_achievements_whole(&self) -> i64 {
        self.current_achievements.round() as i64
    }

    pub fn gamerscore_label(&self) -> String {
        format!(
            "{}/{}",
            self.current_gamerscore.round() as i64,
            self.total_gamerscore.round() as i64
        )
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct TitleHistory {
    pub last_time_played: Option<String>,
    pub visible: bool,
    pub can_hide: bool,
}

impl Default for TitleHistory {
    fn default() -> Self {
        Self {
            last_time_played: None,
            visible: false,
            can_hide: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct TitleDetail {
    pub scid: Option<String>,
    pub genres: Vec<String>,
    pub developer_name: Option<String>,
    pub publisher_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GameFilter {
    #[default]
    All,
    Completed,
    Incomplete,
}

impl GameFilter {
    pub const ALL: [Self; 3] = [Self::All, Self::Completed, Self::Incomplete];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Completed => "Completed",
            Self::Incomplete => "Incomplete Games",
        }
    }
}

impl Title {
    pub fn cover_url(&self) -> Option<String> {
        let image = self
            .display_image
            .as_deref()
            .filter(|image| !image.is_empty())?;
        if image.contains("store-images.s-microsoft.com") {
            Some(format!("{image}?w=256&h=256&format=jpg"))
        } else {
            Some(image.to_owned())
        }
    }

    pub fn matches(&self, filter: GameFilter, search_lower: &str) -> bool {
        if !search_lower.is_empty() {
            let name = self.name.as_deref().unwrap_or("").to_lowercase();
            if !name.contains(search_lower) {
                return false;
            }
        }
        match filter {
            GameFilter::All => true,
            GameFilter::Completed => self
                .achievement
                .as_ref()
                .is_some_and(|achievement| achievement.progress_percentage >= 100.0),
            GameFilter::Incomplete => self
                .achievement
                .as_ref()
                .is_some_and(|achievement| achievement.progress_percentage < 100.0),
        }
    }
}

pub fn title_history_url(xuid: &str) -> String {
    TITLE_HISTORY_URL.replace("{xuid}", xuid)
}

pub fn accept_language(force_region: bool) -> String {
    if force_region {
        "en-GB".to_owned()
    } else {
        system_locale()
    }
}

fn system_locale() -> String {
    use windows::Win32::Globalization::GetUserDefaultLocaleName;
    let mut buffer = [0u16; 85];
    let written = unsafe { GetUserDefaultLocaleName(&mut buffer) };
    if written > 1 {
        let locale = String::from_utf16_lossy(&buffer[..written as usize - 1]);
        if locale.is_empty() {
            "en-US".to_owned()
        } else {
            locale
        }
    } else {
        "en-US".to_owned()
    }
}

pub fn parse_titles(body: &str) -> Result<TitlesList, Error> {
    Ok(serde_json::from_str(body)?)
}

pub async fn fetch_title_history(
    authorization: &str,
    xuid: &str,
    accept_language: &str,
) -> Result<TitlesList, Error> {
    if xuid.trim().is_empty() {
        return Err(Error::Xbox("xuid is empty".into()));
    }

    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(100))
        .build()?
        .get(title_history_url(xuid))
        .header("Authorization", authorization)
        .header("x-xbl-contract-version", "2")
        .header("Accept", "application/json")
        .header("Accept-Language", accept_language)
        .send()
        .await?;

    let status = response.status();
    let body = response.text().await?;
    // #region agent log
    crate::debug_agent::log(
        "S",
        "xbox/titles.rs:fetch_title_history",
        "title history body received",
        serde_json::json!({
            "status": status.as_u16(),
            "bytes": body.len(),
        }),
    );
    // #endregion
    if !status.is_success() {
        let snippet: String = body.chars().take(300).collect();
        return Err(Error::Xbox(format!("title history {status}: {snippet}")));
    }
    let list = parse_titles(&body)?;
    tracing::info!(count = list.titles.len(), "title history loaded");
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_title_history_payload() {
        let list = parse_titles(
            r#"{
                "xuid": "2535428600000000",
                "titles": [{
                    "titleId": "1738320482",
                    "pfn": "Example.Game_8wekyb3d8bbwe",
                    "serviceConfigId": "2a2a2a2a-2a2a-2a2a-2a2a-2a2a2a2a2a2a",
                    "name": "Example Game",
                    "type": "Game",
                    "devices": ["XboxOne", "Win32"],
                    "displayImage": "https://store-images.s-microsoft.com/image/example",
                    "achievement": {
                        "currentAchievements": 4,
                        "totalAchievements": 10,
                        "currentGamerscore": 40,
                        "totalGamerscore": 100,
                        "progressPercentage": 40,
                        "sourceVersion": 1
                    },
                    "titleHistory": {
                        "lastTimePlayed": "2024-05-01T12:00:00.0000000Z",
                        "visible": true,
                        "canHide": true
                    },
                    "detail": { "scid": "2a2a2a2a-2a2a-2a2a-2a2a-2a2a2a2a2a2a" }
                }]
            }"#,
        )
        .unwrap();

        assert_eq!(list.xuid.as_deref(), Some("2535428600000000"));
        let title = &list.titles[0];
        assert_eq!(title.title_id.as_deref(), Some("1738320482"));
        assert_eq!(title.pfn.as_deref(), Some("Example.Game_8wekyb3d8bbwe"));
        assert_eq!(title.name.as_deref(), Some("Example Game"));
        assert_eq!(title.devices, ["XboxOne", "Win32"]);
        assert_eq!(
            title.cover_url().as_deref(),
            Some("https://store-images.s-microsoft.com/image/example?w=256&h=256&format=jpg")
        );
        let achievement = title.achievement.as_ref().unwrap();
        assert_eq!(achievement.current_achievements_whole(), 4);
        assert_eq!(achievement.gamerscore_label(), "40/100");
        assert_eq!(achievement.progress_percentage, 40.0);
        assert_eq!(
            title
                .detail
                .as_ref()
                .and_then(|detail| detail.scid.as_deref()),
            Some("2a2a2a2a-2a2a-2a2a-2a2a-2a2a2a2a2a2a")
        );
    }

    #[test]
    fn filters_match_the_games_page() {
        let series = title("Halo", &["XboxSeries"], 100.0);
        let pc = title("Age of Empires", &["PC"], 10.0);
        let classic = title("Fable", &["Xbox360"], 50.0);
        let win32 = title("Solitaire", &["Win32"], 0.0);
        let titles = [series, pc, classic, win32];

        let names = |filter, search| {
            titles
                .iter()
                .filter(|title| title.matches(filter, search))
                .map(|title| title.name.as_deref().unwrap())
                .collect::<Vec<_>>()
        };

        assert_eq!(
            names(GameFilter::All, ""),
            ["Halo", "Age of Empires", "Fable", "Solitaire"]
        );
        assert_eq!(names(GameFilter::Completed, "hal"), ["Halo"]);
        assert_eq!(
            names(GameFilter::Incomplete, ""),
            ["Age of Empires", "Fable", "Solitaire"]
        );
    }

    #[test]
    fn region_override_uses_en_gb() {
        assert_eq!(accept_language(true), "en-GB");
        assert!(!accept_language(false).is_empty());
    }

    #[test]
    fn title_history_url_matches_the_csharp_constant() {
        assert_eq!(
            title_history_url("9"),
            "https://titlehub.xboxlive.com/users/xuid(9)/titles/titleHistory/decoration/Achievement?maxItems=10000"
        );
    }

    #[test]
    fn empty_xuid_is_rejected() {
        let error = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(fetch_title_history("XBL3.0 x=uhs;token", "  ", "en-US"))
            .unwrap_err();
        assert_eq!(error.to_string(), "xuid is empty");
    }

    fn title(name: &str, devices: &[&str], progress: f64) -> Title {
        Title {
            name: Some(name.to_owned()),
            devices: devices.iter().map(|device| (*device).to_owned()).collect(),
            achievement: Some(AchievementProgress {
                progress_percentage: progress,
                ..AchievementProgress::default()
            }),
            ..Title::default()
        }
    }
}
