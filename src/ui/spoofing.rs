use std::time::{Duration, Instant};

use eframe::egui::{self, Color32};
use egui_lucide::Lucide;

use super::KilogApp;
use super::home::AuthState;
use super::theme::{ACCENT, MUTED, TEXT};
use crate::auth::XboxAuthorization;
use crate::xbox::presence::{HeartbeatControl, HeartbeatNote, HeartbeatSession, spawn_heartbeat};
use crate::xbox::titles::{Title, parse_title_id};

#[derive(Clone, PartialEq, Eq)]
enum PlayedSnapshot {
    Duration(Duration),
    Unknown,
}

struct SpoofArt {
    title_id: u64,
    name: Option<String>,
    cover_url: Option<String>,
    played: PlayedSnapshot,
}

pub(super) struct SpoofState {
    title_id: String,
    live: bool,
    error: Option<String>,
    active_title: Option<u64>,
    active_name: String,
    cover_url: Option<String>,
    started_at: Option<Instant>,
    played: PlayedSnapshot,
    run: Option<HeartbeatRun>,
    retired: Vec<HeartbeatControl>,
    art_rx: Option<tokio::sync::oneshot::Receiver<SpoofArt>>,
}

struct HeartbeatRun {
    control: HeartbeatControl,
    authorization: String,
}

impl SpoofState {
    pub(super) fn new() -> Self {
        Self {
            title_id: String::new(),
            live: false,
            error: None,
            active_title: None,
            active_name: String::new(),
            cover_url: None,
            started_at: None,
            played: PlayedSnapshot::Unknown,
            run: None,
            retired: Vec::new(),
            art_rx: None,
        }
    }
}

impl KilogApp {
    pub(super) fn spoofing_page(&mut self, ui: &mut egui::Ui) {
        if self.spoof.live {
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        }
        ui.set_max_width(640.0);

        if let Some(reason) = self.spoof_block_reason() {
            ui.label(egui::RichText::new(reason).size(15.0).color(MUTED));
            ui.add_space(12.0);
        }

        let mut restart = false;
        let mut start = false;
        let mut stop = false;

        let title_id =
            super::theme::search_field(ui, &mut self.spoof.title_id, "Title ID", 320.0);
        if title_id.lost_focus() {
            restart = true;
            if ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                start = true;
            }
        }
        ui.add_space(14.0);

        if self.spoof.live {
            if accent_button(ui, "Stop Spoofing", 160.0).clicked() {
                stop = true;
            }
        } else if accent_button(ui, "Start Spoofing", 160.0).clicked() {
            start = true;
        }

        if self.spoof.live {
            ui.add_space(18.0);
            cover_card(ui, &self.spoof);
        }

        if let Some(err) = &self.spoof.error {
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(err)
                    .size(13.0)
                    .color(Color32::from_rgb(232, 120, 128)),
            );
        }

        let ctx = ui.ctx().clone();
        if stop {
            self.stop_spoofing();
        } else if start
            || (restart && self.spoof.live && parse_title_id(&self.spoof.title_id).is_some())
        {
            self.start_heartbeat(ctx);
        }
    }

    pub(super) fn poll_heartbeat(&mut self) {
        self.poll_spoof_art();
        if self.spoof.live && self.signed_in_xuid().is_none() {
            self.halt_heartbeat();
            self.spoof.error = Some("Authentication lost. Heartbeat stopped.".into());
        }

        let mut failed: Option<String> = None;
        let live_notes = self.spoof.run.as_mut().map(|run| {
            let mut notes = Vec::new();
            while let Some(note) = run.control.try_recv() {
                notes.push(note);
            }
            notes
        });
        for note in live_notes.unwrap_or_default() {
            self.ingest_heartbeat(note, true, &mut failed);
        }

        let retired = std::mem::take(&mut self.spoof.retired);
        let mut still_running = Vec::new();
        for mut control in retired {
            while let Some(note) = control.try_recv() {
                self.ingest_heartbeat(note, false, &mut failed);
            }
            if !control.is_finished() {
                still_running.push(control);
            }
        }
        self.spoof.retired = still_running;

        let finished = self
            .spoof
            .run
            .as_ref()
            .is_some_and(|run| run.control.is_finished());
        if finished && self.spoof.live && failed.is_none() {
            failed = Some("Heartbeat stopped unexpectedly.".into());
        }
        if let Some(message) = failed {
            self.halt_heartbeat();
            self.spoof.error = Some(message);
        }
    }

    pub(super) fn stop_spoofing(&mut self) {
        self.halt_heartbeat();
        self.spoof.error = None;
    }

    fn halt_heartbeat(&mut self) {
        self.spoof.live = false;
        self.spoof.active_title = None;
        self.spoof.active_name.clear();
        self.spoof.cover_url = None;
        self.spoof.started_at = None;
        self.spoof.played = PlayedSnapshot::Unknown;
        self.spoof.art_rx = None;
        if let Some(run) = self.spoof.run.take() {
            run.control.request_stop();
            self.spoof.retired.push(run.control);
        }
    }

    fn start_heartbeat(&mut self, ctx: egui::Context) {
        if let Some(reason) = self.spoof_block_reason() {
            self.halt_heartbeat();
            self.spoof.error = Some(reason.to_owned());
            return;
        }
        let Some(title_id) = parse_title_id(&self.spoof.title_id) else {
            self.spoof.error = Some("Enter a title ID to spoof.".into());
            return;
        };
        let Some(xuid) = self.signed_in_xuid() else {
            self.halt_heartbeat();
            self.spoof.error = Some("Sign in on Home to spoof presence.".into());
            return;
        };
        let Some(authorization) = self.xbox.as_ref().map(|xbox| xbox.authorization.clone()) else {
            self.halt_heartbeat();
            self.spoof.error = Some("Xbox authorization is not ready.".into());
            return;
        };
        let known = self.known_title(title_id);
        let name = known
            .as_ref()
            .map(|known| known.name.clone())
            .unwrap_or_else(|| format!("Title {title_id}"));
        let runtime = self.runtime.clone();
        let same_auth = self
            .spoof
            .run
            .as_ref()
            .is_some_and(|run| run.authorization == authorization);
        if self.spoof.live && self.spoof.active_title == Some(title_id) && same_auth {
            return;
        }
        self.halt_heartbeat();
        let wake_ctx = ctx.clone();
        let control = spawn_heartbeat(
            &runtime,
            HeartbeatSession {
                xuid: xuid.clone(),
                authorization: String::new(),
                title_id,
            },
            move || wake_ctx.request_repaint(),
        );
        let (art_tx, art_rx) = tokio::sync::oneshot::channel();
        let art_ctx = ctx.clone();
        let art_authorization = authorization.clone();
        let language = crate::xbox::titles::accept_language(self.config.force_region);
        runtime.spawn(async move {
            let art = load_spoof_art(art_authorization, xuid, title_id, language).await;
            let _ = art_tx.send(art);
            art_ctx.request_repaint();
        });
        self.spoof.run = Some(HeartbeatRun {
            control,
            authorization,
        });
        self.spoof.art_rx = Some(art_rx);
        self.spoof.live = true;
        self.spoof.error = None;
        self.spoof.active_title = Some(title_id);
        self.spoof.active_name = name;
        self.spoof.cover_url = known.as_ref().and_then(|known| known.cover_url.clone());
        self.spoof.started_at = Some(Instant::now());
        self.spoof.played = known
            .map(|known| known.played)
            .unwrap_or(PlayedSnapshot::Unknown);
        tracing::info!(title_id, "presence heartbeat started");
    }

    fn known_title(&self, title_id: u64) -> Option<KnownTitle> {
        let title =
            self.titles.as_ref()?.titles.iter().find(|title| {
                title.title_id.as_deref().and_then(parse_title_id) == Some(title_id)
            })?;
        Some(KnownTitle {
            name: title
                .name
                .clone()
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| format!("Title {title_id}")),
            cover_url: title.cover_url(),
            played: played_snapshot(title),
        })
    }

    fn poll_spoof_art(&mut self) {
        let ready = self
            .spoof
            .art_rx
            .as_mut()
            .and_then(|rx| match rx.try_recv() {
                Ok(art) => Some(art),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => None,
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => None,
            });
        let Some(art) = ready else {
            return;
        };
        self.spoof.art_rx = None;
        if !self.spoof.live || self.spoof.active_title != Some(art.title_id) {
            return;
        }
        if let Some(name) = art.name.filter(|name| !name.trim().is_empty()) {
            self.spoof.active_name = name;
        }
        if art.cover_url.is_some() {
            self.spoof.cover_url = art.cover_url;
        }
        if let PlayedSnapshot::Duration(played) = art.played {
            self.spoof.played = PlayedSnapshot::Duration(played);
        }
    }

    fn ingest_heartbeat(
        &mut self,
        note: HeartbeatNote,
        from_live: bool,
        failed: &mut Option<String>,
    ) {
        match note {
            HeartbeatNote::Sent => {
                if self.spoof.live && from_live {
                    self.spoof.error = None;
                }
            }
            HeartbeatNote::Stopped { message } => {
                if from_live && self.spoof.live {
                    *failed = Some(message);
                }
            }
        }
    }

    fn spoof_block_reason(&self) -> Option<&'static str> {
        if self.xbox.as_ref().is_some_and(|xbox| {
            xbox.authorization == XboxAuthorization::developer_mock().authorization
        }) {
            return Some("Developer preview does not send Xbox Live heartbeats.");
        }
        if !matches!(self.auth, AuthState::Authenticated { .. }) {
            return Some("Sign in on Home to spoof presence.");
        }
        if self.signed_in_xuid().is_none() {
            return Some("Xbox profile has no XUID.");
        }
        if self.xbox.is_none() {
            return Some("Xbox authorization is not ready.");
        }
        None
    }
}

struct KnownTitle {
    name: String,
    cover_url: Option<String>,
    played: PlayedSnapshot,
}

fn played_snapshot(title: &Title) -> PlayedSnapshot {
    title
        .minutes_played
        .or_else(|| {
            title
                .title_history
                .as_ref()
                .and_then(|history| history.minutes_played)
        })
        .map(minutes_played)
        .unwrap_or(PlayedSnapshot::Unknown)
}

fn minutes_played(minutes: u64) -> PlayedSnapshot {
    PlayedSnapshot::Duration(Duration::from_secs(minutes.saturating_mul(60)))
}

async fn load_spoof_art(
    authorization: String,
    xuid: String,
    title_id: u64,
    language: String,
) -> SpoofArt {
    let mut art = if let Ok(Some(title)) =
        crate::xbox::titles::fetch_user_title(&authorization, &xuid, title_id, &language).await
    {
        art_from(&title, title_id)
    } else if let Ok(lookup) =
        crate::xbox::titles::lookup_titles(&authorization, &[title_id], &language).await
        && let Some(title) = lookup.titles.into_iter().next()
    {
        art_from(&title, title_id)
    } else {
        SpoofArt {
            title_id,
            name: None,
            cover_url: None,
            played: PlayedSnapshot::Unknown,
        }
    };
    if let Ok(Some(minutes)) =
        crate::xbox::titles::fetch_minutes_played(&authorization, &xuid, title_id).await
    {
        art.played = minutes_played(minutes);
    }
    art
}

fn art_from(title: &Title, title_id: u64) -> SpoofArt {
    SpoofArt {
        title_id,
        name: title.name.clone().filter(|name| !name.trim().is_empty()),
        cover_url: title.cover_url(),
        played: played_snapshot(title),
    }
}

fn cover_card(ui: &mut egui::Ui, spoof: &SpoofState) {
    egui::Frame::new()
        .fill(Color32::from_rgb(26, 28, 34))
        .corner_radius(egui::CornerRadius::same(12))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                cover_image(ui, spoof.cover_url.as_deref());
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(&spoof.active_name)
                            .size(20.0)
                            .strong()
                            .color(TEXT),
                    );
                    if let Some(title_id) = spoof.active_title {
                        ui.label(
                            egui::RichText::new(title_id.to_string())
                                .size(13.0)
                                .color(MUTED),
                        );
                    }
                    ui.add_space(14.0);
                    let spoofed = spoof
                        .started_at
                        .map(|started| format_clock(started.elapsed()))
                        .unwrap_or_else(|| "00:00:00".to_owned());
                    fact(ui, "Spoofed", &spoofed);
                    ui.add_space(8.0);
                    let played = match &spoof.played {
                        PlayedSnapshot::Duration(played) => format_played(*played),
                        PlayedSnapshot::Unknown => "—".to_owned(),
                    };
                    fact(ui, "Time Played", &played);
                });
            });
        });
}

fn cover_image(ui: &mut egui::Ui, url: Option<&str>) {
    let size = egui::vec2(168.0, 168.0);
    if let Some(url) = url {
        ui.add(
            egui::Image::from_uri(url)
                .fit_to_exact_size(size)
                .corner_radius(12),
        );
        return;
    }
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(12),
        Color32::from_rgb(38, 41, 48),
    );
    Lucide::Gamepad2
        .size(32.0)
        .color(MUTED)
        .stroke_width(2.0)
        .image()
        .paint_at(
            ui,
            egui::Rect::from_center_size(rect.center(), egui::vec2(32.0, 32.0)),
        );
}

fn fact(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(egui::RichText::new(label).size(12.0).color(MUTED));
    ui.label(egui::RichText::new(value).size(16.0).color(TEXT));
}

fn format_clock(elapsed: Duration) -> String {
    let total = elapsed.as_secs();
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

fn format_played(played: Duration) -> String {
    let minutes = played.as_secs() / 60;
    let hours = minutes / 60;
    let remainder = minutes % 60;
    if hours > 0 {
        format!("{hours}h {remainder}m")
    } else {
        format!("{remainder}m")
    }
}

fn accent_button(ui: &mut egui::Ui, label: &str, width: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 40.0), egui::Sense::click());
    let fill = if response.hovered() {
        ACCENT.gamma_multiply(1.12)
    } else {
        ACCENT
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(8), fill);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(15.0),
        TEXT,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
