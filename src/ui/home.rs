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
            self.config.anonymous_mode,
        );
    }
}

fn show_profile(
    ui: &mut egui::Ui,
    profile: Option<&PersonResponse>,
    auth_note: Option<&(String, String)>,
    loading: bool,
    error: Option<&str>,
    anonymous: bool,
) {
    let rect = ui.available_rect_before_wrap();
    let radius = egui::CornerRadius::same(16);
    let painter = ui.painter();
    painter.add(
        egui::Shadow {
            offset: [0, 12],
            blur: 32,
            spread: 0,
            color: egui::Color32::from_black_alpha(70),
        }
        .as_shape(rect, radius),
    );
    painter.rect_filled(rect, radius, super::theme::CARD);
    painter.rect_stroke(
        rect,
        radius,
        egui::Stroke::new(1.0, super::theme::CARD_BORDER),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink(36.0);
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
        ui.horizontal(|ui| {
            if let Some(url) = profile
                .filter(|_| !anonymous)
                .and_then(|profile| profile.display_pic_raw.as_deref())
            {
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
                    super::theme::CARD_RAISED,
                );
            }
            ui.add_space(24.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                ui.add_space(8.0);
                let gamertag = if anonymous {
                    if profile.is_some() {
                        "Signed in"
                    } else {
                        "Not signed in"
                    }
                } else {
                    profile
                        .and_then(|profile| profile.gamertag.as_deref())
                        .unwrap_or("Not signed in")
                };
                ui.label(
                    egui::RichText::new(gamertag)
                        .size(36.0)
                        .strong()
                        .color(if profile.is_some() { TEXT } else { MUTED }),
                );
                if !anonymous
                    && let Some(modern) =
                        profile.and_then(|profile| profile.modern_gamertag.as_deref())
                {
                    ui.label(egui::RichText::new(modern).size(16.0).color(MUTED));
                }
                if !anonymous {
                    let score = profile
                        .and_then(|profile| profile.gamer_score.as_deref())
                        .unwrap_or("—");
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(format!("{score} Gamerscore"))
                            .size(20.0)
                            .color(if profile.is_some() { ACCENT } else { MUTED }),
                    );
                }
            });
        });

        ui.add_space(32.0);
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
        let xuid = if anonymous {
            None
        } else {
            profile.xuid.as_deref()
        };
        let facts = [
            ("XUID", xuid),
            ("Account tier", profile.account_tier.as_deref()),
            ("Reputation", profile.reputation.as_deref()),
            ("Location", profile.location.as_deref()),
            ("Bio", profile.bio.as_deref()),
        ];
        let rows: Vec<_> = facts
            .into_iter()
            .filter_map(|(label, value)| {
                value
                    .filter(|value| !value.is_empty())
                    .map(|value| (label, value))
            })
            .collect();
        for (index, (label, value)) in rows.into_iter().enumerate() {
            if index > 0 {
                ui.add(egui::Separator::default().spacing(24.0));
            }
            fact_row(ui, label, value);
        }
    });
}

fn fact_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(egui::RichText::new(label).size(12.5).color(MUTED));
    ui.add_space(6.0);
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
        .rect_filled(rect, egui::CornerRadius::same(super::theme::RADIUS), fill);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(15.0),
        TEXT,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
