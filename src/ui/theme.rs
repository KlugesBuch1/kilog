use eframe::egui::{self, Color32, CornerRadius, Visuals};

pub const CANVAS: Color32 = Color32::from_rgb(16, 17, 20);
pub const SIDEBAR: Color32 = Color32::from_rgb(22, 24, 29);
pub const TEXT: Color32 = Color32::from_rgb(236, 237, 240);
pub const MUTED: Color32 = Color32::from_rgb(148, 154, 166);
pub const ACCENT: Color32 = Color32::from_rgb(194, 43, 114);

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
    visuals.extreme_bg_color = Color32::from_rgb(12, 13, 16);
    visuals.faint_bg_color = Color32::from_rgb(32, 35, 42);
    visuals.window_corner_radius = CornerRadius::same(10);
    visuals.selection.bg_fill = Color32::from_rgb(92, 24, 56);
    visuals.window_stroke.color = Color32::from_rgb(46, 49, 58);

    let radius = CornerRadius::same(8);
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = radius;
    }

    visuals
}
