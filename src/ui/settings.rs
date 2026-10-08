use eframe::egui::{self, Color32};

use super::KilogApp;
use super::theme::{ACCENT, MUTED, TEXT};

impl KilogApp {
    pub(super) fn settings_form(&mut self, ui: &mut egui::Ui) {
        ui.set_max_width(520.0);
        ui.spacing_mut().item_spacing.y = 8.0;

        let mut changed = false;
        changed |= setting_row(
            ui,
            "Force region",
            Some("Recommended"),
            &mut self.config.force_region,
        );

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

    let label_galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(15.0),
        TEXT,
    );
    ui.painter().galley(
        egui::pos2(
            rect.left() + 16.0,
            rect.center().y - label_galley.size().y / 2.0,
        ),
        label_galley.clone(),
        TEXT,
    );
    if let Some(badge) = badge {
        let badge_galley = ui.painter().layout_no_wrap(
            badge.to_owned(),
            egui::FontId::proportional(11.0),
            MUTED,
        );
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
