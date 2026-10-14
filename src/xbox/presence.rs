use std::time::Duration;

const HEARTBEAT_URL: &str =
    "https://presence-heartbeat.xboxlive.com/users/xuid({xuid})/devices/current/";
const CONTRACT_VERSION: &str = "3";
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5 * 60);

pub struct HeartbeatSession {
    pub xuid: String,
    pub authorization: String,
    pub title_id: u64,
}

pub enum HeartbeatNote {
    Sent { code: u16 },
    Stopped { code: Option<u16>, message: String },
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
            .http1_only()
            .build()
        {
            Ok(client) => client,
            Err(err) => {
                emit(
                    &notes,
                    &wake,
                    HeartbeatNote::Stopped {
                        code: None,
                        message: err.to_string(),
                    },
                );
                return;
            }
        };
        let mut session = session;
        loop {
            if *stop_rx.borrow() {
                break;
            }
            match beat_once(&client, &mut session).await {
                Ok(code) => {
                    emit(&notes, &wake, HeartbeatNote::Sent { code });
                    if *stop_rx.borrow() {
                        break;
                    }
                    tokio::select! {
                        _ = tokio::time::sleep(HEARTBEAT_INTERVAL) => {}
                        _ = stop_rx.changed() => {}
                    }
                }
                Err(err) => {
                    tracing::warn!(error = %err, "presence heartbeat stopped");
                    emit(
                        &notes,
                        &wake,
                        HeartbeatNote::Stopped {
                            code: err.code(),
                            message: err.to_string(),
                        },
                    );
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

async fn beat_once(
    client: &reqwest::Client,
    session: &mut HeartbeatSession,
) -> Result<u16, HeartbeatError> {
    let mut last = None;
    for try_index in 0..2 {
        if session.authorization.trim().is_empty() {
            session.authorization = load_app_authorization()
                .await
                .map_err(HeartbeatError::Transport)?;
        }
        match post_heartbeat(client, session).await {
            Ok(code) => return Ok(code),
            Err(err) if err.is_auth() => {
                session.authorization.clear();
                if try_index == 1 {
                    return Err(err);
                }
                last = Some(err);
            }
            Err(err) => {
                if try_index == 1 {
                    return Err(err);
                }
                last = Some(err);
            }
        }
    }
    Err(last.unwrap_or_else(|| HeartbeatError::Transport("heartbeat failed".into())))
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
) -> Result<u16, HeartbeatError> {
    let xuid = session.xuid.trim();
    if xuid.is_empty() {
        return Err(HeartbeatError::Rejected {
            code: None,
            message: "xuid is empty".into(),
        });
    }
    let body = serde_json::to_string(&heartbeat_body(session))
        .map_err(|err| HeartbeatError::Rejected {
            code: None,
            message: format!("heartbeat body failed: {err}"),
        })?;
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
        let code = status.as_u16();
        if code == 401 || code == 403 {
            return Err(HeartbeatError::Auth { code, message });
        }
        return Err(HeartbeatError::Rejected {
            code: Some(code),
            message,
        });
    }
    tracing::debug!(title_id = session.title_id, "presence heartbeat ok");
    Ok(status.as_u16())
}

enum HeartbeatError {
    Auth { code: u16, message: String },
    Rejected { code: Option<u16>, message: String },
    Transport(String),
}

impl HeartbeatError {
    fn is_auth(&self) -> bool {
        matches!(self, Self::Auth { .. })
    }

    fn code(&self) -> Option<u16> {
        match self {
            Self::Auth { code, .. } => Some(*code),
            Self::Rejected { code, .. } => *code,
            Self::Transport(_) => None,
        }
    }
}

impl std::fmt::Display for HeartbeatError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auth { message, .. } | Self::Rejected { message, .. } | Self::Transport(message) => {
                formatter.write_str(message)
            }
        }
    }
}
