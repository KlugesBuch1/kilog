use eframe::egui::{self, Color32};
use egui_lucide::Lucide;

use super::KilogApp;
use super::theme::{ACCENT, MUTED, TEXT};
use crate::xbox::titles::{CatalogPage, Title, TitleLookup, single_title_id};

const GAP: f32 = 12.0;
const BATCH: usize = 8;
const COLS: usize = 4;

enum SearchOutcome {
    Exact(Result<TitleLookup, String>),
    Page {
        offset: usize,
        result: Result<CatalogPage, String>,
    },
}

pub(super) struct TitleSearch {
    pub query: String,
    seen: String,
    searched: String,
    results: Vec<Title>,
    without_cover: Vec<Title>,
    fetched: usize,
    shown: usize,
    total: usize,
    exact: bool,
    pending: bool,
    loading_more: bool,
    missing: bool,
    error: Option<String>,
    rx: Option<tokio::sync::oneshot::Receiver<(u64, SearchOutcome)>>,
    task: Option<tokio::task::JoinHandle<()>>,
    epoch: u64,
    copied_id: Option<String>,
}

impl TitleSearch {
    pub(super) fn new() -> Self {
        Self {
            query: String::new(),
            seen: String::new(),
            searched: String::new(),
            results: Vec::new(),
            without_cover: Vec::new(),
            fetched: 0,
            shown: BATCH,
            total: 0,
            exact: false,
            pending: false,
            loading_more: false,
            missing: false,
            error: None,
            rx: None,
            task: None,
            epoch: 0,
            copied_id: None,
        }
    }

    pub(super) fn clear(&mut self) {
        self.cancel();
        self.query.clear();
        self.seen.clear();
        self.searched.clear();
        self.results.clear();
        self.without_cover.clear();
        self.fetched = 0;
        self.shown = BATCH;
        self.total = 0;
        self.exact = false;
        self.missing = false;
        self.error = None;
        self.copied_id = None;
    }

    fn cancel(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.rx = None;
        self.pending = false;
        self.loading_more = false;
        self.epoch = self.epoch.wrapping_add(1);
    }

    fn has_more(&self) -> bool {
        !self.exact && self.total > 0 && self.fetched < self.total
    }

    fn can_reveal_more(&self) -> bool {
        if self.exact {
            return false;
        }
        self.shown < self.results.len() || self.fetched < self.total
    }
}

impl KilogApp {
    pub(super) fn title_search_page(&mut self, ui: &mut egui::Ui) {
        let mut submit = false;
        let mut more = false;
        ui.horizontal(|ui| {
            let field = super::theme::search_field(
                ui,
                &mut self.title_search.query,
                "Title ID or name",
                440.0,
            );
            let enter = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            if enter || search_button(ui, "Search").clicked() {
                submit = true;
            }
        });
        ui.add_space(8.0);

        if self.title_search.query != self.title_search.seen {
            self.title_search.seen = self.title_search.query.clone();
            self.title_search.copied_id = None;
            self.title_search.searched.clear();
            self.title_search.results.clear();
            self.title_search.without_cover.clear();
            self.title_search.fetched = 0;
            self.title_search.shown = BATCH;
            self.title_search.total = 0;
            self.title_search.exact = false;
            self.title_search.missing = false;
            self.title_search.error = None;
            self.title_search.cancel();
        }

        if submit {
            let ctx = ui.ctx().clone();
            self.submit_title_search(ctx);
        }

        let query = self.title_search.query.trim().to_owned();
        if query.is_empty() {
            ui.label(
                egui::RichText::new("Search Xbox Live by name or Title ID.")
                    .size(15.0)
                    .color(MUTED),
            );
            return;
        }
        if self.title_search.searched != query {
            ui.label(
                egui::RichText::new("Press Enter to search.")
                    .size(15.0)
                    .color(MUTED),
            );
            return;
        }

        if self.title_search.pending && !self.title_search.loading_more {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    egui::RichText::new("Searching Xbox Live…")
                        .size(14.0)
                        .color(MUTED),
                );
            });
            ui.ctx().request_repaint();
            return;
        }

        if let Some(err) = &self.title_search.error {
            ui.label(
                egui::RichText::new(err)
                    .size(14.0)
                    .color(Color32::from_rgb(232, 120, 128)),
            );
            ui.add_space(8.0);
        }

        if self.title_search.missing && self.title_search.results.is_empty() {
            ui.label(
                egui::RichText::new("Title not found")
                    .size(15.0)
                    .color(MUTED),
            );
            return;
        }

        let results = self.title_search.results.clone();
        let count = if self.title_search.exact {
            results.len()
        } else {
            self.title_search.shown.min(results.len())
        };
        let mut copied = self.title_search.copied_id.clone();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                paint_result_grid(ui, &results[..count], &mut copied);
                ui.add_space(GAP);
                if self.title_search.pending && self.title_search.loading_more {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(egui::RichText::new("Loading more…").size(14.0).color(MUTED));
                    });
                    ui.ctx().request_repaint();
                } else if self.title_search.can_reveal_more() && search_button(ui, "More").clicked()
                {
                    more = true;
                }
                if self.title_search.total > count {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(format!("{count} of {}", self.title_search.total))
                            .size(13.0)
                            .color(MUTED),
                    );
                }
            });
        if copied != self.title_search.copied_id {
            self.title_search.copied_id = copied;
        }
        if more {
            let ctx = ui.ctx().clone();
            self.load_more_titles(ctx);
        }
    }

    pub(super) fn poll_title_search(&mut self, ctx: egui::Context) {
        let ready = self
            .title_search
            .rx
            .as_mut()
            .and_then(|rx| match rx.try_recv() {
                Ok(update) => Some(update),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => None,
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => None,
            });
        let Some((epoch, outcome)) = ready else {
            return;
        };
        self.title_search.rx = None;
        self.title_search.task = None;
        if epoch != self.title_search.epoch {
            return;
        }
        self.title_search.pending = false;
        self.title_search.loading_more = false;
        match outcome {
            SearchOutcome::Exact(result) => self.apply_exact(result),
            SearchOutcome::Page { offset, result } => {
                self.apply_page(offset, result);
                self.fill_shown_batch(ctx);
            }
        }
    }

    fn apply_exact(&mut self, result: Result<TitleLookup, String>) {
        match result {
            Ok(lookup) => {
                self.title_search.results = lookup.titles;
                self.title_search.total = self.title_search.results.len();
                self.title_search.missing = self.title_search.results.is_empty();
                self.title_search.error = None;
            }
            Err(err) => {
                self.title_search.results.clear();
                self.title_search.total = 0;
                self.title_search.missing = false;
                self.title_search.error = Some(err);
            }
        }
    }

    fn apply_page(&mut self, offset: usize, result: Result<CatalogPage, String>) {
        match result {
            Ok(page) => {
                if offset == 0 {
                    self.title_search.results.clear();
                    self.title_search.without_cover.clear();
                    self.title_search.fetched = 0;
                }
                if page.titles.is_empty() {
                    self.title_search.fetched =
                        self.title_search.total.max(self.title_search.fetched);
                    self.finish_catalog();
                    return;
                }
                self.title_search.fetched += page.titles.len();
                for title in page.titles {
                    if title.cover_url().is_some() {
                        self.title_search.results.push(title);
                    } else {
                        self.title_search.without_cover.push(title);
                    }
                }
                self.title_search.total = page.total.max(self.title_search.fetched);
                if self.title_search.fetched >= self.title_search.total {
                    self.finish_catalog();
                } else {
                    self.title_search.missing = false;
                    self.title_search.error = None;
                }
            }
            Err(err) => {
                if offset == 0 {
                    self.title_search.results.clear();
                    self.title_search.without_cover.clear();
                    self.title_search.fetched = 0;
                    self.title_search.total = 0;
                }
                self.title_search.missing = false;
                self.title_search.error = Some(err);
            }
        }
    }

    fn finish_catalog(&mut self) {
        let held = std::mem::take(&mut self.title_search.without_cover);
        self.title_search.results.extend(held);
        if self.title_search.total < self.title_search.fetched {
            self.title_search.total = self.title_search.fetched;
        }
        self.title_search.missing = self.title_search.results.is_empty();
        self.title_search.error = None;
    }

    fn submit_title_search(&mut self, ctx: egui::Context) {
        let query = self.title_search.query.trim().to_owned();
        if query.is_empty() {
            return;
        }
        self.title_search.copied_id = None;
        self.title_search.searched = query.clone();
        self.title_search.results.clear();
        self.title_search.without_cover.clear();
        self.title_search.fetched = 0;
        self.title_search.shown = BATCH;
        self.title_search.total = 0;
        self.title_search.missing = false;
        self.title_search.error = None;
        if let Some(title_id) = single_title_id(&query) {
            self.title_search.exact = true;
            if self.signed_in_xuid().is_none() {
                self.title_search.error = Some("Sign in on Home to look up a Title ID.".into());
                return;
            }
            self.spawn_exact_lookup(ctx, title_id);
        } else {
            self.title_search.exact = false;
            self.spawn_catalog_page(ctx, query, 0);
        }
    }

    fn load_more_titles(&mut self, ctx: egui::Context) {
        if self.title_search.pending {
            return;
        }
        self.title_search.shown += BATCH;
        self.fill_shown_batch(ctx);
    }

    fn fill_shown_batch(&mut self, ctx: egui::Context) {
        if self.title_search.exact || self.title_search.pending || self.title_search.error.is_some()
        {
            return;
        }
        if self.title_search.results.len() >= self.title_search.shown {
            return;
        }
        if !self.title_search.has_more() {
            return;
        }
        let query = self.title_search.searched.clone();
        let offset = self.title_search.fetched;
        self.spawn_catalog_page(ctx, query, offset);
    }

    fn spawn_exact_lookup(&mut self, ctx: egui::Context, title_id: u64) {
        let Some(authorization) = self.xbox.as_ref().map(|xbox| xbox.authorization.clone()) else {
            return;
        };
        let (epoch, tx) = self.arm_request(false);
        let language = crate::xbox::titles::accept_language(self.config.force_region);
        self.title_search.task = Some(self.runtime.spawn(async move {
            let result = crate::xbox::titles::lookup_titles(&authorization, &[title_id], &language)
                .await
                .map_err(|err| err.to_string());
            let _ = tx.send((epoch, SearchOutcome::Exact(result)));
            ctx.request_repaint();
        }));
    }

    fn spawn_catalog_page(&mut self, ctx: egui::Context, query: String, offset: usize) {
        let authorization = self.xbox.as_ref().map(|xbox| xbox.authorization.clone());
        let language = crate::xbox::titles::accept_language(self.config.force_region);
        let (epoch, tx) = self.arm_request(offset > 0);
        self.title_search.task = Some(self.runtime.spawn(async move {
            let result = crate::xbox::titles::search_catalog_with_art(
                &query,
                offset,
                authorization.as_deref(),
                &language,
            )
            .await
            .map_err(|err| err.to_string());
            let _ = tx.send((epoch, SearchOutcome::Page { offset, result }));
            ctx.request_repaint();
        }));
    }

    fn arm_request(
        &mut self,
        loading_more: bool,
    ) -> (u64, tokio::sync::oneshot::Sender<(u64, SearchOutcome)>) {
        self.title_search.cancel();
        self.title_search.epoch = self.title_search.epoch.wrapping_add(1);
        let epoch = self.title_search.epoch;
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.title_search.rx = Some(rx);
        self.title_search.pending = true;
        self.title_search.loading_more = loading_more;
        (epoch, tx)
    }
}

fn paint_result_grid(ui: &mut egui::Ui, results: &[Title], copied: &mut Option<String>) {
    if results.is_empty() {
        return;
    }
    let width = ui.available_width();
    let cols = COLS;
    let card_w = (width - GAP * (cols.saturating_sub(1) as f32)) / cols as f32;
    let card_h = card_w + 128.0;
    let rows = results.len().div_ceil(cols);
    ui.spacing_mut().item_spacing.y = GAP;
    for row in 0..rows {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for col in 0..cols {
                let index = row * cols + col;
                if let Some(title) = results.get(index) {
                    result_card(ui, title, index, card_w, card_h, copied);
                }
            }
        });
    }
}

fn result_card(
    ui: &mut egui::Ui,
    title: &Title,
    index: usize,
    card_w: f32,
    card_h: f32,
    copied: &mut Option<String>,
) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(card_w, card_h), egui::Sense::hover());
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
        egui::Image::from_uri(url)
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

    let name = title
        .name
        .as_deref()
        .filter(|name| !name.is_empty())
        .unwrap_or("Unknown");
    let name_rect = egui::Rect::from_min_size(
        inner.min + egui::vec2(0.0, cover + 8.0),
        egui::vec2(cover, 32.0),
    );
    let galley = ui.painter().layout(
        name.to_owned(),
        egui::FontId::proportional(13.0),
        TEXT,
        name_rect.width(),
    );
    ui.painter_at(name_rect).galley(name_rect.min, galley, TEXT);

    let id = title.title_id.as_deref().filter(|id| !id.is_empty());
    let id_rect = egui::Rect::from_min_size(
        inner.min + egui::vec2(0.0, cover + 40.0),
        egui::vec2(cover, 16.0),
    );
    ui.painter().text(
        id_rect.left_center(),
        egui::Align2::LEFT_CENTER,
        id.unwrap_or("—"),
        egui::FontId::proportional(12.0),
        MUTED,
    );

    let button = egui::Rect::from_min_size(
        inner.min + egui::vec2(0.0, cover + 60.0),
        egui::vec2(cover, 24.0),
    );
    let copy = ui.interact(
        button,
        ui.id().with(("title-copy", index)),
        egui::Sense::click(),
    );
    let copied_this = id.is_some_and(|value| copied.as_deref() == Some(value));
    let button_fill = if copy.hovered() {
        ACCENT.gamma_multiply(1.12)
    } else {
        ACCENT
    };
    ui.painter()
        .rect_filled(button, egui::CornerRadius::same(6), button_fill);
    ui.painter().text(
        button.center(),
        egui::Align2::CENTER_CENTER,
        if copied_this {
            "Copied"
        } else {
            "Copy Title ID"
        },
        egui::FontId::proportional(12.0),
        TEXT,
    );
    if copy.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if copy.clicked()
        && let Some(id) = id
    {
        ui.ctx().copy_text(id.to_owned());
        *copied = Some(id.to_owned());
    }

    let tags = egui::Rect::from_min_size(
        inner.min + egui::vec2(0.0, cover + 92.0),
        egui::vec2(cover, 20.0),
    );
    let mut x = tags.left();
    for device in &title.devices {
        let galley =
            ui.painter()
                .layout_no_wrap(device.clone(), egui::FontId::proportional(11.0), TEXT);
        let chip_w = galley.size().x + 12.0;
        if x > tags.left() && x + chip_w > tags.right() {
            break;
        }
        let chip = egui::Rect::from_min_size(egui::pos2(x, tags.top()), egui::vec2(chip_w, 18.0));
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

fn search_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let font = egui::FontId::proportional(14.0);
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, TEXT);
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
        .rect_filled(rect, egui::CornerRadius::same(8), fill);
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
