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
    server::{ConnectionState, Event, ServerRuntime},
    theme,
};
use eframe::egui;
use egui_kittest::Harness;
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
        .with_context_setup(theme::apply)
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
