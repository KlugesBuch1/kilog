#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::error::Error;

const LABEL_BATCH: usize = 10;
const LABEL_NAME_BUDGET: usize = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatType {
    Integer,
    Double,
    String,
    Unknown,
}

impl StatType {
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
}

pub async fn fetch_user_stats(
    authorization: &str,
    xuid: &str,
    scid: &str,
) -> Result<Vec<UserStat>, Error> {
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
    let url = format!("https://userstats.xboxlive.com/users/xuid({xuid})/scids/{scid}");
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?
        .get(url)
        .header("Authorization", authorization)
        .header("x-xbl-contract-version", "1")
        .header("Accept", "application/json")
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        let snippet: String = body.chars().take(180).collect();
        return Err(Error::Xbox(format!(
            "user stats returned {}: {snippet}",
            status.as_u16()
        )));
    }
    parse_user_stats(&body)
}

pub async fn attach_stat_labels(
    authorization: &str,
    xuid: &str,
    scid: &str,
    stats: &mut [UserStat],
) -> Result<(), Error> {
    if stats.is_empty() {
        return Ok(());
    }
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
    let names = unique_stat_names(stats);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?;
    let mut labels = HashMap::new();
    for batch in label_batches(&names) {
        let body = fetch_label_batch(&client, authorization, xuid, scid, &batch).await?;
        labels.extend(parse_stat_labels(&body)?);
    }
    apply_stat_labels(stats, &labels);
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatWriteStatus {
    Updated,
    Unauthorized,
    Forbidden,
    Rejected(String),
}

/// Writes one stat on this service config with the signed-in user's XBL3.0 token.
///
/// Integer and Double values are sent as numbers. String values are sent as text.
/// After [`StatWriteStatus::Updated`], refetch the list to read the stored value.
/// When the stat metadata says the stat is a sum, the service adds the posted
/// number to the stored value instead of replacing it.
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
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?
        .patch(url)
        .header("Authorization", authorization)
        .header("x-xbl-contract-version", "4")
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await?;
    let status = response.status();
    let text = response.text().await?;
    Ok(stat_write_status(status.as_u16(), &text))
}

fn stat_write_body(name: &str, stat_type: StatType, value: &str) -> Result<String, Error> {
    let value = match stat_type {
        StatType::Integer => Value::Number(integer_stat_value(value)?),
        StatType::Double => Value::Number(double_stat_value(value)?),
        StatType::String => Value::String(value.to_owned()),
        StatType::Unknown => return Err(Error::Xbox("stat type is unknown".into())),
    };
    let mut title = serde_json::Map::new();
    title.insert(name.to_owned(), serde_json::json!({ "value": value }));
    Ok(serde_json::json!({ "stats": { "title": title } }).to_string())
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

async fn fetch_label_batch(
    client: &reqwest::Client,
    authorization: &str,
    xuid: &str,
    scid: &str,
    names: &[&str],
) -> Result<String, Error> {
    let joined = names
        .iter()
        .copied()
        .map(encode_stat_name)
        .collect::<Vec<_>>()
        .join(",");
    let url = format!(
        "https://userstats.xboxlive.com/users/xuid({xuid})/scids/{scid}/stats/{joined}"
    );
    let response = client
        .get(url)
        .header("Authorization", authorization)
        .header("x-xbl-contract-version", "3")
        .header("Accept", "application/json")
        .query(&[("include", "valuemetadata")])
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        let snippet: String = body.chars().take(180).collect();
        return Err(Error::Xbox(format!(
            "stat labels returned {}: {snippet}",
            status.as_u16()
        )));
    }
    Ok(body)
}

fn parse_user_stats(body: &str) -> Result<Vec<UserStat>, Error> {
    let body = body.trim();
    if body.is_empty() {
        return Ok(Vec::new());
    }
    let value: Value = serde_json::from_str(body)?;
    let Some(groups) = value
        .get("groups")
        .or_else(|| value.get("Groups"))
        .filter(|groups| !groups.is_null())
    else {
        return Ok(Vec::new());
    };
    let groups = groups
        .as_array()
        .ok_or_else(|| Error::Xbox("user stats groups are not a list".into()))?;
    let mut stats = Vec::new();
    for group in groups {
        let Some(group) = group.as_object() else {
            continue;
        };
        let group_name = group
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_owned();
        let Some(lists) = group
            .get("statlistscollection")
            .or_else(|| group.get("statListsCollection"))
            .and_then(Value::as_array)
        else {
            continue;
        };
        for list in lists {
            let Some(entries) = list.get("stats").and_then(Value::as_array) else {
                continue;
            };
            for entry in entries {
                if let Some(stat) = stat_from_entry(&group_name, entry) {
                    stats.push(stat);
                }
            }
        }
    }
    Ok(stats)
}

fn stat_from_entry(group: &str, entry: &Value) -> Option<UserStat> {
    let name = entry
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())?;
    let raw_type = entry
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("");
    Some(UserStat {
        group: group.to_owned(),
        label: name.to_owned(),
        name: name.to_owned(),
        stat_type: StatType::parse(raw_type),
        value: json_stat_value(entry.get("value")),
    })
}

fn unique_stat_names(stats: &[UserStat]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut names = Vec::new();
    for stat in stats {
        if stat.name.is_empty() || !seen.insert(stat.name.clone()) {
            continue;
        }
        names.push(stat.name.clone());
    }
    names
}

fn label_batches(names: &[String]) -> Vec<Vec<&str>> {
    let mut batches = Vec::new();
    let mut current = Vec::new();
    let mut length = 0;
    for name in names {
        let encoded = encode_stat_name(name).len();
        let extra = encoded + usize::from(!current.is_empty());
        if !current.is_empty() && (current.len() == LABEL_BATCH || length + extra > LABEL_NAME_BUDGET)
        {
            batches.push(std::mem::take(&mut current));
            length = 0;
        }
        length += encoded + usize::from(!current.is_empty());
        current.push(name.as_str());
    }
    if !current.is_empty() {
        batches.push(current);
    }
    batches
}

fn encode_stat_name(name: &str) -> String {
    let mut encoded = String::new();
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn parse_stat_labels(body: &str) -> Result<HashMap<(String, String), String>, Error> {
    let body = body.trim();
    if body.is_empty() {
        return Ok(HashMap::new());
    }
    let value: Value = serde_json::from_str(body)?;
    let mut labels = HashMap::new();
    if let Some(groups) = value
        .get("groups")
        .or_else(|| value.get("Groups"))
        .filter(|groups| !groups.is_null())
        .and_then(Value::as_array)
    {
        for group in groups.iter().filter_map(Value::as_object) {
            let group_name = group
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_owned();
            collect_list_labels(
                group
                    .get("statlistscollection")
                    .or_else(|| group.get("statListsCollection")),
                &group_name,
                &mut labels,
            );
        }
    }
    collect_list_labels(
        value
            .get("statlistscollection")
            .or_else(|| value.get("statListsCollection")),
        "",
        &mut labels,
    );
    Ok(labels)
}

fn collect_list_labels(
    lists: Option<&Value>,
    group: &str,
    labels: &mut HashMap<(String, String), String>,
) {
    let Some(lists) = lists.and_then(Value::as_array) else {
        return;
    };
    for list in lists {
        let Some(entries) = list.get("stats").and_then(Value::as_array) else {
            continue;
        };
        for entry in entries {
            let Some(name) = entry
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
            else {
                continue;
            };
            let Some(label) = stat_display_name(entry) else {
                continue;
            };
            labels.insert((group.to_owned(), name.to_owned()), label);
        }
    }
}

fn stat_display_name(entry: &Value) -> Option<String> {
    let properties = entry
        .get("properties")
        .and_then(|properties| properties.get("DisplayName"))
        .and_then(Value::as_str);
    let group = entry
        .get("groupproperties")
        .and_then(|properties| properties.get("DisplayName"))
        .and_then(Value::as_str);
    let label = properties.or(group)?.trim();
    if label.is_empty() {
        None
    } else {
        Some(label.to_owned())
    }
}

fn apply_stat_labels(stats: &mut [UserStat], labels: &HashMap<(String, String), String>) {
    for stat in stats {
        let grouped = (stat.group.clone(), stat.name.clone());
        let ungrouped = (String::new(), stat.name.clone());
        if let Some(label) = labels.get(&grouped).or_else(|| labels.get(&ungrouped)) {
            stat.label = label.clone();
        }
    }
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
        let stats = parse_user_stats(body).unwrap();
        assert_eq!(stats.len(), 4);
        assert_eq!(stats[0].group, "Garage");
        assert_eq!(stats[0].name, "CarsOwned");
        assert_eq!(stats[0].label, "CarsOwned");
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
                super::fetch_user_stats("XBL3.0 x=;", " ", "scid")
                    .await
                    .unwrap_err()
                    .to_string(),
                "xuid is empty"
            );
            assert_eq!(
                super::fetch_user_stats(" ", "1", "scid")
                    .await
                    .unwrap_err()
                    .to_string(),
                "authorization is empty"
            );
            assert_eq!(
                super::fetch_user_stats("XBL3.0 x=;", "1", " ")
                    .await
                    .unwrap_err()
                    .to_string(),
                "service config id is empty"
            );
        });
    }

    #[test]
    fn hub_labels_merge_onto_stat_records() {
        let mut stats = parse_user_stats(
            r#"{
            "groups": [
                {
                    "name": "Garage",
                    "statlistscollection": [
                        {
                            "stats": [
                                {"name": "CarsOwned", "type": "Integer", "value": "12"},
                                {"name": "MoneyEarned", "type": "Double", "value": "10"},
                                {"name": "FavoriteCar", "type": "String", "value": "Sentinel"}
                            ]
                        }
                    ]
                },
                {
                    "name": "Profile",
                    "statlistscollection": [
                        {
                            "stats": [
                                {"name": "Badge", "type": "Integer", "value": "1"}
                            ]
                        }
                    ]
                }
            ]
        }"#,
        )
        .unwrap();
        let labels = super::parse_stat_labels(
            r#"{
            "groups": [
                {
                    "name": "Garage",
                    "statlistscollection": [
                        {
                            "stats": [
                                {
                                    "groupproperties": {
                                        "Ordinal": 1,
                                        "SortOrder": "Descending",
                                        "DisplayName": "Group label",
                                        "DisplayFormat": "Integer",
                                        "DisplaySemantic": "None"
                                    },
                                    "name": "CarsOwned",
                                    "type": "Integer",
                                    "value": "12",
                                    "properties": {"DisplayName": "Cars owned"}
                                },
                                {
                                    "groupproperties": {"DisplayName": "Money earned"},
                                    "name": "MoneyEarned",
                                    "type": "Double",
                                    "value": 10,
                                    "properties": {}
                                },
                                {
                                    "name": "FavoriteCar",
                                    "type": "String",
                                    "value": "Sentinel",
                                    "properties": {"DisplayName": "  "}
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
                            "value": "90",
                            "properties": {"DisplayName": "Minutes played"}
                        }
                    ]
                }
            ]
        }"#,
        )
        .unwrap();
        super::apply_stat_labels(&mut stats, &labels);
        assert_eq!(stats[0].label, "Cars owned");
        assert_eq!(stats[0].value, "12");
        assert_eq!(stats[1].label, "Money earned");
        assert_eq!(stats[2].label, "FavoriteCar");
        assert_eq!(stats[3].label, "Badge");
        assert!(stats.iter().all(|stat| stat.name != "MinutesPlayed"));
    }

    #[test]
    fn label_requests_stay_within_the_documented_batch() {
        let names: Vec<String> = (0..11).map(|index| format!("Stat{index}")).collect();
        let batches = super::label_batches(&names);
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].len(), 10);
        assert_eq!(batches[1], vec!["Stat10"]);

        let long = "N".repeat(601);
        let names = [long];
        let batches = super::label_batches(&names);
        assert_eq!(batches, vec![vec![names[0].as_str()]]);
    }

    #[test]
    fn stat_writes_send_numbers_or_text() {
        let integer = stat_write_body("CarsOwned", StatType::Integer, " 12 ").unwrap();
        let integer: serde_json::Value = serde_json::from_str(&integer).unwrap();
        assert_eq!(
            integer["stats"]["title"]["CarsOwned"]["value"],
            serde_json::json!(12)
        );

        let double = stat_write_body("MoneyEarned", StatType::Double, "1500.5").unwrap();
        let double: serde_json::Value = serde_json::from_str(&double).unwrap();
        assert_eq!(
            double["stats"]["title"]["MoneyEarned"]["value"],
            serde_json::json!(1500.5)
        );

        let text = stat_write_body("FavoriteCar", StatType::String, "Sentinel").unwrap();
        let text: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            text["stats"]["title"]["FavoriteCar"]["value"],
            serde_json::json!("Sentinel")
        );

        assert!(stat_write_body("CarsOwned", StatType::Integer, "12.5").is_err());
        assert!(stat_write_body("MoneyEarned", StatType::Double, "nope").is_err());
        assert!(stat_write_body("Badge", StatType::Unknown, "1").is_err());
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
        assert!(parse_user_stats("").unwrap().is_empty());
        assert!(parse_user_stats("{}").unwrap().is_empty());
        assert!(parse_user_stats(r#"{"groups":null}"#).unwrap().is_empty());
        assert!(parse_user_stats(r#"{"groups":[]}"#).unwrap().is_empty());
    }
}
