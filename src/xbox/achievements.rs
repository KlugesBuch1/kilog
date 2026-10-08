use std::collections::HashSet;

use serde::Deserialize;

use crate::error::Error;

const UNLOCK_STYLE_URL: &str = "https://achievements.xboxlive.com/users/xuid({xuid})/achievements?titleId={title_id}&maxItems=1000";
const ACHIEVEMENTS_URL: &str = "https://achievements.xboxlive.com/users/xuid({xuid})/achievements";
const ZERO_REQUIREMENT: &str = "00000000000000000000000000000000";
const MAX_PAGES: usize = 25;

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
    require_identity(authorization, xuid)?;
    let url = UNLOCK_STYLE_URL
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
    let body: StyleBody = serde_json::from_str(body)?;
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

fn require_identity(authorization: &str, xuid: &str) -> Result<(), Error> {
    if xuid.trim().is_empty() {
        return Err(Error::Xbox("xuid is empty".into()));
    }
    if authorization.trim().is_empty() {
        return Err(Error::Xbox("authorization is empty".into()));
    }
    Ok(())
}

#[derive(Deserialize)]
struct StyleBody {
    #[serde(default)]
    achievements: Vec<StyleAchievement>,
}

#[derive(Deserialize)]
struct StyleAchievement {
    progression: Option<StyleProgression>,
}

#[derive(Deserialize)]
struct StyleProgression {
    #[serde(default)]
    requirements: Vec<StyleRequirement>,
}

#[derive(Deserialize)]
struct StyleRequirement {
    id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressState {
    Unlocked,
    Locked,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rarity {
    pub category: Option<String>,
    pub percentage: Option<f64>,
}

impl Rarity {
    pub fn label(&self) -> Option<String> {
        let category = self
            .category
            .as_deref()
            .map(str::trim)
            .filter(|category| !category.is_empty());
        match (category, self.percentage) {
            (Some(category), Some(percentage)) => Some(format!("{category} · {percentage:.1}%")),
            (Some(category), None) => Some(category.to_owned()),
            (None, Some(percentage)) => Some(format!("{percentage:.1}%")),
            (None, None) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Achievement {
    pub id: String,
    pub name: String,
    pub description: String,
    pub locked_description: String,
    pub gamerscore: u32,
    pub progress_state: ProgressState,
    pub in_progress: bool,
    pub is_secret: bool,
    pub rarity: Option<Rarity>,
    pub unlocked_at: Option<String>,
    pub icon_url: Option<String>,
}

impl Achievement {
    pub fn is_unlocked(&self) -> bool {
        self.progress_state == ProgressState::Unlocked
    }

    pub fn status_label(&self) -> &'static str {
        if self.is_unlocked() {
            "Unlocked"
        } else if self.in_progress {
            "In progress"
        } else {
            "Locked"
        }
    }

    pub fn shown_description(&self) -> &str {
        if self.is_unlocked() {
            return self.description.as_str();
        }
        if !self.locked_description.is_empty() {
            return self.locked_description.as_str();
        }
        if !self.description.is_empty() {
            return self.description.as_str();
        }
        if self.is_secret {
            "Hidden until unlocked."
        } else {
            ""
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TitleAchievements {
    pub title_name: Option<String>,
    pub service_config_id: Option<String>,
    pub achievements: Vec<Achievement>,
}

impl TitleAchievements {
    pub fn total_count(&self) -> u32 {
        u32::try_from(self.achievements.len()).unwrap_or(u32::MAX)
    }

    pub fn unlocked_count(&self) -> u32 {
        u32::try_from(
            self.achievements
                .iter()
                .filter(|achievement| achievement.is_unlocked())
                .count(),
        )
        .unwrap_or(u32::MAX)
    }

    pub fn total_gamerscore(&self) -> u32 {
        self.achievements.iter().fold(0, |total, achievement| {
            total.saturating_add(achievement.gamerscore)
        })
    }

    pub fn unlocked_gamerscore(&self) -> u32 {
        self.achievements
            .iter()
            .filter(|achievement| achievement.is_unlocked())
            .fold(0, |total, achievement| {
                total.saturating_add(achievement.gamerscore)
            })
    }

    pub fn gamerscore_label(&self) -> String {
        format!(
            "{} / {}",
            self.unlocked_gamerscore(),
            self.total_gamerscore()
        )
    }

    pub fn unlocked_label(&self) -> String {
        format!(
            "{} / {} Unlocked",
            self.unlocked_count(),
            self.total_count()
        )
    }

    pub fn progress_fraction(&self) -> f32 {
        let total = self.total_gamerscore();
        if total > 0 {
            return (self.unlocked_gamerscore() as f32 / total as f32).clamp(0.0, 1.0);
        }
        let count = self.total_count();
        if count == 0 {
            0.0
        } else {
            (self.unlocked_count() as f32 / count as f32).clamp(0.0, 1.0)
        }
    }
}

pub async fn fetch_title_achievements(
    authorization: &str,
    xuid: &str,
    title_id: u64,
    accept_language: &str,
) -> Result<TitleAchievements, Error> {
    require_identity(authorization, xuid)?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?;
    let mut achievements = Vec::new();
    let mut seen = HashSet::new();
    let mut title_name = None;
    let mut service_config_id = None;
    let mut continuation = None;
    for _ in 0..MAX_PAGES {
        let page = fetch_page(
            &client,
            authorization,
            xuid,
            title_id,
            accept_language,
            continuation.as_deref(),
        )
        .await?;
        if title_name.is_none() {
            title_name = page.title_name;
        }
        if service_config_id.is_none() {
            service_config_id = page.service_config_id;
        }
        if page.achievements.is_empty() {
            break;
        }
        for achievement in page.achievements {
            let fresh = achievement.id.is_empty() || seen.insert(achievement.id.clone());
            if fresh {
                achievements.push(achievement);
            }
        }
        let next = page.continuation;
        if next.is_some() && next != continuation {
            continuation = next;
        } else {
            break;
        }
    }
    Ok(TitleAchievements {
        title_name,
        service_config_id,
        achievements,
    })
}

const UPDATE_URL: &str =
    "https://achievements.xboxlive.com/users/xuid({xuid})/achievements/{scid}/update";
const PROGRESS_USER_AGENT: &str = "XboxServicesAPI/2021.10.20211005.0 c";
const PROGRESS_SIGNATURE: &str = "RGFtbklHb3R0YU1ha2VUaGlzU3RyaW5nU3VwZXJMb25nSHVoLkRvbnRFdmVuS25vd1doYXRTaG91bGRCZUhlcmVEcmFmZlN0cmluZw==";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressStatus {
    Updated,
    Unauthorized,
    Forbidden,
    Rejected(String),
}

pub async fn update_achievement_progress(
    authorization: &str,
    xuid: &str,
    title_id: u64,
    service_config_id: &str,
    achievement_id: &str,
    percent_complete: u32,
) -> Result<ProgressStatus, Error> {
    require_identity(authorization, xuid)?;
    let service_config_id = service_config_id.trim();
    if service_config_id.is_empty() {
        return Err(Error::Xbox("service config id is empty".into()));
    }
    let achievement_id = achievement_id.trim();
    if achievement_id.is_empty() {
        return Err(Error::Xbox("achievement id is empty".into()));
    }
    let percent_complete = percent_complete.min(100);
    let url = UPDATE_URL
        .replace("{xuid}", xuid.trim())
        .replace("{scid}", service_config_id);
    let body = progress_payload(
        service_config_id,
        title_id,
        xuid.trim(),
        achievement_id,
        percent_complete,
    );
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?
        .post(url)
        .header("Authorization", authorization)
        .header("x-xbl-contract-version", "2")
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("User-Agent", PROGRESS_USER_AGENT)
        .header("Signature", PROGRESS_SIGNATURE)
        .body(body.to_string())
        .send()
        .await?;
    let status = response.status();
    let text = response.text().await?;
    Ok(progress_status(status.as_u16(), &text))
}

fn progress_payload(
    service_config_id: &str,
    title_id: u64,
    xuid: &str,
    achievement_id: &str,
    percent_complete: u32,
) -> serde_json::Value {
    serde_json::json!({
        "action": "progressUpdate",
        "serviceConfigId": service_config_id,
        "titleId": title_id.to_string(),
        "userId": xuid,
        "achievements": [{
            "id": achievement_id,
            "percentComplete": percent_complete.to_string(),
        }]
    })
}

fn progress_status(status: u16, body: &str) -> ProgressStatus {
    match status {
        200 | 204 => ProgressStatus::Updated,
        401 => ProgressStatus::Unauthorized,
        403 => ProgressStatus::Forbidden,
        _ => {
            let snippet: String = body.chars().take(180).collect();
            ProgressStatus::Rejected(format!("achievements returned {status}: {snippet}"))
        }
    }
}

async fn fetch_page(
    client: &reqwest::Client,
    authorization: &str,
    xuid: &str,
    title_id: u64,
    accept_language: &str,
    continuation: Option<&str>,
) -> Result<ParsedPage, Error> {
    let url = ACHIEVEMENTS_URL.replace("{xuid}", xuid.trim());
    let mut query = vec![
        ("titleId", title_id.to_string()),
        ("maxItems", "1000".to_owned()),
    ];
    if let Some(token) = continuation {
        query.push(("continuationToken", token.to_owned()));
    }
    let response = client
        .get(url)
        .header("Authorization", authorization)
        .header("x-xbl-contract-version", "2")
        .header("Accept", "application/json")
        .header("Accept-Language", accept_language)
        .query(&query)
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
    parse_page(&body)
}

struct ParsedPage {
    achievements: Vec<Achievement>,
    title_name: Option<String>,
    service_config_id: Option<String>,
    continuation: Option<String>,
}

fn parse_page(body: &str) -> Result<ParsedPage, Error> {
    let body: PageBody = serde_json::from_str(body)?;
    let mut title_name = None;
    let mut service_config_id = None;
    let mut achievements = Vec::with_capacity(body.achievements.len());
    for raw in body.achievements {
        if title_name.is_none() {
            title_name = association_name(&raw);
        }
        if service_config_id.is_none() {
            service_config_id = present_id(raw.service_config_id.as_deref());
        }
        achievements.push(Achievement::from_raw(raw));
    }
    let continuation = body.paging_info.and_then(|paging| {
        let token = paging.continuation_token?;
        let token = token.trim();
        if token.is_empty() {
            None
        } else {
            Some(token.to_owned())
        }
    });
    Ok(ParsedPage {
        achievements,
        title_name,
        service_config_id,
        continuation,
    })
}

fn present_id(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

impl Achievement {
    fn from_raw(raw: RawAchievement) -> Self {
        let (progress_state, in_progress) =
            progress_from(raw.progress_state.as_deref(), raw.unlocked);
        let unlocked_at = meaningful_time(
            raw.progression
                .as_ref()
                .and_then(|progression| progression.time_unlocked.clone())
                .or_else(|| raw.time_unlocked.clone()),
        );
        let name = display_name(&raw);
        let gamerscore = gamerscore_of(&raw);
        let icon_url = icon_url(&raw.media_assets);
        Self {
            id: raw.id.trim().to_owned(),
            name,
            description: cleaned(raw.description),
            locked_description: cleaned(raw.locked_description),
            gamerscore,
            progress_state,
            in_progress,
            is_secret: raw.is_secret,
            rarity: rarity_of(raw.rarity),
            unlocked_at,
            icon_url,
        }
    }
}

fn display_name(raw: &RawAchievement) -> String {
    let name = raw.name.as_deref().unwrap_or("").trim();
    if !name.is_empty() {
        return name.to_owned();
    }
    if raw.is_secret {
        "Secret achievement".to_owned()
    } else if raw.id.trim().is_empty() {
        "Achievement".to_owned()
    } else {
        format!("Achievement {}", raw.id.trim())
    }
}

fn cleaned(value: Option<String>) -> String {
    value.unwrap_or_default().trim().to_owned()
}

fn gamerscore_of(raw: &RawAchievement) -> u32 {
    let mut found = false;
    let mut total = 0u32;
    for reward in &raw.rewards {
        let gamerscore = reward
            .kind
            .as_deref()
            .is_some_and(|kind| kind.eq_ignore_ascii_case("gamerscore"));
        if gamerscore {
            found = true;
            total = total.saturating_add(reward.value.unwrap_or(0));
        }
    }
    if found {
        total
    } else {
        raw.gamerscore.unwrap_or(0)
    }
}

fn progress_from(state: Option<&str>, unlocked: Option<bool>) -> (ProgressState, bool) {
    let state = state.map(str::trim).filter(|state| !state.is_empty());
    match state {
        Some(state) if equals_any(state, &["Achieved", "Unlocked"]) => {
            (ProgressState::Unlocked, false)
        }
        Some(state) if equals_any(state, &["InProgress", "In Progress"]) => {
            (ProgressState::Locked, true)
        }
        Some(_) => (ProgressState::Locked, false),
        None if unlocked == Some(true) => (ProgressState::Unlocked, false),
        None => (ProgressState::Locked, false),
    }
}

fn equals_any(value: &str, options: &[&str]) -> bool {
    options
        .iter()
        .any(|option| value.eq_ignore_ascii_case(option))
}

fn meaningful_time(value: Option<String>) -> Option<String> {
    let value = value?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let year = trimmed.get(..4).and_then(|year| year.parse::<i32>().ok());
    if year.is_some_and(|year| year < 1970) {
        return None;
    }
    Some(trimmed.to_owned())
}

fn rarity_of(raw: Option<RawRarity>) -> Option<Rarity> {
    let raw = raw?;
    let category = raw.current_category.and_then(|category| {
        let category = category.trim();
        if category.is_empty() {
            None
        } else {
            Some(category.to_owned())
        }
    });
    if category.is_none() && raw.current_percentage.is_none() {
        None
    } else {
        Some(Rarity {
            category,
            percentage: raw.current_percentage,
        })
    }
}

fn icon_url(assets: &[RawMedia]) -> Option<String> {
    let icon = assets.iter().find(|asset| {
        asset
            .kind
            .as_deref()
            .is_some_and(|kind| kind.eq_ignore_ascii_case("icon"))
    });
    let url = icon.or_else(|| assets.first())?.url.as_deref()?.trim();
    if url.is_empty() {
        return None;
    }
    if let Some(rest) = url.strip_prefix("//") {
        return Some(format!("https://{rest}"));
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        Some(url.to_owned())
    } else {
        None
    }
}

fn association_name(raw: &RawAchievement) -> Option<String> {
    raw.title_associations.iter().find_map(|title| {
        let name = title.name.as_deref()?.trim();
        if name.is_empty() {
            None
        } else {
            Some(name.to_owned())
        }
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageBody {
    #[serde(default, deserialize_with = "empty_on_null")]
    achievements: Vec<RawAchievement>,
    #[serde(default)]
    paging_info: Option<PagingInfo>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PagingInfo {
    #[serde(default)]
    continuation_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAchievement {
    #[serde(default, deserialize_with = "id_string")]
    id: String,
    #[serde(default)]
    service_config_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    locked_description: Option<String>,
    #[serde(default)]
    progress_state: Option<String>,
    #[serde(default)]
    unlocked: Option<bool>,
    #[serde(default)]
    is_secret: bool,
    #[serde(default, deserialize_with = "optional_u32")]
    gamerscore: Option<u32>,
    #[serde(default)]
    progression: Option<RawProgression>,
    #[serde(default, deserialize_with = "empty_on_null")]
    rewards: Vec<RawReward>,
    #[serde(default)]
    rarity: Option<RawRarity>,
    #[serde(default, deserialize_with = "empty_on_null")]
    media_assets: Vec<RawMedia>,
    #[serde(default, deserialize_with = "empty_on_null")]
    title_associations: Vec<RawTitle>,
    #[serde(default)]
    time_unlocked: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawProgression {
    #[serde(default)]
    time_unlocked: Option<String>,
}

#[derive(Deserialize)]
struct RawReward {
    #[serde(default, deserialize_with = "optional_u32")]
    value: Option<u32>,
    #[serde(default, rename = "type")]
    kind: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRarity {
    #[serde(default)]
    current_category: Option<String>,
    #[serde(default, deserialize_with = "optional_f64")]
    current_percentage: Option<f64>,
}

#[derive(Deserialize)]
struct RawMedia {
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

#[derive(Deserialize)]
struct RawTitle {
    #[serde(default)]
    name: Option<String>,
}

fn empty_on_null<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

fn id_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct Visitor;

    impl serde::de::Visitor<'_> for Visitor {
        type Value = String;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or number")
        }

        fn visit_unit<E: serde::de::Error>(self) -> Result<String, E> {
            Ok(String::new())
        }

        fn visit_none<E: serde::de::Error>(self) -> Result<String, E> {
            Ok(String::new())
        }

        fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<String, E> {
            Ok(value.to_owned())
        }

        fn visit_string<E: serde::de::Error>(self, value: String) -> Result<String, E> {
            Ok(value)
        }

        fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<String, E> {
            Ok(value.to_string())
        }

        fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<String, E> {
            Ok(value.to_string())
        }

        fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<String, E> {
            if value.fract() == 0.0 && value.is_finite() {
                Ok((value as i64).to_string())
            } else {
                Ok(value.to_string())
            }
        }
    }

    deserializer.deserialize_any(Visitor)
}

fn optional_u32<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.as_ref().and_then(value_to_u32))
}

fn optional_f64<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.as_ref().and_then(value_to_f64))
}

fn value_to_u32(value: &serde_json::Value) -> Option<u32> {
    match value {
        serde_json::Value::Number(number) => number
            .as_u64()
            .and_then(|number| u32::try_from(number).ok())
            .or_else(|| number.as_f64().map(round_u32)),
        serde_json::Value::String(text) => {
            let text = text.trim();
            text.parse::<u32>()
                .ok()
                .or_else(|| text.parse::<f64>().ok().map(round_u32))
        }
        _ => None,
    }
}

fn round_u32(number: f64) -> u32 {
    if !number.is_finite() || number <= 0.0 {
        0
    } else {
        number.round().min(u32::MAX as f64) as u32
    }
}

fn value_to_f64(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(number) => number.as_f64().filter(|number| number.is_finite()),
        serde_json::Value::String(text) => text
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite()),
        _ => None,
    }
}
