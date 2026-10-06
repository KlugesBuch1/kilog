use eframe::egui;

use super::KilogApp;
use super::theme::{ACCENT, MUTED, TEXT};

pub(super) enum AuthState {
    Disconnected,
    Authenticating {
        user_code: String,
        verification_uri: String,
    },
    #[allow(dead_code)]
    Authenticated {
        user_name: String,
    },
}

impl KilogApp {
    pub(super) fn home_page(&mut self, ui: &mut egui::Ui) {
        ui.set_max_width(520.0);

        match &self.auth {
            AuthState::Disconnected => {
                if login_button(ui).clicked() {
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
            AuthState::Authenticated { user_name } => {
                ui.label(
                    egui::RichText::new(format!("Signed in as {user_name}"))
                        .size(16.0)
                        .color(TEXT),
                );
            }
        }
    }
}

fn login_button(ui: &mut egui::Ui) -> egui::Response {
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
        "Login",
        egui::FontId::proportional(15.0),
        TEXT,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
