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
