use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::error::Error;
use crate::xbox::achievements::ProgressStatus;

const COLLECTOR_URL: &str = "https://v10.events.data.microsoft.com/OneCollector/1.0/";

struct Database {
    dir: PathBuf,
    data: String,
}

struct TitleRecord {
    fully_supported: bool,
    template: String,
    achievements: serde_json::Map<String, Value>,
}

pub(crate) struct EventPlan {
    template: String,
    batches: Vec<Vec<Replacement>>,
}

struct Replacement {
    target: String,
    replacement: String,
}

static DATABASE: OnceLock<Database> = OnceLock::new();
static TITLES: Mutex<Option<HashMap<u64, TitleRecord>>> = Mutex::new(None);
static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub(crate) struct InstalledTemplate {
    pub title_id: u64,
    pub in_catalog: bool,
}

pub(crate) fn install_template(path: &Path, bytes: &[u8]) -> Result<InstalledTemplate, Error> {
    let title_id = template_title_id(path)?;
    let bytes = strip_bom(bytes);
    validate_template(bytes)?;
    let Some(dir) = events_dir()? else {
        return Err(Error::Xbox(
            "event database was not found. Data.json needs to sit in assets/events".into(),
        ));
    };
    let dest = dir.join(format!("{title_id}.json"));
    let same = match (path.canonicalize(), dest.canonicalize()) {
        (Ok(source), Ok(destination)) => source == destination,
        _ => false,
    };
    if !same {
        std::fs::write(&dest, bytes)?;
    }
    if let Ok(mut guard) = titles()
        && let Some(cache) = guard.as_mut()
    {
        cache.remove(&title_id);
    }
    Ok(InstalledTemplate {
        title_id,
        in_catalog: title_has_events(title_id)?,
    })
}

fn events_dir() -> Result<Option<PathBuf>, Error> {
    if let Some(database) = DATABASE.get() {
        return Ok(Some(database.dir.clone()));
    }
    Ok(find_events_dir())
}

fn template_title_id(path: &Path) -> Result<u64, Error> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    let Some(stem) = name
        .strip_suffix(".json")
        .or_else(|| name.strip_suffix(".JSON"))
    else {
        return Err(Error::Xbox(format!("{name} is not a titleid.json file")));
    };
    if stem.eq_ignore_ascii_case("data") {
        return Err(Error::Xbox(
            "Data.json is the event list, not a game template".into(),
        ));
    }
    if stem.is_empty() || !stem.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Error::Xbox(format!("{name} is not a titleid.json file")));
    }
    stem.parse::<u64>()
        .map_err(|_| Error::Xbox(format!("{name} is not a titleid.json file")))
}

fn strip_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes)
}

fn validate_template(bytes: &[u8]) -> Result<(), Error> {
    if bytes.is_empty() {
        return Err(Error::Xbox("event template is empty".into()));
    }
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|err| Error::Xbox(format!("event template is not valid json: {err}")))?;
    if !value.is_object() {
        return Err(Error::Xbox("event template must be a json object".into()));
    }
    Ok(())
}

pub(crate) fn title_has_events(title_id: u64) -> Result<bool, Error> {
    let Some(database) = database()? else {
        return Ok(false);
    };
    Ok(extract_title(&database.data, title_id).is_some())
}

pub(crate) fn event_plan(title_id: u64, achievement_id: &str) -> Result<Option<EventPlan>, Error> {
    let mut guard = titles()?;
    let cache = guard.get_or_insert_with(HashMap::new);
    if !load_title_into(cache, title_id)? {
        return Ok(None);
    }
    let title = &cache[&title_id];
    let achievement_id = achievement_id.trim();
    match title.achievements.get(achievement_id) {
        Some(body) => Ok(Some(EventPlan {
            template: title.template.clone(),
            batches: batches(body)?,
        })),
        None if title.fully_supported => {
            Err(Error::Xbox("this achievement has no event data".into()))
        }
        None => Ok(None),
    }
}

pub(crate) async fn send_event_plan(
    events_token: &str,
    xuid: &str,
    plan: &EventPlan,
) -> Result<ProgressStatus, Error> {
    let events_token = events_token.trim();
    if events_token.is_empty() {
        return Err(Error::Xbox("events authorization is empty".into()));
    }
    let xuid = xuid.trim();
    if xuid.is_empty() {
        return Err(Error::Xbox("xuid is empty".into()));
    }
    if plan.batches.is_empty() {
        return Err(Error::Xbox("this achievement has no event data".into()));
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;
    let mut status = ProgressStatus::Updated;
    for batch in &plan.batches {
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let body = render_event(&plan.template, batch, xuid, seq)?;
        status = post_event(&client, events_token, body).await?;
        if status != ProgressStatus::Updated {
            return Ok(status);
        }
    }
    Ok(status)
}

fn database() -> Result<Option<&'static Database>, Error> {
    if let Some(database) = DATABASE.get() {
        return Ok(Some(database));
    }
    let Some(dir) = find_events_dir() else {
        return Ok(None);
    };
    let data = std::fs::read_to_string(dir.join("Data.json"))?;
    Ok(Some(DATABASE.get_or_init(|| Database { dir, data })))
}

fn find_events_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/events"));
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        candidates.push(parent.join("assets/events"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("assets/events"));
    }
    candidates
        .into_iter()
        .find(|path| path.join("Data.json").is_file())
}

fn titles() -> Result<std::sync::MutexGuard<'static, Option<HashMap<u64, TitleRecord>>>, Error> {
    TITLES
        .lock()
        .map_err(|_| Error::Xbox("event database lock was poisoned".into()))
}

fn load_title_into(cache: &mut HashMap<u64, TitleRecord>, title_id: u64) -> Result<bool, Error> {
    if cache.contains_key(&title_id) {
        return Ok(true);
    }
    let Some(database) = database()? else {
        return Ok(false);
    };
    let Some(slice) = extract_title(&database.data, title_id) else {
        return Ok(false);
    };
    let value: Value = serde_json::from_str(slice)?;
    let obj = value
        .as_object()
        .ok_or_else(|| Error::Xbox("event title entry is not an object".into()))?;
    let fully_supported = obj
        .get("FullySupported")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let achievements = obj
        .get("Achievements")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let template = read_template(&database.dir, title_id)?;
    cache.insert(
        title_id,
        TitleRecord {
            fully_supported,
            template,
            achievements,
        },
    );
    Ok(true)
}

fn read_template(dir: &Path, title_id: u64) -> Result<String, Error> {
    let path = dir.join(format!("{title_id}.json"));
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(text),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(Error::Xbox(format!(
            "event template {title_id}.json was not found"
        ))),
        Err(err) => Err(err.into()),
    }
}

fn extract_title(data: &str, title_id: u64) -> Option<&str> {
    let pattern = format!("\n  \"{title_id}\": {{");
    let start = data.find(&pattern)?;
    let open = start + pattern.len() - 1;
    let end = end_of_object(data, open)?;
    Some(&data[open..=end])
}

fn end_of_object(data: &str, open: usize) -> Option<usize> {
    let bytes = data.as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    let mut index = open;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            if escape {
                escape = false;
            } else if byte == b'\\' {
                escape = true;
            } else if byte == b'"' {
                in_string = false;
            }
        } else if byte == b'"' {
            in_string = true;
        } else if byte == b'{' {
            depth += 1;
        } else if byte == b'}' {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
        index += 1;
    }
    None
}

fn batches(body: &Value) -> Result<Vec<Vec<Replacement>>, Error> {
    let obj = body
        .as_object()
        .ok_or_else(|| Error::Xbox("event achievement data is not an object".into()))?;
    let requests: Vec<&Value> = obj
        .iter()
        .filter(|(key, _)| is_request_key(key))
        .map(|(_, value)| value)
        .collect();
    if !requests.is_empty() {
        return requests.into_iter().map(replacements).collect();
    }
    Ok(vec![replacements(body)?])
}

fn is_request_key(key: &str) -> bool {
    let Some(rest) = key.strip_prefix("Request") else {
        return false;
    };
    !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit())
}

fn replacements(body: &Value) -> Result<Vec<Replacement>, Error> {
    let obj = body
        .as_object()
        .ok_or_else(|| Error::Xbox("event request is not an object".into()))?;
    let mut ops = Vec::new();
    for value in obj.values() {
        if value.get("ReplacementType").is_some() {
            ops.push(Replacement::from_value(value)?);
        }
    }
    Ok(ops)
}

impl Replacement {
    fn from_value(value: &Value) -> Result<Self, Error> {
        let kind = value
            .get("ReplacementType")
            .and_then(Value::as_str)
            .unwrap_or("");
        if kind != "Replace" {
            return Err(Error::Xbox(format!(
                "unsupported event replacement {kind}"
            )));
        }
        let target = value
            .get("Target")
            .and_then(Value::as_str)
            .filter(|target| !target.is_empty())
            .ok_or_else(|| Error::Xbox("event replacement is missing a target".into()))?;
        let replacement = value
            .get("Replacement")
            .ok_or_else(|| Error::Xbox("event replacement is missing a value".into()))?;
        Ok(Self {
            target: target.to_owned(),
            replacement: replacement_text(replacement)?,
        })
    }
}

fn replacement_text(value: &Value) -> Result<String, Error> {
    Ok(match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Null => "null".to_owned(),
        other => serde_json::to_string(other)?,
    })
}

fn render_event(
    template: &str,
    ops: &[Replacement],
    xuid: &str,
    seq: u64,
) -> Result<String, Error> {
    let mut body = template.to_owned();
    for op in ops {
        body = body.replace(&op.target, &op.replacement);
    }
    let session = uuid::Uuid::new_v4().to_string();
    body = body.replace("REPLACEXUID", xuid);
    body = body.replace("REPLACESESSIONGUID", &session);
    body = body.replace("REPLACETIME", &format_utc(system_now()));
    body = body.replace("REPLACESEQ", &seq.to_string());
    let value: Value = serde_json::from_str(&body).map_err(|err| {
        Error::Xbox(format!("event template is not valid json after replacement: {err}"))
    })?;
    Ok(serde_json::to_string(&value)?)
}

fn system_now() -> Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
}

fn format_utc(duration: Duration) -> String {
    let secs = duration.as_secs();
    let frac = duration.subsec_nanos() / 100;
    let days = (secs / 86_400) as i64;
    let time = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = time / 3_600;
    let minute = (time % 3_600) / 60;
    let second = time % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{frac:07}Z")
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

async fn post_event(
    client: &reqwest::Client,
    events_token: &str,
    body: String,
) -> Result<ProgressStatus, Error> {
    let tickets = format!("\"1\"=\"{events_token}\"");
    let response = client
        .post(COLLECTOR_URL)
        .header("Content-Type", "application/x-json-stream")
        .header("Accept", "application/json")
        .header("authxtoken", events_token)
        .header("tickets", tickets)
        .body(body)
        .send()
        .await?;
    let status = response.status().as_u16();
    let text = response.text().await?;
    Ok(event_status(status, &text))
}

fn event_status(status: u16, body: &str) -> ProgressStatus {
    match status {
        401 => return ProgressStatus::Unauthorized,
        403 => return ProgressStatus::Forbidden,
        200..=299 => {}
        _ => {
            let snippet: String = body.chars().take(180).collect();
            return ProgressStatus::Rejected(format!("events returned {status}: {snippet}"));
        }
    }
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return ProgressStatus::Updated;
    };
    if value.get("acc").and_then(Value::as_u64) == Some(0) {
        let snippet: String = body.chars().take(180).collect();
        return ProgressStatus::Rejected(format!("events rejected the payload: {snippet}"));
    }
    if value
        .get("errors")
        .and_then(Value::as_array)
        .is_some_and(|errors| !errors.is_empty())
    {
        let snippet: String = body.chars().take(180).collect();
        return ProgressStatus::Rejected(format!("events rejected the payload: {snippet}"));
    }
    ProgressStatus::Updated
}

#[cfg(test)]
mod tests {
    use super::{
        batches, civil_from_days, event_status, extract_title, format_utc, render_event,
        Replacement,
    };
    use crate::xbox::achievements::ProgressStatus;
    use serde_json::Value;
    use std::time::Duration;

    #[test]
    fn template_names_are_title_ids() {
        assert_eq!(
            super::template_title_id(std::path::Path::new("226038481.json")).unwrap(),
            226038481
        );
        assert!(super::template_title_id(std::path::Path::new("Data.json")).is_err());
        assert!(super::template_title_id(std::path::Path::new("notes.txt")).is_err());
        assert!(super::validate_template(b"[]").is_err());
        assert!(super::validate_template(br#"{"name":"Game"}"#).is_ok());
    }

    #[test]
    fn utc_epoch_and_a_known_day() {
        assert_eq!(format_utc(Duration::from_secs(0)), "1970-01-01T00:00:00.0000000Z");
        assert_eq!(civil_from_days(18_628), (2021, 1, 1));
        assert_eq!(
            format_utc(Duration::from_secs(1_609_459_200)),
            "2021-01-01T00:00:00.0000000Z"
        );
    }

    #[test]
    fn extract_reads_one_top_level_title() {
        let data = "{\n  \"10\": {\"Achievements\": {\"1\": {\"n\": \"}\" }}},\n  \"11\": {\"FullySupported\": true}\n}\n";
        let title = extract_title(data, 10).unwrap();
        let parsed: Value = serde_json::from_str(title).unwrap();
        assert_eq!(parsed["Achievements"]["1"]["n"], "}");
        assert!(extract_title(data, 99).is_none());
    }

    #[test]
    fn requests_stay_in_file_order() {
        let body: Value = serde_json::json!({
            "Request10": {
                "EventReplacement": {
                    "ReplacementType": "Replace",
                    "Target": "REPLACEEVENTNAME",
                    "Replacement": "First"
                }
            },
            "Request2": {
                "DataReplacement": {
                    "ReplacementType": "Replace",
                    "Target": "REPLACEDATA",
                    "Replacement": {"name": "Second"}
                }
            }
        });
        let batches = batches(&body).unwrap();
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0][0].target, "REPLACEEVENTNAME");
        assert_eq!(batches[0][0].replacement, "First");
        assert_eq!(batches[1][0].replacement, "{\"name\":\"Second\"}");
    }

    #[test]
    fn direct_replacements_are_one_event() {
        let body: Value = serde_json::json!({
            "EventReplacement": {
                "ReplacementType": "Replace",
                "Target": "NAME",
                "Replacement": "Score"
            },
            "DataReplacement1": {
                "ReplacementType": "Replace",
                "Target": "DATA",
                "Replacement": 3
            }
        });
        let batches = batches(&body).unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0][1].replacement, "3");
    }

    #[test]
    fn template_tokens_are_filled_and_minified() {
        let template = r#"{
            "name": "Game.NAME",
            "time": "REPLACETIME",
            "seq": REPLACESEQ,
            "data": DATA
        }"#;
        let ops = vec![
            Replacement {
                target: "NAME".into(),
                replacement: "Record".into(),
            },
            Replacement {
                target: "DATA".into(),
                replacement: r#"{"userId":"REPLACEXUID","playerSessionId":"REPLACESESSIONGUID"}"#
                    .into(),
            },
        ];
        let rendered = render_event(template, &ops, "42", 7).unwrap();
        assert!(!rendered.contains('\n'));
        let value: Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(value["name"], "Game.Record");
        assert_eq!(value["seq"], 7);
        assert_eq!(value["data"]["userId"], "42");
        assert_ne!(value["data"]["playerSessionId"], "REPLACESESSIONGUID");
        assert!(value["time"].as_str().unwrap().ends_with('Z'));
    }

    #[test]
    fn collector_errors_are_rejected() {
        assert_eq!(
            event_status(200, r#"{"acc":1}"#),
            ProgressStatus::Updated
        );
        assert!(matches!(
            event_status(200, r#"{"acc":1,"errors":[{"msg":"bad"}]}"#),
            ProgressStatus::Rejected(_)
        ));
        assert_eq!(event_status(401, ""), ProgressStatus::Unauthorized);
    }

    #[test]
    fn count_bout_record_score_renders() {
        let template = include_str!("../../assets/events/226038481.json");
        let body: Value = serde_json::json!({
            "Request1": {
                "EventReplacement": {
                    "ReplacementType": "Replace",
                    "Target": "REPLACEEVENTNAME",
                    "Replacement": "RecordScore"
                },
                "MetaDataReplacement": {
                    "ReplacementType": "Replace",
                    "Target": "REPLACEMETADATA",
                    "Replacement": "\"metadata\":{\"f\":{\"baseData\":{\"f\":{\"properties\":{\"f\":{\"RecordScore\":4}},\"ver\":2}}},\"privTags\":33554432,\"policies\":0},"
                },
                "DataReplacement": {
                    "ReplacementType": "Replace",
                    "Target": "REPLACEDATA",
                    "Replacement": "{\"baseType\":\"Microsoft.XboxLive.InGame\",\"baseData\":{\"measurements\":{},\"name\":\"RecordScore\",\"playerSessionId\":\"REPLACESESSIONGUID\",\"properties\":{\"RecordScore\":100000},\"serviceConfigId\":\"56b30100-30ca-428f-ae8c-86613d594f4d\",\"titleId\":226038481,\"userId\":\"REPLACEXUID\",\"ver\":1}}"
                }
            }
        });
        let batches = batches(&body).unwrap();
        let rendered = render_event(template, &batches[0], "9", 3).unwrap();
        let value: Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(
            value["name"],
            "Microsoft.XboxLive.T226038481.RecordScore"
        );
        assert_eq!(value["ext"]["utc"]["seq"], 3);
        assert_eq!(value["data"]["baseData"]["userId"], "9");
        assert_eq!(value["data"]["baseData"]["properties"]["RecordScore"], 100000);
    }

    #[test]
    fn bundled_catalog_builds_count_bout() {
        let missing = super::event_plan(1, "1").unwrap();
        assert!(missing.is_none());
        let plan = super::event_plan(226038481, "1").unwrap().unwrap();
        assert_eq!(plan.batches.len(), 2);
        let rendered = super::render_event(&plan.template, &plan.batches[0], "9", 4).unwrap();
        let value: Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(value["name"], "Microsoft.XboxLive.T226038481.RecordScore");
        assert!(super::event_plan(226038481, "999999").is_err());
    }
}
