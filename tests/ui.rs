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
        .with_max_steps(20)
        .with_context_setup(|ctx| theme::apply(ctx, ThemeMode::Dark))
        .build_ui_state(|ui, app| app.show(ui), app)
}

#[test]
fn every_page_renders() {
    for page in [
        Page::Contacts,
        Page::Rostering,
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
fn launch_opens_no_pages_and_centres_the_menu() {
    let mut harness = harness(CareLibreApp::new());
    harness.run();

    assert!(!harness.state().page_column_open());
    assert!(harness.state().docked_contact_ids().is_empty());

    let menu = harness.get_by_label("WORKSPACE");
    let centre = menu.rect().center();
    assert!(
        centre.x > 400.0,
        "expected the menu to start centred, got {centre:?}"
    );

    harness
        .get_by_role_and_label(egui::Role::Button, "Rostering")
        .click();
    harness.run();

    let menu = harness.get_by_label("WORKSPACE");
    let centre = menu.rect().center();
    assert!(
        centre.x < 200.0,
        "expected the menu to move to the left, got {centre:?}"
    );
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
    app.set_page(Page::Contacts);
    app.set_client_side_decorations(true);

    let mut harness = harness(app);
    harness.run();

    let button = harness.get_by_label("Sign in");
    let centre = button.rect().center();
    assert!(
        centre.x > 800.0,
        "expected a button on the right-hand side, got {centre:?}"
    );
    button.click();
    harness.run();

    assert_eq!(harness.state().page(), Page::Settings);

    let contacts = harness.get_by_role_and_label(egui::Role::Button, "Contacts");
    let centre = contacts.rect().center();
    assert!(
        centre.x < 300.0 && centre.y > 80.0,
        "expected a button in the left-hand pane, got {centre:?}"
    );
    contacts.click();
    harness.run();

    assert_eq!(harness.state().page(), Page::Contacts);
}

#[test]
fn multiple_contacts_open_as_columns() {
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
fn pages_can_be_closed_and_reopened() {
    let mut app = CareLibreApp::new();
    app.set_page(Page::Contacts);

    let mut harness = harness(app);
    harness.run();
    assert!(harness.state().page_column_open());

    harness.get_by_label("Close column").click();
    harness.run();
    assert!(!harness.state().page_column_open());

    harness.get_by_label("Contacts").click();
    harness.run();
    assert!(harness.state().page_column_open());
    assert_eq!(harness.state().page(), Page::Contacts);
}

#[test]
fn close_all_closes_pages_and_records() {
    let mut app = CareLibreApp::new();
    app.set_page(Page::Contacts);
    app.open_contact("first-contact");

    let mut harness = harness(app);
    harness.run();
    harness
        .get_by_role_and_label(egui::Role::Button, "Rostering")
        .click();
    harness.run();

    harness.get_by_label("Close all").click();
    harness.run();

    assert!(!harness.state().page_column_open());
    assert!(harness.state().docked_contact_ids().is_empty());
    assert_eq!(harness.query_all_by_label("Pop out page").count(), 0);
}

#[test]
fn pages_are_limited_to_the_window() {
    let mut harness = harness(CareLibreApp::new());
    harness.run();

    for page in ["Contacts", "Rostering", "Services & rates", "Documents"] {
        harness
            .get_by_role_and_label(egui::Role::Button, page)
            .click();
        harness.run();
    }
    assert_eq!(harness.get_all_by_label("Pop out page").count(), 4);

    harness
        .get_by_role_and_label(egui::Role::Button, "Communications")
        .click();
    harness.run();

    assert_eq!(harness.get_all_by_label("Pop out page").count(), 4);
}

#[test]
fn multiple_contact_columns_render_without_id_clashes() {
    let mut app = CareLibreApp::new();
    app.open_contact("first-contact");
    app.open_contact("second-contact");
    app.open_contact("third-contact");

    let mut harness = harness(app);
    harness.run_steps(3);
    assert_eq!(harness.state().docked_contact_ids().len(), 3);
}

#[test]
fn open_columns_are_limited() {
    let mut app = CareLibreApp::new();
    for index in 0..6 {
        app.open_contact(&format!("contact-{index}"));
    }

    // No page is open, so the first four records fill the window's slots.
    assert_eq!(app.docked_contact_ids().len(), 4);
}

#[test]
fn open_contacts_render_the_column_strip() {
    let mut app = CareLibreApp::new();
    app.open_contact("first-contact");

    let mut harness = harness(app);
    harness.run();

    harness.get_by_label("Pop out record");
}

#[test]
fn pages_can_pop_out_and_dock() {
    let mut app = CareLibreApp::new();
    app.set_page(Page::Rostering);
    app.set_client_side_decorations(false);

    let mut harness = harness(app);
    harness.run();

    harness
        .get_all_by_label("Pop out page")
        .next()
        .unwrap()
        .click();
    harness.run_steps(2);
    assert!(harness.state().page_popped_out(Page::Rostering));

    harness.get_by_label("Dock page").click_accesskit();
    harness.run_steps(2);
    assert!(!harness.state().page_popped_out(Page::Rostering));
}
