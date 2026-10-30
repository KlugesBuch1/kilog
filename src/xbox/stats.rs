#![allow(dead_code)]

use serde_json::Value;

use crate::error::Error;

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
        name: name.to_owned(),
        stat_type: StatType::parse(raw_type),
        value: json_stat_value(entry.get("value")),
    })
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
    use super::{StatType, parse_user_stats};

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
    fn absent_stats_are_an_empty_list() {
        assert!(parse_user_stats("").unwrap().is_empty());
        assert!(parse_user_stats("{}").unwrap().is_empty());
        assert!(parse_user_stats(r#"{"groups":null}"#).unwrap().is_empty());
        assert!(parse_user_stats(r#"{"groups":[]}"#).unwrap().is_empty());
    }
}
