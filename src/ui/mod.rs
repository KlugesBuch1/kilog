mod home;
mod page;
mod settings;
mod sidebar;
mod theme;

use crate::config::AppConfig;
use eframe::egui;
use home::AuthState;
use page::Page;
use theme::{ACCENT, CANVAS};

pub struct KilogApp {
    runtime: tokio::runtime::Handle,
    page: Page,
    config: AppConfig,
    auth: AuthState,
    save_error: Option<String>,
    profile_rx:
        Option<tokio::sync::oneshot::Receiver<Result<crate::xbox::models::PersonResponse, String>>>,
    profile_error: Option<String>,
}

impl KilogApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        runtime: tokio::runtime::Handle,
        config: AppConfig,
    ) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx);
        Self {
            runtime,
            page: Page::Home,
            config,
            auth: AuthState::Disconnected,
            save_error: None,
            profile_rx: None,
            profile_error: None,
        }
    }

    /// Fetch the people-hub profile on the app runtime, then show it on Home.
    pub fn load_profile(&mut self, ctx: &egui::Context, authorization: String, xuid: String) {
        let ctx = ctx.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.profile_error = None;
        self.profile_rx = Some(rx);
        self.runtime.spawn(async move {
            let result = crate::xbox::profile::fetch_profile(&authorization, &xuid)
                .await
                .map_err(|err| err.to_string());
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    fn poll_profile(&mut self) {
        let ready = self.profile_rx.as_mut().and_then(|rx| match rx.try_recv() {
            Ok(result) => Some(result),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => None,
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                Some(Err("profile request was dropped".to_owned()))
            }
        });
        if let Some(result) = ready {
            self.profile_rx = None;
            match result {
                Ok(profile) => {
                    self.profile_error = None;
                    self.auth = AuthState::Authenticated { profile };
                }
                Err(err) => self.profile_error = Some(err),
            }
        }
    }

    pub fn runtime(&self) -> &tokio::runtime::Handle {
        &self.runtime
    }

    fn content(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new(self.page.title())
                .size(28.0)
                .strong()
                .color(theme::TEXT),
        );
        ui.add_space(10.0);

        let (bar, _) = ui.allocate_exact_size(egui::vec2(32.0, 2.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(bar, egui::CornerRadius::same(1), ACCENT);

        ui.add_space(18.0);

        if self.page == Page::Home {
            self.home_page(ui);
        } else if self.page == Page::Settings {
            self.settings_form(ui);
        } else {
            ui.label(
                egui::RichText::new("Nothing here yet.")
                    .size(15.0)
                    .color(theme::MUTED),
            );
        }
    }
}

impl eframe::App for KilogApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let [r, g, b, a] = CANVAS.to_array();
        [
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        ]
    }

    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_profile();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::left("sidebar")
            .resizable(false)
            .show_separator_line(false)
            .default_size(228.0)
            .min_size(228.0)
            .max_size(228.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::SIDEBAR)
                    .inner_margin(egui::Margin::symmetric(14, 18)),
            )
            .show(ui, |ui| self.sidebar(ui));

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(CANVAS)
                    .inner_margin(egui::Margin::same(32)),
            )
            .show(ui, |ui| self.content(ui));
    }
}
