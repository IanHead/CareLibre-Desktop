//! Landing dashboard for the prototype.

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Layout, Rect, RichText, Sense, Stroke,
    StrokeKind, Ui, pos2, vec2,
};

use crate::pages::{Action, Page};
use crate::theme;

pub fn show(ui: &mut Ui, user_name: Option<&str>, organisation: Option<&str>) -> Option<Action> {
    let mut action: Option<Action> = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            header(ui, user_name, organisation);
            ui.add_space(16.0);
            stats(ui);
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
                        todays_roster(ui, &mut action);
                        ui.add_space(10.0);
                        recent_activity(ui);
                    },
                );
            });

            ui.add_space(24.0);
        });

    action
}

fn header(ui: &mut Ui, user_name: Option<&str>, organisation: Option<&str>) {
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
            ui.label(RichText::new(subtitle).color(theme::MUTED).size(13.5));
        });

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            theme::pill(ui, "PROTOTYPE · SAMPLE DATA", theme::WARNING);
        });
    });
}

fn stats(ui: &mut Ui) {
    let cards = [
        ("Active clients", "128", "+6 this month", theme::ACCENT),
        ("Support workers", "64", "+2 this month", theme::ACCENT_BLUE),
        ("Shifts today", "37", "4 in progress", theme::WARNING),
        ("Unallocated", "5", "Needs review", theme::DANGER),
    ];

    ui.columns(4, |cols| {
        for (col, (label, value, note, colour)) in cols.iter_mut().zip(cards) {
            stat_card(col, label, value, note, colour);
        }
    });
}

fn stat_card(ui: &mut Ui, label: &str, value: &str, note: &str, colour: Color32) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_min_height(74.0);

        ui.horizontal(|ui| {
            ui.label(RichText::new(label).color(theme::MUTED).size(12.0));
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
            Some(Page::Clients),
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
        Color32::from_rgb(28, 42, 66)
    } else {
        theme::SURFACE
    };
    let stroke = Stroke::new(
        1.0,
        if hovered {
            theme::ACCENT.gamma_multiply(0.7)
        } else {
            theme::BORDER
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
            theme::SURFACE_RAISED
        },
    );
    painter.text(
        badge.center(),
        Align2::CENTER_CENTER,
        icon,
        FontId::proportional(17.0),
        theme::TEXT,
    );

    painter.text(
        pos2(rect.left() + 66.0, rect.center().y - 10.0),
        Align2::LEFT_CENTER,
        title,
        theme::strong_font(14.0),
        theme::TEXT,
    );
    painter.text(
        pos2(rect.left() + 66.0, rect.center().y + 10.0),
        Align2::LEFT_CENTER,
        description,
        FontId::proportional(11.5),
        theme::MUTED,
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
            ("Database backend", "PostgreSQL, MySQL or MariaDB", false),
            ("Contacts & rostering", "The core care records", false),
            (
                "Quoting & documents",
                "Templates and natural language",
                false,
            ),
        ];

        for (title, subtitle, done) in steps {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(20.0, 24.0), Sense::hover());
                let centre = rect.center();
                if done {
                    ui.painter()
                        .circle_filled(centre, 8.5, theme::accent_soft(60));
                    ui.painter()
                        .circle_stroke(centre, 7.5, Stroke::new(1.0, theme::ACCENT));
                    let tick = Stroke::new(1.5, theme::ACCENT);
                    ui.painter()
                        .line_segment([centre + vec2(-3.5, 0.0), centre + vec2(-1.0, 2.5)], tick);
                    ui.painter()
                        .line_segment([centre + vec2(-1.0, 2.5), centre + vec2(4.0, -3.0)], tick);
                } else {
                    ui.painter()
                        .circle_stroke(centre, 7.5, Stroke::new(1.0, theme::BORDER));
                }

                ui.add_space(2.0);
                ui.label(
                    RichText::new(title)
                        .color(if done { theme::TEXT } else { theme::MUTED })
                        .font(theme::strong_font(12.5)),
                );
                ui.label(RichText::new(subtitle).color(theme::FAINT).size(11.0));

                if !done {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        theme::pill(ui, "PLANNED", theme::FAINT);
                    });
                }
            });
        }
    });
}

fn todays_roster(ui: &mut Ui, action: &mut Option<Action>) {
    section_header(ui, "Today's roster", "Sample data");

    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());

        let shifts = [
            (
                "09:00 – 12:00",
                "Margaret Chen",
                "Sam Taylor",
                theme::SUCCESS,
                "Confirmed",
            ),
            (
                "13:30 – 15:00",
                "Noah Williams",
                "Priya Nair",
                theme::WARNING,
                "In progress",
            ),
            (
                "16:00 – 18:00",
                "Unassigned",
                "—",
                theme::DANGER,
                "Needs worker",
            ),
        ];

        for (i, (time, client, worker, colour, status)) in shifts.into_iter().enumerate() {
            if i > 0 {
                ui.add_space(2.0);
                ui.separator();
                ui.add_space(6.0);
            }

            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    let detail = if worker == "—" {
                        time.to_owned()
                    } else {
                        format!("{time} · {worker}")
                    };
                    ui.label(
                        RichText::new(detail)
                            .color(theme::MUTED)
                            .size(10.5)
                            .monospace(),
                    );
                    ui.label(RichText::new(client).font(theme::strong_font(13.5)));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    theme::pill(ui, status, colour);
                });
            });
        }

        ui.add_space(6.0);
        if ui
            .add(
                egui::Button::new(
                    RichText::new("Open full roster  →")
                        .color(theme::ACCENT)
                        .size(12.0),
                )
                .frame(false),
            )
            .clicked()
        {
            *action = Some(Action::Navigate(Page::Rostering));
        }
    });
}

fn recent_activity(ui: &mut Ui) {
    egui::Frame::new()
        .fill(theme::SURFACE)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(egui::Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            ui.horizontal(|ui| {
                ui.label(RichText::new("Recent activity").font(theme::strong_font(13.5)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new("Latest changes")
                            .color(theme::FAINT)
                            .size(10.5),
                    );
                });
            });
            ui.add_space(4.0);

            let activity = [
                (
                    theme::ACCENT,
                    "Shift published",
                    "Margaret Chen · 09:00 – 12:00",
                    "5m",
                ),
                (
                    theme::ACCENT_BLUE,
                    "Intake started",
                    "Noah Williams · new client",
                    "22m",
                ),
            ];

            for (i, (colour, title, detail, age)) in activity.into_iter().enumerate() {
                if i > 0 {
                    ui.add_space(6.0);
                }

                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                    ui.painter().circle_filled(rect.center(), 4.0, colour);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(title).font(theme::strong_font(11.5)));
                        ui.label(RichText::new(detail).color(theme::FAINT).size(10.0));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(age)
                                .color(theme::FAINT)
                                .size(10.0)
                                .monospace(),
                        );
                    });
                });
            }
        });
}

fn section_header(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).font(theme::strong_font(15.0)));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(subtitle).color(theme::FAINT).size(11.0));
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
