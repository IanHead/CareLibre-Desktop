//! Headless screenshot of the app, for design review and CI snapshots.
//!
//! Run with:
//! `cargo run --example screenshot -- docs/home.png`
//! `cargo run --example screenshot -- docs/settings.png settings --live`

use std::time::{Duration, Instant};

use carelibre_desktop::{CareLibreApp, pages::Page, server::ConnectionState, theme};
use eframe::egui;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let output = args
        .first()
        .cloned()
        .unwrap_or_else(|| "screenshot.png".to_owned());
    let page = match args.get(1).map(String::as_str) {
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
    let live = args.iter().any(|arg| arg == "--live");
    let perf = args.iter().any(|arg| arg == "--perf");

    let mut app = CareLibreApp::new();
    app.set_page(page);

    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1360.0, 860.0))
        .with_context_setup(theme::apply)
        .wgpu()
        .build_ui_state(|ui, app: &mut CareLibreApp| app.show(ui), app);

    if live {
        harness.state_mut().connect(&egui::Context::default());
        let deadline = Instant::now() + Duration::from_secs(10);
        while !matches!(
            harness.state().connection(),
            ConnectionState::Online { .. } | ConnectionState::Offline { .. }
        ) {
            if Instant::now() >= deadline {
                panic!("timed out waiting for the server");
            }
            harness.run();
            std::thread::sleep(Duration::from_millis(50));
        }

        let deadline = Instant::now() + Duration::from_secs(10);
        while !harness.state().cache_ready() {
            if Instant::now() >= deadline {
                panic!("timed out waiting for the local cache");
            }
            harness.run();
            std::thread::sleep(Duration::from_millis(25));
        }

        if matches!(page, Page::Clients | Page::Workers) {
            let deadline = Instant::now() + Duration::from_secs(5);
            while harness.state().contacts_loaded() == 0 && Instant::now() < deadline {
                harness.run();
                std::thread::sleep(Duration::from_millis(25));
            }
        }
    }

    if perf {
        harness.run();
        harness.key_press(egui::Key::F12);
        harness.run_steps(2);
    } else {
        harness.run();
    }

    let image = harness.render().expect("failed to render screenshot");
    image.save(&output).expect("failed to save screenshot");

    println!("wrote {output}");
}
