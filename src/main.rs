use carelibre_desktop::{CareLibreApp, config::Config, theme};
use eframe::egui;

fn main() -> eframe::Result {
    let config = Config::load();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("CareLibre")
            .with_app_id("carelibre-desktop")
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([1024.0, 680.0])
            .with_decorations(!config.client_side_decorations),
        // eframe's wgpu renderer picks Vulkan/GL/Metal/DX12 at runtime and can
        // fall back to software adapters (lavapipe, llvmpipe) when no GPU is
        // available.
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "CareLibre",
        options,
        Box::new(|cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(CareLibreApp::with_server(&cc.egui_ctx)))
        }),
    )
}
