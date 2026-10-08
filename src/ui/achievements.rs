use eframe::egui::{self, Color32};
use egui_lucide::Lucide;

use super::KilogApp;
use super::theme::{ACCENT, MUTED, TEXT};
use crate::xbox::achievements::{Achievement, TitleAchievements};
use crate::xbox::titles::{Title, TitleLookup, parse_title_id, single_title_id};

const ERROR: Color32 = Color32::from_rgb(232, 120, 128);
const CARD_H: f32 = 76.0;
const CARD_GAP: f32 = 8.0;
const MIN_CARD_W: f32 = 280.0;

struct GameLoad {
    title_id: u64,
    title: Option<Box<Title>>,
    achievements: Result<TitleAchievements, String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AchievementFilter {
    All,
    Unlocked,
    Locked,
}

impl AchievementFilter {
    const ALL: [Self; 3] = [Self::All, Self::Unlocked, Self::Locked];

    fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Unlocked => "Unlocked",
            Self::Locked => "Locked",
        }
    }

    fn matches(self, achievement: &Achievement) -> bool {
        match self {
            Self::All => true,
            Self::Unlocked => achievement.is_unlocked(),
            Self::Locked => !achievement.is_unlocked(),
        }
    }

    fn empty_message(self, total: usize) -> &'static str {
        if total == 0 {
            return "No achievements for this title.";
        }
        match self {
            Self::All => "No achievements for this title.",
            Self::Unlocked => "No unlocked achievements.",
            Self::Locked => "No locked achievements.",
        }
    }
}

pub(super) struct AchievementPage {
    query: String,
    seen: String,
    searched: String,
    search_error: Option<String>,
    selected: Option<Title>,
    board: Option<TitleAchievements>,
    board_error: Option<String>,
    filter: AchievementFilter,
    list_query: String,
    loading: bool,
    rx: Option<tokio::sync::oneshot::Receiver<(u64, GameLoad)>>,
    task: Option<tokio::task::JoinHandle<()>>,
    epoch: u64,
}

impl AchievementPage {
    pub(super) fn new() -> Self {
        Self {
            query: String::new(),
            seen: String::new(),
            searched: String::new(),
            search_error: None,
            selected: None,
            board: None,
            board_error: None,
            filter: AchievementFilter::All,
            list_query: String::new(),
            loading: false,
            rx: None,
            task: None,
            epoch: 0,
        }
    }

    pub(super) fn clear(&mut self) {
        self.cancel();
        let epoch = self.epoch;
        *self = Self::new();
        self.epoch = epoch;
    }

    fn cancel(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.rx = None;
        self.loading = false;
        self.epoch = self.epoch.wrapping_add(1);
    }

    fn begin(&mut self) -> (u64, tokio::sync::oneshot::Sender<(u64, GameLoad)>) {
        self.cancel();
        self.epoch = self.epoch.wrapping_add(1);
        let epoch = self.epoch;
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.rx = Some(rx);
        self.loading = true;
        (epoch, tx)
    }
}

impl KilogApp {
    pub(super) fn achievements_page(&mut self, ui: &mut egui::Ui) {
        self.draw_achievement_search(ui);
        if self.draw_achievement_body(ui) {
            let ctx = ui.ctx().clone();
            self.retry_achievements(ctx);
        }
    }

    pub(super) fn poll_achievements(&mut self) {
        let loading = self.achievements_page.loading;
        let epoch = self.achievements_page.epoch;
        let ready = self
            .achievements_page
            .rx
            .as_mut()
            .and_then(|rx| match rx.try_recv() {
                Ok(update) => Some(Ok(update)),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => None,
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => Some(Err(())),
            });
        let Some(ready) = ready else {
            return;
        };
        self.achievements_page.rx = None;
        self.achievements_page.task = None;
        self.achievements_page.loading = false;
        match ready {
            Ok((got, load)) => {
                if got != epoch {
                    return;
                }
                self.apply_game(load.title_id, load.title, load.achievements);
            }
            Err(()) => {
                if loading {
                    self.achievements_page.board_error =
                        Some("achievement request was dropped".into());
                }
            }
        }
    }

    fn draw_achievement_search(&mut self, ui: &mut egui::Ui) {
        if self.achievements_page.loading {
            ui.ctx().request_repaint();
        }
        let mut submit = false;
        ui.horizontal(|ui| {
            let field = super::theme::search_field(
                ui,
                &mut self.achievements_page.query,
                "Title ID",
                320.0,
            );
            let enter = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            if enter || accent_button(ui, "Search").clicked() {
                submit = true;
            }
        });
        ui.add_space(8.0);

        if self.achievements_page.query != self.achievements_page.seen {
            self.note_query_edited();
        }
        if submit {
            let ctx = ui.ctx().clone();
            self.submit_achievement_search(ctx);
        }

        let query = self.achievements_page.query.trim().to_owned();
        if query.is_empty() {
            let message = if self.signed_in_xuid().is_none() {
                "Sign in on Home, then search by Title ID."
            } else {
                "Search by Title ID to open a game."
            };
            ui.label(egui::RichText::new(message).size(15.0).color(MUTED));
            return;
        }
        if self.achievements_page.searched != query {
            ui.label(
                egui::RichText::new("Press Enter to load this title.")
                    .size(15.0)
                    .color(MUTED),
            );
            return;
        }
        if let Some(err) = &self.achievements_page.search_error {
            ui.label(egui::RichText::new(err).size(14.0).color(ERROR));
        }
    }

    fn draw_achievement_body(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(title) = self.achievements_page.selected.clone() else {
            return false;
        };
        ui.add_space(16.0);
        let loading = self.achievements_page.loading;
        let error = self.achievements_page.board_error.clone();
        let filter = self.achievements_page.filter;
        let mut list_query = self.achievements_page.list_query.clone();
        let mut retry = false;
        let mut next_filter = filter;
        if let Some(board) = &self.achievements_page.board {
            next_filter = paint_board(ui, &title, board, filter, &mut list_query);
        } else {
            game_header(ui, &title, None);
            ui.add_space(12.0);
            if loading {
                loading_row(ui, "Loading achievements…");
            } else if let Some(err) = error.as_deref() {
                ui.label(egui::RichText::new(err).size(14.0).color(ERROR));
                ui.add_space(8.0);
                if accent_button(ui, "Retry").clicked() {
                    retry = true;
                }
            }
        }
        self.achievements_page.filter = next_filter;
        self.achievements_page.list_query = list_query;
        retry
    }

    fn note_query_edited(&mut self) {
        let query = self.achievements_page.query.clone();
        self.achievements_page.clear();
        self.achievements_page.query = query.clone();
        self.achievements_page.seen = query;
    }

    fn submit_achievement_search(&mut self, ctx: egui::Context) {
        let query = self.achievements_page.query.trim().to_owned();
        if query.is_empty() {
            return;
        }
        self.achievements_page.searched = query.clone();
        self.achievements_page.search_error = None;
        self.achievements_page.board = None;
        self.achievements_page.board_error = None;
        self.achievements_page.filter = AchievementFilter::All;
        self.achievements_page.list_query.clear();
        if self.signed_in_xuid().is_none() {
            self.achievements_page.cancel();
            self.achievements_page.selected = None;
            self.achievements_page.search_error =
                Some("Sign in on Home to load achievements.".into());
            return;
        }
        let Some(title_id) = single_title_id(&query) else {
            self.achievements_page.cancel();
            self.achievements_page.selected = None;
            self.achievements_page.search_error = Some("Enter a Title ID.".into());
            return;
        };
        self.achievements_page.selected = Some(Title {
            title_id: Some(title_id.to_string()),
            name: Some(format!("Title {title_id}")),
            ..Title::default()
        });
        self.spawn_game(ctx, title_id, true);
    }

    fn retry_achievements(&mut self, ctx: egui::Context) {
        let Some(title_id) = self
            .achievements_page
            .selected
            .as_ref()
            .and_then(|title| title.title_id.as_deref())
            .and_then(parse_title_id)
        else {
            return;
        };
        let lookup = self
            .achievements_page
            .selected
            .as_ref()
            .is_some_and(is_placeholder_name);
        self.achievements_page.board = None;
        self.achievements_page.board_error = None;
        self.spawn_game(ctx, title_id, lookup);
    }

    fn spawn_game(&mut self, ctx: egui::Context, title_id: u64, lookup: bool) {
        let Some(xuid) = self.signed_in_xuid() else {
            self.achievements_page.cancel();
            self.achievements_page.board_error =
                Some("Sign in on Home to load achievements.".into());
            return;
        };
        let Some(authorization) = self.xbox.as_ref().map(|xbox| xbox.authorization.clone()) else {
            self.achievements_page.cancel();
            self.achievements_page.board_error = Some("Xbox authorization is not ready.".into());
            return;
        };
        let language = crate::xbox::titles::accept_language(self.config.force_region);
        let (epoch, tx) = self.achievements_page.begin();
        self.achievements_page.task = Some(self.runtime.spawn(async move {
            let (title, achievements) = if lookup {
                let title_ids = [title_id];
                let (found, achievements) = tokio::join!(
                    crate::xbox::titles::lookup_titles(&authorization, &title_ids, &language),
                    crate::xbox::achievements::fetch_title_achievements(
                        &authorization,
                        &xuid,
                        title_id,
                        &language,
                    ),
                );
                let achievements = achievements.map_err(|err| err.to_string());
                let title = title_from_lookup(title_id, found, achievements.as_ref().ok());
                (Some(Box::new(title)), achievements)
            } else {
                let achievements = crate::xbox::achievements::fetch_title_achievements(
                    &authorization,
                    &xuid,
                    title_id,
                    &language,
                )
                .await
                .map_err(|err| err.to_string());
                (None, achievements)
            };
            let _ = tx.send((
                epoch,
                GameLoad {
                    title_id,
                    title,
                    achievements,
                },
            ));
            ctx.request_repaint();
        }));
    }

    fn apply_game(
        &mut self,
        title_id: u64,
        title: Option<Box<Title>>,
        achievements: Result<TitleAchievements, String>,
    ) {
        let current = self
            .achievements_page
            .selected
            .as_ref()
            .and_then(|title| title.title_id.as_deref())
            .and_then(parse_title_id);
        if current != Some(title_id) {
            return;
        }
        if let Some(title) = title {
            self.achievements_page.selected = Some(*title);
        }
        match achievements {
            Ok(board) => {
                if let Some(selected) = &mut self.achievements_page.selected
                    && is_placeholder_name(selected)
                    && let Some(name) = board.title_name.clone()
                {
                    selected.name = Some(name);
                }
                tracing::info!(
                    title_id,
                    count = board.achievements.len(),
                    "loaded achievements"
                );
                self.achievements_page.board = Some(board);
                self.achievements_page.board_error = None;
            }
            Err(err) => {
                self.achievements_page.board = None;
                self.achievements_page.board_error = Some(err);
            }
        }
    }
}

fn title_from_lookup(
    title_id: u64,
    lookup: Result<TitleLookup, crate::error::Error>,
    achievements: Option<&TitleAchievements>,
) -> Title {
    let mut title = lookup
        .ok()
        .and_then(|lookup| lookup.titles.into_iter().next())
        .unwrap_or_else(|| Title {
            title_id: Some(title_id.to_string()),
            ..Title::default()
        });
    if title
        .title_id
        .as_deref()
        .is_none_or(|id| id.trim().is_empty())
    {
        title.title_id = Some(title_id.to_string());
    }
    if is_placeholder_name(&title) {
        title.name = achievements
            .and_then(|board| board.title_name.clone())
            .or_else(|| Some(format!("Title {title_id}")));
    }
    title
}

fn is_placeholder_name(title: &Title) -> bool {
    let Some(name) = title
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    else {
        return true;
    };
    match title.title_id.as_deref() {
        Some(id) => name == format!("Title {id}"),
        None => false,
    }
}

fn paint_board(
    ui: &mut egui::Ui,
    title: &Title,
    board: &TitleAchievements,
    filter: AchievementFilter,
    list_query: &mut String,
) -> AchievementFilter {
    game_header(ui, title, Some(board));
    ui.add_space(10.0);
    let filter = filter_row(ui, filter, list_query);
    ui.add_space(8.0);
    achievement_list(ui, board, filter, list_query);
    filter
}

fn game_header(ui: &mut egui::Ui, title: &Title, board: Option<&TitleAchievements>) {
    let width = ui.available_width();
    egui::Frame::new()
        .fill(Color32::from_rgb(26, 28, 34))
        .corner_radius(egui::CornerRadius::same(12))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.set_width((width - 32.0).max(0.0));
            ui.horizontal_top(|ui| {
                cover(ui, title, 72.0);
                ui.add_space(14.0);
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    let name = title
                        .name
                        .as_deref()
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                        .unwrap_or("Unknown");
                    ui.label(egui::RichText::new(name).size(20.0).strong().color(TEXT));
                    if let Some(id) = title.title_id.as_deref().filter(|id| !id.is_empty()) {
                        ui.label(egui::RichText::new(id).size(13.0).color(MUTED));
                    }
                    if let Some(board) = board {
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(board.gamerscore_label())
                                    .size(15.0)
                                    .strong()
                                    .color(TEXT),
                            );
                            ui.add_space(14.0);
                            ui.label(
                                egui::RichText::new(board.unlocked_label())
                                    .size(15.0)
                                    .color(MUTED),
                            );
                        });
                        ui.add_space(8.0);
                        progress_bar(ui, board.progress_fraction());
                    }
                });
            });
        });
}

fn cover(ui: &mut egui::Ui, title: &Title, size: f32) {
    let rect_size = egui::vec2(size, size);
    let (rect, _) = ui.allocate_exact_size(rect_size, egui::Sense::hover());
    if let Some(url) = title.cover_url() {
        egui::Image::from_uri(url)
            .fit_to_exact_size(rect_size)
            .corner_radius(10)
            .paint_at(ui, rect);
    } else {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(10),
            Color32::from_rgb(38, 41, 48),
        );
        Lucide::Gamepad2
            .size(26.0)
            .color(MUTED)
            .stroke_width(1.75)
            .image()
            .paint_at(
                ui,
                egui::Rect::from_center_size(rect.center(), egui::vec2(26.0, 26.0)),
            );
    }
}

fn progress_bar(ui: &mut egui::Ui, fraction: f32) {
    let fraction = fraction.clamp(0.0, 1.0);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 8.0), egui::Sense::hover());
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(4),
        Color32::from_rgb(38, 41, 48),
    );
    if fraction > 0.0 {
        let mut fill = rect;
        fill.set_width((rect.width() * fraction).clamp(4.0, rect.width()));
        ui.painter()
            .rect_filled(fill, egui::CornerRadius::same(4), ACCENT);
    }
}

fn filter_row(
    ui: &mut egui::Ui,
    mut filter: AchievementFilter,
    list_query: &mut String,
) -> AchievementFilter {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for choice in AchievementFilter::ALL {
            if filter_chip(ui, choice.label(), filter == choice).clicked() {
                filter = choice;
            }
        }
        ui.add_space(8.0);
        list_search_field(ui, list_query);
    });
    filter
}

fn list_search_field(ui: &mut egui::Ui, text: &mut String) -> egui::Response {
    let height = 30.0;
    let width = 220.0;
    let icon = 14.0;
    egui::Frame::new()
        .fill(Color32::from_rgb(26, 28, 34))
        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(46, 49, 58)))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(10, 0))
        .show(ui, |ui| {
            ui.set_width(width - 20.0);
            ui.set_max_width(width - 20.0);
            ui.set_min_height(height);
            ui.set_max_height(height);
            ui.horizontal_centered(|ui| {
                ui.add(
                    Lucide::Search
                        .size(icon)
                        .color(MUTED)
                        .stroke_width(1.75)
                        .image(),
                );
                ui.add(
                    egui::TextEdit::singleline(text)
                        .hint_text("Search achievements")
                        .desired_width(width - 20.0 - icon - 8.0)
                        .frame(egui::Frame::NONE)
                        .font(egui::FontId::proportional(13.0))
                        .vertical_align(egui::Align::Center)
                        .margin(egui::Margin::ZERO)
                        .min_size(egui::vec2(80.0, height)),
                )
            })
            .inner
        })
        .inner
}

fn filter_chip(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    let color = if selected { TEXT } else { MUTED };
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), egui::FontId::proportional(13.0), color);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(galley.size().x + 22.0, 30.0),
        egui::Sense::click(),
    );
    let fill = if selected {
        ACCENT.gamma_multiply(0.22)
    } else if response.hovered() {
        Color32::from_rgb(32, 35, 42)
    } else {
        Color32::from_rgb(26, 28, 34)
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
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn achievement_list(
    ui: &mut egui::Ui,
    board: &TitleAchievements,
    filter: AchievementFilter,
    list_query: &str,
) {
    let needle = list_query.trim().to_lowercase();
    let matched: Vec<&Achievement> = board
        .achievements
        .iter()
        .filter(|achievement| filter.matches(achievement) && matches_text(achievement, &needle))
        .collect();
    if matched.is_empty() {
        let message = if needle.is_empty() {
            filter.empty_message(board.achievements.len())
        } else {
            "No achievements match."
        };
        ui.label(egui::RichText::new(message).size(15.0).color(MUTED));
        return;
    }

    let width = ui.available_width();
    let cols = column_count(width);
    let card_w = ((width - CARD_GAP * (cols.saturating_sub(1) as f32)) / cols as f32).floor();
    let rows = matched.len().div_ceil(cols);
    ui.spacing_mut().item_spacing.y = CARD_GAP;
    egui::ScrollArea::vertical()
        .id_salt("achievement_cards")
        .auto_shrink([false, false])
        .show_rows(ui, CARD_H, rows, |ui, range| {
            for row in range {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = CARD_GAP;
                    for col in 0..cols {
                        let index = row * cols + col;
                        if let Some(achievement) = matched.get(index) {
                            achievement_card(ui, achievement, card_w);
                        }
                    }
                });
            }
        });
}

fn matches_text(achievement: &Achievement, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    achievement.name.to_lowercase().contains(needle)
        || achievement.description.to_lowercase().contains(needle)
        || achievement
            .locked_description
            .to_lowercase()
            .contains(needle)
}

fn column_count(width: f32) -> usize {
    if width < 560.0 {
        return 1;
    }
    let cols = ((width + CARD_GAP) / (MIN_CARD_W + CARD_GAP)).floor() as usize;
    cols.clamp(2, 4)
}

fn achievement_card(ui: &mut egui::Ui, achievement: &Achievement, card_w: f32) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(card_w, CARD_H), egui::Sense::hover());
    let unlocked = achievement.is_unlocked();
    let fill = if response.hovered() {
        Color32::from_rgb(32, 35, 42)
    } else if unlocked {
        Color32::from_rgb(22, 23, 27)
    } else {
        Color32::from_rgb(26, 28, 34)
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(10), fill);

    let inner = rect.shrink2(egui::vec2(10.0, 8.0));
    let icon = 44.0;
    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(inner.left(), inner.center().y - icon / 2.0),
        egui::vec2(icon, icon),
    );
    paint_icon(ui, icon_rect, achievement.icon_url.as_deref(), unlocked);

    let text_left = icon_rect.right() + 10.0;
    let text_width = (inner.right() - text_left).max(40.0);
    let badge = format!("{} GS", achievement.gamerscore);
    let badge_galley = ui.painter().layout_no_wrap(
        badge,
        egui::FontId::proportional(11.0),
        if unlocked { MUTED } else { TEXT },
    );
    let badge_w = badge_galley.size().x + 14.0;
    let name_w = (text_width - badge_w - 8.0).max(24.0);
    let name_color = if unlocked {
        Color32::from_rgb(176, 180, 190)
    } else {
        TEXT
    };
    let name = fit_line(ui, &achievement.name, 13.5, name_color, name_w);
    let name_y = inner.top();
    ui.painter()
        .galley(egui::pos2(text_left, name_y), name, name_color);

    let badge_rect = egui::Rect::from_min_size(
        egui::pos2(inner.right() - badge_w, name_y),
        egui::vec2(badge_w, 18.0),
    );
    ui.painter().rect_filled(
        badge_rect,
        egui::CornerRadius::same(5),
        if unlocked {
            Color32::from_rgb(46, 49, 58)
        } else {
            ACCENT
        },
    );
    ui.painter().galley(
        egui::pos2(
            badge_rect.center().x - badge_galley.size().x / 2.0,
            badge_rect.center().y - badge_galley.size().y / 2.0,
        ),
        badge_galley,
        if unlocked { MUTED } else { TEXT },
    );

    let detail = if unlocked {
        Color32::from_rgb(126, 132, 144)
    } else {
        MUTED
    };
    let description = achievement.shown_description().replace(['\n', '\r'], " ");
    let description = if description.trim().is_empty() {
        " "
    } else {
        description.as_str()
    };
    let description = fit_line(ui, description, 12.0, detail, text_width);
    ui.painter()
        .galley(egui::pos2(text_left, name_y + 20.0), description, detail);

    let meta = fit_line(ui, &meta_line(achievement), 11.0, detail, text_width);
    ui.painter()
        .galley(egui::pos2(text_left, name_y + 38.0), meta, detail);

    response.on_hover_text(hover_text(achievement));
}

fn paint_icon(ui: &egui::Ui, rect: egui::Rect, url: Option<&str>, unlocked: bool) {
    if let Some(url) = url {
        egui::Image::from_uri(url)
            .fit_to_exact_size(rect.size())
            .corner_radius(8)
            .paint_at(ui, rect);
    } else {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(8),
            Color32::from_rgb(38, 41, 48),
        );
        Lucide::Trophy
            .size(18.0)
            .color(MUTED)
            .stroke_width(1.75)
            .image()
            .paint_at(
                ui,
                egui::Rect::from_center_size(rect.center(), egui::vec2(18.0, 18.0)),
            );
    }
    if unlocked {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(8),
            Color32::from_rgba_unmultiplied(16, 17, 20, 120),
        );
    }
}

fn fit_line(
    ui: &egui::Ui,
    text: &str,
    size: f32,
    color: Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let text = text.replace(['\n', '\r'], " ");
    let mut job = egui::text::LayoutJob::single_section(
        text,
        egui::TextFormat {
            font_id: egui::FontId::proportional(size),
            color,
            ..Default::default()
        },
    );
    job.wrap.max_width = width.max(1.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    ui.painter().layout_job(job)
}

fn hover_text(achievement: &Achievement) -> String {
    let mut text = achievement.name.clone();
    let description = achievement.shown_description();
    if !description.is_empty() {
        text.push('\n');
        text.push_str(description);
    }
    text.push('\n');
    text.push_str(&meta_line(achievement));
    text
}

fn meta_line(achievement: &Achievement) -> String {
    let mut parts = vec![achievement.status_label().to_owned()];
    if achievement.is_unlocked()
        && let Some(time) = achievement.unlocked_at.as_deref()
    {
        parts.push(format_unlocked(time));
    }
    if let Some(rarity) = achievement
        .rarity
        .as_ref()
        .and_then(|rarity| rarity.label())
    {
        parts.push(rarity);
    }
    if achievement.is_secret {
        parts.push("Secret".to_owned());
    }
    parts.join(" · ")
}

fn format_unlocked(raw: &str) -> String {
    let bytes = raw.as_bytes();
    if raw.len() >= 16 && bytes.get(10) == Some(&b'T') && bytes.get(13) == Some(&b':') {
        format!("{} {}", &raw[..10], &raw[11..16])
    } else {
        raw.to_owned()
    }
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
