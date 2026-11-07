//! Live checks against a running CareLibre server.
//!
//! These tests are ignored by default. Run them with:
//!
//! ```sh
//! cargo test --test server_live -- --ignored
//! ```
//!
//! Set `CARELIBRE_TEST_SERVER` to test another address.

use carelibre_desktop::{
    api::{ApiClient, DEFAULT_BASE_URL},
    contact::{ApiRelationshipWrite, PartyKind},
    server::{ConnectionState, Event, ServerRuntime},
    theme::{self, ThemeMode},
};
use eframe::egui;
use egui_kittest::{Harness, kittest::Queryable};
use std::time::{Duration, Instant};

#[test]
#[ignore = "requires a running CareLibre server"]
fn health_and_readiness() {
    let base_url =
        std::env::var("CARELIBRE_TEST_SERVER").unwrap_or_else(|_| DEFAULT_BASE_URL.to_owned());

    let client = ApiClient::new(&base_url).expect("valid server URL");
    let info = client.health().expect("server should be reachable");

    assert!(!info.version.is_empty(), "server should report a version");
    assert!(
        info.database_up,
        "database should be ready: {:?}",
        info.ready_error
    );
}

#[test]
#[ignore = "requires a running CareLibre server"]
fn background_worker_connects() {
    let base_url =
        std::env::var("CARELIBRE_TEST_SERVER").unwrap_or_else(|_| DEFAULT_BASE_URL.to_owned());
    let ctx = eframe::egui::Context::default();
    let runtime = ServerRuntime::spawn(ctx, base_url);

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match runtime.try_recv() {
            Some(Event::Status { result, .. }) => {
                let info = result.expect("health check should succeed");
                assert!(!info.version.is_empty());
                assert!(info.database_up);
                return;
            }
            Some(_) => {}
            None if Instant::now() >= deadline => {
                panic!("timed out waiting for the server worker");
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

#[test]
#[ignore = "requires a running CareLibre server"]
fn app_connects_to_live_server() {
    let mut app = carelibre_desktop::CareLibreApp::with_server(&egui::Context::default());
    app.set_page(carelibre_desktop::pages::Page::Settings);

    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_context_setup(|ctx| theme::apply(ctx, ThemeMode::Dark))
        .build_ui_state(
            |ui, app: &mut carelibre_desktop::CareLibreApp| app.show(ui),
            app,
        );

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        harness.run();
        if matches!(harness.state().connection(), ConnectionState::Online { .. }) {
            return;
        }
        if Instant::now() >= deadline {
            panic!("timed out waiting for the app to connect");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
#[ignore = "requires a running CareLibre server with an existing account"]
fn login_rejects_bad_credentials() {
    let base_url =
        std::env::var("CARELIBRE_TEST_SERVER").unwrap_or_else(|_| DEFAULT_BASE_URL.to_owned());

    let client = ApiClient::new(&base_url).expect("valid server URL");
    let message = match client.login("nobody@example.com", "not-the-right-password", None) {
        Ok(_) => panic!("bad credentials must fail"),
        Err(err) => err.to_string(),
    };
    assert!(
        message.to_lowercase().contains("credential")
            || message.to_lowercase().contains("password")
            || message.to_lowercase().contains("email"),
        "unexpected error: {message}"
    );
}

#[test]
#[ignore = "requires the seeded Blue Dreams Care server"]
fn server_contacts_load_and_editor_opens() {
    use carelibre_desktop::pages::Page;

    let mut app = carelibre_desktop::CareLibreApp::with_server(&egui::Context::default());
    app.sign_in("owner@bluedreams.example", "BlueDreamsCare!2026");
    app.set_page(Page::Contacts);

    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_context_setup(|ctx| theme::apply(ctx, ThemeMode::Dark))
        .build_ui_state(
            |ui, app: &mut carelibre_desktop::CareLibreApp| app.show(ui),
            app,
        );

    let deadline = Instant::now() + Duration::from_secs(15);
    while app_contacts_loaded(&harness) == 0 {
        if Instant::now() >= deadline {
            panic!("timed out waiting for server contacts");
        }
        harness.run();
        std::thread::sleep(Duration::from_millis(50));
    }

    harness.state_mut().open_first_contact();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !harness.state().contact_editor_ready() {
        if Instant::now() >= deadline {
            panic!("timed out waiting for the contact editor");
        }
        harness.run();
        std::thread::sleep(Duration::from_millis(50));
    }

    assert!(
        harness.state().contact_editor_custom_fields() > 0,
        "the seeded server custom fields should be loaded into the editor"
    );

    harness.get_by_label("Pop out").click();
    harness.run_steps(2);
    assert!(
        harness.state().contact_editor_popped_out(),
        "the modal should detach into a native window"
    );
}

fn app_contacts_loaded(harness: &Harness<'_, carelibre_desktop::CareLibreApp>) -> usize {
    harness.state().contacts_loaded()
}

#[test]
#[ignore = "requires a running CareLibre server"]
fn intake_wizard_pops_out_and_docks() {
    use carelibre_desktop::intake::IntakeKind;
    use carelibre_desktop::pages::Page;

    let mut app = carelibre_desktop::CareLibreApp::with_server(&egui::Context::default());
    app.sign_in("owner@bluedreams.example", "BlueDreamsCare!2026");
    app.set_page(Page::Intake);
    app.start_intake(IntakeKind::Client);

    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_context_setup(|ctx| theme::apply(ctx, ThemeMode::Dark))
        .build_ui_state(
            |ui, app: &mut carelibre_desktop::CareLibreApp| app.show(ui),
            app,
        );

    let deadline = Instant::now() + Duration::from_secs(10);
    while !(harness.state().signed_in() && harness.state().cache_ready()) {
        if Instant::now() >= deadline {
            panic!("timed out preparing the intake wizard");
        }
        harness.run();
        std::thread::sleep(Duration::from_millis(25));
    }

    harness.run();
    harness.get_by_label("Pop out").click();
    harness.run_steps(2);
    assert!(
        harness.state().intake_popped_out(),
        "the intake wizard should detach into a native window"
    );

    harness.get_by_label("Dock intake").click_accesskit();
    harness.run_steps(2);
    assert!(
        !harness.state().intake_popped_out(),
        "the intake wizard should dock back into the page"
    );
}

#[test]
#[ignore = "requires a running CareLibre server with the demo seed"]
fn relationship_graph_round_trip() {
    let base_url =
        std::env::var("CARELIBRE_TEST_SERVER").unwrap_or_else(|_| DEFAULT_BASE_URL.to_owned());
    let client = ApiClient::new(&base_url).expect("valid server URL");
    let session = client
        .login("owner@bluedreams.example", "BlueDreamsCare!2026", None)
        .expect("demo sign-in");

    let graph = client
        .relationship_graph(&session.access_token)
        .expect("graph loads");
    assert!(!graph.nodes.is_empty(), "graph should have nodes");
    assert!(!graph.edges.is_empty(), "demo graph should have edges");

    let contact = graph
        .nodes
        .iter()
        .find(|node| node.kind == PartyKind::Contact)
        .expect("a contact node");
    let organisation = graph
        .nodes
        .iter()
        .find(|node| node.kind == PartyKind::Organisation)
        .expect("an organisation node");

    let suffix = uuid::Uuid::now_v7().simple().to_string();
    let write = ApiRelationshipWrite {
        from_kind: PartyKind::Contact,
        from_id: contact.id.clone(),
        from_role: format!("verify_from_{suffix}"),
        to_kind: PartyKind::Organisation,
        to_id: organisation.id.clone(),
        to_role: format!("verify_to_{suffix}"),
        start_date: None,
        end_date: None,
        notes: Some("desktop live verification".to_owned()),
    };

    let edge = client
        .create_relationship(&session.access_token, &write)
        .expect("relationship created");
    assert_eq!(edge.from_id, contact.id);
    assert_eq!(edge.to_kind, PartyKind::Organisation);

    let listed = client
        .list_relationships(&session.access_token, Some(&contact.id), None)
        .expect("relationships list");
    assert!(
        listed.items.iter().any(|item| item.id == edge.id),
        "created relationship should be listed for the contact"
    );

    client
        .delete_relationship(&session.access_token, &edge.id)
        .expect("relationship deleted");

    let after = client
        .list_relationships(&session.access_token, Some(&contact.id), None)
        .expect("relationships list after delete");
    assert!(
        !after.items.iter().any(|item| item.id == edge.id),
        "deleted relationship should disappear"
    );
}
