use eframe::egui::{self, Color32, CornerRadius, Stroke, Visuals};
use egui_lucide::Lucide;

pub const CANVAS: Color32 = Color32::from_rgb(16, 17, 20);
pub const SIDEBAR: Color32 = Color32::from_rgb(22, 24, 29);
pub const TEXT: Color32 = Color32::from_rgb(236, 237, 240);
pub const MUTED: Color32 = Color32::from_rgb(148, 154, 166);
pub const ACCENT: Color32 = Color32::from_rgb(194, 43, 114);

pub(super) fn search_field(
    ui: &mut egui::Ui,
    text: &mut String,
    hint: &str,
    width: f32,
) -> egui::Response {
    let height = 40.0;
    let icon = 16.0;
    egui::Frame::new()
        .fill(Color32::from_rgb(26, 28, 34))
        .stroke(Stroke::new(1.0, Color32::from_rgb(46, 49, 58)))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(14, 0))
        .show(ui, |ui| {
            ui.set_width(width - 28.0);
            ui.set_min_height(height);
            ui.horizontal_centered(|ui| {
                ui.add(
                    Lucide::Search
                        .size(icon)
                        .color(MUTED)
                        .stroke_width(1.75)
                        .image(),
                );
                ui.add(
                    egui::TextEdit::singleline(text)
                        .hint_text(hint)
                        .desired_width(width - 28.0 - icon - 12.0)
                        .frame(egui::Frame::NONE)
                        .font(egui::FontId::proportional(15.0))
                        .vertical_align(egui::Align::Center)
                        .margin(egui::Margin::symmetric(4, 0))
                        .min_size(egui::vec2(120.0, height)),
                )
            })
            .inner
        })
        .inner
}

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.set_visuals_of(egui::Theme::Dark, visuals());
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(12.0, 8.0);
    });
}

fn visuals() -> Visuals {
    let mut visuals = Visuals::dark();
    visuals.override_text_color = Some(TEXT);
    visuals.weak_text_color = Some(MUTED);
    visuals.hyperlink_color = ACCENT;
    visuals.panel_fill = CANVAS;
    visuals.window_fill = CANVAS;
    let field = Color32::from_rgb(26, 28, 34);
    let field_hover = Color32::from_rgb(32, 35, 42);
    visuals.extreme_bg_color = field;
    visuals.text_edit_bg_color = Some(field);
    visuals.faint_bg_color = field_hover;
    visuals.window_corner_radius = CornerRadius::same(10);
    visuals.selection.bg_fill = Color32::from_rgb(92, 24, 56);
    visuals.window_stroke.color = Color32::from_rgb(46, 49, 58);

    let radius = CornerRadius::same(8);
    let stroke = Stroke::new(1.0, Color32::from_rgb(46, 49, 58));
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = radius;
        widget.bg_stroke = stroke;
        widget.fg_stroke = Stroke::new(1.0, TEXT);
    }
    visuals.widgets.inactive.bg_fill = field;
    visuals.widgets.inactive.weak_bg_fill = field;
    visuals.widgets.hovered.bg_fill = field_hover;
    visuals.widgets.hovered.weak_bg_fill = field_hover;
    visuals.widgets.active.bg_fill = field_hover;
    visuals.widgets.open.bg_fill = field;

    visuals
}
