use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::{self, Color32, CornerRadius, Stroke, Visuals};
use egui_lucide::Lucide;

static TIME_FONT: AtomicBool = AtomicBool::new(false);

pub const CANVAS: Color32 = Color32::from_rgb(0x12, 0x13, 0x1C);
pub const SIDEBAR: Color32 = Color32::from_rgb(0x0E, 0x0F, 0x16);
pub const CARD: Color32 = Color32::from_rgb(0x1E, 0x1F, 0x2E);
pub const CARD_HOVER: Color32 = Color32::from_rgb(0x28, 0x2A, 0x3F);
pub const CARD_RAISED: Color32 = Color32::from_rgb(0x34, 0x36, 0x50);
pub const BORDER: Color32 = Color32::from_rgba_premultiplied(22, 22, 22, 22);
pub const CARD_BORDER: Color32 = Color32::from_rgba_premultiplied(15, 15, 15, 15);
pub const ACTIVE_ICON: Color32 = Color32::WHITE;
pub const TEXT: Color32 = Color32::from_rgb(0xE2, 0xE8, 0xF0);
pub const MUTED: Color32 = Color32::from_rgb(0x94, 0xA3, 0xB8);
pub const ACCENT: Color32 = Color32::from_rgb(0x7C, 0x3A, 0xED);
pub const RADIUS: u8 = 10;
pub const CARD_RADIUS: u8 = 12;
pub const TITLE_BAR_HEIGHT: f32 = 38.0;
pub(super) fn gamerscore_icon(ui: &egui::Ui, rect: egui::Rect) {
    egui::Image::new(egui::include_image!("../../assets/img/gamerscore.svg"))
        .fit_to_exact_size(rect.size())
        .paint_at(ui, rect);
}

pub(super) fn search_field(
    ui: &mut egui::Ui,
    text: &mut String,
    hint: &str,
    width: f32,
) -> egui::Response {
    let height = 40.0;
    let icon_size = 16.0;
    let text_width = width - 28.0 - icon_size - 12.0;
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(egui::Margin::symmetric(14, 0))
        .show(ui, |ui| {
            ui.set_width(width - 28.0);
            ui.set_max_width(width - 28.0);
            ui.set_max_height(height);
            ui.horizontal_centered(|ui| {
                ui.add(
                    Lucide::Search
                        .size(icon_size)
                        .color(MUTED)
                        .stroke_width(1.75)
                        .image(),
                );
                ui.add(
                    egui::TextEdit::singleline(text)
                        .hint_text(hint)
                        .desired_width(text_width)
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

pub(super) fn text_field(
    ui: &mut egui::Ui,
    text: &mut String,
    hint: &str,
    width: f32,
) -> egui::Response {
    let height = 40.0;
    let icon_size = 16.0;
    let text_width = width - 28.0 - icon_size - 12.0;
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(egui::Margin::symmetric(14, 0))
        .show(ui, |ui| {
            ui.set_width(width - 28.0);
            ui.set_max_width(width - 28.0);
            ui.set_max_height(height);
            ui.horizontal_centered(|ui| {
                ui.add(
                    egui::TextEdit::singleline(text)
                        .hint_text(hint)
                        .desired_width(text_width)
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
    install_time_font(ctx);
}

pub(super) fn time_font(size: f32) -> egui::FontId {
    let family = if TIME_FONT.load(Ordering::Relaxed) {
        egui::FontFamily::Name("Time".into())
    } else {
        egui::FontFamily::Proportional
    };
    egui::FontId::new(size, family)
}

fn install_time_font(ctx: &egui::Context) {
    let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf") else {
        return;
    };
    ctx.add_font(egui::epaint::text::FontInsert::new(
        "Time",
        egui::FontData::from_owned(bytes),
        vec![egui::epaint::text::InsertFontFamily {
            family: egui::FontFamily::Name("Time".into()),
            priority: egui::epaint::text::FontPriority::Highest,
        }],
    ));
    TIME_FONT.store(true, Ordering::Relaxed);
}

pub(super) fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, CARD_BORDER))
        .corner_radius(CornerRadius::same(CARD_RADIUS))
        .inner_margin(egui::Margin::same(16))
}

fn visuals() -> Visuals {
    let mut visuals = Visuals::dark();
    visuals.override_text_color = Some(TEXT);
    visuals.weak_text_color = Some(MUTED);
    visuals.hyperlink_color = ACCENT;
    visuals.panel_fill = CANVAS;
    visuals.window_fill = CARD;
    visuals.window_stroke = Stroke::new(1.0, BORDER);
    visuals.window_corner_radius = CornerRadius::same(CARD_RADIUS);
    visuals.menu_corner_radius = CornerRadius::same(RADIUS);
    visuals.window_shadow = egui::Shadow {
        offset: [0, 10],
        blur: 28,
        spread: 0,
        color: Color32::from_black_alpha(110),
    };
    visuals.popup_shadow = visuals.window_shadow;
    visuals.extreme_bg_color = CARD;
    visuals.text_edit_bg_color = Some(CARD);
    visuals.faint_bg_color = CARD_HOVER;
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.55);
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);

    let radius = CornerRadius::same(RADIUS);
    let border = Stroke::new(1.0, BORDER);
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = radius;
        widget.bg_stroke = border;
        widget.fg_stroke = Stroke::new(1.0, TEXT);
        widget.expansion = 0.0;
    }
    visuals.widgets.noninteractive.bg_fill = CARD;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, MUTED);
    visuals.widgets.inactive.bg_fill = CARD;
    visuals.widgets.inactive.weak_bg_fill = CARD;
    visuals.widgets.hovered.bg_fill = CARD_HOVER;
    visuals.widgets.hovered.weak_bg_fill = CARD_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT.gamma_multiply(0.6));
    visuals.widgets.active.bg_fill = ACCENT;
    visuals.widgets.active.weak_bg_fill = ACCENT;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.open.bg_fill = CARD;
    visuals.widgets.open.weak_bg_fill = CARD;

    visuals
}
