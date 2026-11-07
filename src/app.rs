//! Application shell: top bar, navigation, status bar and routing.

use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Frame, Layout, Margin, RichText, Sense,
    Stroke, Ui, vec2,
};

use crate::{
    api,
    config::Config,
    contact::{ApiContactDetail, ApiContactWrite, kind_from_role, row_from_api},
    intake::IntakeKind,
    pages::{
        self, Action, Page,
        contact_editor::{ContactEditorAction, ContactWindowState},
        contacts::{ContactsAction, ContactsState, LOAD_MORE_SIZE, PAGE_SIZE},
        intake::{IntakeAction, IntakeState, IntakeView},
        settings::SettingsForm,
    },
    perf::{PerfHud, PerfInfo},
    remote::{RemoteEvent, RemoteHandle},
    server::{ConnectionState, Event, ServerRuntime, ServerState},
    store::{self, ContactCounts, ContactFilter, ContactKind, StoreEvent, StoreHandle},
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
    contacts_all: ContactsState,
    contacts_clients: ContactsState,
    contacts_workers: ContactsState,
    intake: IntakeState,
    contact_windows: BTreeMap<String, ContactWindowState>,
    remote: Option<RemoteHandle>,
    app_ctx: Option<egui::Context>,
    store: Option<StoreHandle>,
    store_ready: bool,
    store_error: Option<String>,
    store_path: String,
    cache_counts: ContactCounts,
    cache_shifts: Option<usize>,
    perf: PerfHud,
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
            contacts_all: ContactsState::new(ContactFilter::All),
            contacts_clients: ContactsState::new(ContactFilter::Clients),
            contacts_workers: ContactsState::new(ContactFilter::Workers),
            intake: IntakeState::new(),
            contact_windows: BTreeMap::new(),
            remote: None,
            app_ctx: None,
            store: None,
            store_ready: false,
            store_error: None,
            store_path: String::new(),
            cache_counts: ContactCounts::default(),
            cache_shifts: None,
            perf: PerfHud::new(),
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
        self.app_ctx = Some(ctx.clone());
        if self.remote.is_none() {
            self.remote = Some(RemoteHandle::spawn(ctx.clone()));
        }
        if self.runtime.is_none() {
            self.runtime = Some(ServerRuntime::spawn(
                ctx.clone(),
                self.server.base_url.clone(),
            ));
            self.server.connection = ConnectionState::Connecting;
            self.server.last_checked = None;
        }

        if self.store.is_none() {
            match store::default_cache_path() {
                Some(path) => {
                    self.store_path = path.display().to_string();
                    self.store = Some(StoreHandle::spawn(path, ctx.clone()));
                }
                None => {
                    self.store_error =
                        Some("Could not locate a data directory for the local cache.".to_owned());
                }
            }
        }
    }

    /// Navigate to a page. Useful for deep links and headless previews.
    pub fn set_page(&mut self, page: Page) {
        self.page = page;
    }

    pub fn page(&self) -> Page {
        self.page
    }

    /// Sign in programmatically. Used by headless previews and live tests.
    pub fn sign_in(&mut self, email: &str, password: &str) {
        if let Some(runtime) = &self.runtime {
            runtime.login(email.to_owned(), password.to_owned(), None);
            self.server.auth_in_flight = true;
        }
    }

    pub fn signed_in(&self) -> bool {
        self.server.session.is_some()
    }

    /// Current server connection state (for previews and integration checks).
    pub fn connection(&self) -> &ConnectionState {
        &self.server.connection
    }

    /// Whether the local cache has finished opening.
    pub fn cache_ready(&self) -> bool {
        self.store_ready
    }

    /// Open the intake picker (or the current draft, if one is in progress).
    pub fn open_intake(&mut self) {
        self.page = Page::Intake;
        if self.intake.view == IntakeView::Completed {
            self.intake.open_picker();
        }
        if self.intake.view == IntakeView::Picker {
            self.intake.list_requested = false;
        }
    }

    /// Begin a scenario directly. Useful for deep links and previews.
    pub fn start_intake(&mut self, kind: IntakeKind) {
        self.page = Page::Intake;
        self.intake.start(kind);
    }

    /// Number of contacts currently materialised in the active list.
    pub fn contacts_loaded(&self) -> usize {
        match self.page {
            Page::Workers => self.contacts_workers.loaded(),
            Page::Contacts => self.contacts_all.loaded(),
            _ => self.contacts_clients.loaded(),
        }
    }

    /// Contacts loaded for the global list; also proves server counts are ready.
    pub fn server_contacts_loaded(&self) -> usize {
        self.contacts_all.loaded()
    }

    /// The whole application UI. Kept separate from [`eframe::App`] so it can be
    /// rendered by tooling (headless screenshot tests) as well.
    pub fn show(&mut self, ui: &mut Ui) {
        self.perf.begin_frame(ui.ctx());
        self.poll_server(ui.ctx());
        self.poll_store(ui.ctx());
        self.poll_remote(ui.ctx());
        self.tick_contact_search();
        self.tick_intake_autosave();
        self.ensure_contacts_loaded();
        self.ensure_intake_list();
        self.refresh_session_if_needed();

        self.top_bar(ui);
        self.sidebar(ui);
        self.status_bar(ui);

        let mut contacts_action: Option<(ContactFilter, ContactsAction)> = None;
        let mut intake_action: Option<IntakeAction> = None;
        let action = egui::CentralPanel::default()
            .frame(Frame::new().fill(theme::BG).inner_margin(Margin::same(24)))
            .show(ui, |ui| {
                self.page_contents(ui, &mut contacts_action, &mut intake_action)
            })
            .inner;

        if let Some((filter, contacts_action)) = contacts_action {
            self.handle_contacts_action(filter, contacts_action);
        }
        if let Some(intake_action) = intake_action {
            self.handle_intake_action(ui.ctx(), intake_action);
        }

        if let Some(action) = action {
            self.handle_action(ui.ctx(), action);
        }

        self.render_contact_windows(ui.ctx());

        let perf_state = match self.page {
            Page::Workers => &self.contacts_workers,
            Page::Contacts => &self.contacts_all,
            _ => &self.contacts_clients,
        };
        let perf_info = PerfInfo {
            cache_ready: self.store_ready,
            cache_path: (!self.store_path.is_empty()).then_some(self.store_path.as_str()),
            cache_error: self.store_error.as_deref(),
            contacts_loaded: perf_state.loaded(),
            contacts_total: perf_state.total,
            contacts_all: self.cache_counts.all,
            last_query_ms: perf_state.last_query_ms,
            shifts: self.cache_shifts,
        };
        self.perf.show(ui.ctx(), &perf_info);

        self.show_notice(ui.ctx());
    }

    fn page_contents(
        &mut self,
        ui: &mut Ui,
        contacts_action: &mut Option<(ContactFilter, ContactsAction)>,
        intake_action: &mut Option<IntakeAction>,
    ) -> Option<Action> {
        match self.page {
            Page::Settings => pages::settings::show(ui, &mut self.settings, &self.server),
            Page::Home => {
                let account = self.account_badge();
                let counts = self.cache_counts;
                let signed_in = self.access_token().is_some();
                let server_label = match &self.server.connection {
                    ConnectionState::Online { version, .. } => {
                        format!("Online · v{version}")
                    }
                    ConnectionState::Connecting => "Connecting…".to_owned(),
                    ConnectionState::Offline { .. } => "Offline".to_owned(),
                    ConnectionState::NotConnected => "Not connected".to_owned(),
                };
                pages::home::show(
                    ui,
                    account.as_ref().map(|a| a.first_name.as_str()),
                    account.as_ref().map(|a| a.organisation.as_str()),
                    counts,
                    signed_in,
                    &server_label,
                )
            }
            Page::Intake => {
                let signed_in = self.access_token().is_some();
                let store_ready = self.store_ready;
                if let Some(action) =
                    pages::intake::show(ui, &mut self.intake, store_ready, signed_in)
                {
                    *intake_action = Some(action);
                }
                None
            }
            Page::Contacts => {
                let signed_in = self.access_token().is_some();
                let server_error = match &self.server.connection {
                    ConnectionState::Offline { error } => Some(error.clone()),
                    _ => None,
                };
                if let Some(action) = pages::contacts::show(
                    ui,
                    &mut self.contacts_all,
                    signed_in,
                    server_error.as_deref(),
                ) {
                    *contacts_action = Some((ContactFilter::All, action));
                }
                None
            }
            Page::Clients => {
                let signed_in = self.access_token().is_some();
                let server_error = match &self.server.connection {
                    ConnectionState::Offline { error } => Some(error.clone()),
                    _ => None,
                };
                if let Some(action) = pages::contacts::show(
                    ui,
                    &mut self.contacts_clients,
                    signed_in,
                    server_error.as_deref(),
                ) {
                    *contacts_action = Some((ContactFilter::Clients, action));
                }
                None
            }
            Page::Workers => {
                let signed_in = self.access_token().is_some();
                let server_error = match &self.server.connection {
                    ConnectionState::Offline { error } => Some(error.clone()),
                    _ => None,
                };
                if let Some(action) = pages::contacts::show(
                    ui,
                    &mut self.contacts_workers,
                    signed_in,
                    server_error.as_deref(),
                ) {
                    *contacts_action = Some((ContactFilter::Workers, action));
                }
                None
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

    fn contact_state(&self, filter: ContactFilter) -> &ContactsState {
        match filter {
            ContactFilter::All => &self.contacts_all,
            ContactFilter::Workers => &self.contacts_workers,
            ContactFilter::Clients => &self.contacts_clients,
        }
    }

    fn contact_state_mut(&mut self, filter: ContactFilter) -> &mut ContactsState {
        match filter {
            ContactFilter::All => &mut self.contacts_all,
            ContactFilter::Workers => &mut self.contacts_workers,
            ContactFilter::Clients => &mut self.contacts_clients,
        }
    }

    fn poll_store(&mut self, ctx: &egui::Context) {
        let events: Vec<StoreEvent> = match &self.store {
            Some(store) => std::iter::from_fn(|| store.try_recv()).collect(),
            None => Vec::new(),
        };

        for event in events {
            match event {
                StoreEvent::Ready { path, shifts, .. } => {
                    self.store_ready = true;
                    self.store_error = None;
                    self.store_path = path;
                    self.cache_shifts = Some(shifts);
                }
                StoreEvent::Failed { error } => {
                    self.store_ready = false;
                    self.store_error = Some(error);
                }
                StoreEvent::IntakesListed { request_id, result } => {
                    if self.intake.list_request != Some(request_id) {
                        continue;
                    }
                    self.intake.list_loading = false;
                    self.intake.list_requested = true;
                    match result {
                        Ok(drafts) => {
                            self.intake.drafts = drafts;
                            self.intake.error = None;
                        }
                        Err(error) => self.intake.error = Some(error),
                    }
                }
                StoreEvent::IntakeLoaded { request_id, result } => {
                    if self.intake.load_request != Some(request_id) {
                        continue;
                    }
                    self.intake.loading_draft = false;
                    match result {
                        Ok(Some(draft)) => {
                            self.intake.draft = Some(draft);
                            self.intake.view = IntakeView::Wizard;
                            self.intake.dirty = false;
                            self.intake.saving = false;
                            self.intake.error = None;
                        }
                        Ok(None) => {
                            self.intake.error = Some("That draft no longer exists.".to_owned());
                            self.intake.open_picker();
                        }
                        Err(error) => self.intake.error = Some(error),
                    }
                }
                StoreEvent::IntakeSaved { result } => {
                    self.intake.saving = false;
                    match result {
                        Ok(()) => {
                            self.intake.error = None;
                            if self.intake.view == IntakeView::Picker {
                                self.intake.list_requested = false;
                            }
                        }
                        Err(error) => {
                            self.intake.dirty = true;
                            self.intake.error = Some(error);
                        }
                    }
                }
                StoreEvent::IntakeDiscarded {
                    request_id,
                    id: _,
                    result,
                } => {
                    if self.intake.discard_request != Some(request_id) {
                        continue;
                    }
                    self.intake.list_loading = false;
                    self.intake.list_requested = false;
                    match result {
                        Ok(()) => {
                            self.intake.error = None;
                            self.intake.list_requested = false;
                        }
                        Err(error) => self.intake.error = Some(error),
                    }
                }
                StoreEvent::Seeded { shifts, .. } => {
                    self.cache_shifts = Some(shifts);
                    let _ = ctx;
                }
                StoreEvent::IntakeCompleted { .. }
                | StoreEvent::ContactLoaded { .. }
                | StoreEvent::ContactSaved { .. }
                | StoreEvent::Contacts { .. } => {}
            }
        }
    }

    fn tick_contact_search(&mut self) {
        for filter in [ContactFilter::Clients, ContactFilter::Workers] {
            let due = self.contact_state(filter).search_due;
            if let Some(due) = due
                && Instant::now() >= due
            {
                self.contact_state_mut(filter).search_due = None;
                self.send_contacts_query(filter, false);
            }
        }
    }

    fn access_token(&self) -> Option<String> {
        self.server
            .session
            .as_ref()
            .map(|session| session.auth.access_token.clone())
    }

    fn ensure_contacts_loaded(&mut self) {
        if self.access_token().is_none() {
            return;
        }

        let filter = match self.page {
            Page::Contacts => ContactFilter::All,
            Page::Clients => ContactFilter::Clients,
            Page::Workers => ContactFilter::Workers,
            _ => return,
        };

        let needs_load = {
            let state = self.contact_state(filter);
            state.generation == 0 && state.search_due.is_none()
        };
        if needs_load {
            self.send_contacts_query(filter, false);
        }
    }

    fn send_contacts_query(&mut self, filter: ContactFilter, append: bool) {
        let (generation, search, after, limit) = {
            let state = self.contact_state_mut(filter);
            if append && state.next_cursor.is_none() {
                return;
            }

            state.generation += 1;
            state.loading = true;
            state.error = None;

            (
                state.generation,
                state.search.clone(),
                if append {
                    state.next_cursor.clone()
                } else {
                    None
                },
                if append { LOAD_MORE_SIZE } else { PAGE_SIZE },
            )
        };

        let role = match filter {
            ContactFilter::All => "all",
            ContactFilter::Clients => "client",
            ContactFilter::Workers => "worker",
        };

        let Some(token) = self.access_token() else {
            let state = self.contact_state_mut(filter);
            state.loading = false;
            state.error = Some("Sign in to load contacts from the server.".to_owned());
            return;
        };

        if let Some(remote) = &self.remote {
            remote.list_contacts(
                generation,
                self.server.base_url.clone(),
                token,
                filter,
                role,
                search,
                after,
                limit,
                append,
            );
        } else {
            let state = self.contact_state_mut(filter);
            state.loading = false;
            state.error = Some("The server connection is not available.".to_owned());
        }
    }

    fn handle_contacts_action(&mut self, filter: ContactFilter, action: ContactsAction) {
        match action {
            ContactsAction::SearchChanged => {
                let state = self.contact_state_mut(filter);
                state.generation += 1;
                state.search_due = Some(Instant::now() + Duration::from_millis(120));
                state.loading = true;
                state.next_cursor = None;
                state.needs_more = false;
            }
            ContactsAction::OpenContact(id) => self.open_contact(&id),
            ContactsAction::LoadMore => self.send_contacts_query(filter, true),
            ContactsAction::GoToSettings => self.page = Page::Settings,
            ContactsAction::StartIntake => self.open_intake(),
        }
    }

    fn ensure_intake_list(&mut self) {
        if self.page != Page::Intake
            || !self.store_ready
            || self.intake.view != IntakeView::Picker
            || self.intake.list_requested
            || self.intake.list_loading
        {
            return;
        }

        self.request_intake_list();
    }

    fn request_intake_list(&mut self) {
        let request_id = self.intake.next_request();
        self.intake.list_request = Some(request_id);
        self.intake.list_loading = true;
        self.intake.list_requested = true;

        if let Some(store) = &self.store {
            store.list_intakes(request_id);
        } else {
            self.intake.list_loading = false;
            self.intake.list_requested = false;
            self.intake.error = Some("The local cache is not running.".to_owned());
        }
    }

    fn tick_intake_autosave(&mut self) {
        if self.intake.view != IntakeView::Wizard || self.intake.saving || !self.intake.dirty {
            return;
        }

        let due = self.intake.save_due.unwrap_or_else(Instant::now);
        if Instant::now() >= due {
            self.save_intake_now();
        }
    }

    fn save_intake_now(&mut self) {
        let Some(draft) = &self.intake.draft else {
            return;
        };

        if let Some(store) = &self.store {
            store.save_intake(draft.clone());
            self.intake.saving = true;
            self.intake.dirty = false;
            self.intake.save_due = None;
        } else {
            self.intake.error = Some("The local cache is not running.".to_owned());
        }
    }

    fn handle_intake_action(&mut self, ctx: &egui::Context, action: IntakeAction) {
        match action {
            IntakeAction::Start(kind) => {
                self.intake.start(kind);
                self.save_intake_now();
            }
            IntakeAction::Resume(id) => {
                let request_id = self.intake.next_request();
                self.intake.load_request = Some(request_id);
                self.intake.loading_draft = true;
                self.intake.error = None;
                if let Some(store) = &self.store {
                    store.load_intake(request_id, id);
                }
            }
            IntakeAction::Discard(id) => {
                let request_id = self.intake.next_request();
                self.intake.discard_request = Some(request_id);
                self.intake.list_loading = true;
                if let Some(store) = &self.store {
                    store.discard_intake(request_id, id);
                }
            }
            IntakeAction::Edited => {}
            IntakeAction::SaveNow => self.save_intake_now(),
            IntakeAction::BackToPicker => {
                if self.intake.dirty {
                    self.save_intake_now();
                }
                self.intake.open_picker();
                self.intake.list_requested = false;
            }
            IntakeAction::Complete => {
                let payload = self.intake.draft.as_ref().map(intake_write);
                let Some(payload) = payload else {
                    return;
                };
                let Some(token) = self.access_token() else {
                    self.intake.error =
                        Some("Sign in to the server before completing an intake.".to_owned());
                    return;
                };
                let Some(remote) = &self.remote else {
                    self.intake.error = Some("The server connection is not available.".to_owned());
                    return;
                };
                let request_id = self.intake.next_request();
                self.intake.create_request = Some(request_id);
                self.intake.saving = true;
                self.intake.dirty = false;
                self.intake.error = None;
                remote.create_contact(request_id, self.server.base_url.clone(), token, payload);
            }
            IntakeAction::GoToSettings => self.page = Page::Settings,
            IntakeAction::ViewContact { page, name } => {
                self.page = page;
                let filter = match page {
                    Page::Workers => ContactFilter::Workers,
                    Page::Contacts => ContactFilter::All,
                    _ => ContactFilter::Clients,
                };
                let state = self.contact_state_mut(filter);
                state.search = name;
                state.generation += 1;
                state.search_due = Some(Instant::now());
                state.loading = true;
                state.next_cursor = None;
                state.selected = None;
                let _ = ctx;
            }
        }
    }

    fn refresh_contact_lists(&mut self) {
        for state in [
            &mut self.contacts_all,
            &mut self.contacts_clients,
            &mut self.contacts_workers,
        ] {
            state.reset();
            state.generation = 0;
            state.next_cursor = None;
            state.loading = false;
        }
    }

    pub fn open_contact(&mut self, id: &str) {
        if let Some(window) = self.contact_windows.get(id) {
            if let Some(ctx) = &self.app_ctx {
                ctx.send_viewport_cmd_to(contact_viewport_id(id), egui::ViewportCommand::Focus);
            }
            let _ = window;
            return;
        }

        let state = ContactWindowState::new(id.to_owned());
        let request_id = state.request_id.unwrap_or(1);
        self.contact_windows.insert(id.to_owned(), state);
        self.load_contact_window(id, request_id);
    }

    fn load_contact_window(&mut self, id: &str, request_id: u64) {
        let token = match self.access_token() {
            Some(token) => token,
            None => {
                if let Some(window) = self.contact_windows.get_mut(id) {
                    window.loading = false;
                    window.error = Some("Sign in to load this contact.".to_owned());
                }
                return;
            }
        };

        if let Some(remote) = &self.remote {
            remote.load_contact(
                request_id,
                self.server.base_url.clone(),
                token,
                id.to_owned(),
            );
        } else if let Some(window) = self.contact_windows.get_mut(id) {
            window.loading = false;
            window.error = Some("The server connection is not available.".to_owned());
        }
    }

    fn save_contact_window(&mut self, id: &str) {
        let token = self.access_token();
        let Some(window) = self.contact_windows.get_mut(id) else {
            return;
        };
        let payload = match window.build_write() {
            Ok(payload) => payload,
            Err(error) => {
                window.error = Some(error);
                return;
            }
        };
        let request_id = window.request_id();
        window.saving = true;
        window.error = None;

        let Some(token) = token else {
            window.saving = false;
            window.error = Some("Sign in to save contacts.".to_owned());
            return;
        };

        if let Some(remote) = &self.remote {
            remote.save_contact(
                request_id,
                self.server.base_url.clone(),
                token,
                id.to_owned(),
                payload,
            );
        } else {
            window.saving = false;
            window.error = Some("The server connection is not available.".to_owned());
        }
    }

    fn close_contact_window(&mut self, id: &str) {
        self.contact_windows.remove(id);
        if let Some(ctx) = &self.app_ctx {
            ctx.send_viewport_cmd_to(contact_viewport_id(id), egui::ViewportCommand::Close);
        }
    }

    fn close_all_contact_windows(&mut self) {
        let ids: Vec<String> = self.contact_windows.keys().cloned().collect();
        for id in ids {
            self.close_contact_window(&id);
        }
    }

    fn render_contact_windows(&mut self, ctx: &egui::Context) {
        let ids: Vec<String> = self.contact_windows.keys().cloned().collect();
        let mut actions: Vec<(String, ContactEditorAction)> = Vec::new();

        for id in ids {
            let Some(window) = self.contact_windows.get_mut(&id) else {
                continue;
            };
            let viewport_id = contact_viewport_id(&id);
            let builder = egui::ViewportBuilder::default()
                .with_title(window.title())
                .with_app_id("carelibre-contact")
                .with_inner_size([880.0, 780.0])
                .with_min_inner_size([640.0, 520.0]);

            let action = ctx.show_viewport_immediate(viewport_id, builder, |ui, _| {
                pages::contact_editor::show_viewport(ui, window)
            });

            if window.close_requested {
                ctx.send_viewport_cmd_to(viewport_id, egui::ViewportCommand::Close);
            }
            if let Some(action) = action {
                actions.push((id.clone(), action));
            }
        }

        for (id, action) in actions {
            match action {
                ContactEditorAction::Save => self.save_contact_window(&id),
                ContactEditorAction::Cancel => self.close_contact_window(&id),
            }
        }

        self.contact_windows
            .retain(|_, window| !window.close_requested);
    }

    /// Open the first loaded contact. Used by screenshot previews.
    pub fn open_first_contact(&mut self) -> bool {
        let filter = match self.page {
            Page::Contacts => ContactFilter::All,
            Page::Workers => ContactFilter::Workers,
            _ => ContactFilter::Clients,
        };
        let rows = self.contact_state(filter).rows.load_full();
        if let Some(row) = rows.first() {
            let id = row.id.clone();
            self.open_contact(&id);
            true
        } else {
            false
        }
    }

    pub fn contact_editor_ready(&self) -> bool {
        self.contact_windows.values().any(|window| !window.loading)
    }

    /// Number of custom-field definitions loaded in the active editor window.
    pub fn contact_editor_custom_fields(&self) -> usize {
        self.contact_windows
            .values()
            .filter_map(|window| window.detail.as_ref())
            .map(|detail| detail.custom_fields.len())
            .max()
            .unwrap_or(0)
    }

    fn poll_remote(&mut self, ctx: &egui::Context) {
        let events: Vec<RemoteEvent> = match &self.remote {
            Some(remote) => std::iter::from_fn(|| remote.try_recv()).collect(),
            None => Vec::new(),
        };

        for event in events {
            match event {
                RemoteEvent::ContactsListed {
                    request_id,
                    filter,
                    append,
                    result,
                } => {
                    let state = self.contact_state_mut(filter);
                    if request_id != state.generation {
                        continue;
                    }
                    state.loading = false;
                    let mut updated_counts = None;
                    match result {
                        Ok((list, elapsed_ms)) => {
                            let counts = ContactCounts {
                                all: list.counts.all,
                                clients: list.counts.clients,
                                workers: list.counts.workers,
                                dual: list.counts.dual,
                                other: list.counts.other,
                            };
                            updated_counts = Some(counts);
                            state.counts = counts;
                            state.total = list.total;
                            state.next_cursor = list.next.map(|cursor| store::ContactCursor {
                                name: cursor.name,
                                id: cursor.id,
                            });
                            state.last_query_ms = Some(elapsed_ms);
                            state.error = None;

                            let rows: Vec<_> = list.items.iter().map(row_from_api).collect();
                            if append {
                                let mut existing = (*state.rows.load_full()).clone();
                                existing.extend(rows);
                                state.rows.store(Arc::new(existing));
                            } else {
                                state.rows.store(Arc::new(rows));
                            }
                        }
                        Err(error) => state.error = Some(error),
                    }
                    if let Some(counts) = updated_counts {
                        self.cache_counts = counts;
                    }
                }
                RemoteEvent::ContactLoaded {
                    request_id,
                    id,
                    result,
                } => {
                    if let Some(window) = self.contact_windows.get_mut(&id)
                        && window.request_id == Some(request_id)
                    {
                        window.loading = false;
                        match result {
                            Ok(detail) => window.apply(detail),
                            Err(error) => window.error = Some(error),
                        }
                    }
                }
                RemoteEvent::ContactSaved {
                    request_id,
                    id,
                    result,
                } => {
                    let Some(window) = self.contact_windows.get_mut(&id) else {
                        continue;
                    };
                    if window.request_id != Some(request_id) {
                        continue;
                    }
                    let contact_id = id;
                    match result {
                        Ok(_) => {
                            self.close_contact_window(&contact_id);
                            self.refresh_contact_lists();
                            self.send_contacts_query(ContactFilter::All, false);
                            self.notify(ctx, "Contact saved to the server.");
                        }
                        Err(error) => {
                            if let Some(window) = self.contact_windows.get_mut(&contact_id) {
                                window.saving = false;
                                window.error = Some(error);
                            }
                        }
                    }
                }
                RemoteEvent::ContactCreated { request_id, result } => {
                    self.handle_contact_created(ctx, request_id, result);
                }
            }
        }
    }

    fn handle_contact_created(
        &mut self,
        ctx: &egui::Context,
        request_id: u64,
        result: Result<ApiContactDetail, String>,
    ) {
        if self.intake.create_request != Some(request_id) {
            return;
        }
        self.intake.create_request = None;
        self.intake.saving = false;
        match result {
            Ok(detail) => {
                let name = detail.contact.display_name.clone();
                let kind = kind_from_role(&detail.contact.role);
                let contact_id = detail.contact.id.clone();
                self.intake.completed = Some(crate::intake::CompletedIntake {
                    contact_id,
                    name: name.clone(),
                    kind: match kind {
                        ContactKind::Worker => IntakeKind::Worker,
                        ContactKind::Other | ContactKind::Dual => IntakeKind::Other,
                        ContactKind::Client => IntakeKind::Client,
                    },
                });
                self.intake.view = IntakeView::Completed;
                self.intake.dirty = false;
                self.intake.error = None;
                self.refresh_contact_lists();
                self.send_contacts_query(ContactFilter::All, false);

                let draft_id = self.intake.draft.as_ref().map(|draft| draft.id.clone());
                if let Some(draft_id) = draft_id
                    && let Some(store) = &self.store
                {
                    let request = self.intake.next_request();
                    self.intake.discard_request = Some(request);
                    store.discard_intake(request, draft_id);
                }

                self.notify(ctx, format!("{name} has been added to the server."));
            }
            Err(error) => {
                self.intake.dirty = true;
                self.intake.error = Some(error);
            }
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
                        self.refresh_contact_lists();
                        self.send_contacts_query(ContactFilter::All, false);
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
                        self.close_all_contact_windows();
                        self.refresh_contact_lists();
                        self.cache_counts = ContactCounts::default();
                        self.settings.error =
                            Some("Your session expired. Please sign in again.".to_owned());
                        self.notify(ctx, "Session expired. Please sign in again.");
                    }
                }
            }
            Event::LoggedOut { result } => {
                self.refresh_contact_lists();
                self.close_all_contact_windows();
                self.cache_counts = ContactCounts::default();
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
            self.close_all_contact_windows();
            self.refresh_contact_lists();
            self.cache_counts = ContactCounts::default();
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

                        let intake_button = ui.add(
                            egui::Button::new(
                                RichText::new("+  Intake")
                                    .color(theme::BG)
                                    .font(theme::strong_font(13.0)),
                            )
                            .fill(theme::ACCENT)
                            .corner_radius(CornerRadius::same(8))
                            .min_size(vec2(92.0, 32.0)),
                        );
                        if intake_button.clicked() {
                            self.open_intake();
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
                            let query = self.search.trim().to_owned();
                            self.page = Page::Contacts;
                            let state = &mut self.contacts_all;
                            state.search = query;
                            state.generation += 1;
                            state.search_due = Some(Instant::now());
                            state.loading = true;
                            state.next_cursor = None;
                            state.selected = None;
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
                for page in [
                    Page::Home,
                    Page::Contacts,
                    Page::Clients,
                    Page::Workers,
                    Page::Rostering,
                ] {
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
                    edition_card(
                        ui,
                        account.as_ref().map(|a| a.edition.as_str()),
                        self.access_token().is_some(),
                        self.cache_counts.all,
                        matches!(self.server.connection, ConnectionState::Offline { .. }),
                    );
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

fn edition_card(
    ui: &mut Ui,
    edition: Option<&str>,
    server_ready: bool,
    server_contacts: usize,
    server_error: bool,
) {
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

                let (colour, text) = if server_error {
                    (theme::DANGER, "Server offline".to_owned())
                } else if server_ready {
                    (
                        theme::SUCCESS,
                        if server_contacts == 0 {
                            "Server connected".to_owned()
                        } else {
                            format!("Server · {} contacts", format_count(server_contacts))
                        },
                    )
                } else {
                    (theme::WARNING, "Sign in to load contacts".to_owned())
                };

                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(7.0, 7.0), Sense::hover());
                    ui.painter().circle_filled(rect.center(), 3.5, colour);
                    ui.label(RichText::new(text).color(theme::MUTED).size(9.5));
                });
            });
        });
}

fn format_count(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
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

fn intake_write(draft: &crate::intake::IntakeDraft) -> ApiContactWrite {
    use serde_json::Value;

    let mut details = serde_json::Map::new();
    for (key, value) in &draft.fields {
        details.insert(key.clone(), Value::String(value.clone()));
    }
    let role = match draft.kind {
        IntakeKind::Client => "client",
        IntakeKind::Worker => "worker",
        IntakeKind::Other => "other",
    };
    details.insert("contact_role".to_owned(), Value::String(role.to_owned()));
    details.insert(
        "contact_status".to_owned(),
        Value::String("Onboarding".to_owned()),
    );

    let date_of_birth = chrono::NaiveDate::parse_from_str(draft.get("date_of_birth"), "%d/%m/%Y")
        .ok()
        .map(|date| date.format("%Y-%m-%d").to_string());

    let notes = ["intake_notes", "worker_notes", "notes"]
        .iter()
        .map(|key| draft.get(key).trim())
        .find(|value| !value.is_empty())
        .map(str::to_owned);

    ApiContactWrite {
        display_name: draft.display_name(),
        given_name: optional_str(draft.get("first_name")),
        family_name: optional_str(draft.get("last_name")),
        preferred_name: optional_str(draft.get("preferred_name")),
        pronouns: optional_str(draft.get("pronouns")),
        date_of_birth,
        email: optional_str(draft.get("email")),
        phone: optional_str(draft.get("phone")),
        notes,
        details: Value::Object(details),
        is_active: true,
        custom_fields: BTreeMap::new(),
    }
}

fn optional_str(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn contact_viewport_id(id: &str) -> egui::ViewportId {
    egui::ViewportId::from_hash_of(("carelibre-contact", id))
}

fn dot(ui: &mut Ui, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(6.0, 6.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(3), colour);
}
