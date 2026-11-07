//! Visual language for CareLibre Desktop: colours, spacing and shared frames.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, RichText,
    Stroke, TextStyle, Visuals,
};

use crate::server::ConnectionState;

// Core palette -------------------------------------------------------------

pub const BG: Color32 = Color32::from_rgb(9, 14, 26);
pub const SIDEBAR: Color32 = Color32::from_rgb(12, 19, 33);
pub const SURFACE: Color32 = Color32::from_rgb(18, 27, 44);
pub const SURFACE_RAISED: Color32 = Color32::from_rgb(25, 37, 59);
pub const INPUT: Color32 = Color32::from_rgb(15, 23, 39);
pub const BORDER: Color32 = Color32::from_rgb(36, 50, 77);

pub const TEXT: Color32 = Color32::from_rgb(231, 238, 248);
pub const MUTED: Color32 = Color32::from_rgb(143, 160, 191);
pub const FAINT: Color32 = Color32::from_rgb(102, 118, 148);

pub const ACCENT: Color32 = Color32::from_rgb(53, 208, 186);
pub const ACCENT_BLUE: Color32 = Color32::from_rgb(108, 140, 255);
pub const WARNING: Color32 = Color32::from_rgb(242, 184, 75);
pub const DANGER: Color32 = Color32::from_rgb(240, 113, 120);
pub const SUCCESS: Color32 = Color32::from_rgb(94, 211, 145);

// Typography --------------------------------------------------------------

const INTER_REGULAR: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
const INTER_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/Inter-SemiBold.ttf");

const SEMIBOLD_FAMILY: &str = "Inter SemiBold";

/// Semibold variant of the UI font, for headings and emphasis.
pub fn strong_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD_FAMILY.into()))
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        "Inter".to_owned(),
        Arc::new(FontData::from_static(INTER_REGULAR)),
    );
    fonts.font_data.insert(
        "Inter-SemiBold".to_owned(),
        Arc::new(FontData::from_static(INTER_SEMIBOLD)),
    );

    let mut proportional = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    proportional.insert(0, "Inter".to_owned());
    fonts
        .families
        .insert(FontFamily::Proportional, proportional.clone());

    let mut semibold = proportional;
    semibold.insert(0, "Inter-SemiBold".to_owned());
    fonts
        .families
        .insert(FontFamily::Name(SEMIBOLD_FAMILY.into()), semibold);

    ctx.set_fonts(fonts);
}

pub fn accent_soft(alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), alpha)
}

// Shared building blocks ---------------------------------------------------

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);
    install_fonts(ctx);
    ctx.all_styles_mut(|style| {
        style.visuals = Visuals::dark();
        style.visuals.panel_fill = BG;
        style.visuals.window_fill = SURFACE;
        style.visuals.extreme_bg_color = INPUT;
        style.visuals.faint_bg_color = SURFACE_RAISED;
        style.visuals.override_text_color = Some(TEXT);
        style.visuals.hyperlink_color = ACCENT;
        style.visuals.selection.bg_fill = accent_soft(70);
        style.visuals.selection.stroke = Stroke::new(1.0, ACCENT);
        style.visuals.window_stroke = Stroke::new(1.0, BORDER);
        style.visuals.window_corner_radius = CornerRadius::same(12);
        style.visuals.menu_corner_radius = CornerRadius::same(10);

        style.visuals.widgets.noninteractive.bg_fill = SURFACE;
        style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        style.visuals.widgets.inactive.bg_fill = SURFACE_RAISED;
        style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
        style.visuals.widgets.inactive.weak_bg_fill = SURFACE_RAISED;
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(32, 47, 74);
        style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT.gamma_multiply(0.6));
        style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(32, 47, 74);
        style.visuals.widgets.active.bg_fill = Color32::from_rgb(38, 56, 86);
        style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);

        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 7.0);
        style.spacing.interact_size.y = 32.0;
        style.spacing.window_margin = Margin::same(12);

        style.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(24.0, FontFamily::Name(SEMIBOLD_FAMILY.into())),
            ),
            (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(14.0, FontFamily::Name(SEMIBOLD_FAMILY.into())),
            ),
            (
                TextStyle::Small,
                FontId::new(12.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(13.0, FontFamily::Monospace),
            ),
        ]
        .into();
    });
}

/// Shared primary action button.
pub fn primary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(text.to_owned())
            .color(BG)
            .font(strong_font(13.0)),
    )
    .fill(ACCENT)
    .corner_radius(CornerRadius::same(8))
    .min_size(egui::vec2(0.0, 34.0))
}

/// Shared secondary action button.
pub fn secondary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(text.to_owned())
            .color(TEXT)
            .font(strong_font(13.0)),
    )
    .fill(SURFACE_RAISED)
    .stroke(Stroke::new(1.0, BORDER))
    .corner_radius(CornerRadius::same(8))
    .min_size(egui::vec2(0.0, 34.0))
}

/// Human-readable label and colour for a connection state.
pub fn connection_style(state: &ConnectionState) -> (&'static str, Color32) {
    match state {
        ConnectionState::NotConnected => ("Not connected", FAINT),
        ConnectionState::Connecting => ("Connecting", WARNING),
        ConnectionState::Online {
            database_up: true, ..
        } => ("Server online", SUCCESS),
        ConnectionState::Online {
            database_up: false, ..
        } => ("Database unavailable", WARNING),
        ConnectionState::Offline { .. } => ("Server offline", DANGER),
    }
}

pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(15))
}

pub fn pill(ui: &mut egui::Ui, text: &str, color: Color32) {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), strong_font(11.0), color);
    let size = galley.size() + egui::vec2(16.0, 6.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(10),
        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 28),
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, color);
}

pub fn avatar(ui: &mut egui::Ui, initials: &str, size: f32, fill: Color32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    ui.painter().circle_filled(rect.center(), size / 2.0, fill);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        initials,
        strong_font(size * 0.38),
        BG,
    );
    response
}
