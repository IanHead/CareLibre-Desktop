//! Headless screenshot of the app, for design review and CI snapshots.
//!
//! Run with:
//! `cargo run --example screenshot -- docs/home.png`
//! `cargo run --example screenshot -- docs/settings.png settings --live`

use std::time::{Duration, Instant};

use carelibre_desktop::{
    CareLibreApp,
    intake::IntakeKind,
    pages::Page,
    server::ConnectionState,
    theme::{self, ThemeMode},
};
use eframe::egui;
use egui_kittest::kittest::Queryable;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let output = args
        .first()
        .cloned()
        .unwrap_or_else(|| "screenshot.png".to_owned());
    let page = match args.get(1).map(String::as_str) {
        Some("contacts") => Page::Contacts,
        Some("clients") => Page::Clients,
        Some("workers") => Page::Workers,
        Some("rostering") => Page::Rostering,
        Some("relationships") => Page::Relationships,
        Some("services") => Page::Services,
        Some("documents") => Page::Documents,
        Some("communications") => Page::Communications,
        Some("reports") => Page::Reports,
        Some("settings") => Page::Settings,
        Some("intake") => Page::Intake,
        _ => Page::Home,
    };
    let live = args.iter().any(|arg| arg == "--live");
    let perf = args.iter().any(|arg| arg == "--perf");
    let contact = args.iter().any(|arg| arg == "--contact");
    let client_side = args.iter().any(|arg| arg == "--client");
    let theme_mode = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--theme="))
        .map(|value| match value {
            "light" => ThemeMode::Light,
            "dark" => ThemeMode::Dark,
            _ => ThemeMode::System,
        })
        .unwrap_or(ThemeMode::System);
    let login = args.iter().find_map(|arg| {
        arg.strip_prefix("--login=")
            .and_then(|value| value.split_once(':'))
            .map(|(email, password)| (email.to_owned(), password.to_owned()))
    });
    let suitability = args.iter().any(|arg| arg == "--suitability");
    let relationships = args.iter().any(|arg| arg == "--relationships");
    let intake = args.iter().find_map(|arg| match arg.as_str() {
        "--intake=client" => Some(IntakeKind::Client),
        "--intake=worker" => Some(IntakeKind::Worker),
        "--intake=other" => Some(IntakeKind::Other),
        _ => None,
    });

    let mut app = CareLibreApp::new();
    app.set_page(page);
    app.set_theme_mode(theme_mode);
    if client_side {
        app.set_client_side_decorations(true);
    }
    if let Some(kind) = intake {
        app.start_intake(kind);
    }

    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1360.0, 860.0))
        .with_context_setup(move |ctx| theme::apply(ctx, theme_mode))
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

        if let Some((email, password)) = &login {
            harness.state_mut().sign_in(email, password);
            let deadline = Instant::now() + Duration::from_secs(10);
            while !harness.state().signed_in() {
                if Instant::now() >= deadline {
                    panic!("timed out signing in");
                }
                harness.run();
                std::thread::sleep(Duration::from_millis(25));
            }
        }

        let deadline = Instant::now() + Duration::from_secs(10);
        while !harness.state().cache_ready() {
            if Instant::now() >= deadline {
                panic!("timed out waiting for the local cache");
            }
            harness.run();
            std::thread::sleep(Duration::from_millis(25));
        }

        if login.is_some() {
            let deadline = Instant::now() + Duration::from_secs(10);
            while harness.state().server_contacts_loaded() == 0 && Instant::now() < deadline {
                harness.run();
                std::thread::sleep(Duration::from_millis(25));
            }
        }

        if page == Page::Home && login.is_some() {
            // Load a contact page once so the dashboard counts are populated.
            harness.state_mut().set_page(Page::Contacts);
            let deadline = Instant::now() + Duration::from_secs(10);
            while harness.state().contacts_loaded() == 0 && Instant::now() < deadline {
                harness.run();
                std::thread::sleep(Duration::from_millis(25));
            }
            harness.state_mut().set_page(Page::Home);
            harness.run();
        }

        if matches!(page, Page::Clients | Page::Workers | Page::Contacts) {
            let deadline = Instant::now() + Duration::from_secs(5);
            while harness.state().contacts_loaded() == 0 && Instant::now() < deadline {
                harness.run();
                std::thread::sleep(Duration::from_millis(25));
            }
        }

        if page == Page::Relationships {
            let deadline = Instant::now() + Duration::from_secs(15);
            while !harness.state().relationship_graph_ready() && Instant::now() < deadline {
                harness.run();
                std::thread::sleep(Duration::from_millis(25));
            }
        }

        if page == Page::Rostering {
            let deadline = Instant::now() + Duration::from_secs(15);
            while harness.state().roster_shift_count() == 0 && Instant::now() < deadline {
                harness.run();
                std::thread::sleep(Duration::from_millis(25));
            }
            if harness.state_mut().open_first_shift() {
                let deadline = Instant::now() + Duration::from_secs(10);
                while !harness.state().roster_detail_ready() && Instant::now() < deadline {
                    harness.run();
                    std::thread::sleep(Duration::from_millis(25));
                }
            }
            if suitability && harness.state_mut().find_workers_for_selected_shift() {
                let deadline = Instant::now() + Duration::from_secs(15);
                while !harness.state().roster_suitability_ready() && Instant::now() < deadline {
                    harness.run();
                    std::thread::sleep(Duration::from_millis(25));
                }
                harness.get_by_label("Worker suitability").scroll_to_me();
                harness.run();
            }
        }

        if contact {
            harness.state_mut().open_first_contact();
            let deadline = Instant::now() + Duration::from_secs(5);
            while !harness.state().contact_editor_ready() && Instant::now() < deadline {
                harness.run();
                std::thread::sleep(Duration::from_millis(25));
            }
            if relationships {
                let deadline = Instant::now() + Duration::from_secs(10);
                while harness.state().relationship_count() == 0 && Instant::now() < deadline {
                    harness.run();
                    std::thread::sleep(Duration::from_millis(25));
                }
                harness
                    .get_by_label_contains("Add relationship")
                    .scroll_to_me();
                harness.run();
            }
        }
    }

    harness.state_mut().dismiss_notice();
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
