use carelibre_desktop::{
    CareLibreApp,
    api::{AuthSession, OrganisationProfile, UserProfile},
    pages::{
        Page,
        constellation::{ConstellationSource, RelationshipNetwork},
        contact_editor::{ContactWindowState, show_column},
    },
    theme::{self, ThemeMode},
};
use eframe::egui;
use egui_kittest::{Harness, kittest::Queryable};

fn test_auth() -> AuthSession {
    AuthSession {
        access_token: "test-token".to_owned(),
        refresh_token: "test-refresh".to_owned(),
        token_type: "Bearer".to_owned(),
        expires_in: 900,
        user: UserProfile {
            id: "user-1".to_owned(),
            email: "owner@example.com".to_owned(),
            full_name: "Test Owner".to_owned(),
        },
        organisation: OrganisationProfile {
            id: "org-test".to_owned(),
            slug: "test".to_owned(),
            name: "Test Organisation".to_owned(),
            edition: "community".to_owned(),
            role: "owner".to_owned(),
        },
    }
}

fn harness_with(app: CareLibreApp, signed_in: bool) -> Harness<'static, CareLibreApp> {
    let mut app = app;
    // Sidebar tests start from the expanded menu regardless of the developer's
    // saved preference.
    app.set_menu_collapsed(false);
    if signed_in {
        app.use_session(test_auth());
    }
    app.set_theme_mode(ThemeMode::Dark);
    Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_max_steps(20)
        .with_context_setup(|ctx| theme::apply(ctx, ThemeMode::Dark))
        .build_ui_state(|ui, app| app.show(ui), app)
}

fn harness(app: CareLibreApp) -> Harness<'static, CareLibreApp> {
    harness_with(app, true)
}

#[test]
fn every_page_renders() {
    for page in [
        Page::Contacts,
        Page::Relationships,
        Page::Organisations,
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
fn launch_while_signed_out_shows_the_sign_in_form_instead_of_the_menu() {
    let mut harness = harness_with(CareLibreApp::new(), false);
    harness.run();

    assert!(!harness.state().signed_in());
    assert!(!harness.state().page_column_open());
    assert!(harness.state().docked_contact_ids().is_empty());

    assert_eq!(harness.query_all_by_label("WORKSPACE").count(), 0);
    assert_eq!(
        harness.query_all_by_label("Create organisation").count(),
        1,
        "the sign-in form should offer account creation"
    );
    assert!(
        harness.query_all_by_label("Sign in").count() >= 1,
        "the sign-in form should be visible"
    );

    // The form occupies the main menu's centred slot on a fresh launch.
    let sign_in = harness
        .get_all_by_label("Sign in")
        .next()
        .expect("the sign-in form");
    let centre = sign_in.rect().center();
    assert!(
        centre.x > 300.0 && centre.x < 980.0,
        "expected the form to be centred, got {centre:?}"
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
fn sidebar_opens_the_relationship_explorer() {
    let mut harness = harness(CareLibreApp::new());
    harness.run();

    harness
        .get_by_role_and_label(egui::Role::Button, "Relationships")
        .click();
    harness.run();

    assert_eq!(harness.state().page(), Page::Relationships);
    assert!(
        harness
            .query_all_by_label_contains("Choose a starting point")
            .next()
            .is_some(),
        "a fresh explorer asks for a starting party"
    );
}

#[test]
fn launcher_menu_is_not_collapsible() {
    let mut harness = harness(CareLibreApp::new());
    harness.run();

    assert_eq!(harness.query_all_by_label("Collapse menu").count(), 0);
    assert_eq!(harness.query_all_by_label("Expand menu").count(), 0);
}

#[test]
fn ctrl_b_toggles_the_docked_menu() {
    let mut app = CareLibreApp::new();
    app.set_page(Page::Contacts);
    let mut harness = harness(app);
    harness.run();

    assert!(!harness.state().menu_collapsed());
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::B);
    harness.run_steps(40);
    assert!(harness.state().menu_collapsed());

    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::B);
    harness.run_steps(40);
    assert!(!harness.state().menu_collapsed());
}

#[test]
fn ctrl_b_does_not_collapse_the_launcher() {
    let mut harness = harness(CareLibreApp::new());
    harness.run();

    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::B);
    harness.run();

    assert!(!harness.state().menu_collapsed());
}

#[test]
fn rail_icons_show_instant_bubble_tooltips() {
    let mut app = CareLibreApp::new();
    app.set_page(Page::Contacts);
    let mut harness = harness(app);
    harness.state_mut().set_menu_collapsed(true);
    harness.run_steps(40);

    harness
        .get_by_role_and_label(egui::Role::Button, "Contacts")
        .hover();
    harness.run();

    assert_eq!(
        harness
            .query_all_by_label_contains("Everyone the organisation works with")
            .count(),
        1,
        "hovering a rail icon should show its description straight away"
    );
}

#[test]
fn docked_menu_collapses_to_a_rail_and_expands_again() {
    let mut app = CareLibreApp::new();
    app.set_page(Page::Contacts);
    let mut harness = harness(app);
    harness.run();

    assert!(!harness.state().menu_collapsed());
    let expanded_contacts = harness
        .get_by_role_and_label(egui::Role::Button, "Contacts")
        .rect();
    assert!(
        expanded_contacts.right() > 150.0,
        "expected the full docked menu, got {expanded_contacts:?}"
    );

    harness
        .get_by_role_and_label(egui::Role::Button, "Collapse menu")
        .click();
    harness.run_steps(40);

    assert!(harness.state().menu_collapsed());
    let rail_contacts = harness
        .get_by_role_and_label(egui::Role::Button, "Contacts")
        .rect();
    assert!(
        rail_contacts.right() < 100.0,
        "expected the icon rail, got {rail_contacts:?}"
    );
    assert!(rail_contacts.width() < expanded_contacts.width());

    // Navigation stays available from the rail and keeps it collapsed.
    harness
        .get_by_role_and_label(egui::Role::Button, "Rostering")
        .click();
    harness.run();
    assert_eq!(harness.state().page(), Page::Rostering);
    assert!(harness.state().menu_collapsed());

    harness
        .get_by_role_and_label(egui::Role::Button, "Expand menu")
        .click();
    harness.run_steps(40);

    assert!(!harness.state().menu_collapsed());
    let expanded_again = harness
        .get_by_role_and_label(egui::Role::Button, "Contacts")
        .rect();
    assert!(expanded_again.width() > rail_contacts.width());
}

#[test]
fn docked_menu_hides_page_descriptions() {
    let mut harness = harness(CareLibreApp::new());
    harness.run();

    // The full-width launch menu reserves a second line for each description.
    let launch = harness
        .get_by_role_and_label(egui::Role::Button, "Contacts")
        .rect();
    assert!(
        launch.height() > 40.0,
        "expected the launch menu to show page descriptions, got {launch:?}"
    );

    // Opening a page docks the menu beside the work area.
    harness
        .get_by_role_and_label(egui::Role::Button, "Rostering")
        .click();
    harness.run_steps(40);

    let docked = harness
        .get_by_role_and_label(egui::Role::Button, "Contacts")
        .rect();
    assert!(
        docked.height() < 40.0 && docked.height() < launch.height(),
        "the docked menu should hide page descriptions, launch {launch:?} docked {docked:?}"
    );
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
fn settings_shows_the_account_but_never_the_sign_in_form() {
    let mut app = CareLibreApp::new();
    app.set_page(Page::Settings);

    let mut harness = harness(app);
    harness.run();

    harness.get_by_label("Sign out");
    assert_eq!(harness.query_all_by_label("Sign in").count(), 0);
    assert_eq!(harness.query_all_by_label("Create organisation").count(), 0);
}

#[test]
fn pointer_clicks_work_across_the_window() {
    let mut app = CareLibreApp::new();
    app.set_page(Page::Contacts);
    app.set_client_side_decorations(true);

    let mut harness = harness(app);
    harness.run();

    let contacts = harness.get_by_role_and_label(egui::Role::Button, "Contacts");
    let centre = contacts.rect().center();
    assert!(
        centre.x < 300.0 && centre.y > 80.0,
        "expected a button in the left-hand pane, got {centre:?}"
    );
    contacts.click();
    harness.run();

    assert_eq!(harness.state().page(), Page::Contacts);

    let rostering = harness.get_by_role_and_label(egui::Role::Button, "Rostering");
    let centre = rostering.rect().center();
    assert!(
        centre.x < 300.0 && centre.y > 80.0,
        "expected a button in the left-hand pane, got {centre:?}"
    );
    rostering.click();
    harness.run();

    assert_eq!(harness.state().page(), Page::Rostering);
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
fn close_all_button_animates_and_makes_room() {
    let mut app = CareLibreApp::new();
    app.set_page(Page::Contacts);
    app.use_session(test_auth());
    app.set_theme_mode(ThemeMode::Dark);
    app.set_menu_collapsed(false);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_max_steps(40)
        .with_step_dt(1.0 / 60.0)
        .with_context_setup(|ctx| theme::apply(ctx, ThemeMode::Dark))
        .build_ui_state(|ui, app| app.show(ui), app);
    harness.run();

    assert_eq!(
        harness.query_all_by_label("Close all").count(),
        0,
        "Close all should be hidden with a single column"
    );
    let search_single = harness
        .get_all_by_role(egui::Role::TextInput)
        .next()
        .expect("the top-bar search")
        .rect();

    harness
        .get_by_role_and_label(egui::Role::Button, "Rostering")
        .click();
    harness.run_steps(4);

    assert_eq!(
        harness.query_all_by_label("Close all").count(),
        1,
        "Close all should appear when a second column opens"
    );
    let search_partial = harness
        .get_all_by_role(egui::Role::TextInput)
        .next()
        .expect("the top-bar search")
        .rect();
    assert!(
        search_partial.right() < search_single.right() - 2.0,
        "expected the search box to start moving left as the button appears"
    );

    harness.run();
    let search_after = harness
        .get_all_by_role(egui::Role::TextInput)
        .next()
        .expect("the top-bar search")
        .rect();
    assert!(
        search_after.right() < search_partial.right() - 2.0,
        "expected the search box to keep moving left as the button grows"
    );
    assert!(
        search_single.right() - search_after.right() > 60.0,
        "expected the finished button to make room for its full width"
    );
    assert_eq!(
        harness.query_all_by_label("Close all").count(),
        1,
        "Close all should stay visible while several columns are open"
    );
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
    assert!(
        !harness.state().page_column_open(),
        "a popped-out page should not keep a column in the main window"
    );
    assert_eq!(
        harness
            .query_all_by_label("This page is open in its own window.")
            .count(),
        0,
        "a popped-out page should not leave a placeholder behind"
    );

    harness.get_by_label("Dock").click_accesskit();
    harness.run_steps(2);
    assert!(!harness.state().page_popped_out(Page::Rostering));
    assert!(harness.state().page_column_open());
}

#[test]
fn popped_out_pages_free_their_column_for_other_pages() {
    let mut app = CareLibreApp::new();
    app.set_page(Page::Rostering);
    for page in [Page::Contacts, Page::Services, Page::Documents] {
        app.open_page_column(page);
    }
    assert_eq!(app.docked_page_columns().len(), 4);

    app.pop_out_page_column(Page::Rostering);
    assert!(app.page_popped_out(Page::Rostering));
    assert_eq!(
        app.docked_page_columns(),
        vec![Page::Documents, Page::Services, Page::Contacts]
    );
    assert_eq!(app.page(), Page::Documents);

    // The freed slot can be used by another page while Rostering is detached.
    app.open_page_column(Page::Communications);
    assert!(
        app.docked_page_columns().contains(&Page::Communications),
        "the column limit should count only columns docked in the main window"
    );

    // Selecting the detached page focuses its window rather than opening a
    // placeholder column in the main window.
    let page_before_focus = app.page();
    app.open_page_column(Page::Rostering);
    assert_eq!(app.page(), page_before_focus);
    assert_eq!(app.docked_page_columns().len(), 4);

    // Close all also closes pages that live in their own windows.
    app.close_all();
    assert!(app.docked_page_columns().is_empty());
    assert!(!app.page_popped_out(Page::Rostering));
}

#[test]
fn contact_column_footer_stays_inside_the_column() {
    let mut state = ContactWindowState::new("ana".to_owned());
    state.loading = false;
    state.given_name = "Ana".to_owned();
    state.family_name = "Ruiz".to_owned();
    state.role = "other".to_owned();
    state.status = "Active".to_owned();

    let mut harness = Harness::builder()
        .with_size(egui::vec2(390.0, 600.0))
        .with_max_steps(5)
        .with_context_setup(|ctx| theme::apply(ctx, ThemeMode::Dark))
        .build_ui_state(
            |ui, state: &mut ContactWindowState| {
                let panel = ui.max_rect();
                let mut column_ui = ui.new_child(egui::UiBuilder::new().max_rect(panel));
                column_ui.set_clip_rect(panel);
                // The column layout sets a minimum height, which expands the
                // available rect past the clip. The footer must still fit.
                column_ui.set_min_height(panel.height() + 100.0);
                let nodes = [];
                let network = RelationshipNetwork {
                    edges: Vec::new(),
                    revision: 0,
                };
                let source = ConstellationSource {
                    nodes: &nodes,
                    network: &network,
                    loading: false,
                    error: None,
                };
                let _ = show_column(&mut column_ui, state, &source, &[]);
            },
            state,
        );
    harness.run();

    let save = harness.get_by_label("Save contact").rect();
    assert!(
        save.top() >= 0.0 && save.bottom() <= 600.0,
        "Save contact should stay inside the column, got {save:?}"
    );
}
