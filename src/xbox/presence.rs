use std::time::Duration;

const HEARTBEAT_URL: &str =
    "https://presence-heartbeat.xboxlive.com/users/xuid({xuid})/devices/current/";
const CONTRACT_VERSION: &str = "3";

pub struct HeartbeatSession {
    pub xuid: String,
    pub authorization: String,
    pub title_id: u64,
}

pub enum HeartbeatNote {
    Sent,
    Stopped { message: String },
}

pub struct HeartbeatControl {
    stop: tokio::sync::watch::Sender<bool>,
    notes: tokio::sync::mpsc::UnboundedReceiver<HeartbeatNote>,
    task: tokio::task::JoinHandle<()>,
}

impl HeartbeatControl {
    pub fn request_stop(&self) {
        let _ = self.stop.send(true);
    }

    pub fn try_recv(&mut self) -> Option<HeartbeatNote> {
        self.notes.try_recv().ok()
    }

    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }
}

impl Drop for HeartbeatControl {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        self.task.abort();
    }
}

pub fn heartbeat_url(xuid: &str) -> String {
    HEARTBEAT_URL.replace("{xuid}", xuid)
}

pub fn heartbeat_interval(mix: u64) -> Duration {
    Duration::from_secs(30 + (mix % 16))
}

pub fn spawn_heartbeat(
    runtime: &tokio::runtime::Handle,
    session: HeartbeatSession,
    wake: impl Fn() + Send + 'static,
) -> HeartbeatControl {
    let (stop, mut stop_rx) = tokio::sync::watch::channel(false);
    let (notes, notes_rx) = tokio::sync::mpsc::unbounded_channel();
    let task = runtime.spawn(async move {
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
        {
            Ok(client) => client,
            Err(err) => {
                emit(
                    &notes,
                    &wake,
                    HeartbeatNote::Stopped {
                        message: err.to_string(),
                    },
                );
                return;
            }
        };
        let mut session = session;
        let mut tick = 0u64;
        loop {
            if *stop_rx.borrow() {
                break;
            }
            match beat_once(&client, &mut session).await {
                Ok(()) => {
                    let wait = heartbeat_interval(interval_mix(tick));
                    tick = tick.wrapping_add(1);
                    emit(&notes, &wake, HeartbeatNote::Sent);
                    if *stop_rx.borrow() {
                        break;
                    }
                    tokio::select! {
                        _ = tokio::time::sleep(wait) => {}
                        _ = stop_rx.changed() => {}
                    }
                }
                Err(message) => {
                    tracing::warn!(error = %message, "presence heartbeat stopped");
                    emit(&notes, &wake, HeartbeatNote::Stopped { message });
                    break;
                }
            }
        }
        tracing::debug!("presence heartbeat loop exited");
    });
    HeartbeatControl {
        stop,
        notes: notes_rx,
        task,
    }
}

fn emit(
    notes: &tokio::sync::mpsc::UnboundedSender<HeartbeatNote>,
    wake: &impl Fn(),
    note: HeartbeatNote,
) {
    let _ = notes.send(note);
    wake();
}

async fn beat_once(client: &reqwest::Client, session: &mut HeartbeatSession) -> Result<(), String> {
    let mut last = None;
    for try_index in 0..2 {
        if session.authorization.trim().is_empty() {
            session.authorization = load_app_authorization().await?;
        }
        match post_heartbeat(client, session).await {
            Ok(()) => return Ok(()),
            Err(err) if err.is_auth() => {
                session.authorization.clear();
                last = Some(err.to_string());
                if try_index == 1 {
                    return Err(last.unwrap());
                }
            }
            Err(err) => {
                last = Some(err.to_string());
                if try_index == 1 {
                    return Err(last.unwrap());
                }
            }
        }
    }
    Err(last.unwrap_or_else(|| "heartbeat failed".into()))
}

async fn load_app_authorization() -> Result<String, String> {
    tokio::task::spawn_blocking(super::app_token::read_xbox_app_authorization)
        .await
        .map_err(|err| format!("xbox app scan failed: {err}"))?
        .map_err(|err| err.to_string())
}

pub(crate) fn heartbeat_body(session: &HeartbeatSession) -> serde_json::Value {
    serde_json::json!({
        "titles": [{
            "expiration": 600,
            "id": session.title_id.to_string(),
            "state": "active",
            "sandbox": "RETAIL",
        }],
    })
}

async fn post_heartbeat(
    client: &reqwest::Client,
    session: &HeartbeatSession,
) -> Result<(), HeartbeatError> {
    let xuid = session.xuid.trim();
    if xuid.is_empty() {
        return Err(HeartbeatError::Rejected("xuid is empty".into()));
    }
    let body = serde_json::to_string(&heartbeat_body(session))
        .map_err(|err| HeartbeatError::Rejected(format!("heartbeat body failed: {err}")))?;
    let response = client
        .post(heartbeat_url(xuid))
        .header("Authorization", session.authorization.as_str())
        .header("Accept", "application/json")
        .header("Accept-Language", "en-US")
        .header("x-xbl-contract-version", CONTRACT_VERSION)
        .header("Content-Type", "application/json; charset=utf-8")
        .body(body)
        .send()
        .await
        .map_err(|err| HeartbeatError::Transport(format!("heartbeat request failed: {err}")))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        let snippet: String = body.chars().take(180).collect();
        let message = if snippet.is_empty() {
            format!("heartbeat returned {}", status.as_u16())
        } else {
            format!("heartbeat returned {}: {snippet}", status.as_u16())
        };
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(HeartbeatError::Auth(message));
        }
        return Err(HeartbeatError::Rejected(message));
    }
    tracing::debug!(title_id = session.title_id, "presence heartbeat ok");
    Ok(())
}

enum HeartbeatError {
    Auth(String),
    Rejected(String),
    Transport(String),
}

impl HeartbeatError {
    fn is_auth(&self) -> bool {
        matches!(self, Self::Auth(_))
    }
}

impl std::fmt::Display for HeartbeatError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auth(message) => formatter.write_str(message),
            Self::Rejected(message) | Self::Transport(message) => formatter.write_str(message),
        }
    }
}

fn interval_mix(tick: u64) -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.subsec_nanos() as u64)
        .unwrap_or(0);
    nanos ^ tick.wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> HeartbeatSession {
        HeartbeatSession {
            xuid: "9".into(),
            authorization: "XBL3.0 x=uhs;token".into(),
            title_id: 305419896,
        }
    }

    #[test]
    fn heartbeat_url_inserts_xuid() {
        assert_eq!(
            heartbeat_url("2535428600000000"),
            "https://presence-heartbeat.xboxlive.com/users/xuid(2535428600000000)/devices/current/"
        );
    }

    #[test]
    fn heartbeat_interval_stays_between_30_and_45_seconds() {
        for mix in [0, 1, 15, 16, 45, u64::MAX] {
            let secs = heartbeat_interval(mix).as_secs();
            assert!(
                (30..=45).contains(&secs),
                "interval {secs}s is outside 30..=45"
            );
        }
    }

    #[test]
    fn heartbeat_body_matches_the_spoofer_contract() {
        let body = heartbeat_body(&session());
        assert!(body.get("state").is_none());
        assert_eq!(body["titles"][0]["expiration"], 600);
        assert_eq!(body["titles"][0]["id"], "305419896");
        assert_eq!(body["titles"][0]["state"], "active");
        assert_eq!(body["titles"][0]["sandbox"], "RETAIL");
    }

    #[test]
    fn empty_xuid_is_rejected() {
        let mut session = session();
        session.xuid = "  ".into();
        let error = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(post_heartbeat(&reqwest::Client::new(), &session))
            .unwrap_err();
        assert_eq!(error.to_string(), "xuid is empty");
    }
}
