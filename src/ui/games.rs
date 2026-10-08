use eframe::egui::{self, Color32};
use egui_lucide::Lucide;

use super::KilogApp;
use super::home::AuthState;
use super::theme::{ACCENT, MUTED, TEXT};
use crate::xbox::titles::{GameFilter, Title, TitlesList};

const MIN_CARD_W: f32 = 188.0;
const GAP: f32 = 12.0;
const MOCK_AUTHORIZATION: &str = "XBL3.0 x=dev;mock";

impl KilogApp {
    pub(super) fn games_page(&mut self, ui: &mut egui::Ui) {
        let mut refresh = false;
        ui.horizontal(|ui| {
            super::theme::search_field(ui, &mut self.games_search, "Search for a game", 320.0);
            egui::ComboBox::from_id_salt("game_filter")
                .selected_text(self.games_filter.label())
                .width(180.0)
                .show_ui(ui, |ui| {
                    for filter in GameFilter::ALL {
                        ui.selectable_value(&mut self.games_filter, filter, filter.label());
                    }
                });
            if self.can_fetch_titles() && toolbar_button(ui, "Refresh").clicked() {
                refresh = true;
            }
            if let Some(total) = self.titles.as_ref().map(|list| list.titles.len()) {
                ui.label(
                    egui::RichText::new(format!("Search {total} Games"))
                        .size(13.0)
                        .color(MUTED),
                );
            }
        });
        ui.add_space(16.0);

        if refresh {
            let ctx = ui.ctx().clone();
            self.refresh_titles(ctx);
        }

        if self.titles_rx.is_some() && self.titles.is_none() {
            ui.ctx().request_repaint();
            ui.add_space(48.0);
            ui.vertical_centered(|ui| {
                ui.spinner();
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new("Loading games…")
                        .size(15.0)
                        .color(MUTED),
                );
            });
            return;
        }

        if let Some(err) = self.titles_error.clone() {
            ui.label(
                egui::RichText::new(err)
                    .size(14.0)
                    .color(Color32::from_rgb(232, 120, 128)),
            );
            ui.add_space(10.0);
            if toolbar_button(ui, "Retry").clicked() {
                let ctx = ui.ctx().clone();
                self.refresh_titles(ctx);
            }
            return;
        }

        if self.titles.is_none() {
            let message = self.games_placeholder();
            ui.label(egui::RichText::new(message).size(15.0).color(MUTED));
            return;
        }

        let filter = self.games_filter;
        let search = self.games_search.to_lowercase();
        let Some(list) = &self.titles else {
            return;
        };
        let opened = paint_grid(ui, list, filter, &search);
        if let Some(title) = opened {
            let ctx = ui.ctx().clone();
            self.open_game_achievements(ctx, title);
        }
    }

    fn can_fetch_titles(&self) -> bool {
        self.signed_in_xuid().is_some()
    }

    pub(super) fn signed_in_xuid(&self) -> Option<String> {
        if self
            .xbox
            .as_ref()
            .is_some_and(|xbox| xbox.authorization == MOCK_AUTHORIZATION)
        {
            return None;
        }
        let AuthState::Authenticated { profile } = &self.auth else {
            return None;
        };
        profile.xuid.clone().filter(|xuid| !xuid.trim().is_empty())
    }

    fn games_placeholder(&self) -> &'static str {
        if self
            .xbox
            .as_ref()
            .is_some_and(|xbox| xbox.authorization == MOCK_AUTHORIZATION)
        {
            return "Developer preview does not load Xbox Live title history.";
        }
        if !matches!(self.auth, AuthState::Authenticated { .. }) {
            return "Sign in on Home to load your games.";
        }
        "Xbox profile has no XUID."
    }

    pub(super) fn ensure_titles(&mut self, ctx: egui::Context) {
        if self.titles_rx.is_some() || self.titles.is_some() || self.titles_error.is_some() {
            return;
        }
        let Some(xuid) = self.signed_in_xuid() else {
            return;
        };
        let Some(authorization) = self.xbox.as_ref().map(|xbox| xbox.authorization.clone()) else {
            return;
        };
        self.spawn_titles(ctx, authorization, xuid);
    }

    pub(super) fn refresh_titles(&mut self, ctx: egui::Context) {
        self.clear_titles();
        self.ensure_titles(ctx);
    }

    pub(super) fn clear_titles(&mut self) {
        self.titles_epoch = self.titles_epoch.wrapping_add(1);
        self.titles_rx = None;
        self.titles = None;
        self.titles_error = None;
    }

    fn spawn_titles(&mut self, ctx: egui::Context, authorization: String, xuid: String) {
        self.titles_epoch = self.titles_epoch.wrapping_add(1);
        let epoch = self.titles_epoch;
        let language = crate::xbox::titles::accept_language(self.config.force_region);
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.titles_rx = Some(rx);
        self.runtime.spawn(async move {
            let result = crate::xbox::titles::fetch_title_history(&authorization, &xuid, &language)
                .await
                .map_err(|err| err.to_string());
            let _ = tx.send((epoch, result));
            ctx.request_repaint();
        });
    }

    pub(super) fn poll_titles(&mut self) {
        let epoch_now = self.titles_epoch;
        let ready = self.titles_rx.as_mut().and_then(|rx| match rx.try_recv() {
            Ok(update) => Some(update),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => None,
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                Some((epoch_now, Err("title history request was dropped".into())))
            }
        });
        let Some((epoch, result)) = ready else {
            return;
        };
        self.titles_rx = None;
        if epoch != self.titles_epoch {
            return;
        }
        match result {
            Ok(list) => {
                self.titles_error = None;
                self.titles = Some(list);
            }
            Err(err) => self.titles_error = Some(err),
        }
    }
}

fn paint_grid(
    ui: &mut egui::Ui,
    list: &TitlesList,
    filter: GameFilter,
    search: &str,
) -> Option<Title> {
    let matched: Vec<&Title> = list
        .titles
        .iter()
        .filter(|title| title.matches(filter, search))
        .collect();
    if matched.is_empty() {
        ui.label(
            egui::RichText::new("No games found")
                .size(15.0)
                .color(MUTED),
        );
        return None;
    }

    let width = ui.available_width();
    let cols = ((width + GAP) / (MIN_CARD_W + GAP)).floor().max(1.0) as usize;
    let card_w = (width - GAP * (cols.saturating_sub(1) as f32)) / cols as f32;
    let card_h = card_w + 96.0;
    let rows = matched.len().div_ceil(cols);
    let mut opened = None;
    ui.spacing_mut().item_spacing.y = GAP;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, card_h, rows, |ui, range| {
            for row in range {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = GAP;
                    for col in 0..cols {
                        let index = row * cols + col;
                        if let Some(title) = matched.get(index)
                            && title_card(ui, title, card_w, card_h).clicked()
                        {
                            opened = Some((*title).clone());
                        }
                    }
                });
            }
        });
    opened
}

fn title_card(ui: &mut egui::Ui, title: &Title, card_w: f32, card_h: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(card_w, card_h), egui::Sense::click());
    let fill = if response.hovered() {
        Color32::from_rgb(32, 35, 42)
    } else {
        Color32::from_rgb(26, 28, 34)
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(12), fill);

    let inner = rect.shrink(10.0);
    let cover = inner.width();
    let image = egui::Rect::from_min_size(inner.min, egui::vec2(cover, cover));
    if let Some(url) = title.cover_url() {
        egui::Image::from_uri(&url)
            .fit_to_exact_size(image.size())
            .corner_radius(10)
            .paint_at(ui, image);
    } else {
        ui.painter().rect_filled(
            image,
            egui::CornerRadius::same(10),
            Color32::from_rgb(38, 41, 48),
        );
        Lucide::Gamepad2
            .size(28.0)
            .color(MUTED)
            .stroke_width(2.0)
            .image()
            .paint_at(
                ui,
                egui::Rect::from_center_size(image.center(), egui::vec2(28.0, 28.0)),
            );
    }

    let bar = egui::Rect::from_min_size(
        inner.min + egui::vec2(0.0, cover + 8.0),
        egui::vec2(cover, 22.0),
    );
    achievement_bar(ui, bar, title);

    let name = title
        .name
        .as_deref()
        .filter(|name| !name.is_empty())
        .unwrap_or("Unknown");
    let name_rect = egui::Rect::from_min_size(
        inner.min + egui::vec2(0.0, cover + 36.0),
        egui::vec2(cover, 34.0),
    );
    let galley = ui.painter().layout(
        name.to_owned(),
        egui::FontId::proportional(13.0),
        TEXT,
        name_rect.width(),
    );
    ui.painter_at(name_rect).galley(name_rect.min, galley, TEXT);

    let tags = egui::Rect::from_min_size(
        inner.min + egui::vec2(0.0, cover + 76.0),
        egui::vec2(cover, 20.0),
    );
    platform_tags(ui, tags, &title.devices);

    let title_id = title.title_id.as_deref().unwrap_or("—");
    let pfn = title.pfn.as_deref().unwrap_or("—");
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.on_hover_text(format!("{name}\nTitle ID {title_id}\n{pfn}"))
}

fn achievement_bar(ui: &mut egui::Ui, rect: egui::Rect, title: &Title) {
    let progress = title
        .achievement
        .as_ref()
        .map(|achievement| achievement.progress_percentage)
        .unwrap_or(0.0);
    let fraction = (progress / 100.0).clamp(0.0, 1.0) as f32;
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(6),
        Color32::from_rgb(38, 41, 48),
    );
    if fraction > 0.0 {
        let mut fill = rect;
        fill.set_width((rect.width() * fraction).max(6.0));
        ui.painter()
            .rect_filled(fill, egui::CornerRadius::same(6), ACCENT);
    }

    let unlocked = title
        .achievement
        .as_ref()
        .map(|achievement| achievement.current_achievements_whole())
        .unwrap_or(0);
    let score = title
        .achievement
        .as_ref()
        .map(|achievement| achievement.gamerscore_label())
        .unwrap_or_else(|| "0/0".to_owned());
    ui.painter().text(
        rect.left_center() + egui::vec2(8.0, 0.0),
        egui::Align2::LEFT_CENTER,
        unlocked.to_string(),
        egui::FontId::proportional(12.0),
        TEXT,
    );
    ui.painter().text(
        rect.right_center() + egui::vec2(-8.0, 0.0),
        egui::Align2::RIGHT_CENTER,
        score,
        egui::FontId::proportional(12.0),
        TEXT,
    );
}

fn platform_tags(ui: &mut egui::Ui, rect: egui::Rect, devices: &[String]) {
    let mut x = rect.left();
    for device in devices {
        let galley =
            ui.painter()
                .layout_no_wrap(device.clone(), egui::FontId::proportional(11.0), TEXT);
        let chip_w = galley.size().x + 12.0;
        if x > rect.left() && x + chip_w > rect.right() {
            break;
        }
        let chip = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(chip_w, 18.0));
        ui.painter().rect_filled(
            chip,
            egui::CornerRadius::same(4),
            Color32::from_rgb(46, 49, 58),
        );
        ui.painter().galley(
            egui::pos2(x + 6.0, chip.center().y - galley.size().y / 2.0),
            galley,
            TEXT,
        );
        x += chip_w + 4.0;
    }
}

fn toolbar_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(96.0, 32.0), egui::Sense::click());
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
        egui::FontId::proportional(14.0),
        TEXT,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
