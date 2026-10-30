use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::Value;

use crate::error::Error;
use crate::xbox::achievements::ProgressStatus;

const COLLECTOR_URL: &str = "https://v10.events.data.microsoft.com/OneCollector/1.0/";
const EVENTS_LIST_URL: &str =
    "https://api.github.com/repos/Achievement-Labs/achievement-labs/contents/Events?ref=main";
const USER_AGENT: &str = "kilog";
const MANIFEST_FILE: &str = ".manifest.json";

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

static DATABASE: Mutex<Option<Arc<Database>>> = Mutex::new(None);
static TITLES: Mutex<Option<HashMap<u64, TitleRecord>>> = Mutex::new(None);
static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

#[derive(Deserialize)]
struct RemoteEntry {
    name: String,
    sha: String,
    #[serde(rename = "type")]
    kind: String,
    download_url: Option<String>,
}

struct PendingFile {
    name: String,
    sha: String,
    url: String,
}

pub(crate) async fn sync() -> Result<(), Error> {
    let dir = crate::config::events_dir()?;
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(600))
        .build()?;
    let remote = fetch_listing(&client).await?;
    let mut manifest = load_manifest(&dir);
    let mut remote_names = HashSet::new();
    let mut unchanged = 0usize;
    let mut pending = Vec::new();
    for entry in remote {
        if entry.kind != "file" {
            continue;
        }
        let Some(url) = entry.download_url else {
            continue;
        };
        if !safe_event_name(&entry.name) {
            continue;
        }
        remote_names.insert(entry.name.clone());
        let current = manifest.get(&entry.name).map(String::as_str);
        if !needs_download(current, &entry.sha, dir.join(&entry.name).is_file()) {
            manifest.insert(entry.name, entry.sha);
            unchanged += 1;
            continue;
        }
        pending.push(PendingFile {
            name: entry.name,
            sha: entry.sha,
            url,
        });
    }
    pending.sort_by_key(|file| file.name != "Data.json");

    let mut removed = 0usize;
    manifest.retain(|name, _| {
        if remote_names.contains(name) {
            return true;
        }
        let path = dir.join(name);
        if path.is_file() {
            if std::fs::remove_file(&path).is_err() {
                return true;
            }
            removed += 1;
        }
        false
    });
    save_manifest(&dir, &manifest)?;

    tracing::info!(
        pending = pending.len(),
        unchanged,
        removed,
        "syncing event games"
    );

    let manifest = Arc::new(Mutex::new(manifest));
    let mut tasks = tokio::task::JoinSet::new();
    for file in pending {
        let client = client.clone();
        let dir = dir.clone();
        let manifest = Arc::clone(&manifest);
        tasks.spawn(async move {
            let dest = dir.join(&file.name);
            write_download(&client, &file.url, &dest).await?;
            store_sha(&manifest, &dir, file.name, file.sha)?;
            Ok::<(), Error>(())
        });
    }

    let mut updated = 0usize;
    let mut failed = 0usize;
    let mut first_error = None;
    while let Some(joined) = tasks.join_next().await {
        match joined {
            Ok(Ok(())) => updated += 1,
            Ok(Err(err)) => {
                if first_error.is_none() {
                    first_error = Some(err.to_string());
                }
                failed += 1;
            }
            Err(err) => {
                if first_error.is_none() {
                    first_error = Some(err.to_string());
                }
                failed += 1;
            }
        }
    }

    if updated > 0 || removed > 0 {
        refresh_database()?;
    }
    if let Some(err) = first_error {
        return Err(Error::Xbox(format!(
            "{failed} event file(s) failed to download: {err}"
        )));
    }
    tracing::info!(updated, unchanged, removed, "event games updated");
    Ok(())
}

async fn fetch_listing(client: &reqwest::Client) -> Result<Vec<RemoteEntry>, Error> {
    let entries: Vec<RemoteEntry> = client
        .get(EVENTS_LIST_URL)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if entries.len() >= 1000 {
        return Err(Error::Xbox(
            "event listing was truncated; refusing a partial update".into(),
        ));
    }
    Ok(entries)
}

fn needs_download(saved_sha: Option<&str>, remote_sha: &str, present: bool) -> bool {
    !(present && saved_sha == Some(remote_sha))
}

fn safe_event_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && !name.ends_with(".partial")
        && !name.contains(['/', '\\', ':', '\0'])
        && name.bytes().all(|byte| byte >= 32)
}

fn load_manifest(dir: &Path) -> HashMap<String, String> {
    let path = dir.join(MANIFEST_FILE);
    let Ok(text) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn save_manifest(dir: &Path, files: &HashMap<String, String>) -> Result<(), Error> {
    let path = dir.join(MANIFEST_FILE);
    let tmp = partial_path(&path);
    std::fs::write(&tmp, serde_json::to_vec(files)?)?;
    replace_file(&tmp, &path)?;
    Ok(())
}

fn store_sha(
    manifest: &Mutex<HashMap<String, String>>,
    dir: &Path,
    name: String,
    sha: String,
) -> Result<(), Error> {
    let mut guard = manifest
        .lock()
        .map_err(|_| Error::Xbox("event manifest lock was poisoned".into()))?;
    guard.insert(name, sha);
    save_manifest(dir, &guard)
}

async fn write_download(client: &reqwest::Client, url: &str, dest: &Path) -> Result<(), Error> {
    let mut response = client.get(url).send().await?.error_for_status()?;
    let tmp = partial_path(dest);
    let write_result = async {
        let mut file = std::fs::File::create(&tmp)?;
        while let Some(chunk) = response.chunk().await? {
            std::io::Write::write_all(&mut file, &chunk)?;
        }
        file.sync_all()?;
        Ok::<(), Error>(())
    }
    .await;
    if let Err(err) = write_result {
        let _ = std::fs::remove_file(&tmp);
        return Err(err);
    }
    replace_file(&tmp, dest)?;
    Ok(())
}

fn partial_path(dest: &Path) -> PathBuf {
    let name = dest
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("event");
    dest.with_file_name(format!(".{name}.partial"))
}

fn replace_file(tmp: &Path, dest: &Path) -> Result<(), Error> {
    if dest.exists() {
        std::fs::remove_file(dest)?;
    }
    if let Err(err) = std::fs::rename(tmp, dest) {
        let _ = std::fs::remove_file(tmp);
        return Err(err.into());
    }
    Ok(())
}

pub(crate) struct InstalledTemplate {
    pub title_id: u64,
    pub in_catalog: bool,
}

pub(crate) fn install_template(path: &Path, bytes: &[u8]) -> Result<InstalledTemplate, Error> {
    let title_id = template_title_id(path)?;
    let bytes = strip_bom(bytes);
    validate_template(bytes)?;
    let dir = crate::config::events_dir()?;
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

pub(crate) fn missing_event_support(title_id: u64) -> Result<Option<&'static str>, Error> {
    let Some(database) = database()? else {
        return Ok(support_gap(false, false));
    };
    let in_catalog = extract_title(&database.data, title_id).is_some();
    let has_template = database.dir.join(format!("{title_id}.json")).is_file();
    Ok(support_gap(has_template, in_catalog))
}

fn support_gap(has_template: bool, in_catalog: bool) -> Option<&'static str> {
    match (has_template, in_catalog) {
        (true, true) => None,
        (false, false) => Some("This event-based game is not supported."),
        (false, true) => Some("This event-based game has no event template file."),
        (true, false) => Some("This event-based game has no data entry."),
    }
}

pub(crate) fn partial_event_ids(title_id: u64) -> Result<Option<HashSet<String>>, Error> {
    if missing_event_support(title_id)?.is_some() {
        return Ok(None);
    }
    let mut guard = titles()?;
    let cache = guard.get_or_insert_with(HashMap::new);
    if !load_title_into(cache, title_id)? {
        return Ok(None);
    }
    let title = &cache[&title_id];
    if title.fully_supported {
        return Ok(None);
    }
    Ok(Some(title.achievements.keys().cloned().collect()))
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

fn database() -> Result<Option<Arc<Database>>, Error> {
    let guard = database_lock()?;
    if let Some(database) = guard.as_ref() {
        return Ok(Some(Arc::clone(database)));
    }
    drop(guard);
    let dir = crate::config::events_dir()?;
    let path = dir.join("Data.json");
    if !path.is_file() {
        return Ok(None);
    }
    let data = std::fs::read_to_string(path)?;
    let database = Arc::new(Database { dir, data });
    let mut guard = database_lock()?;
    if let Some(existing) = guard.as_ref() {
        return Ok(Some(Arc::clone(existing)));
    }
    *guard = Some(Arc::clone(&database));
    Ok(Some(database))
}

fn refresh_database() -> Result<(), Error> {
    let dir = crate::config::events_dir()?;
    let path = dir.join("Data.json");
    let loaded = if path.is_file() {
        Some(Arc::new(Database {
            dir,
            data: std::fs::read_to_string(path)?,
        }))
    } else {
        None
    };
    *database_lock()? = loaded;
    if let Ok(mut guard) = titles() {
        *guard = None;
    }
    Ok(())
}

fn database_lock() -> Result<std::sync::MutexGuard<'static, Option<Arc<Database>>>, Error> {
    DATABASE
        .lock()
        .map_err(|_| Error::Xbox("event database lock was poisoned".into()))
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
            return Err(Error::Xbox(format!("unsupported event replacement {kind}")));
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
        Error::Xbox(format!(
            "event template is not valid json after replacement: {err}"
        ))
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
        Replacement, batches, civil_from_days, event_status, extract_title, format_utc,
        render_event,
    };
    use crate::xbox::achievements::ProgressStatus;
    use serde_json::Value;
    use std::time::Duration;

    #[test]
    fn support_gap_requires_both_files() {
        assert!(super::support_gap(true, true).is_none());
        assert!(
            super::support_gap(false, false)
                .unwrap()
                .contains("not supported")
        );
        assert!(
            super::support_gap(false, true)
                .unwrap()
                .contains("template")
        );
        assert!(
            super::support_gap(true, false)
                .unwrap()
                .contains("data entry")
        );
    }

    #[test]
    fn event_names_stay_inside_the_events_folder() {
        assert!(super::safe_event_name("226038481.json"));
        assert!(super::safe_event_name("Data.json"));
        assert!(super::safe_event_name("Project Cars.json"));
        assert!(!super::safe_event_name(".manifest.json"));
        assert!(!super::safe_event_name("../config.json"));
        assert!(!super::safe_event_name("nested/file.json"));
        assert!(!super::safe_event_name(".Data.json.partial"));
    }

    #[test]
    fn unchanged_files_are_not_downloaded_again() {
        assert!(!super::needs_download(Some("abc"), "abc", true));
        assert!(super::needs_download(Some("abc"), "def", true));
        assert!(super::needs_download(Some("abc"), "abc", false));
        assert!(super::needs_download(None, "abc", true));
    }

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
        assert_eq!(
            format_utc(Duration::from_secs(0)),
            "1970-01-01T00:00:00.0000000Z"
        );
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
        assert_eq!(event_status(200, r#"{"acc":1}"#), ProgressStatus::Updated);
        assert!(matches!(
            event_status(200, r#"{"acc":1,"errors":[{"msg":"bad"}]}"#),
            ProgressStatus::Rejected(_)
        ));
        assert_eq!(event_status(401, ""), ProgressStatus::Unauthorized);
    }

    #[test]
    fn count_bout_record_score_renders() {
        let template = include_str!("testdata/226038481.json");
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
        assert_eq!(value["name"], "Microsoft.XboxLive.T226038481.RecordScore");
        assert_eq!(value["ext"]["utc"]["seq"], 3);
        assert_eq!(value["data"]["baseData"]["userId"], "9");
        assert_eq!(
            value["data"]["baseData"]["properties"]["RecordScore"],
            100000
        );
    }
}
