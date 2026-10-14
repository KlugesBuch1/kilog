use eframe::egui::{self, Color32};
use egui_lucide::Lucide;

use super::KilogApp;
use super::theme::{ACCENT, MUTED, TEXT};
use crate::config::DelayBand;

impl KilogApp {
    pub(super) fn settings_form(&mut self, ui: &mut egui::Ui) {
        ui.set_max_width(640.0);
        ui.spacing_mut().item_spacing.y = 8.0;

        let mut changed = false;
        changed |= setting_row(
            ui,
            "Force region",
            Some("Recommended"),
            &mut self.config.force_region,
        );
        ui.add_space(8.0);
        changed |= randomize_delays(ui, &mut self.config.randomize_delays);

        if changed {
            self.save_error = self.config.save().err().map(|err| err.to_string());
            if let Some(err) = &self.save_error {
                tracing::error!(error = %err, "failed to save config");
            }
        }

        if let Some(err) = &self.save_error {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!("Could not save settings. {err}"))
                    .size(13.0)
                    .color(Color32::from_rgb(232, 120, 128)),
            );
        }
    }
}

fn setting_row(ui: &mut egui::Ui, label: &str, badge: Option<&str>, value: &mut bool) -> bool {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 52.0), egui::Sense::click());

    let fill = if response.hovered() {
        Color32::from_rgba_unmultiplied(255, 255, 255, 12)
    } else {
        Color32::from_rgb(26, 28, 34)
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(10), fill);

    let label_galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), egui::FontId::proportional(15.0), TEXT);
    ui.painter().galley(
        egui::pos2(
            rect.left() + 16.0,
            rect.center().y - label_galley.size().y / 2.0,
        ),
        label_galley.clone(),
        TEXT,
    );
    if let Some(badge) = badge {
        let badge_galley =
            ui.painter()
                .layout_no_wrap(badge.to_owned(), egui::FontId::proportional(11.0), MUTED);
        let chip = egui::Rect::from_min_size(
            egui::pos2(
                rect.left() + 16.0 + label_galley.size().x + 10.0,
                rect.center().y - 9.0,
            ),
            egui::vec2(badge_galley.size().x + 12.0, 18.0),
        );
        ui.painter().rect_filled(
            chip,
            egui::CornerRadius::same(4),
            Color32::from_rgb(46, 49, 58),
        );
        ui.painter().galley(
            egui::pos2(
                chip.left() + 6.0,
                chip.center().y - badge_galley.size().y / 2.0,
            ),
            badge_galley,
            MUTED,
        );
    }

    let track = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 34.0, rect.center().y),
        egui::vec2(36.0, 20.0),
    );
    let track_fill = if *value {
        ACCENT
    } else {
        Color32::from_rgb(46, 49, 58)
    };
    ui.painter()
        .rect_filled(track, egui::CornerRadius::same(10), track_fill);

    let knob_x = if *value {
        track.right() - 10.0
    } else {
        track.left() + 10.0
    };
    ui.painter()
        .circle_filled(egui::pos2(knob_x, track.center().y), 7.0, TEXT);

    let toggled = response.clicked();
    if toggled {
        *value = !*value;
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand);
    toggled
}

fn randomize_delays(ui: &mut egui::Ui, settings: &mut crate::config::DelayRandomize) -> bool {
    let id = ui.make_persistent_id("randomize-delays-open");
    let mut open = ui.data(|data| data.get_temp::<bool>(id)).unwrap_or(false);
    if dropdown_header(ui, "Randomize Delays", open) {
        open = !open;
    }
    ui.data_mut(|data| data.insert_temp(id, open));
    if !open {
        return false;
    }

    ui.add_space(8.0);
    let mut changed = false;
    egui::Frame::new()
        .fill(Color32::from_rgb(22, 24, 29))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(
                    "Off keeps the queue delay. On shifts it up or down by a random amount in the range. The 5 hour option replaces the delay.",
                )
                .size(12.0)
                .color(MUTED),
            );
            ui.add_space(10.0);
            changed |= band_row(ui, ">1 min", "s", &mut settings.over_1_min);
            ui.add_space(8.0);
            changed |= band_row(ui, ">10 min", "s", &mut settings.over_10_min);
            ui.add_space(8.0);
            changed |= band_row(ui, ">1 h", "min", &mut settings.over_1_hour);
            ui.add_space(8.0);
            changed |= band_row(ui, ">5 h", "h", &mut settings.over_5_hours);
        });
    changed
}

fn dropdown_header(ui: &mut egui::Ui, label: &str, open: bool) -> bool {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 52.0), egui::Sense::click());
    let fill = if response.hovered() {
        Color32::from_rgba_unmultiplied(255, 255, 255, 12)
    } else {
        Color32::from_rgb(26, 28, 34)
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(10), fill);
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), egui::FontId::proportional(15.0), TEXT);
    ui.painter().galley(
        egui::pos2(rect.left() + 16.0, rect.center().y - galley.size().y / 2.0),
        galley,
        TEXT,
    );
    let turn = ui.ctx().animate_bool_with_time(response.id, open, 0.16);
    let icon = Lucide::ChevronDown
        .size(16.0)
        .color(MUTED)
        .stroke_width(1.75)
        .image()
        .rotate(turn * std::f32::consts::PI, egui::Vec2::splat(0.5));
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 22.0, rect.center().y),
        egui::vec2(16.0, 16.0),
    );
    icon.paint_at(ui, icon_rect);
    let clicked = response.clicked();
    response.on_hover_cursor(egui::CursorIcon::PointingHand);
    clicked
}

fn band_row(ui: &mut egui::Ui, label: &str, unit: &str, band: &mut DelayBand) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        changed |= switch(ui, &mut band.enabled);
        ui.label(egui::RichText::new(label).size(14.0).color(TEXT));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.label(egui::RichText::new(unit).size(12.0).color(MUTED));
            changed |= number_field(ui, &mut band.max);
            ui.label(egui::RichText::new("–").size(13.0).color(MUTED));
            changed |= number_field(ui, &mut band.min);
        });
    });
    changed
}

fn switch(ui: &mut egui::Ui, value: &mut bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(36.0, 20.0), egui::Sense::click());
    let fill = if *value {
        ACCENT
    } else {
        Color32::from_rgb(46, 49, 58)
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(10), fill);
    let knob_x = if *value {
        rect.right() - 10.0
    } else {
        rect.left() + 10.0
    };
    ui.painter()
        .circle_filled(egui::pos2(knob_x, rect.center().y), 7.0, TEXT);
    let toggled = response.clicked();
    if toggled {
        *value = !*value;
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand);
    toggled
}

fn number_field(ui: &mut egui::Ui, value: &mut u64) -> bool {
    let mut text = value.to_string();
    let response = ui.add(
        egui::TextEdit::singleline(&mut text)
            .desired_width(48.0)
            .font(egui::FontId::monospace(13.0))
            .horizontal_align(egui::Align::Center)
            .margin(egui::Margin::symmetric(6, 4)),
    );
    if response.changed()
        && let Ok(parsed) = text.trim().parse::<u64>()
    {
        *value = parsed;
        return true;
    }
    false
}
