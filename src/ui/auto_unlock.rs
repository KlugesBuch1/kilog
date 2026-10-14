use eframe::egui::{self, Color32};

use super::KilogApp;
use super::theme::{ACCENT, MUTED, TEXT};
use crate::xbox::achievements::ProgressStatus;
use crate::xbox::queue::{UnlockQueue, build_queue, load_queue, save_queue};
use crate::xbox::titles::parse_title_id;

const ERROR: Color32 = Color32::from_rgb(232, 120, 128);

enum AutoNote {
    Waiting { name: String, seconds: u64 },
    Rolled { id: String, seconds: u64 },
    Unlocked(String),
    Failed { id: String, message: String },
    Finished,
}

pub(super) struct AutoUnlock {
    pub reference_open: bool,
    pub title_id: String,
    pub gamertag: String,
    pub delays_open: bool,
    delay_draft: Option<UnlockQueue>,
    skips_open: bool,
    skip_draft: Option<UnlockQueue>,
    pub queue: Option<UnlockQueue>,
    pub error: Option<String>,
    pub fetching: bool,
    fetch_rx: Option<tokio::sync::oneshot::Receiver<(u64, Result<UnlockQueue, String>)>>,
    pub running: bool,
    spoof_halted: bool,
    stop: Option<tokio::sync::watch::Sender<bool>>,
    task: Option<tokio::task::JoinHandle<()>>,
    notes: Option<tokio::sync::mpsc::UnboundedReceiver<AutoNote>>,
    pub status: Option<String>,
}

impl AutoUnlock {
    pub(super) fn new() -> Self {
        Self {
            reference_open: false,
            title_id: String::new(),
            gamertag: String::new(),
            delays_open: false,
            delay_draft: None,
            skips_open: false,
            skip_draft: None,
            queue: None,
            error: None,
            fetching: false,
            fetch_rx: None,
            running: false,
            spoof_halted: false,
            stop: None,
            task: None,
            notes: None,
            status: None,
        }
    }
}

impl KilogApp {
    pub(super) fn auto_unlock_page(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let gap = 8.0;
            let width = (ui.available_width() - gap) / 2.0;
            super::theme::search_field(ui, &mut self.auto.title_id, "Title ID", width);
            ui.add_space(gap);
            super::theme::text_field(ui, &mut self.auto.gamertag, "Reference Gamertag", width);
        });
        ui.add_space(8.0);
        let mut build = false;
        let mut edit = false;
        let mut skips = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if tool_button(ui, "Build queue", !self.auto.fetching).clicked() {
                build = true;
            }
            if tool_button(ui, "Edit delays", !self.auto.fetching).clicked() {
                edit = true;
            }
            if tool_button(ui, "Add Skips", !self.auto.fetching).clicked() {
                skips = true;
            }
            let mut options_changed = false;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                options_changed |= ui
                    .checkbox(
                        &mut self.config.stop_on_spoof_fail,
                        egui::RichText::new("Stop unlocking when spoofing fails")
                            .size(13.0)
                            .color(TEXT),
                    )
                    .changed();
                let randomized = ui
                    .checkbox(
                        &mut self.config.use_randomized_delays,
                        egui::RichText::new("Use randomized delays")
                            .size(13.0)
                            .color(TEXT),
                    )
                    .changed();
                if randomized && !self.config.use_randomized_delays {
                    self.clear_rolled_delays();
                }
                options_changed |= randomized;
            });
            if options_changed {
                self.persist_config();
            }
        });
        if build {
            self.fetch_reference(ui.ctx().clone());
        }
        if edit {
            self.open_delay_editor();
        }
        if skips {
            self.open_skip_editor();
        }
        self.draw_auto_status(ui);
        self.ensure_saved_queue();
        self.draw_queue_and_bar(ui);
        self.delays_window(ui.ctx());
        self.skips_window(ui.ctx());
    }

    fn draw_queue_and_bar(&mut self, ui: &mut egui::Ui) {
        if self.spoof.live {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(200));
        }
        let width = ui.available_width();
        let list_h = (ui.available_height() - 52.0).max(0.0);
        ui.allocate_ui(egui::vec2(width, list_h), |ui| {
            self.draw_queue_list(ui, width);
        });
        ui.add_space(8.0);
        let mut start = false;
        let mut stop = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if tool_button(
                ui,
                "Start unlock",
                !self.auto.running && !self.auto.fetching,
            )
            .clicked()
            {
                start = true;
            }
            if tool_button(ui, "Stop unlock", self.auto.running || self.spoof.live).clicked() {
                stop = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let color = match self.spoof.code {
                    Some(code) if (200..300).contains(&code) => TEXT,
                    Some(_) => ERROR,
                    None if self.spoof.live => TEXT,
                    None => MUTED,
                };
                ui.label(
                    egui::RichText::new(self.spoof_status_line())
                        .size(14.0)
                        .color(color)
                        .monospace(),
                );
            });
        });
        if start {
            self.start_auto_unlock(ui.ctx().clone());
        }
        if stop {
            self.stop_auto_unlock();
            self.stop_spoofing();
        }
    }

    fn spoof_status_line(&self) -> String {
        if !self.spoof.live {
            return "Spoofing not yet started.".into();
        }
        let clock = self
            .spoof
            .started_at
            .map(|started| format_hms(started.elapsed().as_secs()))
            .unwrap_or_else(|| "00:00:00".to_owned());
        match self.spoof.code {
            Some(code) => format!("Spoofing for: {clock} ({code})"),
            None => format!("Spoofing for: {clock}"),
        }
    }

    fn ensure_saved_queue(&mut self) {
        if self.auto.queue.is_some() {
            return;
        }
        if let Ok(Some(queue)) = load_queue() {
            self.auto.queue = Some(queue);
        }
    }

    fn draw_queue_list(&mut self, ui: &mut egui::Ui, width: f32) {
        if self.config.use_randomized_delays {
            self.fill_missing_rolls();
        }
        let Some(entries) = self.auto.queue.as_ref().map(|queue| queue.entries.clone()) else {
            return;
        };
        if entries.is_empty() {
            return;
        }
        ui.add_space(16.0);
        let show_randomized = self.config.use_randomized_delays;
        let mut elapsed = 0_u64;
        let mut rolled_elapsed = 0_u64;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let row_w = ui.available_width();
                let row_w = if row_w.is_finite() { row_w } else { width };
                for entry in &entries {
                    let (time, randomized) = if entry.skip {
                        ("Skipped".to_owned(), None)
                    } else {
                        elapsed = elapsed.saturating_add(entry.delay_seconds);
                        let randomized = if show_randomized {
                            entry.rolled_delay.map(|delay| {
                                rolled_elapsed = rolled_elapsed.saturating_add(delay);
                                format!("randomized to {}", format_hms(rolled_elapsed))
                            })
                        } else {
                            None
                        };
                        (format_hms(elapsed), randomized)
                    };
                    queue_row(
                        ui,
                        row_w,
                        &entry.name,
                        &time,
                        randomized.as_deref(),
                        entry.skip,
                        entry.done,
                    );
                    ui.add_space(8.0);
                }
            });
    }

    fn fill_missing_rolls(&mut self) {
        let settings = self.config.randomize_delays.clone();
        let Some(queue) = &mut self.auto.queue else {
            return;
        };
        for entry in queue.entries.iter_mut().filter(|entry| !entry.skip) {
            if entry.rolled_delay.is_none() {
                entry.rolled_delay = Some(crate::config::randomize_delay(
                    entry.delay_seconds,
                    &settings,
                ));
            }
        }
    }

    fn clear_rolled_delays(&mut self) {
        let Some(queue) = &mut self.auto.queue else {
            return;
        };
        for entry in &mut queue.entries {
            entry.rolled_delay = None;
        }
    }

    pub(super) fn draw_auto_status(&mut self, ui: &mut egui::Ui) {
        if self.auto.running || self.auto.fetching {
            ui.ctx().request_repaint();
        }
        if self.auto.fetching {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    egui::RichText::new("Building queue…")
                        .size(13.0)
                        .color(MUTED),
                );
            });
        } else if !self.auto.running
            && let Some(status) = &self.auto.status
        {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(status).size(13.0).color(MUTED));
        }
        if self.auto.running {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Spoofing this game while auto unlock runs.")
                    .size(14.0)
                    .color(TEXT),
            );
            if let Some(status) = &self.auto.status {
                ui.label(egui::RichText::new(status).size(13.0).color(MUTED));
            }
            if let Some(err) = &self.spoof.error {
                ui.label(egui::RichText::new(err).size(13.0).color(ERROR));
            }
        }
        if let Some(err) = &self.auto.error {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(err).size(13.0).color(ERROR));
        }
    }

    pub(super) fn draw_auto_windows(&mut self, ctx: &egui::Context) {
        self.reference_window(ctx);
        self.delays_window(ctx);
    }

    pub(super) fn poll_auto_unlock(&mut self) {
        self.poll_reference();
        let notes = {
            let Some(rx) = self.auto.notes.as_mut() else {
                return;
            };
            let mut notes = Vec::new();
            while let Ok(note) = rx.try_recv() {
                notes.push(note);
            }
            notes
        };
        for note in notes {
            self.apply_auto_note(note);
        }
    }

    pub(super) fn load_saved_queue(&mut self, title_id: u64) {
        if self.auto.running {
            return;
        }
        match load_queue() {
            Ok(Some(queue)) if queue.title_id == title_id => {
                self.auto.gamertag = queue.gamertag.clone();
                self.auto.queue = Some(queue);
                self.auto.error = None;
            }
            Ok(_) => {
                if self
                    .auto
                    .queue
                    .as_ref()
                    .is_some_and(|queue| queue.title_id != title_id)
                {
                    self.auto.queue = None;
                }
            }
            Err(err) => self.auto.error = Some(err.to_string()),
        }
    }

    pub(super) fn halt_unlock_for_spoof(&mut self) {
        if !self.config.stop_on_spoof_fail || !self.auto.running {
            return;
        }
        self.auto.spoof_halted = true;
        self.stop_auto_unlock();
        self.auto.error = Some("Unlocking stopped because spoofing failed.".into());
    }

    pub(super) fn stop_auto_unlock(&mut self) {
        if let Some(stop) = &self.auto.stop {
            let _ = stop.send(true);
        }
        self.auto.running = false;
        self.auto.status = None;
        if let Err(err) = self.persist_queue() {
            self.auto.error = Some(err);
        }
    }

    fn reference_window(&mut self, ctx: &egui::Context) {
        if !self.auto.reference_open {
            return;
        }
        let mut open = true;
        let mut submit = false;
        egui::Window::new("Reference gamertag")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.set_min_width(320.0);
                ui.label(
                    egui::RichText::new(
                        "Copy unlock times for achievements you have not unlocked yet.",
                    )
                    .size(13.0)
                    .color(MUTED),
                );
                ui.add_space(8.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.auto.gamertag)
                        .hint_text("Gamertag")
                        .desired_width(280.0),
                );
                ui.add_space(8.0);
                if self.auto.fetching {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(
                            egui::RichText::new("Building queue…")
                                .size(13.0)
                                .color(MUTED),
                        );
                    });
                } else if tool_button(ui, "Build queue", !self.auto.gamertag.trim().is_empty())
                    .clicked()
                {
                    submit = true;
                }
            });
        self.auto.reference_open = open;
        if submit {
            let ctx = ctx.clone();
            self.fetch_reference(ctx);
        }
    }

    fn open_delay_editor(&mut self) {
        match load_queue() {
            Ok(Some(queue)) => {
                self.auto.delay_draft = Some(queue);
                self.auto.delays_open = true;
                self.auto.error = None;
            }
            Ok(None) => self.auto.error = Some("No saved queue yet.".into()),
            Err(err) => self.auto.error = Some(err.to_string()),
        }
    }

    fn delays_window(&mut self, ctx: &egui::Context) {
        if !self.auto.delays_open {
            return;
        }
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        egui::Window::new("Edit delays")
            .open(&mut open)
            .default_width(520.0)
            .show(ctx, |ui| {
                let Some(queue) = self.auto.delay_draft.as_mut() else {
                    ui.label(egui::RichText::new("No queue yet.").size(14.0).color(MUTED));
                    return;
                };
                ui.label(
                    egui::RichText::new(format!("{} · Title {}", queue.gamertag, queue.title_id))
                        .size(13.0)
                        .color(MUTED),
                );
                ui.add_space(8.0);
                egui::ScrollArea::vertical()
                    .max_height(420.0)
                    .show(ui, |ui| {
                        for (index, entry) in queue.entries.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                ui.set_min_width(480.0);
                                let name = if entry.done {
                                    format!("{}. {} · done", index + 1, entry.name)
                                } else {
                                    format!("{}. {}", index + 1, entry.name)
                                };
                                ui.label(egui::RichText::new(name).size(14.0).color(TEXT).strong());
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let mut seconds = entry.delay_seconds.to_string();
                                        let field = ui.add(
                                            egui::TextEdit::singleline(&mut seconds)
                                                .desired_width(72.0),
                                        );
                                        ui.label(
                                            egui::RichText::new("sec").size(12.0).color(MUTED),
                                        );
                                        if field.changed()
                                            && let Ok(value) = seconds.trim().parse::<u64>()
                                        {
                                            entry.delay_seconds = value;
                                        }
                                    },
                                );
                            });
                            ui.add_space(6.0);
                        }
                    });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if tool_button(ui, "Save", true).clicked() {
                        save = true;
                    }
                    if tool_button(ui, "Cancel", true).clicked() {
                        cancel = true;
                    }
                });
            });
        if save {
            let Some(queue) = self.auto.delay_draft.clone() else {
                self.auto.delays_open = false;
                return;
            };
            if let Err(err) = save_queue(&queue) {
                self.auto.error = Some(err.to_string());
                return;
            }
            self.auto.queue = Some(queue);
            self.auto.delay_draft = None;
            self.auto.delays_open = false;
            self.auto.error = None;
            if !self.auto.running {
                self.auto.status = Some("Saved delays.".into());
            }
        } else if cancel || !open {
            self.auto.delay_draft = None;
            self.auto.delays_open = false;
        }
    }

    fn open_skip_editor(&mut self) {
        match load_queue() {
            Ok(Some(queue)) => {
                self.auto.skip_draft = Some(queue);
                self.auto.skips_open = true;
                self.auto.error = None;
            }
            Ok(None) => self.auto.error = Some("No saved queue yet.".into()),
            Err(err) => self.auto.error = Some(err.to_string()),
        }
    }

    fn skips_window(&mut self, ctx: &egui::Context) {
        if !self.auto.skips_open {
            return;
        }
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        egui::Window::new("Add skips")
            .id(egui::Id::new("add-skips-edge"))
            .open(&mut open)
            .auto_sized()
            .resizable(false)
            .collapsible(false)
            .frame(
                egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin {
                    left: 12,
                    right: 0,
                    top: 8,
                    bottom: 8,
                }),
            )
            .show(ctx, |ui| {
                let Some(queue) = self.auto.skip_draft.as_mut() else {
                    ui.label(egui::RichText::new("No queue yet.").size(14.0).color(MUTED));
                    return;
                };
                {
                    let scroll = &mut ui.style_mut().spacing.scroll;
                    scroll.floating = false;
                    scroll.bar_width = 8.0;
                    scroll.bar_inner_margin = 0.0;
                    scroll.bar_outer_margin = 0.0;
                }
                let font = egui::FontId::proportional(14.0);
                let text_width =
                    queue
                        .entries
                        .iter()
                        .enumerate()
                        .fold(0.0_f32, |widest, (index, entry)| {
                            let label = format!("{}. {}", index + 1, entry.name);
                            let width = ui
                                .painter()
                                .layout_no_wrap(label, font.clone(), TEXT)
                                .size()
                                .x;
                            widest.max(width)
                        });
                let width = (text_width + 40.0).clamp(220.0, 560.0);
                ui.set_min_width(width);
                ui.set_max_width(width);
                ui.label(
                    egui::RichText::new(format!("{} · Title {}", queue.gamertag, queue.title_id))
                        .size(13.0)
                        .color(MUTED),
                );
                ui.add_space(8.0);
                let list_height = (queue.entries.len().max(1) as f32 * 26.0).min(420.0);
                egui::ScrollArea::vertical()
                    .max_height(list_height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (index, entry) in queue.entries.iter_mut().enumerate() {
                            let label = format!("{}. {}", index + 1, entry.name);
                            let _ = ui.checkbox(&mut entry.skip, label);
                        }
                    });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if tool_button(ui, "Save", true).clicked() {
                        save = true;
                    }
                    if tool_button(ui, "Cancel", true).clicked() {
                        cancel = true;
                    }
                });
            });
        if save {
            let Some(queue) = self.auto.skip_draft.clone() else {
                self.auto.skips_open = false;
                return;
            };
            if let Err(err) = save_queue(&queue) {
                self.auto.error = Some(err.to_string());
                return;
            }
            self.auto.queue = Some(queue);
            self.auto.skip_draft = None;
            self.auto.skips_open = false;
            self.auto.error = None;
            if !self.auto.running {
                self.auto.status = Some("Saved skips.".into());
            }
        } else if cancel || !open {
            self.auto.skip_draft = None;
            self.auto.skips_open = false;
        }
    }

    fn fetch_reference(&mut self, ctx: egui::Context) {
        let gamertag = self.auto.gamertag.trim().to_owned();
        if self.auto.fetching {
            return;
        }
        if gamertag.is_empty() {
            self.auto.error = Some("Enter a gamertag.".into());
            return;
        }
        let Some(title_id) = parse_title_id(&self.auto.title_id) else {
            self.auto.error = Some("Enter a Title ID.".into());
            return;
        };
        let Some(xuid) = self.signed_in_xuid() else {
            self.auto.error = Some("Sign in on Home first.".into());
            return;
        };
        let Some(authorization) = self.xbox.as_ref().map(|xbox| xbox.authorization.clone()) else {
            self.auto.error = Some("Xbox authorization is not ready.".into());
            return;
        };
        let language = crate::xbox::titles::accept_language(self.config.force_region);
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.auto.fetching = true;
        self.auto.error = None;
        self.auto.status = None;
        self.auto.fetch_rx = Some(rx);
        self.runtime.spawn(async move {
            let result = async {
                let reference_xuid =
                    crate::xbox::profile::xuid_for_gamertag(&authorization, &gamertag).await?;
                let reference = crate::xbox::achievements::fetch_title_achievements(
                    &authorization,
                    &reference_xuid,
                    title_id,
                    &language,
                )
                .await?;
                let local = crate::xbox::achievements::fetch_title_achievements(
                    &authorization,
                    &xuid,
                    title_id,
                    &language,
                )
                .await?;
                let queue = build_queue(title_id, &gamertag, &local, &reference);
                if queue.entries.is_empty() {
                    return Err(crate::error::Error::Xbox(
                        "No unlock times to copy for achievements you still have locked.".into(),
                    ));
                }
                save_queue(&queue)?;
                Ok(queue)
            }
            .await
            .map_err(|err| err.to_string());
            let _ = tx.send((title_id, result));
            ctx.request_repaint();
        });
    }

    fn poll_reference(&mut self) {
        let ready = self
            .auto
            .fetch_rx
            .as_mut()
            .and_then(|rx| match rx.try_recv() {
                Ok(update) => Some(update),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => None,
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    Some((0, Err("queue request was dropped".into())))
                }
            });
        let Some((title_id, result)) = ready else {
            return;
        };
        self.auto.fetch_rx = None;
        self.auto.fetching = false;
        if parse_title_id(&self.auto.title_id) != Some(title_id) {
            return;
        }
        match result {
            Ok(queue) => {
                let count = queue.entries.len();
                self.auto.gamertag = queue.gamertag.clone();
                self.auto.queue = Some(queue);
                self.auto.error = None;
                self.auto.reference_open = false;
                if !self.auto.running {
                    self.auto.status = Some(format!("Saved {count} unlocks to queue.json"));
                }
            }
            Err(err) => self.auto.error = Some(err),
        }
    }

    fn start_auto_unlock(&mut self, ctx: egui::Context) {
        if self.auto.running {
            return;
        }
        let Some(queue) = self.auto.queue.clone() else {
            self.auto.error = Some("Build a queue first.".into());
            return;
        };
        if !queue.pending() {
            self.auto.error = Some("Nothing left to unlock.".into());
            return;
        }
        let service_config_id = self
            .achievements_page
            .board
            .as_ref()
            .filter(|_| self.current_title_id() == Some(queue.title_id))
            .and_then(|board| board.service_config_id.clone())
            .filter(|id| !id.trim().is_empty());
        let Some(xuid) = self.signed_in_xuid() else {
            self.auto.error = Some("Sign in on Home to unlock achievements.".into());
            return;
        };
        let Some(authorization) = self.xbox.as_ref().map(|xbox| xbox.authorization.clone()) else {
            self.auto.error = Some("Xbox authorization is not ready.".into());
            return;
        };
        let language = crate::xbox::titles::accept_language(self.config.force_region);
        let delays = self.config.randomize_delays.clone();
        let use_randomized = self.config.use_randomized_delays;
        self.begin_spoof(ctx.clone(), queue.title_id);
        if let Some(task) = self.auto.task.take() {
            task.abort();
        }
        let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        let (note_tx, note_rx) = tokio::sync::mpsc::unbounded_channel();
        self.auto.stop = Some(stop_tx);
        self.auto.notes = Some(note_rx);
        self.auto.running = true;
        self.auto.spoof_halted = false;
        self.auto.error = None;
        self.auto.status = Some("Spoofing this game.".into());
        self.auto.task = Some(self.runtime.spawn(async move {
            let service_config_id = match service_config_id {
                Some(id) => id,
                None => {
                    match crate::xbox::achievements::fetch_title_achievements(
                        &authorization,
                        &xuid,
                        queue.title_id,
                        &language,
                    )
                    .await
                    {
                        Ok(board) => {
                            match board.service_config_id.filter(|id| !id.trim().is_empty()) {
                                Some(id) => id,
                                None => {
                                    let _ = note_tx.send(AutoNote::Failed {
                                        id: String::new(),
                                        message: "This title has no service config id.".into(),
                                    });
                                    ctx.request_repaint();
                                    return;
                                }
                            }
                        }
                        Err(err) => {
                            let _ = note_tx.send(AutoNote::Failed {
                                id: String::new(),
                                message: err.to_string(),
                            });
                            ctx.request_repaint();
                            return;
                        }
                    }
                }
            };
            run_queue(
                queue,
                delays,
                use_randomized,
                authorization,
                xuid,
                service_config_id,
                stop_rx,
                note_tx,
                ctx,
            )
            .await;
        }));
    }

    fn apply_auto_note(&mut self, note: AutoNote) {
        match note {
            AutoNote::Waiting { name, seconds } => {
                self.auto.status = Some(format!("Next: {name} in {seconds}s"));
            }
            AutoNote::Rolled { id, seconds } => {
                if let Some(entry) = self
                    .auto
                    .queue
                    .as_mut()
                    .and_then(|queue| queue.entries.iter_mut().find(|entry| entry.id == id))
                {
                    entry.rolled_delay = Some(seconds);
                }
            }
            AutoNote::Unlocked(id) => {
                if let Some(queue) = &mut self.auto.queue {
                    if let Some(entry) = queue.entries.iter_mut().find(|entry| entry.id == id) {
                        entry.done = true;
                        self.auto.status = Some(format!("Unlocked {}", entry.name));
                    }
                }
                self.mark_unlocked(&id);
                if let Err(err) = self.persist_queue() {
                    self.auto.error = Some(err);
                }
            }
            AutoNote::Failed { id, message } => {
                self.auto.running = false;
                self.auto.status = None;
                let name = self
                    .auto
                    .queue
                    .as_ref()
                    .and_then(|queue| queue.entries.iter().find(|entry| entry.id == id))
                    .map(|entry| entry.name.clone())
                    .unwrap_or(id);
                self.auto.error = Some(format!("{name}: {message}"));
                let _ = self.persist_queue();
            }
            AutoNote::Finished => {
                self.auto.running = false;
                self.auto.status = None;
                if !self.auto.spoof_halted {
                    self.auto.error = None;
                }
                self.auto.spoof_halted = false;
            }
        }
    }

    fn persist_config(&mut self) {
        self.save_error = self.config.save().err().map(|err| err.to_string());
        if let Some(err) = &self.save_error {
            tracing::error!(error = %err, "failed to save config");
        }
    }

    fn persist_queue(&self) -> Result<(), String> {
        let Some(queue) = &self.auto.queue else {
            return Ok(());
        };
        save_queue(queue).map_err(|err| err.to_string())
    }

    fn current_title_id(&self) -> Option<u64> {
        self.achievements_page
            .selected
            .as_ref()
            .and_then(|title| title.title_id.as_deref())
            .and_then(parse_title_id)
    }
}

async fn run_queue(
    queue: UnlockQueue,
    delays: crate::config::DelayRandomize,
    use_randomized: bool,
    authorization: String,
    xuid: String,
    service_config_id: String,
    mut stop: tokio::sync::watch::Receiver<bool>,
    notes: tokio::sync::mpsc::UnboundedSender<AutoNote>,
    ctx: egui::Context,
) {
    for entry in queue
        .entries
        .into_iter()
        .filter(|entry| !entry.done && !entry.skip)
    {
        if *stop.borrow() {
            break;
        }
        let wait = if use_randomized {
            entry.rolled_delay.unwrap_or_else(|| {
                let seconds = crate::config::randomize_delay(entry.delay_seconds, &delays);
                let _ = notes.send(AutoNote::Rolled {
                    id: entry.id.clone(),
                    seconds,
                });
                ctx.request_repaint();
                seconds
            })
        } else {
            entry.delay_seconds
        };
        if wait > 0 {
            let _ = notes.send(AutoNote::Waiting {
                name: entry.name.clone(),
                seconds: wait,
            });
            ctx.request_repaint();
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(wait)) => {}
                _ = stop.changed() => {}
            }
            if *stop.borrow() {
                break;
            }
        }
        let status = crate::xbox::achievements::update_achievement_progress(
            &authorization,
            &xuid,
            queue.title_id,
            &service_config_id,
            &entry.id,
            100,
        )
        .await;
        match status {
            Ok(ProgressStatus::Updated) => {
                let _ = notes.send(AutoNote::Unlocked(entry.id));
            }
            Ok(ProgressStatus::Unauthorized) => {
                let _ = notes.send(AutoNote::Failed {
                    id: entry.id,
                    message: "Xbox authorization was rejected.".into(),
                });
                ctx.request_repaint();
                return;
            }
            Ok(ProgressStatus::Forbidden) => {
                let _ = notes.send(AutoNote::Failed {
                    id: entry.id,
                    message: "Xbox refused this achievement update.".into(),
                });
                ctx.request_repaint();
                return;
            }
            Ok(ProgressStatus::Rejected(message)) => {
                let _ = notes.send(AutoNote::Failed {
                    id: entry.id,
                    message,
                });
                ctx.request_repaint();
                return;
            }
            Err(err) => {
                let _ = notes.send(AutoNote::Failed {
                    id: entry.id,
                    message: err.to_string(),
                });
                ctx.request_repaint();
                return;
            }
        }
        ctx.request_repaint();
    }
    let _ = notes.send(AutoNote::Finished);
    ctx.request_repaint();
}

fn queue_row(
    ui: &mut egui::Ui,
    width: f32,
    name: &str,
    time: &str,
    randomized: Option<&str>,
    skip: bool,
    done: bool,
) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 52.0), egui::Sense::hover());
    let (fill, name_color, time_color) = if done {
        (
            Color32::from_rgb(24, 52, 36),
            Color32::from_rgb(146, 220, 164),
            Color32::from_rgb(146, 220, 164),
        )
    } else if skip {
        (Color32::from_rgb(22, 23, 27), MUTED, MUTED)
    } else {
        (Color32::from_rgb(26, 28, 34), TEXT, TEXT)
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(10), fill);
    if done {
        let bar = egui::Rect::from_min_max(
            rect.left_top() + egui::vec2(0.0, 10.0),
            egui::pos2(rect.left() + 3.0, rect.bottom() - 10.0),
        );
        ui.painter().rect_filled(
            bar,
            egui::CornerRadius::same(2),
            Color32::from_rgb(72, 168, 104),
        );
    }
    let inner = rect.shrink2(egui::vec2(16.0, 0.0));
    let time_galley = ui
        .painter()
        .layout_job(egui::text::LayoutJob::single_section(
            time.to_owned(),
            egui::TextFormat {
                font_id: egui::FontId::monospace(14.0),
                color: if randomized.is_some() {
                    MUTED
                } else {
                    time_color
                },
                strikethrough: if randomized.is_some() {
                    egui::Stroke::new(1.0, MUTED)
                } else {
                    egui::Stroke::NONE
                },
                ..Default::default()
            },
        ));
    let randomized_galley = randomized.map(|label| {
        ui.painter()
            .layout_no_wrap(label.to_owned(), egui::FontId::monospace(14.0), time_color)
    });
    let randomized_width = randomized_galley
        .as_ref()
        .map(|galley| galley.size().x + 10.0)
        .unwrap_or(0.0);
    let time_width = time_galley.size().x + randomized_width;
    let name_width = (inner.width() - time_width - 16.0).max(24.0);
    let mut name_job = egui::text::LayoutJob::single_section(
        name.replace(['\n', '\r'], " "),
        egui::TextFormat {
            font_id: egui::FontId::proportional(14.5),
            color: name_color,
            ..Default::default()
        },
    );
    name_job.wrap.max_width = name_width;
    name_job.wrap.max_rows = 1;
    name_job.wrap.break_anywhere = true;
    let name_galley = ui.painter().layout_job(name_job);
    ui.painter().galley(
        egui::pos2(inner.left(), inner.center().y - name_galley.size().y / 2.0),
        name_galley,
        name_color,
    );
    let time_x = inner.right() - time_width;
    ui.painter().galley(
        egui::pos2(time_x, inner.center().y - time_galley.size().y / 2.0),
        time_galley,
        if randomized.is_some() {
            MUTED
        } else {
            time_color
        },
    );
    if let Some(galley) = randomized_galley {
        ui.painter().galley(
            egui::pos2(
                inner.right() - galley.size().x,
                inner.center().y - galley.size().y / 2.0,
            ),
            galley,
            time_color,
        );
    }
}

fn format_hms(total_seconds: u64) -> String {
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

fn tool_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> egui::Response {
    let color = if enabled { TEXT } else { MUTED };
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), egui::FontId::proportional(13.0), color);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(galley.size().x + 22.0, 32.0),
        if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    let fill = if !enabled {
        Color32::from_rgb(38, 41, 48)
    } else if response.hovered() {
        ACCENT.gamma_multiply(1.12)
    } else {
        ACCENT
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(8), fill);
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        color,
    );
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}
