use carelibre_desktop::{CareLibreApp, pages::Page, theme};
use eframe::egui;
use egui_kittest::{Harness, kittest::Queryable};

fn harness(app: CareLibreApp) -> Harness<'static, CareLibreApp> {
    Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_context_setup(theme::apply)
        .build_ui_state(|ui, app| app.show(ui), app)
}

#[test]
fn every_page_renders() {
    for page in [
        Page::Home,
        Page::Clients,
        Page::Workers,
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
fn sidebar_navigation_works() {
    let mut harness = harness(CareLibreApp::new());
    harness.run();

    harness.get_by_label("Rostering").click();
    harness.run();

    assert_eq!(harness.state().page(), Page::Rostering);
}
