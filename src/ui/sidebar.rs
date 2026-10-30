use eframe::egui::{self, Color32};

use super::KilogApp;
use super::page::Page;
use super::theme::{ACCENT, ACTIVE_ICON, MUTED, RADIUS, TEXT};

impl KilogApp {
    pub(super) fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("kilog").size(20.0).strong().color(TEXT));
        ui.add_space(24.0);
        ui.spacing_mut().item_spacing.y = 6.0;

        for page in Page::MAIN {
            if nav_item(ui, page, self.page == page).clicked() {
                self.page = page;
            }
        }

        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            if nav_item(ui, Page::Settings, self.page == Page::Settings).clicked() {
                self.page = Page::Settings;
            }
        });
    }
}

fn nav_item(ui: &mut egui::Ui, page: Page, selected: bool) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 40.0), egui::Sense::click());
    let hovered = response.hovered();
    let radius = egui::CornerRadius::same(RADIUS);

    if selected {
        ui.painter()
            .rect_filled(rect, radius, ACCENT.gamma_multiply(0.30));
        ui.painter().rect_stroke(
            rect,
            radius,
            egui::Stroke::new(1.0, ACCENT.gamma_multiply(0.55)),
            egui::StrokeKind::Inside,
        );
    } else if hovered {
        ui.painter()
            .rect_filled(rect, radius, Color32::from_white_alpha(10));
    }

    let (icon_color, text_color) = match (selected, hovered) {
        (true, _) => (ACTIVE_ICON, TEXT),
        (false, true) => (TEXT, TEXT),
        (false, false) => (MUTED, MUTED),
    };
    let icon_size = 18.0;
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 24.0, rect.center().y),
        egui::vec2(icon_size, icon_size),
    );
    page.icon()
        .size(icon_size)
        .color(icon_color)
        .stroke_width(if selected { 2.0 } else { 1.75 })
        .image()
        .paint_at(ui, icon_rect);

    ui.painter().text(
        rect.left_center() + egui::vec2(44.0, 0.0),
        egui::Align2::LEFT_CENTER,
        page.title(),
        egui::FontId::proportional(14.5),
        text_color,
    );

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
