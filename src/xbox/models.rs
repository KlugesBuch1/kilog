use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonResponse {
    pub xuid: Option<String>,
    pub gamertag: Option<String>,
    pub gamer_score: Option<String>,
    pub display_pic_raw: Option<String>,
    pub display_name: Option<String>,
    pub modern_gamertag: Option<String>,
    pub presence_state: Option<String>,
    pub presence_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    #[serde(default)]
    pub people: Vec<PersonResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct XboxTitle {
    pub title_id: Option<String>,
    pub name: String,
    pub service_config_id: Option<String>,
    pub display_image: Option<String>,
    pub pfn: Option<String>,
    #[serde(rename = "type")]
    pub title_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Title {
    pub title_id: Option<String>,
    pub name: Option<String>,
    pub service_config_id: Option<String>,
    pub display_image: Option<String>,
    pub modern_title_id: Option<String>,
    #[serde(default)]
    pub is_bundle: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TitlesList {
    pub xuid: Option<String>,
    #[serde(default)]
    pub titles: Vec<Title>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OneCoreAchievementResponse {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub locked_description: Option<String>,
    #[serde(default)]
    pub gamerscore: Option<i64>,
    #[serde(default)]
    pub is_secret: bool,
    pub progress_state: Option<String>,
    pub service_config_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AchievementsResponse {
    #[serde(default)]
    pub achievements: Vec<OneCoreAchievementResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Xbox360AchievementEntry {
    pub id: i32,
    pub title_id: Option<i64>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub locked_description: Option<String>,
    #[serde(default)]
    pub gamerscore: Option<i32>,
    #[serde(default)]
    pub is_secret: bool,
    pub progress_state: Option<String>,
    pub time_unlocked: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Xbox360AchievementResponse {
    #[serde(default)]
    pub achievements: Vec<Xbox360AchievementEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stat {
    pub xuid: Option<String>,
    pub scid: Option<String>,
    pub title_id: Option<String>,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub stat_type: Option<String>,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatListCollection {
    pub arrange_by_field: Option<String>,
    pub arrange_by_field_id: Option<String>,
    #[serde(default)]
    pub stats: Vec<Stat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameStatsResponse {
    #[serde(default)]
    pub stat_lists_collection: Vec<StatListCollection>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_profile_and_titles() {
        let profile: Profile = serde_json::from_str(
            r#"{"people":[{"xuid":"1","gamertag":"Example","gamerScore":"100","displayPicRaw":"https://example/pic"}]}"#,
        )
        .unwrap();
        let person = &profile.people[0];
        assert_eq!(person.xuid.as_deref(), Some("1"));
        assert_eq!(person.gamertag.as_deref(), Some("Example"));
        assert_eq!(person.gamer_score.as_deref(), Some("100"));
        assert_eq!(
            person.display_pic_raw.as_deref(),
            Some("https://example/pic")
        );

        let titles: TitlesList = serde_json::from_str(
            r#"{"xuid":"1","titles":[{"titleId":"42","name":"Game","serviceConfigId":"scid","displayImage":"img"}]}"#,
        )
        .unwrap();
        assert_eq!(titles.titles[0].service_config_id.as_deref(), Some("scid"));
    }

    #[test]
    fn parses_achievements() {
        let modern: AchievementsResponse = serde_json::from_str(
            r#"{"achievements":[{"id":"1","name":"First","description":"Do it","gamerscore":10,"isSecret":true,"progressState":"Achieved"}]}"#,
        )
        .unwrap();
        let achievement = &modern.achievements[0];
        assert_eq!(achievement.gamerscore, Some(10));
        assert!(achievement.is_secret);
        assert_eq!(achievement.progress_state.as_deref(), Some("Achieved"));

        let legacy: Xbox360AchievementResponse = serde_json::from_str(
            r#"{"achievements":[{"id":2,"titleId":99,"name":"Old","gamerscore":5,"isSecret":false}]}"#,
        )
        .unwrap();
        assert_eq!(legacy.achievements[0].title_id, Some(99));
        assert_eq!(legacy.achievements[0].progress_state, None);
    }
}
