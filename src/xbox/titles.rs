use std::collections::{HashMap, HashSet};

use serde::Deserialize;

use crate::error::Error;

const TITLE_HISTORY_URL: &str = "https://titlehub.xboxlive.com/users/xuid({xuid})/titles/titleHistory/decoration/Achievement?maxItems=10000";
const USER_TITLE_URL: &str = "https://titlehub.xboxlive.com/users/xuid({xuid})/titles/titleid({title_id})/decoration/achievement,image,detail,titleHistory";

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
    #[serde(default, deserialize_with = "optional_string_or_number")]
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
    #[serde(default, deserialize_with = "optional_whole_minutes")]
    pub minutes_played: Option<u64>,
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
            minutes_played: None,
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
    #[serde(default, deserialize_with = "optional_whole_minutes")]
    pub minutes_played: Option<u64>,
    pub visible: bool,
    pub can_hide: bool,
}

impl Default for TitleHistory {
    fn default() -> Self {
        Self {
            last_time_played: None,
            minutes_played: None,
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
    pub fn passes_filter(&self, filter: GameFilter) -> bool {
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
        self.passes_filter(filter)
    }
}

pub fn title_history_url(xuid: &str) -> String {
    TITLE_HISTORY_URL.replace("{xuid}", xuid)
}

pub fn user_title_url(xuid: &str, title_id: u64) -> String {
    USER_TITLE_URL
        .replace("{xuid}", xuid)
        .replace("{title_id}", &title_id.to_string())
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
    crate::debug_agent::log(
        "S",
        "xbox/titles.rs:fetch_title_history",
        "title history body received",
        serde_json::json!({
            "status": status.as_u16(),
            "bytes": body.len(),
        }),
    );
    if !status.is_success() {
        let snippet: String = body.chars().take(300).collect();
        return Err(Error::Xbox(format!("title history {status}: {snippet}")));
    }
    let list = parse_titles(&body)?;
    tracing::info!(count = list.titles.len(), "title history loaded");
    Ok(list)
}

pub async fn fetch_user_title(
    authorization: &str,
    xuid: &str,
    title_id: u64,
    accept_language: &str,
) -> Result<Option<Title>, Error> {
    if xuid.trim().is_empty() {
        return Err(Error::Xbox("xuid is empty".into()));
    }
    if authorization.trim().is_empty() {
        return Err(Error::Xbox("authorization is empty".into()));
    }

    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?
        .get(user_title_url(xuid, title_id))
        .header("Authorization", authorization)
        .header("x-xbl-contract-version", "2")
        .header("Accept", "application/json")
        .header("Accept-Language", accept_language)
        .send()
        .await?;
    let status = response.status();
    if status.as_u16() == 404 {
        return Ok(None);
    }
    let body = response.text().await?;
    if !status.is_success() {
        let snippet: String = body.chars().take(300).collect();
        return Err(Error::Xbox(format!("title lookup {status}: {snippet}")));
    }
    let mut titles = parse_titles(&body)?.titles;
    if titles.is_empty() {
        return Ok(None);
    }
    let index = titles
        .iter()
        .position(|title| title.title_id.as_deref().and_then(parse_title_id) == Some(title_id));
    Ok(Some(match index {
        Some(index) => titles.swap_remove(index),
        None => titles.remove(0),
    }))
}

const TITLE_DETAIL_URL: &str =
    "https://titlehub.xboxlive.com/titles/titleid({title_id})/decoration/detail";
const TITLE_BATCH_URL: &str = "https://titlehub.xboxlive.com/titles/batch/decoration/detail";
const DECIMAL_TITLE_ID_DIGITS: usize = 7;

#[derive(Debug, Clone, PartialEq)]
pub struct TitleLookup {
    pub titles: Vec<Title>,
    pub missing: Vec<u64>,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LocalSearch {
    pub hits: Vec<usize>,
    pub missing_ids: Vec<u64>,
    pub is_id_query: bool,
}

#[cfg(test)]
#[derive(Debug)]
pub struct TitleIndex {
    entries: Vec<SearchEntry>,
    by_id: HashMap<u64, Vec<usize>>,
    by_pfn: HashMap<String, Vec<usize>>,
}

#[cfg(test)]
#[derive(Debug)]
struct SearchEntry {
    name_lower: String,
    pfn_lower: String,
    id_text: String,
}

#[cfg(test)]
impl TitleIndex {
    pub fn build(titles: &[Title]) -> Self {
        let mut entries = Vec::with_capacity(titles.len());
        let mut by_id: HashMap<u64, Vec<usize>> = HashMap::with_capacity(titles.len());
        let mut by_pfn: HashMap<String, Vec<usize>> = HashMap::new();
        for (index, title) in titles.iter().enumerate() {
            let name_lower = title.name.as_deref().unwrap_or("").to_lowercase();
            let pfn_lower = title.pfn.as_deref().unwrap_or("").to_lowercase();
            let parsed = title.title_id.as_deref().and_then(parse_title_id);
            let id_text = parsed.map(|id| id.to_string()).unwrap_or_default();
            if let Some(id) = parsed {
                by_id.entry(id).or_default().push(index);
            }
            if !pfn_lower.is_empty() {
                by_pfn.entry(pfn_lower.clone()).or_default().push(index);
            }
            entries.push(SearchEntry {
                name_lower,
                pfn_lower,
                id_text,
            });
        }
        Self {
            entries,
            by_id,
            by_pfn,
        }
    }

    pub fn contains_id(&self, id: u64) -> bool {
        self.by_id.contains_key(&id)
    }

    pub fn search(&self, titles: &[Title], query: &str, filter: GameFilter) -> LocalSearch {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return LocalSearch {
                hits: passing(titles, filter),
                missing_ids: Vec::new(),
                is_id_query: false,
            };
        }
        if let Some(ids) = title_ids_in_query(trimmed) {
            return self.search_ids(titles, &ids, filter);
        }

        let needle = trimmed.to_lowercase();
        if let Some(indexes) = self.by_pfn.get(&needle) {
            return LocalSearch {
                hits: indexes
                    .iter()
                    .copied()
                    .filter(|index| {
                        titles
                            .get(*index)
                            .is_some_and(|title| title.passes_filter(filter))
                    })
                    .collect(),
                missing_ids: Vec::new(),
                is_id_query: false,
            };
        }

        LocalSearch {
            hits: self
                .entries
                .iter()
                .enumerate()
                .filter(|(index, entry)| {
                    titles
                        .get(*index)
                        .is_some_and(|title| title.passes_filter(filter))
                        && entry.matches_keyword(&needle)
                })
                .map(|(index, _)| index)
                .collect(),
            missing_ids: Vec::new(),
            is_id_query: false,
        }
    }

    fn search_ids(&self, titles: &[Title], ids: &[u64], filter: GameFilter) -> LocalSearch {
        let mut hits = Vec::new();
        let mut missing_ids = Vec::new();
        for id in ids {
            match self.by_id.get(id) {
                Some(indexes) => {
                    hits.extend(indexes.iter().copied().filter(|index| {
                        titles
                            .get(*index)
                            .is_some_and(|title| title.passes_filter(filter))
                    }));
                }
                None => missing_ids.push(*id),
            }
        }
        LocalSearch {
            hits,
            missing_ids,
            is_id_query: true,
        }
    }
}

#[cfg(test)]
impl SearchEntry {
    fn matches_keyword(&self, needle: &str) -> bool {
        field_hit(&self.name_lower, &self.pfn_lower, &self.id_text, needle)
    }
}

pub fn title_detail_url(title_id: u64) -> String {
    TITLE_DETAIL_URL.replace("{title_id}", &title_id.to_string())
}

#[cfg(test)]
pub fn title_id_key(ids: &[u64]) -> String {
    let mut key = String::new();
    for (index, id) in ids.iter().enumerate() {
        if index > 0 {
            key.push(',');
        }
        key.push_str(&id.to_string());
    }
    key
}

pub fn parse_title_id(raw: &str) -> Option<u64> {
    let token = raw.trim();
    if token.is_empty() || !token.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    token.parse().ok()
}

pub fn title_ids_in_query(query: &str) -> Option<Vec<u64>> {
    let tokens: Vec<&str> = query
        .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .filter(|token| !token.is_empty())
        .collect();
    if tokens.is_empty() || !tokens.iter().copied().all(is_exact_title_token) {
        return None;
    }
    let mut ids = Vec::with_capacity(tokens.len());
    let mut seen = HashSet::with_capacity(tokens.len());
    for token in tokens {
        let id = parse_title_id(token)?;
        if seen.insert(id) {
            ids.push(id);
        }
    }
    Some(ids)
}

pub async fn lookup_titles(
    authorization: &str,
    title_ids: &[u64],
    accept_language: &str,
) -> Result<TitleLookup, Error> {
    if authorization.trim().is_empty() {
        return Err(Error::Xbox("authorization is empty".into()));
    }
    if title_ids.is_empty() {
        return Ok(TitleLookup {
            titles: Vec::new(),
            missing: Vec::new(),
        });
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?;
    if title_ids.len() == 1 {
        return lookup_get(&client, authorization, title_ids[0], accept_language).await;
    }
    match lookup_batch(&client, authorization, title_ids, accept_language).await? {
        Some(lookup) => Ok(lookup),
        None => lookup_each(&client, authorization, title_ids, accept_language).await,
    }
}

pub const CATALOG_PAGE_SIZE: usize = 8;

const CATALOG_SEARCH_URL: &str = "https://dbox.tools/api/title_ids/";

#[derive(Debug, Clone, PartialEq)]
pub struct CatalogPage {
    pub titles: Vec<Title>,
    pub total: usize,
}

pub fn single_title_id(query: &str) -> Option<u64> {
    let query = query.trim();
    if query.is_empty() || query.contains(|c: char| c.is_whitespace() || c == ',' || c == ';') {
        return None;
    }
    let mut ids = title_ids_in_query(query)?;
    if ids.len() == 1 { ids.pop() } else { None }
}

pub async fn search_catalog(name: &str, offset: usize) -> Result<CatalogPage, Error> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Xbox("search text is empty".into()));
    }
    let limit = CATALOG_PAGE_SIZE.to_string();
    let offset_text = offset.to_string();
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?
        .get(CATALOG_SEARCH_URL)
        .header("Accept", "application/json")
        .query(&[
            ("name", name),
            ("limit", limit.as_str()),
            ("offset", offset_text.as_str()),
        ])
        .send()
        .await?;
    let status = response.status().as_u16();
    let body = response.text().await?;
    if !(200..300).contains(&status) {
        let snippet: String = body.chars().take(180).collect();
        return Err(Error::Xbox(format!("title search {status}: {snippet}")));
    }
    parse_catalog_page(&body)
}

pub async fn search_catalog_with_art(
    name: &str,
    offset: usize,
    authorization: Option<&str>,
    accept_language: &str,
) -> Result<CatalogPage, Error> {
    let mut page = search_catalog(name, offset).await?;
    if let Some(authorization) = authorization
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        attach_title_art(&mut page, authorization, accept_language).await;
    }
    Ok(page)
}

async fn attach_title_art(page: &mut CatalogPage, authorization: &str, accept_language: &str) {
    let ids: Vec<u64> = page
        .titles
        .iter()
        .filter_map(|title| title.title_id.as_deref()?.parse().ok())
        .collect();
    if ids.is_empty() {
        return;
    }
    let Ok(lookup) = lookup_titles(authorization, &ids, accept_language).await else {
        return;
    };
    let mut art = HashMap::new();
    for title in lookup.titles {
        if let Some(id) = title
            .title_id
            .as_deref()
            .and_then(|id| id.parse::<u64>().ok())
        {
            art.insert(id, title);
        }
    }
    for title in &mut page.titles {
        let Some(id) = title
            .title_id
            .as_deref()
            .and_then(|id| id.parse::<u64>().ok())
        else {
            continue;
        };
        let Some(live) = art.remove(&id) else {
            continue;
        };
        if title.display_image.is_none() {
            title.display_image = live.display_image;
        }
        if title.pfn.is_none() {
            title.pfn = live.pfn;
        }
        if title.service_config_id.is_none() {
            title.service_config_id = live.service_config_id;
        }
        if title.achievement.is_none() {
            title.achievement = live.achievement;
        }
        if title.detail.is_none() {
            title.detail = live.detail;
        }
        if title.devices.is_empty() {
            title.devices = live.devices;
        }
    }
}

fn parse_catalog_page(body: &str) -> Result<CatalogPage, Error> {
    #[derive(Deserialize)]
    struct Body {
        #[serde(default)]
        items: Vec<CatalogItem>,
        #[serde(default)]
        count: usize,
    }
    #[derive(Deserialize)]
    struct CatalogItem {
        title_id: Option<String>,
        name: Option<String>,
        #[serde(default)]
        systems: Vec<String>,
        bing_id: Option<String>,
        service_config_id: Option<String>,
        pfn: Option<String>,
    }

    let body: Body = serde_json::from_str(body)?;
    let titles = body
        .items
        .into_iter()
        .map(|item| {
            let title_id = item
                .title_id
                .as_deref()
                .and_then(parse_catalog_title_id)
                .map(|id| id.to_string());
            Title {
                title_id,
                pfn: item.pfn.filter(|value| !value.is_empty()),
                bing_id: item.bing_id.filter(|value| !value.is_empty()),
                service_config_id: item.service_config_id.filter(|value| !value.is_empty()),
                name: item.name.filter(|value| !value.is_empty()),
                devices: item
                    .systems
                    .into_iter()
                    .map(|system| catalog_system(&system))
                    .collect(),
                ..Title::default()
            }
        })
        .collect();
    Ok(CatalogPage {
        titles,
        total: body.count,
    })
}

fn parse_catalog_title_id(raw: &str) -> Option<u64> {
    let token = raw.trim();
    if token.is_empty() || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    u64::from_str_radix(token, 16).ok()
}

fn catalog_system(raw: &str) -> String {
    match raw.to_ascii_uppercase().as_str() {
        "XBOXONE" => "Xbox One".to_owned(),
        "XBOX360" | "XBOX_360" => "Xbox 360".to_owned(),
        "XBOXSERIES" | "XBOXSERIESX" | "XBOX_SERIES" => "Xbox Series".to_owned(),
        "PC" | "WIN32" => "PC".to_owned(),
        _ => raw.to_owned(),
    }
}

fn is_exact_title_token(token: &str) -> bool {
    let token = token.trim();
    token.len() >= DECIMAL_TITLE_ID_DIGITS
        && token.bytes().all(|byte| byte.is_ascii_digit())
        && parse_title_id(token).is_some()
}

#[cfg(test)]
fn field_hit(name_lower: &str, pfn_lower: &str, id_text: &str, needle: &str) -> bool {
    if name_lower.contains(needle) || (!pfn_lower.is_empty() && pfn_lower.contains(needle)) {
        return true;
    }
    needle.bytes().any(|byte| byte.is_ascii_digit())
        && !id_text.is_empty()
        && id_text.contains(needle)
}

#[cfg(test)]
fn passing(titles: &[Title], filter: GameFilter) -> Vec<usize> {
    titles
        .iter()
        .enumerate()
        .filter(|(_, title)| title.passes_filter(filter))
        .map(|(index, _)| index)
        .collect()
}

fn optional_whole_minutes<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct Visitor;

    impl serde::de::Visitor<'_> for Visitor {
        type Value = Option<u64>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("minutes played")
        }

        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
            Ok(Some(value))
        }

        fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
            if value < 0 {
                Ok(None)
            } else {
                Ok(Some(value as u64))
            }
        }

        fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Self::Value, E> {
            if value.is_finite() && value >= 0.0 {
                Ok(Some(value.round() as u64))
            } else {
                Ok(None)
            }
        }

        fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
            Ok(value.trim().parse().ok())
        }
    }

    deserializer.deserialize_any(Visitor)
}

fn optional_string_or_number<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct Visitor;

    impl serde::de::Visitor<'_> for Visitor {
        type Value = Option<String>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string, number, or null")
        }

        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
            if value.is_empty() {
                Ok(None)
            } else {
                Ok(Some(value.to_owned()))
            }
        }

        fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Self::Value, E> {
            if value.is_empty() {
                Ok(None)
            } else {
                Ok(Some(value))
            }
        }

        fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
            Ok(Some(value.to_string()))
        }

        fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
            if value < 0 {
                Err(E::custom("title id is negative"))
            } else {
                Ok(Some(value.to_string()))
            }
        }
    }

    deserializer.deserialize_any(Visitor)
}

fn apply_title_headers(
    builder: reqwest::RequestBuilder,
    authorization: &str,
    accept_language: &str,
) -> reqwest::RequestBuilder {
    builder
        .header("Authorization", authorization)
        .header("x-xbl-contract-version", "2")
        .header("Accept", "application/json")
        .header("Accept-Language", accept_language)
}

async fn lookup_get(
    client: &reqwest::Client,
    authorization: &str,
    title_id: u64,
    accept_language: &str,
) -> Result<TitleLookup, Error> {
    let response = apply_title_headers(
        client.get(title_detail_url(title_id)),
        authorization,
        accept_language,
    )
    .send()
    .await?;
    let status = response.status().as_u16();
    let body = response.text().await?;
    lookup_from_response(status, &body, &[title_id])
}

async fn lookup_batch(
    client: &reqwest::Client,
    authorization: &str,
    title_ids: &[u64],
    accept_language: &str,
) -> Result<Option<TitleLookup>, Error> {
    #[derive(serde::Serialize)]
    struct Body<'a> {
        #[serde(rename = "titleIds")]
        title_ids: &'a [String],
        pfns: Option<&'a [&'a str]>,
    }

    let title_ids_text: Vec<String> = title_ids.iter().map(u64::to_string).collect();
    let response =
        apply_title_headers(client.post(TITLE_BATCH_URL), authorization, accept_language)
            .json(&Body {
                title_ids: &title_ids_text,
                pfns: None,
            })
            .send()
            .await?;
    let status = response.status().as_u16();
    if matches!(status, 400 | 404 | 405 | 501) {
        return Ok(None);
    }
    let body = response.text().await?;
    lookup_from_response(status, &body, title_ids).map(Some)
}

async fn lookup_each(
    client: &reqwest::Client,
    authorization: &str,
    title_ids: &[u64],
    accept_language: &str,
) -> Result<TitleLookup, Error> {
    let mut pending = tokio::task::JoinSet::new();
    for title_id in title_ids {
        let client = client.clone();
        let authorization = authorization.to_owned();
        let accept_language = accept_language.to_owned();
        let title_id = *title_id;
        pending.spawn(async move {
            lookup_get(&client, &authorization, title_id, &accept_language).await
        });
    }

    let mut titles = Vec::new();
    while let Some(joined) = pending.join_next().await {
        let lookup = joined.map_err(|err| Error::Xbox(err.to_string()))??;
        titles.extend(lookup.titles);
    }
    Ok(reconcile_lookup(title_ids, titles))
}

fn lookup_from_response(status: u16, body: &str, requested: &[u64]) -> Result<TitleLookup, Error> {
    if status == 404 || status == 204 {
        return Ok(TitleLookup {
            titles: Vec::new(),
            missing: requested.to_vec(),
        });
    }
    if !(200..300).contains(&status) {
        let snippet: String = body.chars().take(180).collect();
        return Err(Error::Xbox(format!("title lookup {status}: {snippet}")));
    }
    if body.trim().is_empty() {
        return Ok(TitleLookup {
            titles: Vec::new(),
            missing: requested.to_vec(),
        });
    }
    let list = parse_lookup_body(body)?;
    Ok(reconcile_lookup(requested, list.titles))
}

fn parse_lookup_body(body: &str) -> Result<TitlesList, Error> {
    let value: serde_json::Value = serde_json::from_str(body)?;
    if value.get("titles").is_some() {
        return Ok(serde_json::from_value(value)?);
    }
    let title: Title = serde_json::from_value(value)?;
    if title.title_id.is_none() && title.name.is_none() {
        return Ok(TitlesList {
            xuid: None,
            titles: Vec::new(),
        });
    }
    Ok(TitlesList {
        xuid: None,
        titles: vec![title],
    })
}

fn reconcile_lookup(requested: &[u64], titles: Vec<Title>) -> TitleLookup {
    let mut grouped: HashMap<u64, Vec<Title>> = HashMap::new();
    let mut extras = Vec::new();
    for title in titles {
        match title.title_id.as_deref().and_then(parse_title_id) {
            Some(id) => grouped.entry(id).or_default().push(title),
            None => extras.push(title),
        }
    }

    let mut ordered = Vec::new();
    let mut missing = Vec::new();
    let mut seen = HashSet::new();
    for id in requested {
        if !seen.insert(*id) {
            continue;
        }
        match grouped.remove(id) {
            Some(found) => ordered.extend(found),
            None => missing.push(*id),
        }
    }
    ordered.extend(extras);
    for found in grouped.into_values() {
        ordered.extend(found);
    }
    TitleLookup {
        titles: ordered,
        missing,
    }
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
    fn user_title_url_targets_one_title() {
        assert_eq!(
            user_title_url("9", 1738320482),
            "https://titlehub.xboxlive.com/users/xuid(9)/titles/titleid(1738320482)/decoration/achievement,image,detail,titleHistory"
        );
    }

    #[test]
    fn minutes_played_accepts_numbers_on_the_title_and_history() {
        let list = parse_titles(
            r#"{"titles":[{"titleId":"9","minutesPlayed":125.4,"titleHistory":{"lastTimePlayed":"2024-05-01T12:00:00Z","minutesPlayed":"90"}}]}"#,
        )
        .unwrap();
        let title = &list.titles[0];
        assert_eq!(title.minutes_played, Some(125));
        assert_eq!(
            title
                .title_history
                .as_ref()
                .and_then(|history| history.minutes_played),
            Some(90)
        );
    }

    #[test]
    fn user_title_rejects_an_empty_xuid() {
        let error = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(fetch_user_title("XBL3.0 x=uhs;token", "  ", 9, "en-US"))
            .unwrap_err();
        assert_eq!(error.to_string(), "xuid is empty");
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

    #[test]
    fn title_ids_accept_decimal() {
        assert_eq!(parse_title_id("305419896"), Some(305419896));
        assert_eq!(parse_title_id("0x12345678"), None);
        assert_eq!(parse_title_id("67A4C6E2"), None);
        assert_eq!(parse_title_id("Fable"), None);
        assert_eq!(
            title_ids_in_query("305419896, 1738320482"),
            Some(vec![305419896, 1738320482])
        );
        assert_eq!(title_ids_in_query("halo"), None);
        assert_eq!(title_ids_in_query("2"), None);
        assert_eq!(title_ids_in_query("123456"), None);
        assert_eq!(
            title_detail_url(305419896),
            "https://titlehub.xboxlive.com/titles/titleid(305419896)/decoration/detail"
        );
        assert_eq!(title_id_key(&[16, 305419896]), "16,305419896");
    }

    #[test]
    fn index_matches_id_name_and_pfn_without_rescanning_case() {
        let halo = titled(
            "Halo Infinite",
            "1738320482",
            "Halo.Infinite_8wekyb3d8bbwe",
            100.0,
        );
        let hex = titled("Hex Game", "305419896", "Hex.Game_8wekyb3d8bbwe", 10.0);
        let age = titled(
            "Age of Empires",
            "1234567890",
            "Age.Game_8wekyb3d8bbwe",
            40.0,
        );
        let titles = [halo, hex, age];
        let index = TitleIndex::build(&titles);

        let names = |query, filter| {
            index
                .search(&titles, query, filter)
                .hits
                .into_iter()
                .map(|index| titles[index].name.as_deref().unwrap())
                .collect::<Vec<_>>()
        };

        assert_eq!(names("HALO", GameFilter::All), ["Halo Infinite"]);
        assert_eq!(
            names("8wekyb3d8bbwe", GameFilter::Incomplete),
            ["Hex Game", "Age of Empires"]
        );
        assert_eq!(
            names("hex.game_8wekyb3d8bbwe", GameFilter::All),
            ["Hex Game"]
        );
        assert_eq!(names("305419896", GameFilter::All), ["Hex Game"]);
        assert_eq!(
            names("305419896", GameFilter::Completed),
            Vec::<&str>::new()
        );
        assert!(index.contains_id(305419896));

        let missed = index.search(&titles, "10000001, 10000002", GameFilter::All);
        assert!(missed.hits.is_empty());
        assert_eq!(missed.missing_ids, [10_000_001, 10_000_002]);
        assert!(missed.is_id_query);

        let partial = index.search(&titles, "123456", GameFilter::All);
        assert!(!partial.is_id_query);
        assert_eq!(
            partial
                .hits
                .into_iter()
                .map(|index| titles[index].name.as_deref().unwrap())
                .collect::<Vec<_>>(),
            ["Age of Empires"]
        );
    }

    #[test]
    fn lookup_responses_keep_missing_titles_quiet() {
        let missing = lookup_from_response(404, "not found", &[305419896]).unwrap();
        assert!(missing.titles.is_empty());
        assert_eq!(missing.missing, [305419896]);

        let empty = lookup_from_response(200, r#"{"titles":[]}"#, &[1]).unwrap();
        assert_eq!(empty.missing, [1]);

        let error = lookup_from_response(503, "unavailable", &[1]).unwrap_err();
        assert!(error.to_string().contains("503"));

        let numeric = lookup_from_response(
            200,
            r#"{"titles":[{"titleId":305419896,"name":"Hex Game","pfn":"Hex.Game_8wekyb3d8bbwe"}]}"#,
            &[16, 305419896],
        )
        .unwrap();
        assert_eq!(numeric.missing, [16]);
        assert_eq!(numeric.titles[0].name.as_deref(), Some("Hex Game"));
        assert_eq!(numeric.titles[0].title_id.as_deref(), Some("305419896"));

        let single = lookup_from_response(200, r#"{"titleId":"42","name":"Solo"}"#, &[42]).unwrap();
        assert!(single.missing.is_empty());
        assert_eq!(single.titles[0].name.as_deref(), Some("Solo"));

        let ignored = lookup_from_response(200, r#"{"error":"nope"}"#, &[7]).unwrap();
        assert_eq!(ignored.missing, [7]);
    }

    #[test]
    fn catalog_page_uses_hex_title_ids_and_the_total_count() {
        let page = parse_catalog_page(
            r#"{
                "count": 249,
                "items": [
                    {
                        "title_id": "053D8785",
                        "name": "LEGO Marvel's Avengers",
                        "systems": ["XBOXONE"],
                        "pfn": null,
                        "service_config_id": null
                    },
                    {
                        "title_id": "00000010",
                        "name": "Example Movie",
                        "systems": ["XBOX360", "PC"],
                        "pfn": "Example.Movie_8wekyb3d8bbwe"
                    }
                ]
            }"#,
        )
        .unwrap();
        assert_eq!(page.total, 249);
        assert_eq!(page.titles.len(), 2);
        assert_eq!(page.titles[0].title_id.as_deref(), Some("87918469"));
        assert_eq!(page.titles[0].devices, ["Xbox One"]);
        assert_eq!(page.titles[1].title_id.as_deref(), Some("16"));
        assert_eq!(page.titles[1].devices, ["Xbox 360", "PC"]);
        assert_eq!(
            page.titles[1].pfn.as_deref(),
            Some("Example.Movie_8wekyb3d8bbwe")
        );
        assert_eq!(single_title_id("lego"), None);
        assert_eq!(single_title_id("0x053D8785"), None);
        assert_eq!(single_title_id("87918469"), Some(87_918_469));
    }

    #[test]
    fn empty_authorization_is_rejected() {
        let error = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(lookup_titles("  ", &[1], "en-US"))
            .unwrap_err();
        assert_eq!(error.to_string(), "authorization is empty");
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

    fn titled(name: &str, title_id: &str, pfn: &str, progress: f64) -> Title {
        Title {
            title_id: Some(title_id.to_owned()),
            pfn: Some(pfn.to_owned()),
            ..title(name, &["XboxOne"], progress)
        }
    }
}
