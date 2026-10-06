mod theme;

use eframe::egui::{self, Color32};
use theme::{ACCENT, CANVAS, MUTED, SIDEBAR, TEXT};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    Games,
    Achievements,
    Settings,
}

impl Page {
    const ALL: [Self; 4] = [
        Self::Home,
        Self::Games,
        Self::Achievements,
        Self::Settings,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Games => "Games",
            Self::Achievements => "Achievements",
            Self::Settings => "Settings",
        }
    }
}

pub struct KilogApp {
    runtime: tokio::runtime::Handle,
    page: Page,
}

impl KilogApp {
    pub fn new(cc: &eframe::CreationContext<'_>, runtime: tokio::runtime::Handle) -> Self {
        theme::apply(&cc.egui_ctx);
        Self {
            runtime,
            page: Page::Home,
        }
    }

    pub fn runtime(&self) -> &tokio::runtime::Handle {
        &self.runtime
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
                    .fill(SIDEBAR)
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

impl KilogApp {
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("Kilog").size(20.0).strong().color(TEXT));
        ui.add_space(20.0);
        ui.spacing_mut().item_spacing.y = 4.0;

        for page in Page::ALL {
            if nav_item(ui, page.title(), self.page == page).clicked() {
                self.page = page;
            }
        }
    }

    fn content(&self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new(self.page.title())
                .size(28.0)
                .strong()
                .color(TEXT),
        );
        ui.add_space(10.0);

        let (bar, _) = ui.allocate_exact_size(egui::vec2(32.0, 2.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(bar, egui::CornerRadius::same(1), ACCENT);

        ui.add_space(18.0);
        ui.label(
            egui::RichText::new("Nothing here yet.")
                .size(15.0)
                .color(MUTED),
        );
    }
}

fn nav_item(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
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
    ui.painter().text(
        rect.left_center() + egui::vec2(16.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(14.5),
        color,
    );

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
