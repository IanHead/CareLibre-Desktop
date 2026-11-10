//! Headless screenshot of the app, for design review and CI snapshots.
//!
//! Run with:
//! `cargo run --example screenshot -- docs/contacts.png`
//! `cargo run --example screenshot -- docs/settings.png settings --live`
//! `--server=http://127.0.0.1:8485` uses another server for this run only.

use std::time::{Duration, Instant};

use carelibre_desktop::{
    CareLibreApp,
    intake::IntakeKind,
    pages::Page,
    server::ConnectionState,
    store::ContactFilter,
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
    // `clients` and `workers` open Contacts narrowed to that relationship.
    let contact_filter = match args.get(1).map(String::as_str) {
        Some("clients") => Some(ContactFilter::Clients),
        Some("workers") => Some(ContactFilter::Workers),
        _ => None,
    };
    let page = match args.get(1).map(String::as_str) {
        Some("contacts" | "clients" | "workers") => Page::Contacts,
        Some("rostering") => Page::Rostering,
        Some("relationships") => Page::Relationships,
        Some("services") => Page::Services,
        Some("documents") => Page::Documents,
        Some("communications") => Page::Communications,
        Some("reports") => Page::Reports,
        Some("settings") => Page::Settings,
        Some("intake") => Page::Intake,
        _ => Page::Contacts,
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
    // With --relationships: open the first relationship's history or actions menu.
    let history = args.iter().any(|arg| arg == "--history");
    let actions = args.iter().any(|arg| arg == "--actions");
    let roles = args.iter().any(|arg| arg == "--roles");
    let intake = args.iter().find_map(|arg| match arg.as_str() {
        "--intake=client" => Some(IntakeKind::Client),
        "--intake=worker" => Some(IntakeKind::Worker),
        "--intake=other" => Some(IntakeKind::Other),
        _ => None,
    });

    let server = args.iter().find_map(|arg| arg.strip_prefix("--server="));
    let search = args.iter().find_map(|arg| arg.strip_prefix("--search="));

    let mut app = CareLibreApp::new();
    if let Some(server) = server {
        app.use_server(server).expect("valid --server URL");
    }
    app.set_page(page);
    if let Some(filter) = contact_filter {
        app.set_contact_filter(filter);
    }
    app.set_theme_mode(theme_mode);
    if client_side {
        app.set_client_side_decorations(true);
    }
    if let Some(kind) = intake {
        app.start_intake(kind);
    }

    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1360.0, 860.0))
        .with_max_steps(40)
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

        if page == Page::Contacts {
            if let Some(search) = search {
                harness.state_mut().search_contacts(search);
                for _ in 0..10 {
                    harness.run();
                    std::thread::sleep(Duration::from_millis(25));
                }
            }
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
                if roles {
                    harness
                        .get_all_by_label("Relationship type")
                        .next()
                        .expect("the add form's relationship picker")
                        .click_accesskit();
                    for _ in 0..20 {
                        harness.run();
                        std::thread::sleep(Duration::from_millis(25));
                    }
                }
                if history || actions {
                    let label = if history { "History" } else { "Actions…" };
                    let button = harness
                        .get_all_by_label(label)
                        .next()
                        .expect("a relationship");
                    button.click_accesskit();
                    for _ in 0..40 {
                        harness.run();
                        std::thread::sleep(Duration::from_millis(25));
                    }
                    if history {
                        harness.get_by_label("Hide history").scroll_to_me();
                        harness.run();
                    }
                }
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
