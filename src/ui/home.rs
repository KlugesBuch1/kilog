use eframe::egui;

use super::KilogApp;
use super::theme::{ACCENT, MUTED, TEXT};
use crate::xbox::profile::PersonResponse;

pub(super) enum AuthUpdate {
    Waiting,
    Token(crate::auth::MicrosoftOAuthResponse),
    Failed(String),
}

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
        let signed_in = self.token.is_some();
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), 40.0),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                if signed_in {
                    if login_button(ui, "Logout").clicked() {
                        self.logout();
                    }
                } else if !self.restoring && login_button(ui, "Login").clicked() {
                    let ctx = ui.ctx().clone();
                    self.start_login(ctx);
                }
            },
        );
        ui.add_space(16.0);

        let profile = if let AuthState::Authenticated { profile } = &self.auth {
            Some(profile.clone())
        } else {
            None
        };
        let auth_note = match &self.auth {
            AuthState::Authenticating {
                user_code,
                verification_uri,
            } => Some((user_code.clone(), verification_uri.clone())),
            _ => None,
        };
        let loading = self.profile_rx.is_some() || self.restoring;
        let error = self.profile_error.clone();

        show_profile(
            ui,
            profile.as_ref(),
            auth_note.as_ref(),
            loading,
            error.as_deref(),
        );
    }
}

fn show_profile(
    ui: &mut egui::Ui,
    profile: Option<&PersonResponse>,
    auth_note: Option<&(String, String)>,
    loading: bool,
    error: Option<&str>,
) {
    let rect = ui.available_rect_before_wrap();
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(16),
        egui::Color32::from_rgb(26, 28, 34),
    );
    let inner = rect.shrink(28.0);
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
        ui.horizontal(|ui| {
            if let Some(url) = profile.and_then(|profile| profile.display_pic_raw.as_deref()) {
                ui.add(
                    egui::Image::from_uri(url)
                        .fit_to_exact_size(egui::vec2(148.0, 148.0))
                        .corner_radius(16),
                );
            } else {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(148.0, 148.0), egui::Sense::hover());
                ui.painter().rect_filled(
                    rect,
                    egui::CornerRadius::same(16),
                    egui::Color32::from_rgb(38, 41, 48),
                );
            }
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.add_space(8.0);
                let gamertag = profile
                    .and_then(|profile| profile.gamertag.as_deref())
                    .unwrap_or("Not signed in");
                ui.label(
                    egui::RichText::new(gamertag)
                        .size(36.0)
                        .strong()
                        .color(if profile.is_some() { TEXT } else { MUTED }),
                );
                if let Some(modern) = profile.and_then(|profile| profile.modern_gamertag.as_deref())
                {
                    ui.label(egui::RichText::new(modern).size(16.0).color(MUTED));
                }
                let score = profile
                    .and_then(|profile| profile.gamer_score.as_deref())
                    .unwrap_or("—");
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(format!("{score} Gamerscore"))
                        .size(20.0)
                        .color(if profile.is_some() { ACCENT } else { MUTED }),
                );
            });
        });

        ui.add_space(28.0);
        if let Some(err) = error {
            ui.label(
                egui::RichText::new(err)
                    .size(14.0)
                    .color(egui::Color32::from_rgb(232, 120, 128)),
            );
            ui.add_space(10.0);
        }
        if loading {
            ui.label(
                egui::RichText::new("Loading profile…")
                    .size(16.0)
                    .color(MUTED),
            );
            ui.add_space(10.0);
        }
        if let Some((user_code, verification_uri)) = auth_note {
            ui.label(
                egui::RichText::new(if user_code.is_empty() {
                    "Complete sign-in in the window"
                } else {
                    "Waiting to sign in"
                })
                .size(16.0)
                .color(TEXT),
            );
            if !user_code.is_empty() {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(user_code)
                        .size(28.0)
                        .strong()
                        .color(ACCENT),
                );
                ui.add_space(6.0);
                let link = ui.add(
                    egui::Label::new(
                        egui::RichText::new(verification_uri)
                            .size(14.0)
                            .color(ACCENT),
                    )
                    .sense(egui::Sense::click()),
                );
                if link.clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(verification_uri));
                }
            }
            return;
        }

        let Some(profile) = profile else {
            return;
        };
        let facts = [
            ("XUID", profile.xuid.as_deref()),
            ("Account tier", profile.account_tier.as_deref()),
            ("Reputation", profile.reputation.as_deref()),
            ("Location", profile.location.as_deref()),
            ("Bio", profile.bio.as_deref()),
        ];
        for (label, value) in facts {
            let Some(value) = value.filter(|value| !value.is_empty()) else {
                continue;
            };
            fact_row(ui, label, value);
            ui.add_space(10.0);
        }
    });
}

fn fact_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(egui::RichText::new(label).size(13.0).color(MUTED));
    ui.label(egui::RichText::new(value).size(16.0).color(TEXT));
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
