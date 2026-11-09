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
    contact::{ApiRelationshipWrite, PartyKind, RelationshipChange},
    rostering::{ApiBatchShiftWrite, ApiCopyShiftWrite, ApiShiftWrite},
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

    // Roles must come from the organisation's relationship type catalogue.
    let write = ApiRelationshipWrite {
        from_kind: PartyKind::Contact,
        from_id: contact.id.clone(),
        from_role: "person_supported".to_owned(),
        to_kind: PartyKind::Organisation,
        to_id: organisation.id.clone(),
        to_role: "other".to_owned(),
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
        .change_relationship(
            &session.access_token,
            &edge.id,
            &RelationshipChange::Remove {
                reason: "Created by the live integration test.".to_owned(),
            },
        )
        .expect("relationship removed");

    let after = client
        .list_relationships(&session.access_token, Some(&contact.id), None)
        .expect("relationships list after delete");
    assert!(
        !after.items.iter().any(|item| item.id == edge.id),
        "deleted relationship should disappear"
    );
}

#[test]
#[ignore = "requires a running CareLibre server with the demo seed"]
fn rostering_endpoints_round_trip() {
    let base_url =
        std::env::var("CARELIBRE_TEST_SERVER").unwrap_or_else(|_| DEFAULT_BASE_URL.to_owned());
    let client = ApiClient::new(&base_url).expect("valid server URL");
    let session = client
        .login("owner@bluedreams.example", "BlueDreamsCare!2026", None)
        .expect("demo sign-in");

    let sites = client
        .list_sites(&session.access_token)
        .expect("sites load");
    assert!(!sites.is_empty(), "demo data should include sites");

    let shifts = client
        .list_shifts(
            &session.access_token,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("shifts load");
    assert!(!shifts.items.is_empty(), "demo data should include shifts");

    let first = &shifts.items[0];
    let detail = client
        .get_shift(&session.access_token, &first.id)
        .expect("shift detail loads");
    assert_eq!(detail.shift.id, first.id);
    assert!(
        !detail.assignments.is_empty(),
        "demo shift should have assignments"
    );

    let suitability = client
        .shift_suitability(&session.access_token, &first.id)
        .expect("suitability loads");
    assert_eq!(suitability.shift_id, first.id);

    assert!(
        !first.clients.is_empty() || !first.workers.is_empty(),
        "shift list should include assignments"
    );
    assert!(
        first.cost_centre_id.is_some(),
        "demo shifts should resolve a cost centre"
    );

    let divisions = client
        .list_divisions(&session.access_token)
        .expect("divisions load");
    let services = client
        .list_services(&session.access_token)
        .expect("services load");
    let cost_centres = client
        .list_cost_centres(&session.access_token)
        .expect("cost centres load");
    assert!(!divisions.is_empty());
    assert!(!services.is_empty());
    assert!(!cost_centres.is_empty());

    let filtered = client
        .list_shifts(
            &session.access_token,
            None,
            None,
            None,
            None,
            first.division_id.as_deref(),
            None,
            None,
        )
        .expect("division filter");
    assert!(
        filtered
            .items
            .iter()
            .all(|shift| shift.division_id == first.division_id),
        "division filter should be applied"
    );

    let batch = ApiBatchShiftWrite {
        shifts: vec![
            ApiShiftWrite {
                title: Some("Desktop batch test".to_owned()),
                kind: Some("service_delivery".to_owned()),
                starts_at: "2027-04-01T02:00:00Z".to_owned(),
                ends_at: "2027-04-01T03:00:00Z".to_owned(),
                location: Some("Test".to_owned()),
                status: Some("draft".to_owned()),
                division_id: None,
                service_id: None,
                site_id: None,
                notes: None,
            },
            ApiShiftWrite {
                title: Some("Desktop batch test".to_owned()),
                kind: Some("service_delivery".to_owned()),
                starts_at: "2027-04-08T02:00:00Z".to_owned(),
                ends_at: "2027-04-08T03:00:00Z".to_owned(),
                location: Some("Test".to_owned()),
                status: Some("draft".to_owned()),
                division_id: None,
                service_id: None,
                site_id: None,
                notes: None,
            },
        ],
        recurrence: Some("weekly".to_owned()),
    };
    let created = client
        .create_shifts(&session.access_token, &batch)
        .expect("batch create");
    assert_eq!(created.items.len(), 2);
    assert_eq!(created.items[0].series_id, created.items[1].series_id);
    assert_eq!(created.items[0].recurrence.as_deref(), Some("weekly"));

    let copied = client
        .copy_shift(
            &session.access_token,
            &created.items[0].id,
            &ApiCopyShiftWrite {
                starts_at: Some("2027-04-15T02:00:00Z".to_owned()),
                ends_at: Some("2027-04-15T03:00:00Z".to_owned()),
                include_assignments: false,
            },
        )
        .expect("copy shift");
    assert_eq!(copied.shift.status, "draft");

    for shift in created.items.iter() {
        client
            .delete_shift(&session.access_token, &shift.id)
            .expect("delete batch shift");
    }
    client
        .delete_shift(&session.access_token, &copied.shift.id)
        .expect("delete copied shift");
}
