//! Landing dashboard for the prototype.

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Layout, Rect, RichText, Sense, Stroke,
    StrokeKind, Ui, pos2, vec2,
};

use crate::pages::{Action, Page};
use crate::store::ContactCounts;
use crate::theme;

pub fn show(
    ui: &mut Ui,
    user_name: Option<&str>,
    organisation: Option<&str>,
    counts: ContactCounts,
    signed_in: bool,
    server_label: &str,
) -> Option<Action> {
    let mut action: Option<Action> = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            header(ui, user_name, organisation, signed_in);
            ui.add_space(16.0);
            stats(ui, counts);
            ui.add_space(14.0);

            let total_width = ui.available_width();
            let gap = 18.0;
            let right_width = 340.0f32.min(total_width * 0.34);
            let left_width = (total_width - right_width - gap).max(360.0);

            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    vec2(left_width, 0.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        quick_actions(ui, &mut action);
                        ui.add_space(22.0);
                        getting_started(ui);
                    },
                );

                ui.add_space(gap);

                ui.allocate_ui_with_layout(
                    vec2(right_width, 0.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        data_source_card(
                            ui,
                            &mut action,
                            counts,
                            signed_in,
                            organisation,
                            server_label,
                        );
                    },
                );
            });

            ui.add_space(24.0);
        });

    action
}

fn header(ui: &mut Ui, user_name: Option<&str>, organisation: Option<&str>, signed_in: bool) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            let salutation = match greeting_hour() {
                5..=11 => "Good morning",
                12..=17 => "Good afternoon",
                _ => "Good evening",
            };
            let greeting = match user_name.filter(|name| !name.is_empty()) {
                Some(name) => format!("{salutation}, {name}"),
                None => salutation.to_owned(),
            };
            let subtitle = match organisation {
                Some(org) => format!("Here's what's happening at {org} today."),
                None => "Here's what's happening across your care operation today.".to_owned(),
            };
            ui.label(RichText::new(greeting).font(theme::strong_font(28.0)));
            ui.label(RichText::new(subtitle).color(theme::muted()).size(13.5));
        });

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if signed_in {
                theme::pill(ui, "LIVE SERVER DATA", theme::accent());
            } else {
                theme::pill(ui, "SIGN IN TO LOAD DATA", theme::warning());
            }
        });
    });
}

fn stats(ui: &mut Ui, counts: ContactCounts) {
    let cards = [
        (
            "Contacts",
            counts.all,
            "Live from the server",
            theme::accent(),
        ),
        (
            "Clients",
            counts.clients,
            "Client role",
            theme::accent_blue(),
        ),
        ("Workers", counts.workers, "Worker role", theme::accent()),
        (
            "Other contacts",
            counts.other,
            "Support network",
            theme::warning(),
        ),
    ];

    ui.columns(4, |cols| {
        for (col, (label, value, note, colour)) in cols.iter_mut().zip(cards) {
            stat_card(col, label, &value.to_string(), note, colour);
        }
    });
}

fn stat_card(ui: &mut Ui, label: &str, value: &str, note: &str, colour: Color32) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_min_height(74.0);

        ui.horizontal(|ui| {
            ui.label(RichText::new(label).color(theme::muted()).size(12.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.0, colour);
            });
        });

        ui.add_space(2.0);
        ui.label(RichText::new(value).font(theme::strong_font(30.0)));
        ui.label(RichText::new(note).color(colour).size(11.5));
    });
}

fn quick_actions(ui: &mut Ui, action: &mut Option<Action>) {
    section_header(ui, "Quick actions", "Common jobs, one click away");

    let items: [(&str, &str, &str, Option<Page>); 6] = [
        (
            "👤",
            "New client",
            "Participant record",
            Some(Page::Contacts),
        ),
        (
            "📅",
            "New shift",
            "Add to the roster",
            Some(Page::Rostering),
        ),
        ("📝", "Start intake", "Wizard intake", Some(Page::Intake)),
        ("✨", "Quote with AI", "Natural language", None),
        (
            "📄",
            "Merge document",
            "From a template",
            Some(Page::Documents),
        ),
        (
            "📋",
            "Open roster",
            "Full roster view",
            Some(Page::Rostering),
        ),
    ];

    ui.columns(3, |cols| {
        for (i, (icon, title, description, target)) in items.into_iter().enumerate() {
            if action_tile(&mut cols[i % 3], icon, title, description).clicked() {
                *action = Some(match target {
                    Some(page) => Action::Navigate(page),
                    None => Action::Notice(
                        "Natural-language quoting arrives in a future build.".to_owned(),
                    ),
                });
            }
        }
    });
}

fn action_tile(ui: &mut Ui, icon: &str, title: &str, description: &str) -> egui::Response {
    let size = vec2(ui.available_width(), 62.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());

    let hovered = response.hovered();
    let fill = if hovered {
        theme::hover()
    } else {
        theme::surface()
    };
    let stroke = Stroke::new(
        1.0,
        if hovered {
            theme::accent().gamma_multiply(0.7)
        } else {
            theme::border()
        },
    );

    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(12),
        fill,
        stroke,
        StrokeKind::Inside,
    );

    let badge = Rect::from_center_size(rect.left_center() + vec2(36.0, 0.0), vec2(38.0, 38.0));
    painter.rect_filled(
        badge,
        CornerRadius::same(10),
        if hovered {
            theme::accent_soft(40)
        } else {
            theme::surface_raised()
        },
    );
    painter.text(
        badge.center(),
        Align2::CENTER_CENTER,
        icon,
        FontId::proportional(17.0),
        theme::text(),
    );

    painter.text(
        pos2(rect.left() + 66.0, rect.center().y - 10.0),
        Align2::LEFT_CENTER,
        title,
        theme::strong_font(14.0),
        theme::text(),
    );
    painter.text(
        pos2(rect.left() + 66.0, rect.center().y + 10.0),
        Align2::LEFT_CENTER,
        description,
        FontId::proportional(11.5),
        theme::muted(),
    );

    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), title));

    response
}

fn getting_started(ui: &mut Ui) {
    section_header(ui, "Getting started", "The build path for the desktop app");

    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());

        let steps = [
            ("Native shell", "egui on wgpu, Rust only", true),
            ("Home dashboard", "You are looking at it", true),
            ("Server contacts", "Live API data and custom fields", true),
            (
                "Guided intake",
                "Saves onboarding records to the server",
                true,
            ),
            ("Rostering", "The next major vertical", false),
        ];

        for (title, subtitle, done) in steps {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(20.0, 24.0), Sense::hover());
                let centre = rect.center();
                if done {
                    ui.painter()
                        .circle_filled(centre, 8.5, theme::accent_soft(60));
                    ui.painter()
                        .circle_stroke(centre, 7.5, Stroke::new(1.0, theme::accent()));
                    let tick = Stroke::new(1.5, theme::accent());
                    ui.painter()
                        .line_segment([centre + vec2(-3.5, 0.0), centre + vec2(-1.0, 2.5)], tick);
                    ui.painter()
                        .line_segment([centre + vec2(-1.0, 2.5), centre + vec2(4.0, -3.0)], tick);
                } else {
                    ui.painter()
                        .circle_stroke(centre, 7.5, Stroke::new(1.0, theme::border()));
                }

                ui.add_space(2.0);
                ui.label(
                    RichText::new(title)
                        .color(if done { theme::text() } else { theme::muted() })
                        .font(theme::strong_font(12.5)),
                );
                ui.label(RichText::new(subtitle).color(theme::faint()).size(11.0));

                if !done {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        theme::pill(ui, "PLANNED", theme::faint());
                    });
                }
            });
        }
    });
}

fn data_source_card(
    ui: &mut Ui,
    action: &mut Option<Action>,
    counts: ContactCounts,
    signed_in: bool,
    organisation: Option<&str>,
    server_label: &str,
) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());

        ui.horizontal(|ui| {
            ui.label(RichText::new("Data source").font(theme::strong_font(13.5)));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                theme::pill(
                    ui,
                    if signed_in { "SERVER" } else { "OFFLINE" },
                    if signed_in { theme::success() } else { theme::warning() },
                );
            });
        });
        ui.add_space(8.0);

        row(ui, "Connection", server_label);
        row(
            ui,
            "Organisation",
            organisation.unwrap_or("Not signed in"),
        );
        row(ui, "Contacts", &counts.all.to_string());
        row(ui, "Custom fields", "Database metadata");

        ui.add_space(8.0);
        ui.label(
            RichText::new(
                "Contacts, profile fields and custom fields are read from the CareLibre server. Edits are saved back to the server; nothing is mocked locally.",
            )
            .color(theme::faint())
            .size(10.0),
        );

        if !signed_in {
            ui.add_space(10.0);
            if ui.add(theme::primary_button("Sign in to the server")).clicked() {
                *action = Some(Action::Navigate(Page::Settings));
            }
        }
    });
}

fn row(ui: &mut Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(theme::muted()).size(10.5));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(value).color(theme::text()).size(10.5));
        });
    });
    ui.add_space(2.0);
}

fn section_header(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).font(theme::strong_font(15.0)));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(subtitle).color(theme::faint()).size(11.0));
        });
    });
    ui.add_space(6.0);
}

fn greeting_hour() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| (d.as_secs() / 3600) % 24)
        .unwrap_or(9)
}
