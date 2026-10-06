use eframe::egui;

use super::KilogApp;
use super::theme::{ACCENT, MUTED, TEXT};
use crate::xbox::models::PersonResponse;

pub(super) enum AuthState {
    Disconnected,
    Authenticating {
        user_code: String,
        verification_uri: String,
    },
    Authenticated {
        profile: PersonResponse,
    },
}

impl KilogApp {
    pub(super) fn home_page(&mut self, ui: &mut egui::Ui) {
        ui.set_max_width(520.0);

        if self.profile_rx.is_some() {
            ui.label(
                egui::RichText::new("Loading profile…")
                    .size(15.0)
                    .color(MUTED),
            );
            ui.add_space(12.0);
        }

        if let Some(err) = &self.profile_error {
            ui.label(
                egui::RichText::new(err)
                    .size(13.0)
                    .color(egui::Color32::from_rgb(232, 120, 128)),
            );
            ui.add_space(12.0);
        }

        match &self.auth {
            AuthState::Disconnected => {
                if login_button(ui, "Login").clicked() {
                    self.auth = AuthState::Authenticating {
                        user_code: "Not available".to_owned(),
                        verification_uri: "Sign-in is not connected yet.".to_owned(),
                    };
                }
            }
            AuthState::Authenticating {
                user_code,
                verification_uri,
            } => {
                ui.label(
                    egui::RichText::new("Waiting to sign in")
                        .size(16.0)
                        .color(TEXT),
                );
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new(user_code)
                        .size(22.0)
                        .strong()
                        .color(ACCENT),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(verification_uri)
                        .size(14.0)
                        .color(MUTED),
                );
            }
            AuthState::Authenticated { profile } => {
                let profile = profile.clone();
                show_profile(ui, &profile);
                ui.add_space(16.0);
                if login_button(ui, "Logout").clicked() {
                    self.auth = AuthState::Disconnected;
                    self.profile_rx = None;
                    self.profile_error = None;
                }
            }
        }
    }
}

fn show_profile(ui: &mut egui::Ui, profile: &PersonResponse) {
    ui.horizontal(|ui| {
        if let Some(url) = &profile.display_pic_raw {
            ui.add(
                egui::Image::from_uri(url)
                    .max_size(egui::vec2(72.0, 72.0))
                    .corner_radius(8),
            );
        }
        ui.vertical(|ui| {
            let gamertag = profile.gamertag.as_deref().unwrap_or("Unknown gamertag");
            ui.label(
                egui::RichText::new(gamertag)
                    .size(22.0)
                    .strong()
                    .color(TEXT),
            );
            let score = profile.gamer_score.as_deref().unwrap_or("—");
            ui.label(
                egui::RichText::new(format!("Gamerscore {score}"))
                    .size(14.0)
                    .color(MUTED),
            );
        });
    });
}

fn login_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(140.0, 40.0), egui::Sense::click());
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
