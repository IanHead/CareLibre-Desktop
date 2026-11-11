//! Client-side window chrome: title bar, window buttons and resize handles.
//!
//! New installs draw this chrome by default so the window matches the app on
//! every compositor. It can be disabled in Settings to use the operating
//! system's native decorations; existing configs keep their saved choice.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, FontId, Id, Order, Rect, ResizeDirection,
    Sense, Stroke, StrokeKind, Ui, ViewportCommand, pos2, vec2,
};

use crate::theme;

pub const HEIGHT: f32 = 30.0;
const RESIZE_EDGE: f32 = 5.0;

pub fn title_bar(ui: &mut Ui, ctx: &egui::Context, title: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEIGHT), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::ZERO, theme::sidebar());
    painter.hline(
        rect.x_range(),
        rect.bottom(),
        Stroke::new(1.0, theme::border()),
    );

    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));

    match ctx.os() {
        egui::os::OperatingSystem::Mac => {
            let mut drag_rect = rect;
            let start = rect.left() + 12.0;
            for (index, kind) in [
                ButtonKind::Close,
                ButtonKind::Minimize,
                ButtonKind::Maximize,
            ]
            .into_iter()
            .enumerate()
            {
                let center = pos2(start + index as f32 * 20.0, rect.center().y);
                let hit = Rect::from_center_size(center, vec2(16.0, 16.0));
                let response = ui.interact(
                    hit,
                    ui.scope_id().with(("chrome_mac", index)),
                    Sense::click(),
                );
                let colour = match kind {
                    ButtonKind::Close => Color32::from_rgb(255, 95, 87),
                    ButtonKind::Minimize => Color32::from_rgb(254, 188, 46),
                    ButtonKind::Maximize => Color32::from_rgb(40, 200, 64),
                };
                ui.painter().circle_filled(
                    center,
                    6.0,
                    if focused { colour } else { theme::border() },
                );
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::Role::Button,
                        true,
                        match kind {
                            ButtonKind::Close => "Close window",
                            ButtonKind::Minimize => "Minimize window",
                            ButtonKind::Maximize => "Maximize window",
                        },
                    )
                });
                if response.clicked() {
                    send_button(ctx, kind, maximized);
                }
                drag_rect.min.x = start + 66.0;
            }

            painter.text(
                pos2(rect.center().x, rect.center().y),
                Align2::CENTER_CENTER,
                title,
                FontId::proportional(11.5),
                theme::muted(),
            );
            drag(ui, ctx, drag_rect);
        }
        _ => {
            painter.text(
                pos2(rect.left() + 10.0, rect.center().y),
                Align2::LEFT_CENTER,
                title,
                FontId::proportional(11.5),
                theme::muted(),
            );

            let button_width = 46.0;
            let start = rect.right() - button_width * 3.0;
            for (index, kind) in [
                ButtonKind::Minimize,
                ButtonKind::Maximize,
                ButtonKind::Close,
            ]
            .into_iter()
            .enumerate()
            {
                let button_rect = Rect::from_min_size(
                    pos2(start + index as f32 * button_width, rect.top()),
                    vec2(button_width, rect.height()),
                );
                let response = ui.interact(
                    button_rect,
                    ui.scope_id().with(("chrome_win", index)),
                    Sense::click(),
                );
                let hover = response.hovered();
                if hover {
                    let fill = if kind == ButtonKind::Close {
                        Color32::from_rgb(196, 43, 28)
                    } else {
                        theme::surface_raised()
                    };
                    ui.painter()
                        .rect_filled(button_rect, CornerRadius::ZERO, fill);
                }
                let colour = if hover && kind == ButtonKind::Close {
                    Color32::WHITE
                } else {
                    theme::text()
                };
                draw_icon(ui.painter(), button_rect, kind, maximized, colour);
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::Role::Button,
                        true,
                        match kind {
                            ButtonKind::Close => "Close window",
                            ButtonKind::Minimize => "Minimize window",
                            ButtonKind::Maximize => "Maximize window",
                        },
                    )
                });
                if response.clicked() {
                    send_button(ctx, kind, maximized);
                }
            }

            let drag_rect = Rect::from_min_max(rect.min, pos2(start, rect.bottom()));
            drag(ui, ctx, drag_rect);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ButtonKind {
    Minimize,
    Maximize,
    Close,
}

fn send_button(ctx: &egui::Context, kind: ButtonKind, maximized: bool) {
    match kind {
        ButtonKind::Minimize => ctx.send_viewport_cmd(ViewportCommand::Minimized(true)),
        ButtonKind::Maximize => ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized)),
        ButtonKind::Close => ctx.send_viewport_cmd(ViewportCommand::Close),
    }
}

fn draw_icon(
    painter: &egui::Painter,
    rect: Rect,
    kind: ButtonKind,
    maximized: bool,
    colour: Color32,
) {
    let stroke = Stroke::new(1.0, colour);
    let centre = rect.center();
    match kind {
        ButtonKind::Minimize => {
            painter.line_segment([centre + vec2(-5.0, 0.0), centre + vec2(5.0, 0.0)], stroke);
        }
        ButtonKind::Maximize => {
            let size = 9.0;
            let icon = Rect::from_center_size(centre, vec2(size, size));
            painter.rect_stroke(icon, CornerRadius::ZERO, stroke, StrokeKind::Inside);
            if maximized {
                painter.rect_stroke(
                    icon.translate(vec2(2.5, -2.5)),
                    CornerRadius::ZERO,
                    stroke,
                    StrokeKind::Inside,
                );
            }
        }
        ButtonKind::Close => {
            painter.line_segment([centre + vec2(-4.5, -4.5), centre + vec2(4.5, 4.5)], stroke);
            painter.line_segment([centre + vec2(-4.5, 4.5), centre + vec2(4.5, -4.5)], stroke);
        }
    }
}

fn drag(ui: &mut Ui, ctx: &egui::Context, rect: Rect) {
    if rect.width() <= 0.0 {
        return;
    }
    let response = ui.interact(
        rect,
        ui.scope_id().with("chrome_drag"),
        Sense::click_and_drag(),
    );
    if response.drag_started() {
        // Hand the move to the window manager. The OS move loop is smoother
        // than per-frame position updates; egui's drag state is cleared so it
        // does not keep capturing the pointer after the hand-off.
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        ctx.stop_dragging();
    }

    if response.double_clicked() {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    }
}

/// Invisible resize handles around the viewport content.
pub fn resize_handles(ctx: &egui::Context) {
    let rect = ctx.content_rect();
    let e = RESIZE_EDGE;

    let zones: [(&str, Rect, ResizeDirection, CursorIcon); 8] = [
        (
            "north",
            Rect::from_min_max(
                pos2(rect.left() + e, rect.top()),
                pos2(rect.right() - e, rect.top() + e),
            ),
            ResizeDirection::North,
            CursorIcon::ResizeVertical,
        ),
        (
            "south",
            Rect::from_min_max(
                pos2(rect.left() + e, rect.bottom() - e),
                pos2(rect.right() - e, rect.bottom()),
            ),
            ResizeDirection::South,
            CursorIcon::ResizeVertical,
        ),
        (
            "west",
            Rect::from_min_max(
                pos2(rect.left(), rect.top() + e),
                pos2(rect.left() + e, rect.bottom() - e),
            ),
            ResizeDirection::West,
            CursorIcon::ResizeHorizontal,
        ),
        (
            "east",
            Rect::from_min_max(
                pos2(rect.right() - e, rect.top() + e),
                pos2(rect.right(), rect.bottom() - e),
            ),
            ResizeDirection::East,
            CursorIcon::ResizeHorizontal,
        ),
        (
            "nw",
            Rect::from_min_size(rect.min, vec2(e, e)),
            ResizeDirection::NorthWest,
            CursorIcon::ResizeNwSe,
        ),
        (
            "ne",
            Rect::from_min_size(pos2(rect.right() - e, rect.top()), vec2(e, e)),
            ResizeDirection::NorthEast,
            CursorIcon::ResizeNeSw,
        ),
        (
            "sw",
            Rect::from_min_size(pos2(rect.left(), rect.bottom() - e), vec2(e, e)),
            ResizeDirection::SouthWest,
            CursorIcon::ResizeNeSw,
        ),
        (
            "se",
            Rect::from_min_size(pos2(rect.right() - e, rect.bottom() - e), vec2(e, e)),
            ResizeDirection::SouthEast,
            CursorIcon::ResizeNwSe,
        ),
    ];

    for (name, zone, direction, cursor) in zones {
        if zone.width() <= 0.0 || zone.height() <= 0.0 {
            continue;
        }
        egui::Area::new(Id::unique(("carelibre-resize", ctx.viewport_id(), name)))
            .fixed_pos(zone.min)
            // Without this, egui's first sizing pass uses the default 600x400
            // area size, inflating the bottom/right handles until they cover
            // much of the window and steal every click.
            .default_size(zone.size())
            .movable(false)
            .interactable(true)
            .order(Order::Foreground)
            .show(ctx, |ui| {
                ui.set_min_size(zone.size());
                let response =
                    ui.allocate_rect(Rect::from_min_size(zone.min, zone.size()), Sense::drag());
                if response.hovered() {
                    ui.ctx().set_cursor_icon(cursor);
                }
                if response.drag_started() {
                    ui.ctx()
                        .send_viewport_cmd(ViewportCommand::BeginResize(direction));
                    ui.ctx().stop_dragging();
                }
            });
    }
}
