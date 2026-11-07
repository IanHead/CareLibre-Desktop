//! Shared placeholder for areas that are not built yet.

use eframe::egui::{self, Align, Align2, CornerRadius, FontId, Layout, RichText, Sense, Ui, vec2};

use crate::pages::{Action, Page};
use crate::theme;

pub fn show(ui: &mut Ui, page: Page) -> Option<Action> {
    let mut action = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(52.0, 52.0), Sense::hover());
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(14), theme::surface_raised());
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    page.icon(),
                    FontId::proportional(22.0),
                    theme::text(),
                );

                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(page.label()).font(theme::strong_font(26.0)));
                        theme::pill(ui, "PLANNED", theme::warning());
                    });
                    ui.label(RichText::new(page.blurb()).color(theme::muted()).size(13.0));
                });
            });

            ui.add_space(24.0);

            let width = ui.available_width();
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    vec2((width - 18.0) * 0.62, 0.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        theme::card().show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new("What's coming").font(theme::strong_font(15.0)));
                            ui.add_space(10.0);

                            for feature in page.features() {
                                ui.horizontal(|ui| {
                                    let (rect, _) =
                                        ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                                    ui.painter()
                                        .circle_filled(rect.center(), 3.0, theme::accent());
                                    ui.label(
                                        RichText::new(*feature).color(theme::muted()).size(12.5),
                                    );
                                });
                                ui.add_space(3.0);
                            }
                        });
                    },
                );

                ui.add_space(18.0);

                ui.allocate_ui_with_layout(
                    vec2((width - 18.0) * 0.38, 0.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        theme::card().show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(
                                RichText::new("Prototype scope").font(theme::strong_font(15.0)),
                            );
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(
                                    "This build focuses on the native desktop shell and the \
                                     home dashboard. Each module will be wired up to the \
                                     shared CareLibre API as it lands.",
                                )
                                .color(theme::muted())
                                .size(12.5),
                            );
                            ui.add_space(12.0);
                            ui.label(
                                RichText::new("Rust · egui · wgpu · no web view")
                                    .color(theme::faint())
                                    .size(11.0)
                                    .monospace(),
                            );
                        });

                        ui.add_space(14.0);

                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("←  Back to home")
                                        .color(theme::on_accent())
                                        .font(theme::strong_font(13.0)),
                                )
                                .fill(theme::accent())
                                .corner_radius(CornerRadius::same(8))
                                .min_size(vec2(150.0, 34.0)),
                            )
                            .clicked()
                        {
                            action = Some(Action::Navigate(Page::Home));
                        }
                    },
                );
            });

            ui.add_space(24.0);
        });

    action
}
