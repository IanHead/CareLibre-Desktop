//! Settings: server address, connection status and account.

use std::time::Instant;

use eframe::egui::{
    self, Align, Color32, CornerRadius, Layout, Margin, RichText, Sense, Stroke, Ui, vec2,
};

use crate::{
    pages::{Action, Page},
    server::{ConnectionState, ServerState, Session},
    theme::{self, ThemeMode},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    Login,
    Register,
}

pub struct SettingsForm {
    pub base_url: String,
    pub mode: AuthMode,
    pub email: String,
    pub password: String,
    pub login_slug: String,
    pub full_name: String,
    pub organisation_name: String,
    pub new_slug: String,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub client_side_decorations: bool,
    pub remember_login: bool,
    pub theme: ThemeMode,
}

impl SettingsForm {
    pub fn new(
        base_url: &str,
        theme: ThemeMode,
        client_side_decorations: bool,
        remember_login: bool,
    ) -> Self {
        Self {
            base_url: base_url.to_owned(),
            mode: AuthMode::Login,
            email: String::new(),
            password: String::new(),
            login_slug: String::new(),
            full_name: String::new(),
            organisation_name: String::new(),
            new_slug: String::new(),
            error: None,
            notice: None,
            client_side_decorations,
            remember_login,
            theme,
        }
    }

    pub fn on_signed_in(&mut self) {
        self.password.clear();
        self.error = None;
    }

    pub fn on_signed_out(&mut self) {
        self.password.clear();
        self.error = None;
    }
}

pub fn show(ui: &mut Ui, form: &mut SettingsForm, server: &ServerState) -> Option<Action> {
    let mut action = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            header(ui, server);
            ui.add_space(22.0);

            let width = ui.available_width();
            let gap = 18.0;

            if width < 720.0 {
                server_card(ui, form, server, &mut action);
                ui.add_space(14.0);
                account_card(ui, form, server, &mut action);
                ui.add_space(14.0);
                appearance_card(ui, form, &mut action);
            } else {
                let left_width = (width - gap) * 0.56;
                let right_width = width - gap - left_width;

                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        vec2(left_width, 0.0),
                        Layout::top_down(Align::Min),
                        |ui| {
                            server_card(ui, form, server, &mut action);
                        },
                    );

                    ui.add_space(gap);

                    ui.allocate_ui_with_layout(
                        vec2(right_width, 0.0),
                        Layout::top_down(Align::Min),
                        |ui| {
                            account_card(ui, form, server, &mut action);
                            ui.add_space(14.0);
                            appearance_card(ui, form, &mut action);
                        },
                    );
                });
            }

            ui.add_space(24.0);
        });

    action
}

fn appearance_card(ui: &mut Ui, form: &mut SettingsForm, action: &mut Option<Action>) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Application").font(theme::strong_font(15.0)));
        ui.label(
            RichText::new("Appearance preferences for this device.")
                .color(theme::muted())
                .size(11.0),
        );
        ui.add_space(10.0);

        ui.label(RichText::new("Appearance").color(theme::muted()).size(11.0));
        ui.horizontal(|ui| {
            let before = form.theme;
            for mode in ThemeMode::ALL {
                ui.selectable_value(&mut form.theme, mode, mode.label());
            }
            if form.theme != before {
                *action = Some(Action::SetTheme(form.theme));
            }
        });
        ui.label(
            RichText::new(
                "Follow System uses your desktop's light or dark preference and updates when it changes.",
            )
            .color(theme::faint())
            .size(10.0),
        );

        ui.add_space(12.0);
        let mut client_side = form.client_side_decorations;
        if ui
            .checkbox(
                &mut client_side,
                RichText::new("Use client-side window decorations").size(12.0),
            )
            .changed()
        {
            form.client_side_decorations = client_side;
            *action = Some(Action::SetClientSideDecorations(client_side));
        }
        ui.label(
            RichText::new(
                "Draw CareLibre's own title bar. Turn this off to use the operating system's native window decorations.",
            )
            .color(theme::faint())
            .size(10.0),
        );
    });
}

fn remember_me(ui: &mut Ui, form: &mut SettingsForm, action: &mut Option<Action>) {
    let mut remember = form.remember_login;
    if ui
        .checkbox(
            &mut remember,
            RichText::new("Remember me on this device").size(12.0),
        )
        .changed()
    {
        form.remember_login = remember;
        *action = Some(Action::SetRememberLogin(remember));
    }
    ui.label(
        RichText::new(
            "Stores a refresh token in the OS keychain (or an encrypted local vault when no keychain is available). Your password is never stored.",
        )
        .color(theme::faint())
        .size(10.0),
    );
}

fn header(ui: &mut Ui, server: &ServerState) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(52.0, 52.0), Sense::hover());
        ui.painter()
            .rect_filled(rect, CornerRadius::same(14), theme::surface_raised());
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            Page::Settings.icon(),
            egui::FontId::proportional(22.0),
            theme::text(),
        );

        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.label(RichText::new("Settings").font(theme::strong_font(26.0)));
            ui.label(
                RichText::new("Server connection, account and organisation.")
                    .color(theme::muted())
                    .size(13.0),
            );
        });

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let (label, colour) = theme::connection_style(&server.connection);
            theme::pill(ui, &label.to_uppercase(), colour);
        });
    });
}

fn server_card(
    ui: &mut Ui,
    form: &mut SettingsForm,
    server: &ServerState,
    action: &mut Option<Action>,
) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());

        ui.label(RichText::new("Server connection").font(theme::strong_font(15.0)));
        ui.label(
            RichText::new(
                "The desktop client talks to the CareLibre REST API at this address. \
                 Use the local server for development, or your organisation's domain in production.",
            )
            .color(theme::muted())
            .size(12.0),
        );
        ui.add_space(12.0);

        field_label(ui, "Server URL");
        let edit = ui.add_sized(
            [ui.available_width(), 34.0],
            egui::TextEdit::singleline(&mut form.base_url)
                .hint_text("http://127.0.0.1:8484")
                .margin(Margin::symmetric(10, 8)),
        );
        let pressed_enter =
            edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.add(theme::primary_button("Save & test")).clicked() || pressed_enter {
                form.error = None;
                form.notice = None;
                *action = Some(Action::SaveServer {
                    base_url: form.base_url.clone(),
                });
            }
            if ui.add(theme::secondary_button("Test now")).clicked() {
                *action = Some(Action::TestServer);
            }
        });

        ui.add_space(14.0);
        connection_panel(ui, server);

        if let Some(error) = &form.error {
            ui.add_space(10.0);
            message_box(ui, error, theme::danger());
        }
        if let Some(notice) = &form.notice {
            ui.add_space(8.0);
            ui.label(RichText::new(notice).color(theme::success()).size(11.5));
        }
    });
}

fn connection_panel(ui: &mut Ui, server: &ServerState) {
    egui::Frame::new()
        .fill(theme::bg())
        .stroke(Stroke::new(1.0, theme::border()))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            let (label, colour) = theme::connection_style(&server.connection);
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.0, colour);
                ui.label(
                    RichText::new(label)
                        .color(colour)
                        .font(theme::strong_font(12.5)),
                );
            });
            ui.add_space(5.0);

            match &server.connection {
                ConnectionState::NotConnected => {
                    ui.label(
                        RichText::new(
                            "Live connection checks are disabled in this preview. \
                             Start the desktop app to connect.",
                        )
                        .color(theme::muted())
                        .size(11.0),
                    );
                }
                ConnectionState::Connecting => {
                    ui.label(
                        RichText::new(format!("Contacting {}…", server.base_url))
                            .color(theme::muted())
                            .size(11.0)
                            .monospace(),
                    );
                }
                ConnectionState::Online {
                    version,
                    database_up,
                    ready_error,
                } => {
                    ui.label(
                        RichText::new(format!("CareLibre Server {version}"))
                            .color(theme::text())
                            .size(12.0),
                    );
                    ui.label(
                        RichText::new(if *database_up {
                            "Database: up".to_owned()
                        } else {
                            format!(
                                "Database: down — {}",
                                ready_error.as_deref().unwrap_or("readiness check failed")
                            )
                        })
                        .color(if *database_up {
                            theme::success()
                        } else {
                            theme::warning()
                        })
                        .size(11.0),
                    );
                    ui.add_space(3.0);
                    ui.label(
                        RichText::new(server.base_url.clone())
                            .color(theme::faint())
                            .size(10.5)
                            .monospace(),
                    );
                }
                ConnectionState::Offline { error } => {
                    ui.label(RichText::new(error).color(theme::danger()).size(11.0));
                    ui.add_space(3.0);
                    ui.label(
                        RichText::new(server.base_url.clone())
                            .color(theme::faint())
                            .size(10.5)
                            .monospace(),
                    );
                }
            }

            if let Some(checked) = server.last_checked {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!("Checked {}", relative_time(checked)))
                        .color(theme::faint())
                        .size(10.0),
                );
            }
        });
}

fn account_card(
    ui: &mut Ui,
    form: &mut SettingsForm,
    server: &ServerState,
    action: &mut Option<Action>,
) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());

        ui.label(RichText::new("Account").font(theme::strong_font(15.0)));
        ui.add_space(8.0);

        if let Some(session) = &server.session {
            signed_in(ui, form, session, action);
        } else {
            signed_out(ui, form, server, action);
        }
    });
}

fn signed_in(ui: &mut Ui, form: &mut SettingsForm, session: &Session, action: &mut Option<Action>) {
    ui.horizontal(|ui| {
        theme::avatar(ui, &session.initials(), 40.0, theme::accent_blue());
        ui.vertical(|ui| {
            ui.label(RichText::new(&session.auth.user.full_name).font(theme::strong_font(14.0)));
            ui.label(
                RichText::new(&session.auth.user.email)
                    .color(theme::muted())
                    .size(11.0),
            );
        });
    });

    ui.add_space(12.0);
    ui.separator();
    ui.add_space(12.0);

    ui.label(
        RichText::new("ORGANISATION")
            .color(theme::faint())
            .size(9.5),
    );
    ui.label(RichText::new(&session.auth.organisation.name).font(theme::strong_font(13.0)));
    ui.label(
        RichText::new(format!(
            "{} · {} edition · {}",
            session.auth.organisation.slug,
            session.auth.organisation.edition,
            session.role()
        ))
        .color(theme::muted())
        .size(11.0),
    );

    ui.add_space(12.0);
    remember_me(ui, form, action);

    ui.add_space(14.0);
    if ui.add(theme::secondary_button("Sign out")).clicked() {
        *action = Some(Action::Logout);
    }
}

fn signed_out(
    ui: &mut Ui,
    form: &mut SettingsForm,
    server: &ServerState,
    action: &mut Option<Action>,
) {
    ui.horizontal(|ui| {
        if mode_tab(ui, "Sign in", form.mode == AuthMode::Login).clicked() {
            form.mode = AuthMode::Login;
            form.error = None;
        }
        if mode_tab(ui, "Create organisation", form.mode == AuthMode::Register).clicked() {
            form.mode = AuthMode::Register;
            form.error = None;
        }
    });
    ui.add_space(10.0);

    match form.mode {
        AuthMode::Login => {
            field(ui, "Email", &mut form.email, "you@example.com", false);
            field(ui, "Password", &mut form.password, "Your password", true);
            remember_me(ui, form, action);
            ui.add_space(8.0);
            field(
                ui,
                "Organisation slug (optional)",
                &mut form.login_slug,
                "Only if you belong to more than one organisation",
                false,
            );
        }
        AuthMode::Register => {
            field(
                ui,
                "Organisation name",
                &mut form.organisation_name,
                "Example Care Pty Ltd",
                false,
            );
            field(
                ui,
                "Organisation slug",
                &mut form.new_slug,
                "example-care",
                false,
            );
            field(ui, "Full name", &mut form.full_name, "Jane Doe", false);
            field(ui, "Email", &mut form.email, "you@example.com", false);
            field(
                ui,
                "Password",
                &mut form.password,
                "At least 12 characters",
                true,
            );
            remember_me(ui, form, action);
        }
    }

    if let Some(error) = &form.error {
        ui.add_space(4.0);
        message_box(ui, error, theme::danger());
    }

    ui.add_space(10.0);
    let label = match (form.mode, server.auth_in_flight) {
        (_, true) => "Please wait…",
        (AuthMode::Login, false) => "Sign in",
        (AuthMode::Register, false) => "Create organisation",
    };

    if ui
        .add_enabled(!server.auth_in_flight, theme::primary_button(label))
        .clicked()
    {
        form.error = None;
        form.notice = None;

        match form.mode {
            AuthMode::Login => {
                if form.email.trim().is_empty() || form.password.is_empty() {
                    form.error = Some("Enter your email and password.".to_owned());
                } else {
                    *action = Some(Action::Login {
                        email: form.email.trim().to_owned(),
                        password: form.password.clone(),
                        organisation_slug: Some(form.login_slug.trim().to_owned())
                            .filter(|slug| !slug.is_empty()),
                    });
                }
            }
            AuthMode::Register => {
                if let Some(error) = validate_registration(form) {
                    form.error = Some(error);
                } else {
                    *action = Some(Action::Register {
                        organisation_name: form.organisation_name.trim().to_owned(),
                        organisation_slug: form.new_slug.trim().to_lowercase(),
                        full_name: form.full_name.trim().to_owned(),
                        email: form.email.trim().to_owned(),
                        password: form.password.clone(),
                    });
                }
            }
        }
    }

    ui.add_space(10.0);
    ui.label(
        RichText::new(
            "Accounts live on your CareLibre server. The first organisation is created \
             with \"Create organisation\".",
        )
        .color(theme::faint())
        .size(10.5),
    );
}

fn validate_registration(form: &SettingsForm) -> Option<String> {
    if form.organisation_name.trim().is_empty()
        || form.new_slug.trim().is_empty()
        || form.full_name.trim().is_empty()
        || form.email.trim().is_empty()
        || form.password.is_empty()
    {
        return Some("Fill in every field to create your organisation.".to_owned());
    }

    let slug = form.new_slug.trim();
    if !(3..=63).contains(&slug.len())
        || !slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Some(
            "The organisation slug must be 3–63 characters of a-z, 0-9 or hyphens.".to_owned(),
        );
    }

    if form.password.chars().count() < 12 {
        return Some("The password must be at least 12 characters.".to_owned());
    }

    None
}

fn field_label(ui: &mut Ui, label: &str) {
    ui.label(RichText::new(label).color(theme::muted()).size(11.0));
    ui.add_space(3.0);
}

fn field(ui: &mut Ui, label: &str, value: &mut String, hint: &str, password: bool) {
    field_label(ui, label);
    let mut edit = egui::TextEdit::singleline(value)
        .hint_text(hint)
        .margin(Margin::symmetric(10, 8));
    if password {
        edit = edit.password(true);
    }
    ui.add_sized([ui.available_width(), 34.0], edit);
    ui.add_space(5.0);
}

fn mode_tab(ui: &mut Ui, label: &str, selected: bool) -> egui::Response {
    let button = egui::Button::new(
        RichText::new(label)
            .color(if selected {
                theme::accent()
            } else {
                theme::muted()
            })
            .font(theme::strong_font(11.5)),
    )
    .fill(if selected {
        theme::accent_soft(28)
    } else {
        Color32::TRANSPARENT
    })
    .corner_radius(CornerRadius::same(7));

    let response = ui.add(button);
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

fn message_box(ui: &mut Ui, message: &str, colour: Color32) {
    egui::Frame::new()
        .fill(Color32::from_rgba_unmultiplied(
            colour.r(),
            colour.g(),
            colour.b(),
            20,
        ))
        .stroke(Stroke::new(1.0, colour.gamma_multiply(0.55)))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(message).color(colour).size(11.0));
        });
}

fn relative_time(instant: Instant) -> String {
    let seconds = instant.elapsed().as_secs();
    match seconds {
        0..=2 => "just now".to_owned(),
        3..=59 => format!("{seconds}s ago"),
        60..=3599 => format!("{}m ago", seconds / 60),
        _ => format!("{}h ago", seconds / 3600),
    }
}
