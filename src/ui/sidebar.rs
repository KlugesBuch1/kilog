use eframe::egui::{self, Color32};
use egui_lucide::Lucide;

use super::KilogApp;
use super::page::Page;
use super::theme::{ACCENT, MUTED, TEXT};

const REPO_URL: &str = "https://github.com/KlugesBuch1/kilog";

impl KilogApp {
    pub(super) fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("Kilog").size(20.0).strong().color(TEXT));
        ui.add_space(20.0);
        ui.spacing_mut().item_spacing.y = 4.0;

        for page in Page::MAIN {
            if nav_item(ui, page, self.page == page).clicked() {
                self.page = page;
            }
        }

        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            star_cta(ui);
            ui.add_space(10.0);
            if nav_item(ui, Page::Settings, self.page == Page::Settings).clicked() {
                self.page = Page::Settings;
            }
        });
    }
}

fn star_cta(ui: &mut egui::Ui) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 40.0), egui::Sense::click());

    let fill = if response.hovered() {
        ACCENT.gamma_multiply(1.12)
    } else {
        ACCENT
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(8), fill);

    let label = "Star on GitHub";
    let font = egui::FontId::proportional(14.0);
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, TEXT);
    let icon_size = 15.0;
    let gap = 8.0;
    let total = icon_size + gap + galley.size().x;
    let start_x = rect.center().x - total / 2.0;

    Lucide::Star
        .size(icon_size)
        .color(TEXT)
        .stroke_width(2.0)
        .image()
        .paint_at(
            ui,
            egui::Rect::from_center_size(
                egui::pos2(start_x + icon_size / 2.0, rect.center().y),
                egui::vec2(icon_size, icon_size),
            ),
        );

    ui.painter().galley(
        egui::pos2(
            start_x + icon_size + gap,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        TEXT,
    );

    if response.clicked() {
        ui.ctx().open_url(egui::OpenUrl::new_tab(REPO_URL));
    }

    response.on_hover_cursor(egui::CursorIcon::PointingHand);
}

fn nav_item(ui: &mut egui::Ui, page: Page, selected: bool) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 36.0), egui::Sense::click());

    let fill = if selected {
        ACCENT.gamma_multiply(0.18)
    } else if response.hovered() {
        Color32::from_rgba_unmultiplied(255, 255, 255, 12)
    } else {
        Color32::TRANSPARENT
    };

    if fill != Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(8), fill);
    }

    if selected {
        let bar = egui::Rect::from_min_max(
            rect.left_top() + egui::vec2(0.0, 9.0),
            egui::pos2(rect.left() + 3.0, rect.bottom() - 9.0),
        );
        ui.painter()
            .rect_filled(bar, egui::CornerRadius::same(2), ACCENT);
    }

    let color = if selected { TEXT } else { MUTED };
    let icon_size = 16.0;
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 22.0, rect.center().y),
        egui::vec2(icon_size, icon_size),
    );
    page.icon()
        .size(icon_size)
        .color(color)
        .stroke_width(1.75)
        .image()
        .paint_at(ui, icon_rect);

    ui.painter().text(
        rect.left_center() + egui::vec2(40.0, 0.0),
        egui::Align2::LEFT_CENTER,
        page.title(),
        egui::FontId::proportional(14.5),
        color,
    );

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
