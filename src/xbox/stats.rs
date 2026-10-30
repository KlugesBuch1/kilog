use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::error::Error;

const STATS_BATCH_URL: &str = "https://userstats.xboxlive.com/batch";
const STATS_SCHEMA: &str = "http://stats.xboxlive.com/2017-1/schema#";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatType {
    Integer,
    Double,
    String,
    Unknown,
}

impl StatType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Integer => "Integer",
            Self::Double => "Double",
            Self::String => "String",
            Self::Unknown => "Unknown",
        }
    }

    fn parse(raw: &str) -> Self {
        if raw.eq_ignore_ascii_case("Integer") {
            Self::Integer
        } else if raw.eq_ignore_ascii_case("Double") {
            Self::Double
        } else if raw.eq_ignore_ascii_case("String") {
            Self::String
        } else {
            Self::Unknown
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserStat {
    pub group: String,
    pub name: String,
    pub label: String,
    pub stat_type: StatType,
    pub value: String,
    pub scid: String,
}

pub async fn fetch_user_stats(
    authorization: &str,
    xuid: &str,
    title_id: &str,
    scid: &str,
) -> Result<Vec<UserStat>, Error> {
    let xuid = xuid.trim();
    if xuid.is_empty() {
        return Err(Error::Xbox("xuid is empty".into()));
    }
    if authorization.trim().is_empty() {
        return Err(Error::Xbox("authorization is empty".into()));
    }
    let title_id = title_id.trim();
    let scid = scid.trim();
    if title_id.is_empty() && scid.is_empty() {
        return Err(Error::Xbox("service config id is empty".into()));
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let mut hero_error = None;
    let mut hero_stats = None;
    if !title_id.is_empty() {
        match post_stats_batch(&client, authorization, "2", &hero_body(xuid, title_id)).await {
            Ok(body) => {
                let stats = parse_user_stats(&body, scid)?;
                if !stats.is_empty() || scid.is_empty() {
                    return Ok(stats);
                }
                hero_stats = Some(stats);
            }
            Err(err) => hero_error = Some(err),
        }
    }
    if !scid.is_empty() {
        match post_stats_batch(&client, authorization, "1", &scid_body(xuid, scid)).await {
            Ok(body) => return Ok(parse_user_stats(&body, scid)?),
            Err(err) => {
                if let Some(stats) = hero_stats {
                    return Ok(stats);
                }
                return Err(hero_error.unwrap_or(err));
            }
        }
    }
    Err(hero_error.unwrap_or_else(|| Error::Xbox("service config id is empty".into())))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatWriteStatus {
    Updated,
    Unauthorized,
    Forbidden,
    Rejected(String),
}

pub async fn write_user_stat(
    authorization: &str,
    xuid: &str,
    scid: &str,
    name: &str,
    stat_type: StatType,
    value: &str,
) -> Result<StatWriteStatus, Error> {
    let xuid = xuid.trim();
    if xuid.is_empty() {
        return Err(Error::Xbox("xuid is empty".into()));
    }
    if authorization.trim().is_empty() {
        return Err(Error::Xbox("authorization is empty".into()));
    }
    let scid = scid.trim();
    if scid.is_empty() {
        return Err(Error::Xbox("service config id is empty".into()));
    }
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Xbox("stat name is empty".into()));
    }
    let body = stat_write_body(name, stat_type, value)?;
    let url = format!("https://statswrite.xboxlive.com/stats/users/{xuid}/scids/{scid}");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let mut last = StatWriteStatus::Rejected("stat write failed".into());
    for attempt in 0..3 {
        let response = client
            .patch(&url)
            .header("Authorization", authorization)
            .header("x-xbl-contract-version", "4")
            .header("Accept", "application/json")
            .header("Accept-Language", "en-US")
            .header("Content-Type", "application/json; charset=utf-8")
            .body(body.clone())
            .send()
            .await;
        match response {
            Ok(response) => {
                let status = response.status();
                let text = response.text().await?;
                let parsed = stat_write_status(status.as_u16(), &text);
                if matches!(parsed, StatWriteStatus::Updated) {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    return Ok(parsed);
                }
                if matches!(
                    parsed,
                    StatWriteStatus::Unauthorized | StatWriteStatus::Forbidden
                ) {
                    return Ok(parsed);
                }
                last = parsed;
            }
            Err(err) => last = StatWriteStatus::Rejected(err.to_string()),
        }
        if attempt < 2 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }
    Ok(last)
}

fn hero_body(xuid: &str, title_id: &str) -> String {
    serde_json::json!({
        "arrangebyfield": "xuid",
        "xuids": [xuid],
        "groups": [{ "name": "Hero", "titleId": title_id }]
    })
    .to_string()
}

fn scid_body(xuid: &str, scid: &str) -> String {
    serde_json::json!({
        "requestedusers": [xuid],
        "requestedscids": [{ "scid": scid, "requestedstats": [] }]
    })
    .to_string()
}

async fn post_stats_batch(
    client: &reqwest::Client,
    authorization: &str,
    contract: &str,
    body: &str,
) -> Result<String, Error> {
    let response = client
        .post(STATS_BATCH_URL)
        .header("Authorization", authorization)
        .header("x-xbl-contract-version", contract)
        .header("Accept", "application/json")
        .header("Accept-Language", "en-US")
        .header("Content-Type", "application/json; charset=utf-8")
        .body(body.to_owned())
        .send()
        .await?;
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        let snippet: String = text.chars().take(180).collect();
        return Err(Error::Xbox(format!(
            "user stats returned {}: {snippet}",
            status.as_u16()
        )));
    }
    Ok(text)
}

fn stat_write_body(name: &str, stat_type: StatType, value: &str) -> Result<String, Error> {
    let value = stat_write_value(stat_type, value)?;
    let (revision, timestamp) = xbox_clock();
    Ok(stat_write_document(name, &value, revision, &timestamp))
}

fn stat_write_value(stat_type: StatType, value: &str) -> Result<String, Error> {
    let value = value.trim();
    if value.is_empty() {
        return Err(Error::Xbox("stat value is empty".into()));
    }
    match stat_type {
        StatType::Integer => {
            integer_stat_value(value)?;
            Ok(value.to_owned())
        }
        StatType::Double => {
            double_stat_value(value)?;
            Ok(value.to_owned())
        }
        StatType::String | StatType::Unknown => Ok(value.to_owned()),
    }
}

fn stat_write_document(name: &str, value: &str, revision: u64, timestamp: &str) -> String {
    serde_json::json!({
        "$schema": STATS_SCHEMA,
        "previousRevision": 0,
        "revision": revision,
        "timestamp": timestamp,
        "stats": {
            "title": {
                name: { "value": value }
            }
        }
    })
    .to_string()
}

fn xbox_clock() -> (u64, String) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    (secs, format_unix(secs, now.subsec_micros()))
}

fn format_unix(secs: u64, micros: u32) -> String {
    let days = (secs / 86_400) as i64;
    let tod = secs % 86_400;
    let hour = tod / 3_600;
    let minute = (tod % 3_600) / 60;
    let second = tod % 60;
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{micros:06}Z")
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    (year as i32, month as u32, day as u32)
}

fn integer_stat_value(value: &str) -> Result<serde_json::Number, Error> {
    let value = value.trim();
    let digits = value.strip_prefix('-').unwrap_or(value);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Error::Xbox("stat value is not an integer".into()));
    }
    serde_json::from_str(value).map_err(|_| Error::Xbox("stat value is not an integer".into()))
}

fn double_stat_value(value: &str) -> Result<serde_json::Number, Error> {
    let value = value.trim();
    let number = serde_json::from_str::<serde_json::Number>(value)
        .map_err(|_| Error::Xbox("stat value is not a number".into()))?;
    if number.as_f64().is_some_and(|number| !number.is_finite()) {
        return Err(Error::Xbox("stat value is not a number".into()));
    }
    Ok(number)
}

fn stat_write_status(status: u16, body: &str) -> StatWriteStatus {
    match status {
        200 | 204 => StatWriteStatus::Updated,
        401 => StatWriteStatus::Unauthorized,
        403 => StatWriteStatus::Forbidden,
        _ => {
            let snippet: String = body.chars().take(180).collect();
            StatWriteStatus::Rejected(format!("stat write returned {status}: {snippet}"))
        }
    }
}

fn parse_user_stats(body: &str, fallback_scid: &str) -> Result<Vec<UserStat>, Error> {
    let body = body.trim();
    if body.is_empty() {
        return Ok(Vec::new());
    }
    let value: Value = serde_json::from_str(body)?;
    let mut stats = Vec::new();
    if let Some(groups) = array_field(&value, &["groups", "Groups"]) {
        for group in groups {
            let group_name = text_field(Some(group), &["name"]).unwrap_or_default();
            let Some(lists) = array_field(
                group,
                &[
                    "statlistscollection",
                    "statListsCollection",
                    "statlistcollection",
                ],
            ) else {
                continue;
            };
            for list in lists {
                collect_stats(list, &group_name, fallback_scid, &mut stats);
            }
        }
    }
    if stats.is_empty()
        && let Some(lists) = array_field(&value, &["statlistscollection", "statListsCollection"])
    {
        for list in lists {
            collect_stats(list, "", fallback_scid, &mut stats);
        }
    }
    if stats.is_empty()
        && let Some(users) = array_field(&value, &["users"])
    {
        for user in users {
            let Some(scids) = array_field(user, &["scids"]) else {
                continue;
            };
            for scid in scids {
                collect_stats(scid, "", fallback_scid, &mut stats);
            }
        }
    }
    if stats.is_empty() {
        collect_stats(&value, "", fallback_scid, &mut stats);
    }
    Ok(dedup_stats(stats))
}

fn collect_stats(collection: &Value, group: &str, fallback_scid: &str, stats: &mut Vec<UserStat>) {
    let collection_scid =
        text_field(Some(collection), &["scid"]).unwrap_or_else(|| fallback_scid.to_owned());
    let Some(entries) = array_field(collection, &["stats", "statlist"]) else {
        return;
    };
    for entry in entries {
        if let Some(stat) = stat_from_entry(group, entry, &collection_scid) {
            stats.push(stat);
        }
    }
}

fn stat_from_entry(group: &str, entry: &Value, fallback_scid: &str) -> Option<UserStat> {
    let name = text_field(Some(entry), &["name", "statname", "statName"])?;
    let raw_type = text_field(Some(entry), &["type"]).unwrap_or_default();
    let scid = text_field(Some(entry), &["scid"]).unwrap_or_else(|| fallback_scid.to_owned());
    Some(UserStat {
        group: group.to_owned(),
        label: stat_display_name(entry, &name),
        name,
        stat_type: StatType::parse(&raw_type),
        value: json_stat_value(entry.get("value")),
        scid,
    })
}

fn dedup_stats(stats: Vec<UserStat>) -> Vec<UserStat> {
    let mut seen = HashSet::new();
    stats
        .into_iter()
        .filter(|stat| !stat.name.is_empty() && seen.insert(stat.name.clone()))
        .collect()
}

fn array_field<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Vec<Value>> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_array))
}

fn text_field(value: Option<&Value>, keys: &[&str]) -> Option<String> {
    let value = value?;
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    })
}

fn stat_display_name(entry: &Value, fallback: &str) -> String {
    text_field(
        entry.get("groupproperties"),
        &["DisplayName", "displayName"],
    )
    .or_else(|| text_field(entry.get("properties"), &["DisplayName", "displayName"]))
    .unwrap_or_else(|| fallback.to_owned())
}

fn json_stat_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{StatType, StatWriteStatus, parse_user_stats, stat_write_body, stat_write_status};

    #[test]
    fn groups_become_stat_records() {
        let body = r#"{
            "groups": [
                {
                    "name": "Garage",
                    "titleid": "123456789",
                    "statlistscollection": [
                        {
                            "arrangebyfield": "xuid",
                            "arrangebyfieldid": "2533274790395904",
                            "stats": [
                                {
                                    "xuid": "2533274790395904",
                                    "scid": "c4060100-4951-4a51-a630-dfa4252c1ed3",
                                    "name": "CarsOwned",
                                    "type": "Integer",
                                    "value": "12"
                                },
                                {
                                    "name": "MoneyEarned",
                                    "type": "Double",
                                    "value": 1500.5
                                }
                            ]
                        }
                    ]
                },
                {
                    "name": "Profile",
                    "statListsCollection": [
                        {
                            "stats": [
                                {
                                    "name": "FavoriteCar",
                                    "type": "String",
                                    "value": "Sentinel"
                                },
                                {
                                    "name": "Badge",
                                    "type": "Boolean",
                                    "value": "gold"
                                },
                                {
                                    "name": "   ",
                                    "type": "Integer",
                                    "value": "1"
                                }
                            ]
                        }
                    ]
                }
            ],
            "statlistscollection": [
                {
                    "stats": [
                        {
                            "name": "MinutesPlayed",
                            "type": "Integer",
                            "value": "90"
                        }
                    ]
                }
            ]
        }"#;
        let stats = parse_user_stats(body, "fallback-scid").unwrap();
        assert_eq!(stats.len(), 4);
        assert_eq!(stats[0].group, "Garage");
        assert_eq!(stats[0].name, "CarsOwned");
        assert_eq!(stats[0].label, "CarsOwned");
        assert_eq!(stats[0].scid, "c4060100-4951-4a51-a630-dfa4252c1ed3");
        assert_eq!(stats[1].scid, "fallback-scid");
        assert_eq!(stats[0].stat_type, StatType::Integer);
        assert_eq!(stats[0].value, "12");
        assert_eq!(stats[1].name, "MoneyEarned");
        assert_eq!(stats[1].stat_type, StatType::Double);
        assert_eq!(stats[1].value, "1500.5");
        assert_eq!(stats[2].group, "Profile");
        assert_eq!(stats[2].name, "FavoriteCar");
        assert_eq!(stats[2].stat_type, StatType::String);
        assert_eq!(stats[2].value, "Sentinel");
        assert_eq!(stats[3].name, "Badge");
        assert_eq!(stats[3].stat_type, StatType::Unknown);
        assert!(stats.iter().all(|stat| stat.name != "MinutesPlayed"));
    }

    #[test]
    fn empty_xuid_auth_and_scid_are_errors() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            assert_eq!(
                super::fetch_user_stats("XBL3.0 x=;", " ", "", "scid")
                    .await
                    .unwrap_err()
                    .to_string(),
                "xuid is empty"
            );
            assert_eq!(
                super::fetch_user_stats(" ", "1", "", "scid")
                    .await
                    .unwrap_err()
                    .to_string(),
                "authorization is empty"
            );
            assert_eq!(
                super::fetch_user_stats("XBL3.0 x=;", "1", " ", " ")
                    .await
                    .unwrap_err()
                    .to_string(),
                "service config id is empty"
            );
        });
    }

    #[test]
    fn batch_shapes_keep_labels_scids_and_skip_duplicates() {
        let stats = parse_user_stats(
            r#"{
            "users": [
                {
                    "scids": [
                        {
                            "scid": "title-scid",
                            "stats": [
                                {
                                    "statname": "CarsOwned",
                                    "type": "Integer",
                                    "value": "12",
                                    "groupproperties": {"DisplayName": "Cars owned"},
                                    "properties": {"DisplayName": "Ignored"}
                                },
                                {
                                    "name": "CarsOwned",
                                    "type": "Integer",
                                    "value": "99"
                                }
                            ]
                        }
                    ]
                }
            ]
        }"#,
            "fallback-scid",
        )
        .unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].name, "CarsOwned");
        assert_eq!(stats[0].label, "Cars owned");
        assert_eq!(stats[0].value, "12");
        assert_eq!(stats[0].scid, "title-scid");
    }

    #[test]
    fn stat_writes_send_the_2017_schema() {
        let integer = stat_write_body("CarsOwned", StatType::Integer, " 12 ").unwrap();
        let integer: serde_json::Value = serde_json::from_str(&integer).unwrap();
        assert_eq!(
            integer["$schema"],
            "http://stats.xboxlive.com/2017-1/schema#"
        );
        assert_eq!(integer["previousRevision"], 0);
        assert!(integer["revision"].as_u64().is_some());
        assert!(
            integer["timestamp"]
                .as_str()
                .is_some_and(|timestamp| timestamp.contains('T') && timestamp.ends_with('Z'))
        );
        assert_eq!(
            integer["stats"]["title"]["CarsOwned"]["value"],
            serde_json::json!("12")
        );

        let double = stat_write_body("MoneyEarned", StatType::Double, "1500.5").unwrap();
        let double: serde_json::Value = serde_json::from_str(&double).unwrap();
        assert_eq!(
            double["stats"]["title"]["MoneyEarned"]["value"],
            serde_json::json!("1500.5")
        );

        let text = stat_write_body("FavoriteCar", StatType::String, "Sentinel").unwrap();
        let text: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            text["stats"]["title"]["FavoriteCar"]["value"],
            serde_json::json!("Sentinel")
        );

        let unknown = stat_write_body("Badge", StatType::Unknown, "gold").unwrap();
        let unknown: serde_json::Value = serde_json::from_str(&unknown).unwrap();
        assert_eq!(unknown["stats"]["title"]["Badge"]["value"], "gold");

        assert!(stat_write_body("CarsOwned", StatType::Integer, "12.5").is_err());
        assert!(stat_write_body("MoneyEarned", StatType::Double, "nope").is_err());
        assert!(stat_write_body("CarsOwned", StatType::Integer, " ").is_err());
    }

    #[test]
    fn xbox_timestamp_is_utc() {
        assert_eq!(super::format_unix(0, 0), "1970-01-01T00:00:00.000000Z");
        assert_eq!(
            super::format_unix(1_700_000_000, 123_456),
            "2023-11-14T22:13:20.123456Z"
        );
    }

    #[test]
    fn stat_write_status_matches_achievement_progress() {
        assert_eq!(stat_write_status(200, ""), StatWriteStatus::Updated);
        assert_eq!(stat_write_status(204, ""), StatWriteStatus::Updated);
        assert_eq!(stat_write_status(401, ""), StatWriteStatus::Unauthorized);
        assert_eq!(stat_write_status(403, "no"), StatWriteStatus::Forbidden);
        assert_eq!(
            stat_write_status(400, "bad value"),
            StatWriteStatus::Rejected("stat write returned 400: bad value".into())
        );
    }

    #[test]
    fn absent_stats_are_an_empty_list() {
        assert!(parse_user_stats("", "").unwrap().is_empty());
        assert!(parse_user_stats("{}", "").unwrap().is_empty());
        assert!(
            parse_user_stats(r#"{"groups":null}"#, "")
                .unwrap()
                .is_empty()
        );
        assert!(parse_user_stats(r#"{"groups":[]}"#, "").unwrap().is_empty());
    }
}
