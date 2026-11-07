//! Headless screenshot of the app, for design review and CI snapshots.
//!
//! Run with: `cargo run --example screenshot -- /tmp/carelibre.png`

use carelibre_desktop::{CareLibreApp, pages::Page, theme};
use eframe::egui;

fn main() {
    let mut args = std::env::args().skip(1);
    let output = args.next().unwrap_or_else(|| "screenshot.png".to_owned());
    let page = match args.next().as_deref() {
        Some("clients") => Page::Clients,
        Some("workers") => Page::Workers,
        Some("rostering") => Page::Rostering,
        Some("services") => Page::Services,
        Some("documents") => Page::Documents,
        Some("communications") => Page::Communications,
        Some("reports") => Page::Reports,
        Some("settings") => Page::Settings,
        _ => Page::Home,
    };

    let mut app = CareLibreApp::new();
    app.set_page(page);

    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1360.0, 860.0))
        .with_context_setup(theme::apply)
        .wgpu()
        .build_ui(|ui| app.show(ui));

    harness.run();
    let image = harness.render().expect("failed to render screenshot");
    image.save(&output).expect("failed to save screenshot");

    println!("wrote {output}");
}
