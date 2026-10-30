use eframe::egui::{
    self, Color32, CornerRadius, CursorIcon, Id, PointerButton, Pos2, Rect, ResizeDirection, Sense,
    Stroke, StrokeKind, ViewportCommand, pos2, vec2,
};
use egui_lucide::Lucide;

use super::theme::{BORDER, SIDEBAR, TEXT, TITLE_BAR_HEIGHT};

const BUTTON_WIDTH: f32 = 46.0;
const RESIZE_MARGIN: f32 = 5.0;
const CLOSE_HOVER: Color32 = Color32::from_rgb(0xE8, 0x11, 0x23);

pub(super) fn title_bar(ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));

    egui::Panel::top("title-bar")
        .exact_size(TITLE_BAR_HEIGHT)
        .resizable(false)
        .show_separator_line(false)
        .frame(egui::Frame::new().fill(SIDEBAR))
        .show(ui, |ui| {
            let bar = ui.max_rect();
            let drag = ui.interact(bar, Id::new("title-bar-drag"), Sense::click_and_drag());
            if drag.double_clicked() {
                ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
            } else if drag.drag_started_by(PointerButton::Primary) && edge_direction(&ctx).is_none()
            {
                ctx.send_viewport_cmd(ViewportCommand::StartDrag);
            }

            let slot = |index: f32| {
                Rect::from_min_size(
                    pos2(bar.right() - BUTTON_WIDTH * (index + 1.0), bar.top()),
                    vec2(BUTTON_WIDTH, bar.height()),
                )
            };
            let hover = Color32::from_white_alpha(16);

            if window_button(ui, slot(0.0), "win-close", Lucide::X, CLOSE_HOVER).clicked() {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            let (icon, id) = if maximized {
                (Lucide::Copy, "win-restore")
            } else {
                (Lucide::Square, "win-maximize")
            };
            if window_button(ui, slot(1.0), id, icon, hover).clicked() {
                ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
            }
            if window_button(ui, slot(2.0), "win-minimize", Lucide::Minus, hover).clicked() {
                ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
            }
        });
}

pub(super) fn window_chrome(ctx: &egui::Context) {
    let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
    if maximized {
        return;
    }

    ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        Id::new("window-outline"),
    ))
    .rect_stroke(
        ctx.content_rect(),
        CornerRadius::ZERO,
        Stroke::new(1.0, BORDER),
        StrokeKind::Inside,
    );

    if let Some(direction) = edge_direction(ctx) {
        ctx.set_cursor_icon(cursor_for(direction));
        if ctx.input(|input| input.pointer.button_pressed(PointerButton::Primary)) {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
        }
    }
}

fn window_button(
    ui: &mut egui::Ui,
    rect: Rect,
    id: &'static str,
    icon: Lucide,
    hover_fill: Color32,
) -> egui::Response {
    let response = ui.interact(rect, Id::new(id), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::ZERO, hover_fill);
    }
    let size = 14.0;
    icon.size(size)
        .color(TEXT)
        .stroke_width(1.75)
        .image()
        .paint_at(ui, Rect::from_center_size(rect.center(), vec2(size, size)));
    response
}

fn edge_direction(ctx: &egui::Context) -> Option<ResizeDirection> {
    let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
    if maximized {
        return None;
    }
    let pos = ctx.input(|input| input.pointer.latest_pos())?;
    resize_direction(ctx.content_rect(), pos)
}

fn resize_direction(rect: Rect, pos: Pos2) -> Option<ResizeDirection> {
    if !rect.contains(pos) {
        return None;
    }
    let west = pos.x - rect.left() <= RESIZE_MARGIN;
    let east = rect.right() - pos.x <= RESIZE_MARGIN;
    let north = pos.y - rect.top() <= RESIZE_MARGIN;
    let south = rect.bottom() - pos.y <= RESIZE_MARGIN;
    match (west, east, north, south) {
        (true, _, true, _) => Some(ResizeDirection::NorthWest),
        (_, true, true, _) => Some(ResizeDirection::NorthEast),
        (true, _, _, true) => Some(ResizeDirection::SouthWest),
        (_, true, _, true) => Some(ResizeDirection::SouthEast),
        (true, ..) => Some(ResizeDirection::West),
        (_, true, ..) => Some(ResizeDirection::East),
        (_, _, true, _) => Some(ResizeDirection::North),
        (.., true) => Some(ResizeDirection::South),
        _ => None,
    }
}

fn cursor_for(direction: ResizeDirection) -> CursorIcon {
    match direction {
        ResizeDirection::North | ResizeDirection::South => CursorIcon::ResizeVertical,
        ResizeDirection::East | ResizeDirection::West => CursorIcon::ResizeHorizontal,
        ResizeDirection::NorthEast | ResizeDirection::SouthWest => CursorIcon::ResizeNeSw,
        ResizeDirection::NorthWest | ResizeDirection::SouthEast => CursorIcon::ResizeNwSe,
    }
}
