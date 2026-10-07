use std::io::Write;
use std::sync::Mutex;

const LOG_PATH: &str = r"c:\Users\Administrator\Documents\GitHub\kilog\debug-4c776f.log";

pub fn log(hypothesis_id: &str, location: &str, message: &str, data: serde_json::Value) {
    static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let _guard = SEEN.lock().unwrap_or_else(|err| err.into_inner());
    write_log(hypothesis_id, location, message, data);
}

pub fn log_once(
    key: &str,
    hypothesis_id: &str,
    location: &str,
    message: &str,
    data: serde_json::Value,
) {
    static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let mut seen = SEEN.lock().unwrap_or_else(|err| err.into_inner());
    if seen.iter().any(|existing| existing == key) || seen.len() > 80 {
        return;
    }
    seen.push(key.to_owned());
    drop(seen);
    write_log(hypothesis_id, location, message, data);
}

fn write_log(hypothesis_id: &str, location: &str, message: &str, data: serde_json::Value) {
    static FILE: Mutex<()> = Mutex::new(());
    let _guard = FILE.lock().unwrap_or_else(|err| err.into_inner());
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0);
    let line = serde_json::json!({
        "sessionId": "4c776f",
        "runId": "post-fix",
        "hypothesisId": hypothesis_id,
        "location": location,
        "message": message,
        "data": data,
        "timestamp": timestamp,
    });
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(LOG_PATH)
    {
        let _ = writeln!(file, "{line}");
    }
}
