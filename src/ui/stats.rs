use std::collections::HashMap;

use eframe::egui::{self, Color32};

use super::KilogApp;
use super::theme::{ACCENT, MUTED, TEXT};
use crate::xbox::stats::{StatWriteStatus, UserStat};
use crate::xbox::titles::{Title, parse_title_id, single_title_id};

const ERROR: Color32 = Color32::from_rgb(232, 120, 128);

struct LoadedStats {
    stats: Vec<UserStat>,
    scid: String,
}

enum LoadResult {
    Ready(LoadedStats),
    Failed(String),
}

pub(super) struct StatsPage {
    query: String,
    seen: String,
    searched: String,
    title_id: Option<u64>,
    scid: String,
    stats: Option<Vec<UserStat>>,
    drafts: HashMap<String, String>,
    loading: bool,
    writing: Option<String>,
    error: Option<String>,
    write_error: Option<String>,
    load_rx: Option<tokio::sync::oneshot::Receiver<(u64, LoadResult)>>,
    load_task: Option<tokio::task::JoinHandle<()>>,
    write_rx: Option<tokio::sync::oneshot::Receiver<(String, StatWriteStatus)>>,
    write_task: Option<tokio::task::JoinHandle<()>>,
    epoch: u64,
}

impl StatsPage {
    pub(super) fn new() -> Self {
        Self {
            query: String::new(),
            seen: String::new(),
            searched: String::new(),
            title_id: None,
            scid: String::new(),
            stats: None,
            drafts: HashMap::new(),
            loading: false,
            writing: None,
            error: None,
            write_error: None,
            load_rx: None,
            load_task: None,
            write_rx: None,
            write_task: None,
            epoch: 0,
        }
    }

    pub(super) fn clear(&mut self) {
        self.abort();
        let epoch = self.epoch;
        *self = Self::new();
        self.epoch = epoch;
    }

    fn abort(&mut self) {
        if let Some(task) = self.load_task.take() {
            task.abort();
        }
        self.load_rx = None;
        if let Some(task) = self.write_task.take() {
            task.abort();
        }
        self.write_rx = None;
        self.loading = false;
        self.writing = None;
        self.epoch = self.epoch.wrapping_add(1);
    }
}

impl KilogApp {
    pub(super) fn stats_page(&mut self, ui: &mut egui::Ui) {
        if self.stats_page.loading || self.stats_page.writing.is_some() {
            ui.ctx().request_repaint();
        }
        self.draw_stats_menu(ui);
        self.draw_stats_editor(ui);
    }

    pub(super) fn poll_stats(&mut self) {
        self.poll_stats_load();
        self.poll_stats_write();
    }

    fn draw_stats_menu(&mut self, ui: &mut egui::Ui) {
        let mut submit = false;
        ui.horizontal(|ui| {
            let field =
                super::theme::search_field(ui, &mut self.stats_page.query, "Title ID", 320.0);
            let enter = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            if enter || accent_button(ui, "Search").clicked() {
                submit = true;
            }
        });
        ui.add_space(8.0);

        if self.stats_page.query != self.stats_page.seen {
            self.note_stats_query_edited();
        }
        if submit {
            let ctx = ui.ctx().clone();
            self.submit_stats_search(ctx);
        }

        let query = self.stats_page.query.trim().to_owned();
        if query.is_empty() {
            let message = if self.signed_in_xuid().is_none() {
                "Sign in on Home, then search by Title ID."
            } else {
                "Search by Title ID to open a game."
            };
            ui.label(egui::RichText::new(message).size(15.0).color(MUTED));
            return;
        }
        if self.stats_page.searched != query {
            ui.label(
                egui::RichText::new("Press Enter to load this title.")
                    .size(15.0)
                    .color(MUTED),
            );
            return;
        }
        if self.stats_page.title_id.is_none()
            && let Some(err) = &self.stats_page.error
        {
            ui.label(egui::RichText::new(err).size(14.0).color(ERROR));
        }
    }

    fn note_stats_query_edited(&mut self) {
        let query = self.stats_page.query.clone();
        self.stats_page.clear();
        self.stats_page.query = query.clone();
        self.stats_page.seen = query;
    }

    fn submit_stats_search(&mut self, ctx: egui::Context) {
        let query = self.stats_page.query.trim().to_owned();
        if query.is_empty() {
            return;
        }
        self.stats_page.searched = query.clone();
        self.stats_page.error = None;
        self.stats_page.write_error = None;
        self.stats_page.stats = None;
        self.stats_page.drafts.clear();
        self.stats_page.scid.clear();
        self.stats_page.title_id = None;
        if self.signed_in_xuid().is_none() {
            self.stats_page.abort();
            self.stats_page.error = Some("Sign in on Home to edit stats.".into());
            return;
        }
        let Some(title_id) = single_title_id(&query) else {
            self.stats_page.abort();
            self.stats_page.error = Some("Enter a Title ID.".into());
            return;
        };
        self.stats_page.title_id = Some(title_id);
        self.load_stats(ctx);
    }

    fn load_stats(&mut self, ctx: egui::Context) {
        let Some(title_id) = self.stats_page.title_id else {
            return;
        };
        let Some(xuid) = self.signed_in_xuid() else {
            self.stats_page.loading = false;
            self.stats_page.error = Some("Sign in on Home to edit stats.".into());
            return;
        };
        let Some(authorization) = self.xbox.as_ref().map(|xbox| xbox.authorization.clone()) else {
            self.stats_page.loading = false;
            self.stats_page.error = Some("Xbox authorization is not ready.".into());
            return;
        };
        let language = crate::xbox::titles::accept_language(self.config.force_region);
        if let Some(task) = self.stats_page.load_task.take() {
            task.abort();
        }
        self.stats_page.epoch = self.stats_page.epoch.wrapping_add(1);
        let epoch = self.stats_page.epoch;
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.stats_page.load_rx = Some(rx);
        self.stats_page.loading = true;
        self.stats_page.error = None;
        let title_text = title_id.to_string();
        self.stats_page.load_task = Some(self.runtime.spawn(async move {
            let scid = service_config_id(&authorization, title_id, &language).await;
            let result = match crate::xbox::stats::fetch_user_stats(
                &authorization,
                &xuid,
                &title_text,
                &scid,
            )
            .await
            {
                Ok(stats) => LoadResult::Ready(LoadedStats { stats, scid }),
                Err(err) => LoadResult::Failed(stats_failure(&err.to_string())),
            };
            let _ = tx.send((epoch, result));
            ctx.request_repaint();
        }));
    }

    fn poll_stats_load(&mut self) {
        let ready = self
            .stats_page
            .load_rx
            .as_mut()
            .and_then(|rx| match rx.try_recv() {
                Ok(update) => Some(update),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => None,
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => None,
            });
        let Some((epoch, result)) = ready else {
            return;
        };
        self.stats_page.load_rx = None;
        self.stats_page.load_task = None;
        if epoch != self.stats_page.epoch {
            return;
        }
        self.stats_page.loading = false;
        match result {
            LoadResult::Ready(loaded) => {
                for stat in &loaded.stats {
                    self.stats_page
                        .drafts
                        .entry(draft_key(stat))
                        .or_insert_with(|| stat.value.clone());
                }
                self.stats_page.scid = loaded.scid;
                self.stats_page.stats = Some(loaded.stats);
                self.stats_page.error = None;
            }
            LoadResult::Failed(message) => self.stats_page.error = Some(message),
        }
    }

    fn submit_stat(&mut self, ctx: egui::Context, stat: &UserStat) {
        if self.stats_page.writing.is_some() {
            return;
        }
        let scid = if stat.scid.trim().is_empty() {
            self.stats_page.scid.clone()
        } else {
            stat.scid.clone()
        };
        if scid.trim().is_empty() {
            self.stats_page.write_error = Some("This stat has no service config id.".into());
            return;
        }
        let Some(xuid) = self.signed_in_xuid() else {
            self.stats_page.write_error = Some("Sign in on Home to edit stats.".into());
            return;
        };
        let Some(authorization) = self.xbox.as_ref().map(|xbox| xbox.authorization.clone()) else {
            self.stats_page.write_error = Some("Xbox authorization is not ready.".into());
            return;
        };
        let key = draft_key(stat);
        let value = self
            .stats_page
            .drafts
            .get(&key)
            .cloned()
            .unwrap_or_default();
        let name = stat.name.clone();
        let stat_type = stat.stat_type;
        if let Some(task) = self.stats_page.write_task.take() {
            task.abort();
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.stats_page.writing = Some(key);
        self.stats_page.write_error = None;
        self.stats_page.write_rx = Some(rx);
        self.stats_page.write_task = Some(self.runtime.spawn(async move {
            let status = match crate::xbox::stats::write_user_stat(
                &authorization,
                &xuid,
                &scid,
                &name,
                stat_type,
                &value,
            )
            .await
            {
                Ok(status) => status,
                Err(err) => StatWriteStatus::Rejected(err.to_string()),
            };
            let _ = tx.send((name, status));
            ctx.request_repaint();
        }));
    }

    fn poll_stats_write(&mut self) {
        let ready = self
            .stats_page
            .write_rx
            .as_mut()
            .and_then(|rx| match rx.try_recv() {
                Ok(update) => Some(update),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => None,
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => None,
            });
        let Some((name, status)) = ready else {
            return;
        };
        self.stats_page.write_rx = None;
        self.stats_page.write_task = None;
        let writing = self.stats_page.writing.take();
        let matches = writing
            .as_deref()
            .is_some_and(|key| key.rsplit('\u{0}').next() == Some(name.as_str()));
        if !matches {
            return;
        }
        match status {
            StatWriteStatus::Updated => {
                self.stats_page.write_error = None;
                if let Some(key) = writing.as_deref()
                    && let Some(value) = self.stats_page.drafts.get(key).cloned()
                    && let Some(stats) = &mut self.stats_page.stats
                    && let Some(stat) = stats.iter_mut().find(|stat| draft_key(stat) == key)
                {
                    stat.value = value;
                }
                self.stats_page.loading = true;
            }
            StatWriteStatus::Unauthorized => {
                self.stats_page.write_error = Some("Xbox authorization was rejected.".into());
            }
            StatWriteStatus::Forbidden => {
                self.stats_page.write_error = Some("Xbox refused this stat update.".into());
            }
            StatWriteStatus::Rejected(message) => {
                self.stats_page.write_error = Some(message);
            }
        }
    }

    fn draw_stats_editor(&mut self, ui: &mut egui::Ui) {
        let query = self.stats_page.query.trim();
        if query.is_empty()
            || self.stats_page.searched != query
            || self.stats_page.title_id.is_none()
        {
            return;
        }
        if let Some(err) = self
            .stats_page
            .write_error
            .clone()
            .or(self.stats_page.error.clone())
        {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(err).size(14.0).color(ERROR));
        }
        if self.stats_page.loading && self.stats_page.stats.is_none() {
            ui.add_space(12.0);
            loading_row(ui, "Loading stats…");
            return;
        }
        if self
            .stats_page
            .stats
            .as_ref()
            .is_some_and(|stats| stats.is_empty())
            && !self.stats_page.loading
        {
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new("No stats to edit for this title.")
                    .size(15.0)
                    .color(MUTED),
            );
            return;
        }
        if self.stats_page.stats.is_none() {
            return;
        }
        ui.add_space(12.0);
        if self.stats_page.loading {
            loading_row(ui, "Refreshing stats…");
            ui.add_space(8.0);
        }
        let stats = self.stats_page.stats.clone().unwrap_or_default();
        let writing = self.stats_page.writing.clone();
        let mut drafts = self.stats_page.drafts.clone();
        let mut submit = None;
        egui::ScrollArea::vertical()
            .id_salt("stats_list")
            .show(ui, |ui| {
                for stat in &stats {
                    let key = draft_key(stat);
                    let draft = drafts.entry(key.clone()).or_default();
                    let busy = writing.as_deref() == Some(key.as_str());
                    if stat_row(ui, stat, draft, busy) {
                        submit = Some(stat.clone());
                    }
                    ui.add_space(8.0);
                }
            });
        self.stats_page.drafts = drafts;
        if let Some(stat) = submit {
            let ctx = ui.ctx().clone();
            self.submit_stat(ctx, &stat);
        }
        if self.stats_page.loading && self.stats_page.load_rx.is_none() {
            let ctx = ui.ctx().clone();
            self.load_stats(ctx);
        }
    }
}

async fn service_config_id(authorization: &str, title_id: u64, language: &str) -> String {
    let Ok(lookup) = crate::xbox::titles::lookup_titles(authorization, &[title_id], language).await
    else {
        return String::new();
    };
    lookup
        .titles
        .into_iter()
        .find(|title| title.title_id.as_deref().and_then(parse_title_id) == Some(title_id))
        .and_then(|title| scid_of(&title))
        .unwrap_or_default()
}

fn scid_of(title: &Title) -> Option<String> {
    present_id(title.service_config_id.as_deref()).or_else(|| {
        title
            .detail
            .as_ref()
            .and_then(|detail| present_id(detail.scid.as_deref()))
    })
}

fn present_id(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn draft_key(stat: &UserStat) -> String {
    format!("{}\u{0}{}", stat.group, stat.name)
}

fn stats_failure(message: &str) -> String {
    if message.contains("returned 401") {
        "Xbox authorization was rejected.".into()
    } else if message.contains("returned 403") {
        "Xbox refused this stat update.".into()
    } else {
        message.to_owned()
    }
}

fn stat_row(ui: &mut egui::Ui, stat: &UserStat, draft: &mut String, busy: bool) -> bool {
    let width = ui.available_width();
    let mut submit = false;
    super::theme::card_frame().show(ui, |ui| {
        ui.set_width((width - 32.0).max(0.0));
        ui.label(
            egui::RichText::new(&stat.label)
                .size(16.0)
                .strong()
                .color(TEXT),
        );
        ui.label(
            egui::RichText::new(format!("{} · {}", stat.name, stat.stat_type.label()))
                .size(13.0)
                .color(MUTED),
        );
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(format!("Current value  {}", stat.value))
                .size(14.0)
                .color(TEXT),
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let field = ui.add(
                egui::TextEdit::singleline(draft)
                    .hint_text("New value")
                    .desired_width(220.0)
                    .font(egui::FontId::proportional(14.0)),
            );
            if busy {
                ui.spinner();
                ui.label(egui::RichText::new("Saving…").size(13.0).color(MUTED));
            } else if accent_button(ui, "Update").clicked()
                || (field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)))
            {
                submit = true;
            }
        });
    });
    submit
}

fn loading_row(ui: &mut egui::Ui, message: &str) {
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(egui::RichText::new(message).size(14.0).color(MUTED));
    });
    ui.ctx().request_repaint();
}

fn accent_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), egui::FontId::proportional(14.0), TEXT);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(galley.size().x + 28.0, 32.0),
        egui::Sense::click(),
    );
    let fill = if response.hovered() {
        ACCENT.gamma_multiply(1.12)
    } else {
        ACCENT
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(super::theme::RADIUS), fill);
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        TEXT,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
