//! Application shell: top bar, navigation, status bar and routing.

use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Frame, Layout, Margin, RichText, Sense,
    Stroke, Ui, vec2,
};

use crate::{
    api,
    config::Config,
    pages::{self, Action, Page, settings::SettingsForm},
    server::{ConnectionState, Event, ServerRuntime, ServerState},
    theme,
};

pub struct CareLibreApp {
    page: Page,
    search: String,
    notice: Option<Notice>,
    config: Config,
    settings: SettingsForm,
    server: ServerState,
    runtime: Option<ServerRuntime>,
}

struct Notice {
    text: String,
    expires_at: f64,
}

struct AccountBadge {
    initials: String,
    name: String,
    first_name: String,
    organisation: String,
    role: String,
    edition: String,
}

impl CareLibreApp {
    /// Build the app without a live server connection (used by previews/tests).
    pub fn new() -> Self {
        let config = Config::load();
        let server = ServerState::new(config.server_url.clone());
        let settings = SettingsForm::new(&config.server_url);

        Self {
            page: Page::Home,
            search: String::new(),
            notice: None,
            config,
            settings,
            server,
            runtime: None,
        }
    }

    /// Build the app and start the background server worker.
    pub fn with_server(ctx: &egui::Context) -> Self {
        let mut app = Self::new();
        app.connect(ctx);
        app
    }

    /// Start the server worker if it is not already running.
    pub fn connect(&mut self, ctx: &egui::Context) {
        if self.runtime.is_none() {
            self.runtime = Some(ServerRuntime::spawn(
                ctx.clone(),
                self.server.base_url.clone(),
            ));
            self.server.connection = ConnectionState::Connecting;
            self.server.last_checked = None;
        }
    }

    /// Navigate to a page. Useful for deep links and headless previews.
    pub fn set_page(&mut self, page: Page) {
        self.page = page;
    }

    pub fn page(&self) -> Page {
        self.page
    }

    /// Current server connection state (for previews and integration checks).
    pub fn connection(&self) -> &ConnectionState {
        &self.server.connection
    }

    /// The whole application UI. Kept separate from [`eframe::App`] so it can be
    /// rendered by tooling (headless screenshot tests) as well.
    pub fn show(&mut self, ui: &mut Ui) {
        self.poll_server(ui.ctx());
        self.refresh_session_if_needed();

        self.top_bar(ui);
        self.sidebar(ui);
        self.status_bar(ui);

        let action = egui::CentralPanel::default()
            .frame(Frame::new().fill(theme::BG).inner_margin(Margin::same(24)))
            .show(ui, |ui| self.page_contents(ui))
            .inner;

        if let Some(action) = action {
            self.handle_action(ui.ctx(), action);
        }

        self.show_notice(ui.ctx());
    }

    fn page_contents(&mut self, ui: &mut Ui) -> Option<Action> {
        match self.page {
            Page::Settings => pages::settings::show(ui, &mut self.settings, &self.server),
            Page::Home => {
                let account = self.account_badge();
                pages::home::show(
                    ui,
                    account.as_ref().map(|a| a.first_name.as_str()),
                    account.as_ref().map(|a| a.organisation.as_str()),
                )
            }
            other => pages::placeholder::show(ui, other),
        }
    }

    fn account_badge(&self) -> Option<AccountBadge> {
        let session = self.server.session.as_ref()?;
        Some(AccountBadge {
            initials: session.initials(),
            name: session.auth.user.full_name.clone(),
            first_name: session.first_name().to_owned(),
            organisation: session.auth.organisation.name.clone(),
            role: session.role().to_owned(),
            edition: session.auth.organisation.edition.clone(),
        })
    }

    fn notify(&mut self, ctx: &egui::Context, text: impl Into<String>) {
        let now = ctx.input(|i| i.time);
        self.notice = Some(Notice {
            text: text.into(),
            expires_at: now + 4.5,
        });
    }

    // Server events -------------------------------------------------------

    fn poll_server(&mut self, ctx: &egui::Context) {
        let events: Vec<Event> = match &self.runtime {
            Some(runtime) => std::iter::from_fn(|| runtime.try_recv()).collect(),
            None => Vec::new(),
        };

        for event in events {
            self.handle_server_event(ctx, event);
        }
    }

    fn handle_server_event(&mut self, ctx: &egui::Context, event: Event) {
        match event {
            Event::Status { base_url, result } => {
                if base_url != self.server.base_url {
                    return;
                }

                self.server.last_checked = Some(Instant::now());
                self.server.connection = match result {
                    Ok(info) => ConnectionState::Online {
                        version: info.version,
                        database_up: info.database_up,
                        ready_error: info.ready_error,
                    },
                    Err(error) => ConnectionState::Offline { error },
                };
            }
            Event::Authenticated { base_url, result } => {
                if base_url != self.server.base_url {
                    return;
                }
                self.server.auth_in_flight = false;
                match result {
                    Ok(auth) => {
                        let organisation = auth.organisation.name.clone();
                        self.server.session = Some(crate::server::Session {
                            expires_at: Instant::now()
                                + Duration::from_secs(auth.expires_in.max(60)),
                            auth,
                        });
                        self.server.refresh_in_flight = false;
                        self.settings.on_signed_in();
                        self.notify(ctx, format!("Signed in to {organisation}."));
                    }
                    Err(error) => {
                        self.settings.notice = None;
                        self.settings.error = Some(error);
                    }
                }
            }
            Event::Refreshed { base_url, result } => {
                if base_url != self.server.base_url {
                    return;
                }
                self.server.refresh_in_flight = false;
                match result {
                    Ok(auth) => {
                        self.server.session = Some(crate::server::Session {
                            expires_at: Instant::now()
                                + Duration::from_secs(auth.expires_in.max(60)),
                            auth,
                        });
                    }
                    Err(_) => {
                        self.server.session = None;
                        self.settings.error =
                            Some("Your session expired. Please sign in again.".to_owned());
                        self.notify(ctx, "Session expired. Please sign in again.");
                    }
                }
            }
            Event::LoggedOut { result } => {
                if let Err(error) = result {
                    self.notify(ctx, format!("Signed out locally. Server error: {error}"));
                }
            }
        }
    }

    fn refresh_session_if_needed(&mut self) {
        if self.server.refresh_in_flight {
            return;
        }

        let Some(token) = self
            .server
            .session
            .as_ref()
            .filter(|session| session.needs_refresh())
            .map(|session| session.auth.refresh_token.clone())
        else {
            return;
        };

        if let Some(runtime) = &self.runtime {
            runtime.refresh(token);
            self.server.refresh_in_flight = true;
        }
    }

    fn handle_action(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::Navigate(page) => self.page = page,
            Action::Notice(message) => self.notify(ctx, message),
            Action::SaveServer { base_url } => self.save_server(ctx, &base_url),
            Action::TestServer => self.test_server(),
            Action::Login {
                email,
                password,
                organisation_slug,
            } => {
                if let Some(runtime) = &self.runtime {
                    runtime.login(email, password, organisation_slug);
                    self.server.auth_in_flight = true;
                    self.settings.error = None;
                    self.settings.notice = None;
                } else {
                    self.settings.error =
                        Some("Start the desktop app to connect to a server.".to_owned());
                }
            }
            Action::Register {
                organisation_name,
                organisation_slug,
                full_name,
                email,
                password,
            } => {
                if let Some(runtime) = &self.runtime {
                    runtime.register(
                        organisation_name,
                        organisation_slug,
                        full_name,
                        email,
                        password,
                    );
                    self.server.auth_in_flight = true;
                    self.settings.error = None;
                    self.settings.notice = None;
                } else {
                    self.settings.error =
                        Some("Start the desktop app to connect to a server.".to_owned());
                }
            }
            Action::Logout => {
                if let Some(session) = self.server.session.take() {
                    if let Some(runtime) = &self.runtime {
                        runtime.logout(session.auth.refresh_token.clone());
                    }
                    self.server.refresh_in_flight = false;
                    self.settings.on_signed_out();
                    self.settings.notice = None;
                    self.notify(ctx, "Signed out.");
                }
            }
        }
    }

    fn save_server(&mut self, ctx: &egui::Context, base_url: &str) {
        let url = match api::normalize_base_url(base_url) {
            Ok(url) => url,
            Err(message) => {
                self.settings.error = Some(message);
                return;
            }
        };

        let server_changed = self.server.base_url != url;
        if server_changed && self.server.session.is_some() {
            self.server.session = None;
            self.server.refresh_in_flight = false;
            self.server.auth_in_flight = false;
            self.settings.on_signed_out();
            self.notify(ctx, "Server changed. Please sign in again.");
        }

        self.settings.base_url = url.clone();
        self.server.base_url = url.clone();
        self.server.last_checked = None;
        self.server.connection = ConnectionState::Connecting;
        self.config.server_url = url.clone();

        if let Err(error) = self.config.save() {
            self.settings.error = Some(error);
        }

        if let Some(runtime) = &self.runtime {
            runtime.set_base_url(url);
            self.settings.notice = Some("Server address saved. Checking connection…".to_owned());
            self.notify(ctx, "Checking server connection…");
        } else {
            self.server.connection = ConnectionState::NotConnected;
            self.settings.notice = Some("Server address saved.".to_owned());
        }
    }

    fn test_server(&mut self) {
        if let Some(runtime) = &self.runtime {
            runtime.check();
            self.server.connection = ConnectionState::Connecting;
            self.server.last_checked = None;
            self.settings.error = None;
            self.settings.notice = Some("Checking connection…".to_owned());
        } else {
            self.settings.error =
                Some("Live connection checks are unavailable in this preview.".to_owned());
        }
    }

    // Shell ---------------------------------------------------------------

    fn top_bar(&mut self, ui: &mut Ui) {
        let account = self.account_badge();

        egui::Panel::top("top_bar")
            .exact_size(64.0)
            .show_separator_line(false)
            .frame(
                Frame::new()
                    .fill(theme::BG)
                    .inner_margin(Margin {
                        left: 20,
                        right: 20,
                        top: 0,
                        bottom: 0,
                    })
                    .stroke(Stroke::new(1.0, theme::BORDER)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    // Brand
                    let (logo, _) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
                    ui.painter()
                        .rect_filled(logo, CornerRadius::same(9), theme::ACCENT);
                    ui.painter().text(
                        logo.center(),
                        Align2::CENTER_CENTER,
                        "C",
                        FontId::proportional(19.0),
                        theme::BG,
                    );
                    ui.add_space(2.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("CareLibre").font(theme::strong_font(16.0)));
                        ui.label(RichText::new("CARE PLATFORM").color(theme::FAINT).size(8.5));
                    });

                    ui.add_space(14.0);
                    let (separator, _) = ui.allocate_exact_size(vec2(1.0, 26.0), Sense::hover());
                    ui.painter()
                        .rect_filled(separator, CornerRadius::ZERO, theme::BORDER);
                    ui.add_space(14.0);

                    ui.label(
                        RichText::new(format!("{}  {}", self.page.icon(), self.page.label()))
                            .color(theme::MUTED)
                            .size(13.0),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        match &account {
                            Some(account) => {
                                let response =
                                    theme::avatar(ui, &account.initials, 32.0, theme::ACCENT_BLUE);
                                if response.clicked() {
                                    self.page = Page::Settings;
                                }
                                response.on_hover_text(format!(
                                    "{}\n{} · {}",
                                    account.name, account.organisation, account.role
                                ));
                            }
                            None => {
                                if ui.add(theme::secondary_button("Sign in")).clicked() {
                                    self.page = Page::Settings;
                                }
                            }
                        }
                        ui.add_space(4.0);

                        let bell = ui.add(
                            egui::Button::new(RichText::new("🔔").size(15.0))
                                .frame(false)
                                .min_size(vec2(32.0, 32.0)),
                        );
                        if bell.clicked() {
                            self.notify(ui.ctx(), "No new notifications in the prototype.");
                        }

                        ui.add_space(4.0);

                        let new_button = ui.add(
                            egui::Button::new(
                                RichText::new("+  New")
                                    .color(theme::BG)
                                    .font(theme::strong_font(13.0)),
                            )
                            .fill(theme::ACCENT)
                            .corner_radius(CornerRadius::same(8))
                            .min_size(vec2(78.0, 32.0)),
                        );
                        if new_button.clicked() {
                            self.notify(
                                ui.ctx(),
                                "Quick-create actions arrive with the contacts module.",
                            );
                        }

                        ui.add_space(6.0);

                        let search = ui.add_sized(
                            [260.0, 34.0],
                            egui::TextEdit::singleline(&mut self.search)
                                .hint_text("Search clients, workers, shifts…")
                                .margin(Margin::symmetric(10, 8)),
                        );
                        if search.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter))
                            && !self.search.trim().is_empty()
                        {
                            let query = self.search.clone();
                            self.notify(ui.ctx(), format!("No results yet for “{query}”."));
                        }
                    });
                });
            });
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        let account = self.account_badge();

        egui::Panel::left("sidebar")
            .exact_size(232.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                Frame::new()
                    .fill(theme::SIDEBAR)
                    .inner_margin(Margin::symmetric(12, 16))
                    .stroke(Stroke::new(1.0, theme::BORDER)),
            )
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                group_label(ui, "WORKSPACE");
                for page in [Page::Home, Page::Clients, Page::Workers, Page::Rostering] {
                    if nav_item(ui, self.page == page, page).clicked() {
                        self.page = page;
                    }
                }

                ui.add_space(14.0);
                group_label(ui, "OPERATIONS");
                for page in [
                    Page::Services,
                    Page::Documents,
                    Page::Communications,
                    Page::Reports,
                ] {
                    if nav_item(ui, self.page == page, page).clicked() {
                        self.page = page;
                    }
                }

                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    if user_row(ui, account.as_ref()) {
                        self.page = Page::Settings;
                    }
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);
                    if nav_item(ui, self.page == Page::Settings, Page::Settings).clicked() {
                        self.page = Page::Settings;
                    }
                    ui.add_space(10.0);
                    edition_card(ui, account.as_ref().map(|a| a.edition.as_str()));
                });
            });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let (server_label, server_colour, server_tooltip) = match &self.server.connection {
            ConnectionState::NotConnected => (
                "Server not connected".to_owned(),
                theme::FAINT,
                format!("Server URL: {}", self.server.base_url),
            ),
            ConnectionState::Connecting => (
                "Connecting to server…".to_owned(),
                theme::WARNING,
                format!("Contacting {}", self.server.base_url),
            ),
            ConnectionState::Online {
                version,
                database_up: true,
                ..
            } => (
                format!("Server online · v{version}"),
                theme::SUCCESS,
                format!("{} · database up", self.server.base_url),
            ),
            ConnectionState::Online {
                database_up: false,
                ready_error,
                ..
            } => (
                "Server online · database down".to_owned(),
                theme::WARNING,
                format!(
                    "{} · database down: {}",
                    self.server.base_url,
                    ready_error.as_deref().unwrap_or("readiness check failed")
                ),
            ),
            ConnectionState::Offline { error } => (
                "Server offline".to_owned(),
                theme::DANGER,
                format!("{} · {error}", self.server.base_url),
            ),
        };

        egui::Panel::bottom("status_bar")
            .exact_size(30.0)
            .show_separator_line(false)
            .frame(
                Frame::new()
                    .fill(theme::SIDEBAR)
                    .inner_margin(Margin::symmetric(20, 0))
                    .stroke(Stroke::new(1.0, theme::BORDER)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(
                        RichText::new("CareLibre Desktop v0.1.0 · Prototype")
                            .color(theme::FAINT)
                            .size(10.5),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new("Community Edition")
                                .color(theme::FAINT)
                                .size(10.5),
                        );
                        dot(ui, theme::BORDER);
                        status_segment(ui, "wgpu", theme::FAINT, None);
                        dot(ui, theme::BORDER);
                        status_segment(ui, &server_label, server_colour, Some(&server_tooltip));
                        dot(ui, theme::BORDER);
                        ui.label(
                            RichText::new("Rust · native")
                                .color(theme::FAINT)
                                .size(10.5),
                        );
                    });
                });
            });
    }

    fn show_notice(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let Some(notice) = &self.notice else {
            return;
        };
        if now >= notice.expires_at {
            self.notice = None;
            return;
        }

        let text = notice.text.clone();
        let remaining = (notice.expires_at - now) as f32;
        let alpha = remaining.min(0.5) / 0.5;

        egui::Area::new(egui::Id::unique("notice_toast"))
            .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -46.0))
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                Frame::new()
                    .fill(theme::SURFACE_RAISED.gamma_multiply(alpha))
                    .stroke(Stroke::new(1.0, theme::ACCENT.gamma_multiply(alpha)))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::symmetric(16, 10))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(text)
                                .color(theme::TEXT)
                                .font(theme::strong_font(12.5)),
                        );
                    });
            });
    }
}

impl Default for CareLibreApp {
    fn default() -> Self {
        Self::new()
    }
}

impl eframe::App for CareLibreApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

fn group_label(ui: &mut Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(
        RichText::new(text)
            .color(theme::FAINT)
            .font(theme::strong_font(9.5)),
    );
    ui.add_space(4.0);
}

fn nav_item(ui: &mut Ui, selected: bool, page: Page) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::click());

    if selected {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(9), theme::accent_soft(24));
        let bar = egui::Rect::from_min_size(rect.left_center() + vec2(7.0, -9.0), vec2(3.0, 18.0));
        ui.painter()
            .rect_filled(bar, CornerRadius::same(2), theme::ACCENT);
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(9), theme::SURFACE_RAISED);
    }

    let text_colour = if selected { theme::TEXT } else { theme::MUTED };
    let icon_colour = if selected {
        theme::ACCENT
    } else {
        theme::FAINT
    };

    ui.painter().text(
        rect.left_center() + vec2(24.0, 0.0),
        Align2::LEFT_CENTER,
        page.icon(),
        FontId::proportional(14.0),
        icon_colour,
    );
    ui.painter().text(
        rect.left_center() + vec2(50.0, 0.0),
        Align2::LEFT_CENTER,
        page.label(),
        if selected {
            theme::strong_font(13.0)
        } else {
            FontId::proportional(13.0)
        },
        text_colour,
    );

    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), page.label())
    });

    response.on_hover_text(page.blurb())
}

fn user_row(ui: &mut Ui, account: Option<&AccountBadge>) -> bool {
    let mut clicked = false;

    ui.horizontal(|ui| match account {
        Some(account) => {
            let response = theme::avatar(ui, &account.initials, 28.0, theme::ACCENT_BLUE);
            if response.clicked() {
                clicked = true;
            }
            response.on_hover_text(format!(
                "{}\n{} · {}",
                account.name, account.organisation, account.role
            ));
            ui.vertical(|ui| {
                ui.label(RichText::new(&account.name).font(theme::strong_font(12.0)));
                ui.label(
                    RichText::new(format!(
                        "{} · {}",
                        capitalize(&account.role),
                        account.organisation
                    ))
                    .color(theme::FAINT)
                    .size(10.0),
                );
            });
        }
        None => {
            theme::avatar(ui, "?", 28.0, theme::SURFACE_RAISED);
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("Not signed in")
                        .color(theme::MUTED)
                        .font(theme::strong_font(12.0)),
                );
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Connect to server  →")
                                .color(theme::ACCENT)
                                .size(10.0),
                        )
                        .frame(false),
                    )
                    .clicked()
                {
                    clicked = true;
                }
            });
        }
    });

    clicked
}

fn edition_card(ui: &mut Ui, edition: Option<&str>) {
    let title = match edition {
        Some(edition) if !edition.eq_ignore_ascii_case("community") => {
            format!("{} EDITION", edition.to_uppercase())
        }
        _ => "COMMUNITY EDITION".to_owned(),
    };

    Frame::new()
        .fill(theme::BG)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            // The sidebar lays its footer out bottom-up; force normal flow here.
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                    ui.painter()
                        .circle_filled(rect.center(), 4.0, theme::SUCCESS);
                    ui.label(
                        RichText::new(title)
                            .color(theme::MUTED)
                            .font(theme::strong_font(9.5)),
                    );
                });
                ui.add_space(3.0);
                ui.label(
                    RichText::new("Self-hosted · free forever")
                        .color(theme::FAINT)
                        .size(10.5),
                );
                ui.add_space(8.0);

                let (rect, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 4.0), Sense::hover());
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(2), theme::SURFACE_RAISED);
                let filled =
                    egui::Rect::from_min_size(rect.min, vec2(rect.width() * 0.25, rect.height()));
                ui.painter()
                    .rect_filled(filled, CornerRadius::same(2), theme::ACCENT);
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Prototype · 2 of 8 milestones")
                        .color(theme::FAINT)
                        .size(9.5),
                );
            });
        });
}

fn status_segment(ui: &mut Ui, text: &str, colour: Color32, tooltip: Option<&str>) {
    let response = ui.label(RichText::new(text).color(colour).size(10.5));
    if let Some(tooltip) = tooltip {
        response.on_hover_text(tooltip);
    }
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn dot(ui: &mut Ui, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(6.0, 6.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(3), colour);
}
