//! Visual language for CareLibre Desktop: colours, spacing and shared frames.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, RichText,
    Stroke, TextStyle, Visuals,
};
use serde::{Deserialize, Serialize};

use crate::server::ConnectionState;

/// The user's appearance preference.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    Light,
    Dark,
    #[default]
    System,
}

impl ThemeMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
            Self::System => "Follow System",
        }
    }

    pub const ALL: [Self; 3] = [Self::Light, Self::Dark, Self::System];
}

// Core palette -------------------------------------------------------------

pub struct Palette {
    dark: bool,
    bg: Color32,
    sidebar: Color32,
    surface: Color32,
    surface_raised: Color32,
    input: Color32,
    border: Color32,
    text: Color32,
    muted: Color32,
    faint: Color32,
    accent: Color32,
    accent_blue: Color32,
    warning: Color32,
    danger: Color32,
    success: Color32,
    hover: Color32,
    active: Color32,
    row_alt: Color32,
    on_accent: Color32,
}

const DARK_PALETTE: Palette = Palette {
    dark: true,
    bg: Color32::from_rgb(9, 14, 26),
    sidebar: Color32::from_rgb(12, 19, 33),
    surface: Color32::from_rgb(18, 27, 44),
    surface_raised: Color32::from_rgb(25, 37, 59),
    input: Color32::from_rgb(15, 23, 39),
    border: Color32::from_rgb(36, 50, 77),
    text: Color32::from_rgb(231, 238, 248),
    muted: Color32::from_rgb(143, 160, 191),
    faint: Color32::from_rgb(102, 118, 148),
    accent: Color32::from_rgb(53, 208, 186),
    accent_blue: Color32::from_rgb(108, 140, 255),
    warning: Color32::from_rgb(242, 184, 75),
    danger: Color32::from_rgb(240, 113, 120),
    success: Color32::from_rgb(94, 211, 145),
    hover: Color32::from_rgb(32, 47, 74),
    active: Color32::from_rgb(38, 56, 86),
    row_alt: Color32::from_rgb(12, 19, 32),
    on_accent: Color32::from_rgb(9, 14, 26),
};

const LIGHT_PALETTE: Palette = Palette {
    dark: false,
    bg: Color32::from_rgb(244, 247, 251),
    sidebar: Color32::from_rgb(255, 255, 255),
    surface: Color32::from_rgb(255, 255, 255),
    surface_raised: Color32::from_rgb(237, 242, 248),
    input: Color32::from_rgb(246, 249, 252),
    border: Color32::from_rgb(214, 223, 235),
    text: Color32::from_rgb(23, 33, 48),
    muted: Color32::from_rgb(91, 107, 130),
    faint: Color32::from_rgb(124, 138, 160),
    accent: Color32::from_rgb(12, 133, 119),
    accent_blue: Color32::from_rgb(63, 94, 214),
    warning: Color32::from_rgb(166, 106, 7),
    danger: Color32::from_rgb(190, 51, 51),
    success: Color32::from_rgb(22, 128, 78),
    hover: Color32::from_rgb(231, 238, 247),
    active: Color32::from_rgb(219, 229, 242),
    row_alt: Color32::from_rgb(249, 251, 254),
    on_accent: Color32::from_rgb(255, 255, 255),
};

const LIGHT_ID: u8 = 0;
const DARK_ID: u8 = 1;

static ACTIVE: AtomicU8 = AtomicU8::new(DARK_ID);

fn palette() -> &'static Palette {
    match ACTIVE.load(Ordering::Relaxed) {
        LIGHT_ID => &LIGHT_PALETTE,
        _ => &DARK_PALETTE,
    }
}

macro_rules! palette_colors {
    ($($name:ident),+ $(,)?) => {
        $(
            pub fn $name() -> Color32 {
                palette().$name
            }
        )+
    };
}

palette_colors!(
    bg,
    sidebar,
    surface,
    surface_raised,
    input,
    border,
    text,
    muted,
    faint,
    accent,
    accent_blue,
    warning,
    danger,
    success,
    hover,
    active,
    row_alt,
    on_accent,
);

/// Slightly stronger border used by column frames so their rounded edges stay
/// visible against the workspace background.
pub fn border_strong() -> Color32 {
    lerp_color(border(), muted(), 0.35)
}

fn set_palette(theme: egui::Theme) {
    ACTIVE.store(
        if theme == egui::Theme::Light {
            LIGHT_ID
        } else {
            DARK_ID
        },
        Ordering::Relaxed,
    );
}

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
    let accent = accent();
    Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), alpha)
}

/// Blend two colours in sRGB space. Used for animated hover and selection states.
pub fn lerp_color(from: Color32, to: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let channel = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        channel(from.r(), to.r()),
        channel(from.g(), to.g()),
        channel(from.b(), to.b()),
        channel(from.a(), to.a()),
    )
}

/// Symmetric quintic ease used by transitions. Compared with a simple
/// smoothstep, the extra curvature keeps acceleration near zero at both ends,
/// which reads as a softer start and a gentler stop.
pub fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Decelerating ease used by short UI transitions.
pub fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

// Shared building blocks ---------------------------------------------------

/// Apply the chosen appearance. `mode` is the persisted preference; when it is
/// [`ThemeMode::System`] the desktop's current light/dark setting is used, and
/// a background poller keeps it up to date.
pub fn apply(ctx: &egui::Context, mode: ThemeMode) {
    install_fonts(ctx);
    ctx.set_style_of(egui::Theme::Dark, style_for(&DARK_PALETTE));
    ctx.set_style_of(egui::Theme::Light, style_for(&LIGHT_PALETTE));
    sync(ctx, mode);
    crate::system_theme::start(ctx.clone());
}

/// Switch the active theme when the resolved appearance changes. Cheap enough
/// to call once per frame.
pub fn sync(ctx: &egui::Context, mode: ThemeMode) {
    let theme = resolve(ctx, mode);
    set_palette(theme);
    if ctx.theme() != theme {
        ctx.set_theme(theme);
        ctx.request_repaint();
    }
}

fn resolve(ctx: &egui::Context, mode: ThemeMode) -> egui::Theme {
    match mode {
        ThemeMode::Light => egui::Theme::Light,
        ThemeMode::Dark => egui::Theme::Dark,
        // winit reports the system theme on Windows and macOS. On Linux it is
        // unavailable, so fall back to the desktop portal probe.
        ThemeMode::System => ctx
            .system_theme()
            .or_else(crate::system_theme::detected)
            .unwrap_or(egui::Theme::Dark),
    }
}

fn style_for(palette: &Palette) -> egui::Style {
    let mut style = egui::Style::default();
    if !palette.dark {
        style.visuals = Visuals::light();
    }

    let visuals = &mut style.visuals;
    visuals.panel_fill = palette.bg;
    visuals.window_fill = palette.surface;
    visuals.extreme_bg_color = palette.input;
    visuals.faint_bg_color = palette.surface_raised;
    visuals.override_text_color = Some(palette.text);
    visuals.hyperlink_color = palette.accent;
    visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(
        palette.accent.r(),
        palette.accent.g(),
        palette.accent.b(),
        70,
    );
    visuals.selection.stroke = Stroke::new(1.0, palette.accent);
    visuals.window_stroke = Stroke::new(1.0, palette.border);
    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.menu_corner_radius = CornerRadius::same(10);

    visuals.widgets.noninteractive.bg_fill = palette.surface;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.border);
    visuals.widgets.inactive.bg_fill = palette.surface_raised;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, palette.border);
    visuals.widgets.inactive.weak_bg_fill = palette.surface_raised;
    visuals.widgets.hovered.bg_fill = palette.hover;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, palette.accent.gamma_multiply(0.6));
    visuals.widgets.hovered.weak_bg_fill = palette.hover;
    visuals.widgets.active.bg_fill = palette.active;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, palette.accent);

    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.spacing.interact_size.y = 32.0;
    style.spacing.window_margin = Margin::same(12);
    style.animation_time = 0.0;

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

    style
}

/// Shared primary action button.
pub fn primary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(text.to_owned())
            .color(on_accent())
            .font(strong_font(13.0)),
    )
    .fill(accent())
    .corner_radius(CornerRadius::same(8))
    .min_size(egui::vec2(0.0, 34.0))
}

/// Shared secondary action button.
pub fn secondary_button(label: &str) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(label.to_owned())
            .color(text())
            .font(strong_font(13.0)),
    )
    .fill(surface_raised())
    .stroke(Stroke::new(1.0, border()))
    .corner_radius(CornerRadius::same(8))
    .min_size(egui::vec2(0.0, 34.0))
}

/// Human-readable label and colour for a connection state.
pub fn connection_style(state: &ConnectionState) -> (&'static str, Color32) {
    match state {
        ConnectionState::NotConnected => ("Not connected", faint()),
        ConnectionState::Connecting => ("Connecting", warning()),
        ConnectionState::Online {
            database_up: true, ..
        } => ("Server online", success()),
        ConnectionState::Online {
            database_up: false, ..
        } => ("Database unavailable", warning()),
        ConnectionState::Offline { .. } => ("Server offline", danger()),
    }
}

pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(surface())
        .stroke(Stroke::new(1.0, border()))
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
        ink_on(fill),
    );
    response
}

/// Pick dark or light ink depending on the fill, independent of the active
/// palette (avatars can be accent-blue, teal or a neutral surface).
fn ink_on(fill: Color32) -> Color32 {
    let luminance = 0.299 * fill.r() as f32 + 0.587 * fill.g() as f32 + 0.114 * fill.b() as f32;
    if luminance > 120.0 {
        Color32::from_rgb(23, 33, 48)
    } else {
        Color32::WHITE
    }
}
