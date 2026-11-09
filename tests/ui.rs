use carelibre_desktop::{
    CareLibreApp,
    pages::Page,
    theme::{self, ThemeMode},
};
use eframe::egui;
use egui_kittest::{Harness, kittest::Queryable};

fn harness(app: CareLibreApp) -> Harness<'static, CareLibreApp> {
    let mut app = app;
    app.set_theme_mode(ThemeMode::Dark);
    Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_context_setup(|ctx| theme::apply(ctx, ThemeMode::Dark))
        .build_ui_state(|ui, app| app.show(ui), app)
}

#[test]
fn every_page_renders() {
    for page in [
        Page::Home,
        Page::Contacts,
        Page::Rostering,
        Page::Relationships,
        Page::Services,
        Page::Documents,
        Page::Communications,
        Page::Reports,
        Page::Settings,
    ] {
        let mut app = CareLibreApp::new();
        app.set_page(page);

        let mut harness = harness(app);
        harness.run();

        assert_eq!(harness.state().page(), page);
    }
}

#[test]
fn sidebar_navigation_works() {
    let mut harness = harness(CareLibreApp::new());
    harness.run();

    harness
        .get_by_role_and_label(egui::Role::Button, "Rostering")
        .click();
    harness.run();

    assert_eq!(harness.state().page(), Page::Rostering);
}

#[test]
fn settings_offers_all_appearance_modes() {
    let mut app = CareLibreApp::new();
    app.set_page(Page::Settings);

    let mut harness = harness(app);
    harness.run();

    for label in ["Light", "Dark", "Follow System"] {
        harness.get_by_label(label);
    }
}

#[test]
fn pointer_clicks_work_across_the_window() {
    let mut app = CareLibreApp::new();
    app.set_page(Page::Home);
    app.set_client_side_decorations(true);

    let mut harness = harness(app);
    harness.run();

    let button = harness.get_by_label("Sign in to the server");
    let centre = button.rect().center();
    assert!(
        centre.x > 800.0 && centre.y > 500.0,
        "expected a button in the lower-right quadrant, got {centre:?}"
    );
    button.click();
    harness.run();

    assert_eq!(harness.state().page(), Page::Settings);
}

#[test]
fn multiple_contacts_open_as_dock_tabs() {
    let mut app = CareLibreApp::new();
    app.open_contact("first-contact");
    app.open_contact("second-contact");
    // Re-opening an existing record must focus its tab, not duplicate it.
    app.open_contact("first-contact");

    assert_eq!(
        app.docked_contact_ids(),
        vec!["first-contact".to_owned(), "second-contact".to_owned()]
    );
}

#[test]
fn open_contacts_render_the_sliding_dock() {
    let mut app = CareLibreApp::new();
    app.open_contact("first-contact");

    let mut harness = harness(app);
    harness.run();

    harness.get_by_label("OPEN RECORDS");
}

#[test]
fn pages_can_pop_out_and_dock() {
    let mut app = CareLibreApp::new();
    app.set_page(Page::Rostering);
    app.set_client_side_decorations(false);

    let mut harness = harness(app);
    harness.run();

    harness.get_by_label("Pop out page").click();
    harness.run_steps(2);
    assert!(harness.state().page_popped_out(Page::Rostering));

    harness.get_by_label("Dock page").click_accesskit();
    harness.run_steps(2);
    assert!(!harness.state().page_popped_out(Page::Rostering));
}
