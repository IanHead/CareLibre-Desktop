//! Application shell: top bar, navigation, status bar and routing.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::{Duration, Instant},
};

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Frame, Layout, Margin, RichText, Sense,
    Stroke, Ui, vec2,
};
use serde_json::Value;

use crate::{
    api, chrome,
    config::Config,
    contact::{
        ApiContactDetail, ApiContactWrite, ApiGraphNode, ApiRelationship, ApiRelationshipWrite,
        PartyKind, row_from_api,
    },
    credentials,
    intake::IntakeKind,
    pages::{
        self, Action, Page,
        contact_editor::{ContactEditorAction, ContactWindowState, NewRelationshipContact},
        contacts::{ContactsAction, ContactsState, LOAD_MORE_SIZE, PAGE_SIZE},
        intake::{IntakeAction, IntakeState, IntakeView},
        relationships::{RelationshipsAction, RelationshipsForm},
        rostering::{RosteringAction, RosteringState},
        settings::SettingsForm,
    },
    perf::{PerfHud, PerfInfo},
    remote::{RemoteEvent, RemoteHandle},
    server::{ConnectionState, Event, ServerRuntime, ServerState},
    store::{self, ContactCounts, ContactFilter, ContactKind, ContactRow, StoreEvent, StoreHandle},
    theme,
};

const DOCK_ANIM_SECS: f32 = 0.20;

pub struct CareLibreApp {
    page: Page,
    search: String,
    notice: Option<Notice>,
    config: Config,
    settings: SettingsForm,
    server: ServerState,
    runtime: Option<ServerRuntime>,
    contacts: ContactsState,
    intake: IntakeState,
    contact_windows: BTreeMap<String, ContactWindowState>,
    contact_order: Vec<String>,
    active_contact: Option<String>,
    drag_tab: Option<String>,
    relationship_request_seq: u64,
    relationship_list_requests: BTreeMap<u64, String>,
    relationship_create_requests: BTreeMap<u64, Option<String>>,
    /// Client/worker links to the organisation requested after an intake,
    /// keyed by request id, holding the new contact's name.
    intake_role_links: BTreeMap<u64, String>,
    relationship_delete_requests: BTreeMap<u64, Option<String>>,
    relationship_contact_creates: BTreeMap<u64, PendingInlineRelationship>,
    party_catalog: Vec<ApiGraphNode>,
    party_catalog_request: Option<u64>,
    relationship_edges: Vec<ApiRelationship>,
    relationship_graph_loaded: bool,
    relationship_form: RelationshipsForm,
    rostering: RosteringState,
    rostering_request_seq: u64,
    rostering_requests: BTreeMap<u64, RosterRequest>,
    known_contacts: BTreeMap<String, ContactRow>,
    last_page: Page,
    page_changed_at: f64,
    dock_closing: Option<String>,
    dock_close_started: Option<f64>,
    popped_pages: BTreeSet<Page>,
    remote: Option<RemoteHandle>,
    app_ctx: Option<egui::Context>,
    store: Option<StoreHandle>,
    store_ready: bool,
    store_error: Option<String>,
    store_path: String,
    cache_counts: ContactCounts,
    cache_shifts: Option<usize>,
    restoring_session: bool,
    perf: PerfHud,
}

struct Notice {
    text: String,
    created_at: f64,
    expires_at: f64,
}

enum RosterRequest {
    Sites,
    Shifts,
    Detail,
    Suitability,
    Filters,
    CreateShifts,
    UpdateShift,
    CopyShift,
    DeleteShift,
    CreateSite,
    CreateAssignment,
    UpdateAssignment,
    DeleteAssignment,
}

struct PendingInlineRelationship {
    window_id: String,
    from_role: String,
    to_role: String,
    notes: String,
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
        let settings = SettingsForm::new(
            &config.server_url,
            config.theme,
            config.client_side_decorations,
            config.remember_login,
        );

        Self {
            page: Page::Home,
            search: String::new(),
            notice: None,
            config,
            settings,
            server,
            runtime: None,
            contacts: ContactsState::new(ContactFilter::All),
            intake: IntakeState::new(),
            contact_windows: BTreeMap::new(),
            contact_order: Vec::new(),
            active_contact: None,
            drag_tab: None,
            // Keep relationship request ids in a separate range so they can
            // never collide with intake creation request ids.
            relationship_request_seq: 1 << 32,
            relationship_list_requests: BTreeMap::new(),
            relationship_create_requests: BTreeMap::new(),
            intake_role_links: BTreeMap::new(),
            relationship_delete_requests: BTreeMap::new(),
            relationship_contact_creates: BTreeMap::new(),
            party_catalog: Vec::new(),
            party_catalog_request: None,
            relationship_edges: Vec::new(),
            relationship_graph_loaded: false,
            relationship_form: RelationshipsForm::default(),
            rostering: RosteringState::default(),
            // Rostering request ids live in their own range.
            rostering_request_seq: 1 << 40,
            rostering_requests: BTreeMap::new(),
            known_contacts: BTreeMap::new(),
            last_page: Page::Home,
            page_changed_at: f64::NEG_INFINITY,
            dock_closing: None,
            dock_close_started: None,
            popped_pages: BTreeSet::new(),
            remote: None,
            app_ctx: None,
            store: None,
            store_ready: false,
            store_error: None,
            store_path: String::new(),
            cache_counts: ContactCounts::default(),
            cache_shifts: None,
            restoring_session: false,
            perf: PerfHud::new(),
        }
    }

    /// Build the app and start the background server worker.
    pub fn with_server(ctx: &egui::Context) -> Self {
        let mut app = Self::new();
        app.connect(ctx);
        app.try_restore_session();
        app
    }

    fn try_restore_session(&mut self) {
        if !self.config.remember_login {
            return;
        }
        let Some(email) = self.config.remembered_email.clone() else {
            return;
        };

        match credentials::load(&self.config.server_url, &email) {
            Ok(Some(token)) => {
                if let Some(runtime) = &self.runtime {
                    runtime.refresh(token);
                    self.server.refresh_in_flight = true;
                    self.restoring_session = true;
                }
            }
            Ok(None) => {
                self.config.remembered_email = None;
                let _ = self.config.save();
            }
            Err(error) => {
                self.settings.error = Some(format!("Could not read the saved login: {error}"));
            }
        }
    }

    fn remember_current_session(&mut self) -> Result<(), String> {
        let Some(session) = &self.server.session else {
            return Ok(());
        };
        let email = session.auth.user.email.clone();
        let token = session.auth.refresh_token.clone();
        credentials::save(&self.config.server_url, &email, &token)?;
        if self.config.remembered_email.as_deref() != Some(email.as_str()) {
            self.config.remembered_email = Some(email);
            self.config.save()?;
        }
        Ok(())
    }

    fn forget_saved_login(&mut self) {
        if let Some(email) = self.config.remembered_email.clone() {
            let _ = credentials::delete(&self.config.server_url, &email);
        }
        self.config.remembered_email = None;
        let _ = self.config.save();
    }

    fn apply_window_decorations(&self, ctx: &egui::Context, client_side: bool) {
        let decorations = !client_side;
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(decorations));
        for (id, window) in &self.contact_windows {
            if window.popped_out {
                ctx.send_viewport_cmd_to(
                    contact_viewport_id(id),
                    egui::ViewportCommand::Decorations(decorations),
                );
            }
        }
        if self.intake.popped_out {
            ctx.send_viewport_cmd_to(
                intake_viewport_id(),
                egui::ViewportCommand::Decorations(decorations),
            );
        }
        ctx.request_repaint();
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
        self.last_page = page;
    }

    pub fn docked_contact_ids(&self) -> Vec<String> {
        self.contact_order
            .iter()
            .filter(|id| {
                self.contact_windows
                    .get(*id)
                    .is_some_and(|window| !window.popped_out)
            })
            .cloned()
            .collect()
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

    /// Change the decoration mode for previews/tests without persisting it.
    pub fn set_client_side_decorations(&mut self, enabled: bool) {
        self.config.client_side_decorations = enabled;
        self.settings.client_side_decorations = enabled;
    }

    /// Clear transient notifications for headless screenshots.
    pub fn dismiss_notice(&mut self) {
        self.notice = None;
    }

    /// Change the appearance for previews/tests without persisting it.
    pub fn set_theme_mode(&mut self, mode: theme::ThemeMode) {
        self.config.theme = mode;
        self.settings.theme = mode;
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
        self.contacts.loaded()
    }

    /// Contacts loaded for the global list; also proves server counts are ready.
    pub fn server_contacts_loaded(&self) -> usize {
        self.contacts.loaded()
    }

    /// The whole application UI. Kept separate from [`eframe::App`] so it can be
    /// rendered by tooling (headless screenshot tests) as well.
    pub fn show(&mut self, ui: &mut Ui) {
        theme::sync(ui.ctx(), self.config.theme);
        let now = ui.ctx().input(|i| i.time);
        if self.page != self.last_page {
            self.last_page = self.page;
            self.page_changed_at = now;
        }
        let page_progress = ((now - self.page_changed_at) / 0.18).clamp(0.0, 1.0) as f32;
        if page_progress < 1.0 {
            ui.ctx().request_repaint();
        }
        self.perf.begin_frame(ui.ctx());
        self.poll_server(ui.ctx());
        self.poll_store(ui.ctx());
        self.poll_remote(ui.ctx());
        self.tick_contact_search();
        self.tick_intake_autosave();
        self.ensure_contacts_loaded();
        self.ensure_intake_list();
        self.refresh_session_if_needed();

        if self.config.client_side_decorations {
            egui::Panel::top("app_window_chrome")
                .exact_size(chrome::HEIGHT)
                .show_separator_line(false)
                .frame(
                    Frame::new()
                        .fill(theme::sidebar())
                        .inner_margin(Margin::ZERO),
                )
                .show(ui, |ui| {
                    let ctx = ui.ctx().clone();
                    chrome::title_bar(ui, &ctx, "CareLibre");
                });
        }

        self.top_bar(ui);
        self.sidebar(ui);
        self.status_bar(ui);
        self.render_contact_dock(ui);

        let mut contacts_action: Option<ContactsAction> = None;
        let mut intake_action: Option<IntakeAction> = None;
        let mut relationship_action: Option<RelationshipsAction> = None;
        let mut rostering_action: Option<RosteringAction> = None;
        let page_popped = self.page_popped_out(self.page);
        let action = egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(theme::bg())
                    .inner_margin(Margin::same(24)),
            )
            .show(ui, |ui| {
                self.page_toolbar(ui);
                ui.add_space(10.0);
                if page_progress < 1.0 {
                    let enter = theme::ease_out(page_progress);
                    ui.multiply_opacity(0.35 + 0.65 * enter);
                    ui.add_space((1.0 - enter) * 8.0);
                }
                if page_popped {
                    self.detached_page_placeholder(ui);
                    None
                } else {
                    self.page_contents(
                        ui,
                        self.page,
                        &mut contacts_action,
                        &mut intake_action,
                        &mut relationship_action,
                        &mut rostering_action,
                    )
                }
            })
            .inner;

        if let Some(contacts_action) = contacts_action {
            self.handle_contacts_action(contacts_action);
        }
        if let Some(intake_action) = intake_action {
            self.handle_intake_action(ui.ctx(), intake_action);
        }
        if let Some(relationship_action) = relationship_action {
            self.handle_relationships_action(ui.ctx(), relationship_action);
        }
        if let Some(rostering_action) = rostering_action {
            self.handle_rostering_action(ui.ctx(), rostering_action);
        }

        if let Some(action) = action {
            self.handle_action(ui.ctx(), action);
        }

        self.render_contact_windows(ui.ctx());
        self.render_intake_window(ui.ctx());
        self.render_page_viewports(ui.ctx());

        if self.config.client_side_decorations {
            chrome::resize_handles(ui.ctx());
        }

        let perf_state = &self.contacts;
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
        page: Page,
        contacts_action: &mut Option<ContactsAction>,
        intake_action: &mut Option<IntakeAction>,
        relationship_action: &mut Option<RelationshipsAction>,
        rostering_action: &mut Option<RosteringAction>,
    ) -> Option<Action> {
        if page == Page::Relationships {
            self.ensure_party_catalog();
        }
        match page {
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
                    &mut self.contacts,
                    signed_in,
                    server_error.as_deref(),
                ) {
                    *contacts_action = Some(action);
                }
                None
            }
            Page::Rostering => {
                self.ensure_roster_loaded();
                let catalog = self.lookup_parties();
                if let Some(action) = pages::rostering::show(ui, &mut self.rostering, &catalog) {
                    *rostering_action = Some(action);
                }
                None
            }
            Page::Relationships => {
                let loading =
                    self.party_catalog_request.is_some() || !self.relationship_graph_loaded;
                if let Some(action) = pages::relationships::show(
                    ui,
                    &mut self.relationship_form,
                    &self.party_catalog,
                    &self.relationship_edges,
                    loading,
                ) {
                    *relationship_action = Some(action);
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
            created_at: now,
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
        if let Some(due) = self.contacts.search_due
            && Instant::now() >= due
        {
            self.contacts.search_due = None;
            self.send_contacts_query(false);
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

        if self.page != Page::Contacts {
            return;
        }

        if self.contacts.generation == 0 && self.contacts.search_due.is_none() {
            self.send_contacts_query(false);
        }
    }

    fn send_contacts_query(&mut self, append: bool) {
        let filter = self.contacts.filter;
        let (generation, search, after, limit) = {
            let state = &mut self.contacts;
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

        let role = filter.role_param();

        let Some(token) = self.access_token() else {
            let state = &mut self.contacts;
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
            let state = &mut self.contacts;
            state.loading = false;
            state.error = Some("The server connection is not available.".to_owned());
        }
    }

    fn handle_contacts_action(&mut self, action: ContactsAction) {
        match action {
            ContactsAction::FilterChanged(filter) => self.show_contacts(filter, None),
            ContactsAction::SearchChanged => {
                let state = &mut self.contacts;
                state.generation += 1;
                state.search_due = Some(Instant::now() + Duration::from_millis(120));
                state.loading = true;
                state.next_cursor = None;
                state.needs_more = false;
            }
            ContactsAction::OpenContact(id) => self.open_contact(&id),
            ContactsAction::LoadMore => self.send_contacts_query(true),
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
                let was_popped = self.intake.popped_out;
                if self.intake.dirty {
                    self.save_intake_now();
                }
                self.intake.open_picker();
                self.intake.list_requested = false;
                if was_popped && let Some(ctx) = &self.app_ctx {
                    ctx.send_viewport_cmd_to(intake_viewport_id(), egui::ViewportCommand::Close);
                }
            }
            IntakeAction::PopOut => {
                self.intake.popped_out = true;
            }
            IntakeAction::Dock => {
                self.intake.popped_out = false;
                if let Some(ctx) = &self.app_ctx {
                    ctx.send_viewport_cmd_to(intake_viewport_id(), egui::ViewportCommand::Close);
                }
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
            IntakeAction::ViewContact { filter, name } => {
                self.page = Page::Contacts;
                self.show_contacts(filter, Some(name));
                let _ = ctx;
            }
        }
    }

    fn refresh_contact_lists(&mut self) {
        let state = &mut self.contacts;
        state.reset();
        state.generation = 0;
        state.next_cursor = None;
        state.loading = false;
    }

    /// Switch the contact list to a relationship filter, optionally searching
    /// for a name, and reload it from the first page.
    fn show_contacts(&mut self, filter: ContactFilter, search: Option<String>) {
        let state = &mut self.contacts;
        state.filter = filter;
        if let Some(search) = search {
            state.search = search;
        }
        state.reset();
        state.total = 0;
        state.generation += 1;
        state.search_due = Some(Instant::now());
        state.loading = true;
        state.next_cursor = None;
        state.needs_more = false;
        state.selected = None;
    }

    /// Show the contact list narrowed to a relationship. Used by screenshot previews.
    pub fn set_contact_filter(&mut self, filter: ContactFilter) {
        self.page = Page::Contacts;
        self.last_page = Page::Contacts;
        self.contacts.filter = filter;
        self.refresh_contact_lists();
    }

    pub fn open_contact(&mut self, id: &str) {
        self.dock_closing = None;
        if let Some(window) = self.contact_windows.get(id) {
            if window.popped_out
                && let Some(ctx) = &self.app_ctx
            {
                ctx.send_viewport_cmd_to(contact_viewport_id(id), egui::ViewportCommand::Focus);
            } else {
                self.active_contact = Some(id.to_owned());
            }
            let needs_relationships =
                window.relationships.is_empty() && !window.relationships_loading;
            self.ensure_party_catalog();
            if needs_relationships {
                self.load_contact_relationships(id);
            }
            return;
        }

        let state = ContactWindowState::new(id.to_owned());
        let request_id = state.request_id.unwrap_or(1);
        self.contact_windows.insert(id.to_owned(), state);
        self.contact_order.push(id.to_owned());
        self.active_contact = Some(id.to_owned());
        self.load_contact_window(id, request_id);
        self.ensure_party_catalog();
        self.load_contact_relationships(id);
    }

    fn next_relationship_request(&mut self) -> u64 {
        self.relationship_request_seq += 1;
        self.relationship_request_seq
    }

    fn ensure_party_catalog(&mut self) {
        if self.relationship_graph_loaded || self.party_catalog_request.is_some() {
            return;
        }
        let Some(token) = self.access_token() else {
            return;
        };
        if self.remote.is_none() {
            return;
        }
        let request = self.next_relationship_request();
        if let Some(remote) = &self.remote {
            remote.relationship_graph(request, self.server.base_url.clone(), token);
        }
        self.party_catalog_request = Some(request);
    }

    fn load_contact_relationships(&mut self, id: &str) {
        let Some(token) = self.access_token() else {
            return;
        };
        if self.remote.is_none() {
            return;
        }
        let request = self.next_relationship_request();
        if let Some(remote) = &self.remote {
            remote.list_relationships(
                request,
                self.server.base_url.clone(),
                token,
                Some(id.to_owned()),
                None,
            );
        }
        self.relationship_list_requests
            .insert(request, id.to_owned());
        if let Some(window) = self.contact_windows.get_mut(id) {
            window.relationships_loading = true;
            window.relationship_request_id = Some(request);
        }
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

    /// Close a record. If it is the last docked record, keep it around for the
    /// dock's slide-out animation before removing it.
    fn close_contact_window(&mut self, id: &str) {
        let is_docked = self
            .contact_windows
            .get(id)
            .is_some_and(|window| !window.popped_out);
        let docked_count = self
            .contact_order
            .iter()
            .filter(|open| {
                self.contact_windows
                    .get(*open)
                    .is_some_and(|window| !window.popped_out)
            })
            .count();

        if is_docked && docked_count <= 1 && self.dock_closing.is_none() {
            self.dock_closing = Some(id.to_owned());
            self.dock_close_started = self
                .app_ctx
                .as_ref()
                .map(|ctx| ctx.input(|input| input.time));
            if let Some(ctx) = &self.app_ctx {
                ctx.request_repaint();
            }
            return;
        }

        self.remove_contact_window(id);
    }

    fn remove_contact_window(&mut self, id: &str) {
        let popped_out = self
            .contact_windows
            .get(id)
            .map(|window| window.popped_out)
            .unwrap_or(false);
        self.contact_windows.remove(id);
        self.contact_order.retain(|open| open != id);
        self.relationship_list_requests
            .retain(|_, contact_id| contact_id != id);
        self.relationship_create_requests
            .retain(|_, window_id| window_id.as_deref() != Some(id));
        self.relationship_contact_creates
            .retain(|_, pending| pending.window_id != id);
        if self.active_contact.as_deref() == Some(id) {
            self.active_contact = self
                .contact_order
                .iter()
                .find(|open| {
                    self.contact_windows
                        .get(*open)
                        .is_some_and(|window| !window.popped_out)
                })
                .cloned();
        }
        if popped_out && let Some(ctx) = &self.app_ctx {
            ctx.send_viewport_cmd_to(contact_viewport_id(id), egui::ViewportCommand::Close);
        }
    }

    fn close_all_contact_windows(&mut self) {
        let ids: Vec<String> = self.contact_windows.keys().cloned().collect();
        for id in ids {
            self.close_contact_window(&id);
        }
    }

    /// The pane toolbar: identifies the open page and offers the page-level
    /// pop-out control.
    fn page_toolbar(&mut self, ui: &mut Ui) {
        let page = self.page;
        let popped = self.popped_pages.contains(&page);
        let mut toggle = false;

        Frame::new()
            .fill(theme::surface())
            .stroke(Stroke::new(1.0, theme::border()))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::symmetric(12, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{}  {}", page.icon(), page.label()))
                            .color(theme::muted())
                            .font(theme::strong_font(11.5)),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let label = if popped { "Dock page" } else { "Pop out page" };
                        let glyph = if popped { "↙" } else { "↗" };
                        let button = egui::Button::new(
                            RichText::new(format!("{glyph}  {label}"))
                                .color(theme::accent())
                                .font(theme::strong_font(11.5)),
                        )
                        .fill(theme::surface_raised())
                        .stroke(Stroke::new(1.0, theme::border()))
                        .corner_radius(CornerRadius::same(8))
                        .min_size(vec2(0.0, 30.0));
                        let response = ui.add(button);
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::Role::Button, true, label)
                        });
                        if response.clicked() {
                            toggle = true;
                        }
                        response.on_hover_text(if popped {
                            "Bring this page back into the main window"
                        } else {
                            "Open this page in its own native window"
                        });
                    });
                });
            });

        if toggle {
            self.toggle_page_popout(ui.ctx(), page);
        }
    }

    /// Placeholder shown in the main window when the current page lives in its
    /// own native window.
    fn detached_page_placeholder(&mut self, ui: &mut Ui) {
        let page = self.page;
        let mut focus = false;
        let mut dock = false;
        ui.vertical_centered(|ui| {
            ui.add_space(70.0);
            let (rect, _) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::hover());
            ui.painter()
                .rect_filled(rect, CornerRadius::same(15), theme::surface_raised());
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                page.icon(),
                FontId::proportional(24.0),
                theme::text(),
            );
            ui.add_space(12.0);
            ui.label(RichText::new(page.label()).font(theme::strong_font(24.0)));
            ui.label(
                RichText::new("This page is open in its own window.")
                    .color(theme::muted())
                    .size(12.5),
            );
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if ui.add(theme::primary_button("Focus window")).clicked() {
                    focus = true;
                }
                if ui.add(theme::secondary_button("Dock back")).clicked() {
                    dock = true;
                }
            });
        });

        if focus {
            ui.ctx()
                .send_viewport_cmd_to(page_viewport_id(page), egui::ViewportCommand::Focus);
        }
        if dock {
            self.dock_page(ui.ctx(), page);
        }
    }

    pub fn page_popped_out(&self, page: Page) -> bool {
        self.popped_pages.contains(&page)
    }

    fn toggle_page_popout(&mut self, ctx: &egui::Context, page: Page) {
        if self.popped_pages.contains(&page) {
            self.dock_page(ctx, page);
        } else {
            self.popped_pages.insert(page);
            ctx.request_repaint();
        }
    }

    fn dock_page(&mut self, ctx: &egui::Context, page: Page) {
        self.popped_pages.remove(&page);
        ctx.send_viewport_cmd_to(page_viewport_id(page), egui::ViewportCommand::Close);
        ctx.request_repaint();
    }

    /// Render every page that has been detached into a native window. Each
    /// window reuses the page renderer, so pop-out/dock works for every page.
    fn render_page_viewports(&mut self, ctx: &egui::Context) {
        let pages: Vec<Page> = self.popped_pages.iter().copied().collect();
        for page in pages {
            let viewport_id = page_viewport_id(page);
            let title = format!("{} — CareLibre", page.label());
            let builder = egui::ViewportBuilder::default()
                .with_title(&title)
                .with_app_id("carelibre-page")
                .with_decorations(!self.config.client_side_decorations)
                .with_inner_size([1180.0, 800.0])
                .with_min_inner_size([720.0, 520.0]);
            let client_side = self.config.client_side_decorations;
            let mut close = false;
            let mut contacts_action: Option<ContactsAction> = None;
            let mut intake_action: Option<IntakeAction> = None;
            let mut relationship_action: Option<RelationshipsAction> = None;
            let mut rostering_action: Option<RosteringAction> = None;
            let mut page_action: Option<Action> = None;

            ctx.show_viewport_immediate(viewport_id, builder, |ui, _| {
                if ui.ctx().input(|input| input.viewport().close_requested()) {
                    close = true;
                    return;
                }

                if client_side {
                    egui::Panel::top(format!("page-window-chrome-{}", page.label()))
                        .exact_size(chrome::HEIGHT)
                        .show_separator_line(false)
                        .frame(
                            Frame::new()
                                .fill(theme::sidebar())
                                .inner_margin(Margin::ZERO),
                        )
                        .show(ui, |ui| {
                            let ctx = ui.ctx().clone();
                            chrome::title_bar(ui, &ctx, &title);
                        });
                }

                egui::Panel::top(format!("page-window-bar-{}", page.label()))
                    .exact_size(38.0)
                    .show_separator_line(false)
                    .frame(
                        Frame::new()
                            .fill(theme::sidebar())
                            .inner_margin(Margin::symmetric(14, 4))
                            .stroke(Stroke::new(1.0, theme::border())),
                    )
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("{}  {}", page.icon(), page.label()))
                                    .color(theme::text())
                                    .font(theme::strong_font(12.5)),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.add(theme::secondary_button("Dock")).clicked() {
                                    close = true;
                                }
                            });
                        });
                    });

                egui::CentralPanel::default()
                    .frame(
                        Frame::new()
                            .fill(theme::bg())
                            .inner_margin(Margin::same(24)),
                    )
                    .show(ui, |ui| {
                        page_action = self.page_contents(
                            ui,
                            page,
                            &mut contacts_action,
                            &mut intake_action,
                            &mut relationship_action,
                            &mut rostering_action,
                        );
                    });

                if client_side {
                    chrome::resize_handles(ui.ctx());
                }
            });

            if close {
                self.popped_pages.remove(&page);
                ctx.send_viewport_cmd_to(viewport_id, egui::ViewportCommand::Close);
                continue;
            }
            if let Some(action) = contacts_action {
                self.handle_contacts_action(action);
            }
            if let Some(action) = intake_action {
                self.handle_intake_action(ctx, action);
            }
            if let Some(action) = relationship_action {
                self.handle_relationships_action(ctx, action);
            }
            if let Some(action) = rostering_action {
                self.handle_rostering_action(ctx, action);
            }
            if let Some(action) = page_action {
                self.handle_action(ctx, action);
            }
        }
    }

    fn render_contact_windows(&mut self, ctx: &egui::Context) {
        let popped: Vec<String> = self
            .contact_order
            .iter()
            .filter(|id| {
                self.contact_windows
                    .get(*id)
                    .is_some_and(|window| window.popped_out)
            })
            .cloned()
            .collect();
        let catalog = self.lookup_parties();
        let mut actions: Vec<(String, ContactEditorAction)> = Vec::new();

        for id in popped {
            let Some(window) = self.contact_windows.get_mut(&id) else {
                continue;
            };
            let viewport_id = contact_viewport_id(&id);
            let builder = egui::ViewportBuilder::default()
                .with_title(window.title())
                .with_app_id("carelibre-contact")
                .with_decorations(!self.config.client_side_decorations)
                .with_inner_size([880.0, 780.0])
                .with_min_inner_size([640.0, 520.0]);

            let client_side = self.config.client_side_decorations;
            let action = ctx.show_viewport_immediate(viewport_id, builder, |ui, _| {
                pages::contact_editor::show_viewport(ui, window, client_side, &catalog)
            });

            if window.close_requested {
                ctx.send_viewport_cmd_to(viewport_id, egui::ViewportCommand::Close);
            }
            if let Some(action) = action {
                actions.push((id.clone(), action));
            }
        }

        for (id, action) in actions {
            self.handle_editor_action(ctx, &id, action);
        }

        self.contact_windows
            .retain(|_, window| !window.close_requested);
    }

    /// The right-hand record dock: tabs for every open record, a draggable tab
    /// strip, and the active contact editor.
    fn render_contact_dock(&mut self, ui: &mut Ui) {
        let mut docked: Vec<String> = self
            .contact_order
            .iter()
            .filter(|id| {
                self.contact_windows
                    .get(*id)
                    .is_some_and(|window| !window.popped_out)
            })
            .cloned()
            .collect();
        for (id, window) in &self.contact_windows {
            if !window.popped_out && !self.contact_order.contains(id) {
                docked.push(id.clone());
            }
        }

        if let Some(closing) = self.dock_closing.clone()
            && !docked.iter().any(|id| id == &closing)
        {
            self.dock_closing = None;
            self.dock_close_started = None;
        }

        if docked.is_empty() {
            // Keep the slide animation seeded at 0 while closed so the first
            // record opens with an eased 0 → 1 slide instead of jumping open.
            let animation_id = ui.scope_id().with("contact_dock").with("animation");
            let _ = ui.ctx().animate_bool_responsive(animation_id, false);
            self.active_contact = None;
            self.dock_closing = None;
            self.dock_close_started = None;
            return;
        }

        let now = ui.ctx().input(|input| input.time);
        if let Some(closing) = self.dock_closing.clone() {
            let started = *self.dock_close_started.get_or_insert(now);
            if now - started >= DOCK_ANIM_SECS as f64 {
                self.dock_closing = None;
                self.dock_close_started = None;
                self.remove_contact_window(&closing);
                ui.ctx().request_repaint();
                return;
            }
            ui.ctx().request_repaint();
        }

        let active = self
            .active_contact
            .clone()
            .filter(|id| docked.iter().any(|open| open == id))
            .unwrap_or_else(|| docked[0].clone());
        self.active_contact = Some(active.clone());

        let catalog = self.lookup_parties();
        let mut close: Option<String> = None;
        let mut activate: Option<String> = None;
        let mut reorder: Option<(String, String)> = None;
        let mut close_all = false;
        let mut editor_action: Option<(String, ContactEditorAction)> = None;

        let collapsed = egui::Panel::right("contact_dock_collapsed")
            .exact_size(0.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new());
        let expanded = egui::Panel::right("contact_dock")
            .resizable(true)
            .default_size(520.0)
            .size_range(380.0..=780.0)
            .show_separator_line(false)
            .frame(
                Frame::new()
                    .fill(theme::surface())
                    .stroke(Stroke::new(1.0, theme::border()))
                    .inner_margin(Margin::ZERO),
            );

        // `show_switched` animates the expanded panel toward its edge and clips
        // it, using egui's responsive cubic-out easing. Closing keeps the last
        // record in place until the slide-out completes.
        let mut is_expanded = self.dock_closing.is_none();
        egui::Panel::show_switched(
            ui,
            &mut is_expanded,
            collapsed,
            expanded,
            |ui, show_expanded| {
                if !show_expanded {
                    return;
                }

                self.dock_tab_strip(
                    ui,
                    &docked,
                    &mut close,
                    &mut activate,
                    &mut reorder,
                    &mut close_all,
                );

                if let Some(action) = self
                    .contact_windows
                    .get_mut(&active)
                    .and_then(|window| pages::contact_editor::show_docked(ui, window, &catalog))
                {
                    editor_action = Some((active.clone(), action));
                }
            },
        );

        if let Some((dragged, target)) = reorder {
            let from = self.contact_order.iter().position(|id| id == &dragged);
            let to = self.contact_order.iter().position(|id| id == &target);
            if let (Some(from), Some(to)) = (from, to)
                && dragged != target
            {
                let target = if from < to { to - 1 } else { to };
                let id = self.contact_order.remove(from);
                let target = target.min(self.contact_order.len());
                self.contact_order.insert(target, id);
            }
        }

        if let Some(id) = activate {
            self.active_contact = Some(id);
        }
        if let Some(id) = close {
            self.close_contact_window(&id);
        }
        if close_all {
            let ids: Vec<String> = docked.clone();
            for id in ids {
                self.close_contact_window(&id);
            }
        }
        if let Some((id, action)) = editor_action {
            let ctx = ui.ctx().clone();
            self.handle_editor_action(&ctx, &id, action);
        }
    }

    fn dock_tab_strip(
        &mut self,
        ui: &mut Ui,
        docked: &[String],
        close: &mut Option<String>,
        activate: &mut Option<String>,
        reorder: &mut Option<(String, String)>,
        close_all: &mut bool,
    ) {
        Frame::new()
            .fill(theme::sidebar())
            .inner_margin(Margin::symmetric(10, 7))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("OPEN RECORDS")
                            .color(theme::faint())
                            .font(theme::strong_font(9.0)),
                    );
                    ui.add_space(4.0);

                    let mut rects: Vec<(String, egui::Rect)> = Vec::new();
                    egui::ScrollArea::horizontal()
                        .id_salt("contact_dock_tabs")
                        .max_height(34.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                for id in docked {
                                    let name = self
                                        .contact_windows
                                        .get(id)
                                        .map(|window| {
                                            if window.display_name.is_empty() {
                                                "Loading…".to_owned()
                                            } else {
                                                window.display_name.clone()
                                            }
                                        })
                                        .unwrap_or_else(|| "Contact".to_owned());
                                    let selected =
                                        self.active_contact.as_deref() == Some(id.as_str());
                                    let (response, close_clicked) =
                                        dock_tab(ui, id, &name, selected);
                                    rects.push((id.clone(), response.rect));

                                    if close_clicked {
                                        *close = Some(id.clone());
                                    } else if response.clicked() {
                                        *activate = Some(id.clone());
                                    }
                                    if response.drag_started() {
                                        self.drag_tab = Some(id.clone());
                                    }
                                    if response.dragged() {
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                                    }
                                }
                            });
                        });

                    if let Some(dragged) = self.drag_tab.clone() {
                        if let Some(pointer) = ui.ctx().input(|i| i.pointer.interact_pos())
                            && let Some((target, _)) =
                                rects.iter().find(|(_, rect)| rect.contains(pointer))
                            && target != &dragged
                        {
                            *reorder = Some((dragged.clone(), target.clone()));
                        }
                        if ui.ctx().input(|i| i.pointer.any_released()) {
                            self.drag_tab = None;
                        }
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Close all").color(theme::faint()).size(10.0),
                                )
                                .frame(false),
                            )
                            .clicked()
                        {
                            *close_all = true;
                        }
                    });
                });
            });
    }

    fn next_rostering_request(&mut self) -> u64 {
        self.rostering_request_seq += 1;
        self.rostering_request_seq
    }

    fn ensure_roster_loaded(&mut self) {
        if self.rostering.requested {
            return;
        }
        if self.access_token().is_none() || self.remote.is_none() {
            return;
        }
        self.rostering.requested = true;
        self.rostering.loading_shifts = true;
        self.load_roster_sites();
        self.load_roster_filters();
        self.load_roster_shifts();
    }

    fn load_roster_sites(&mut self) {
        let Some(token) = self.access_token() else {
            return;
        };
        if self.remote.is_none() {
            return;
        }
        let request = self.next_rostering_request();
        self.rostering_requests
            .insert(request, RosterRequest::Sites);
        if let Some(remote) = &self.remote {
            remote.list_sites(request, self.server.base_url.clone(), token);
        }
    }

    fn load_roster_filters(&mut self) {
        let Some(token) = self.access_token() else {
            return;
        };
        if self.remote.is_none() {
            return;
        }
        let request = self.next_rostering_request();
        self.rostering_requests
            .insert(request, RosterRequest::Filters);
        if let Some(remote) = &self.remote {
            remote.roster_filters(request, self.server.base_url.clone(), token);
        }
    }

    fn load_roster_shifts(&mut self) {
        let Some(token) = self.access_token() else {
            return;
        };
        if self.remote.is_none() {
            return;
        }
        let from = match crate::rostering::local_day_start(&self.rostering.week_start) {
            Ok(value) => value,
            Err(error) => {
                self.rostering.error = Some(error);
                return;
            }
        };
        let last_day = crate::rostering::add_days(&self.rostering.week_start, 6);
        let to = match crate::rostering::local_day_end(&last_day) {
            Ok(value) => value,
            Err(error) => {
                self.rostering.error = Some(error);
                return;
            }
        };
        self.rostering.loading_shifts = true;
        let request = self.next_rostering_request();
        self.rostering_requests
            .insert(request, RosterRequest::Shifts);
        if let Some(remote) = &self.remote {
            remote.list_shifts(
                request,
                self.server.base_url.clone(),
                token,
                Some(from),
                Some(to),
                Some(self.rostering.filter_status.clone()),
                self.rostering.filter_site.clone(),
                self.rostering.filter_division.clone(),
                self.rostering.filter_service.clone(),
                self.rostering.filter_cost_centre.clone(),
            );
        }
    }

    fn load_roster_detail(&mut self, id: String) {
        let Some(token) = self.access_token() else {
            return;
        };
        if self.remote.is_none() {
            return;
        }
        self.rostering.loading_detail = true;
        let request = self.next_rostering_request();
        self.rostering_requests
            .insert(request, RosterRequest::Detail);
        if let Some(remote) = &self.remote {
            remote.get_shift(request, self.server.base_url.clone(), token, id);
        }
    }

    fn load_roster_suitability(&mut self, id: String) {
        let Some(token) = self.access_token() else {
            return;
        };
        if self.remote.is_none() {
            return;
        }
        self.rostering.loading_suitability = true;
        let request = self.next_rostering_request();
        self.rostering_requests
            .insert(request, RosterRequest::Suitability);
        if let Some(remote) = &self.remote {
            remote.shift_suitability(request, self.server.base_url.clone(), token, id);
        }
    }

    fn refresh_roster_after_assignment(&mut self) {
        if let Some(id) = self.rostering.selected.clone() {
            self.load_roster_detail(id.clone());
            if self.rostering.suitability.is_some() {
                self.load_roster_suitability(id);
            }
        }
        self.load_roster_shifts();
    }

    fn handle_rostering_action(&mut self, ctx: &egui::Context, action: RosteringAction) {
        let Some(token) = self.access_token() else {
            self.rostering.error = Some("Sign in to use rostering.".to_owned());
            return;
        };
        if self.remote.is_none() {
            self.rostering.error = Some("The server connection is not available.".to_owned());
            return;
        }

        match action {
            RosteringAction::OpenCreate(date) => {
                self.rostering.editor = Some(pages::rostering::ShiftEditor::new_for(date));
            }
            RosteringAction::Refresh => {
                self.rostering.requested = false;
                self.rostering.error = None;
                self.ensure_roster_loaded();
            }
            RosteringAction::LoadShift(id) => {
                self.rostering.selected = Some(id.clone());
                self.rostering.detail = None;
                self.rostering.suitability = None;
                self.rostering.suitability_error = None;
                self.load_roster_detail(id);
            }
            RosteringAction::CreateShifts { shifts, recurrence } => {
                if let Some(editor) = &mut self.rostering.editor {
                    editor.saving = true;
                    editor.error = None;
                }
                let request = self.next_rostering_request();
                self.rostering_requests
                    .insert(request, RosterRequest::CreateShifts);
                if let Some(remote) = &self.remote {
                    remote.create_shifts(
                        request,
                        self.server.base_url.clone(),
                        token,
                        crate::rostering::ApiBatchShiftWrite {
                            shifts,
                            recurrence: (!recurrence.trim().is_empty())
                                .then(|| recurrence.trim().to_owned()),
                        },
                    );
                }
            }
            RosteringAction::CopyShift { id, payload } => {
                if let Some(paste) = &mut self.rostering.paste {
                    paste.saving = true;
                    paste.error = None;
                }
                let request = self.next_rostering_request();
                self.rostering_requests
                    .insert(request, RosterRequest::CopyShift);
                if let Some(remote) = &self.remote {
                    remote.copy_shift(request, self.server.base_url.clone(), token, id, payload);
                }
            }
            RosteringAction::DeleteShift(id) => {
                let request = self.next_rostering_request();
                self.rostering_requests
                    .insert(request, RosterRequest::DeleteShift);
                if let Some(remote) = &self.remote {
                    remote.delete_shift(request, self.server.base_url.clone(), token, id);
                }
            }
            RosteringAction::UpdateShift { id, payload } => {
                if let Some(editor) = &mut self.rostering.editor {
                    editor.saving = true;
                    editor.error = None;
                }
                let request = self.next_rostering_request();
                self.rostering_requests
                    .insert(request, RosterRequest::UpdateShift);
                if let Some(remote) = &self.remote {
                    remote.update_shift(request, self.server.base_url.clone(), token, id, payload);
                }
            }
            RosteringAction::CreateSite(payload) => {
                if let Some(form) = &mut self.rostering.site_form {
                    form.saving = true;
                    form.error = None;
                }
                let request = self.next_rostering_request();
                self.rostering_requests
                    .insert(request, RosterRequest::CreateSite);
                if let Some(remote) = &self.remote {
                    remote.create_site(request, self.server.base_url.clone(), token, payload);
                }
            }
            RosteringAction::CreateAssignment {
                shift_id,
                contact_id,
                role,
            } => {
                self.rostering.assignment_pending = true;
                self.rostering.assignment_error = None;
                let request = self.next_rostering_request();
                self.rostering_requests
                    .insert(request, RosterRequest::CreateAssignment);
                if let Some(remote) = &self.remote {
                    remote.create_assignment(
                        request,
                        self.server.base_url.clone(),
                        token,
                        shift_id,
                        crate::rostering::ApiAssignmentWrite {
                            contact_id,
                            role,
                            session_id: None,
                            status: None,
                            notes: None,
                        },
                    );
                }
            }
            RosteringAction::UpdateAssignment { id, payload } => {
                let request = self.next_rostering_request();
                self.rostering_requests
                    .insert(request, RosterRequest::UpdateAssignment);
                if let Some(remote) = &self.remote {
                    remote.update_assignment(
                        request,
                        self.server.base_url.clone(),
                        token,
                        id,
                        payload,
                    );
                }
            }
            RosteringAction::DeleteAssignment(id) => {
                let request = self.next_rostering_request();
                self.rostering_requests
                    .insert(request, RosterRequest::DeleteAssignment);
                if let Some(remote) = &self.remote {
                    remote.delete_assignment(request, self.server.base_url.clone(), token, id);
                }
            }
            RosteringAction::FindWorkers(id) => {
                self.rostering.loading_suitability = true;
                self.rostering.suitability_error = None;
                self.load_roster_suitability(id);
            }
        }
        let _ = ctx;
    }

    fn handle_editor_action(&mut self, ctx: &egui::Context, id: &str, action: ContactEditorAction) {
        match action {
            ContactEditorAction::Save => self.save_contact_window(id),
            ContactEditorAction::Cancel => self.close_contact_window(id),
            ContactEditorAction::PopOut => {
                if let Some(window) = self.contact_windows.get_mut(id) {
                    window.popped_out = true;
                    window.close_requested = false;
                }
                if self.active_contact.as_deref() == Some(id) {
                    self.active_contact = self
                        .contact_order
                        .iter()
                        .find(|open| {
                            self.contact_windows
                                .get(*open)
                                .is_some_and(|window| !window.popped_out)
                        })
                        .cloned();
                }
            }
            ContactEditorAction::Dock => {
                if let Some(window) = self.contact_windows.get_mut(id) {
                    window.popped_out = false;
                    window.close_requested = false;
                }
                self.active_contact = Some(id.to_owned());
                ctx.send_viewport_cmd_to(contact_viewport_id(id), egui::ViewportCommand::Close);
            }
            ContactEditorAction::OpenContact(contact_id) => {
                self.open_contact(&contact_id);
            }
            ContactEditorAction::CreateRelationship {
                from_role,
                to_kind,
                to_id,
                to_role,
                notes,
            } => {
                self.create_relationship(ctx, id, from_role, to_kind, to_id, to_role, notes);
            }
            ContactEditorAction::DeleteRelationship(edge_id) => {
                self.delete_relationship(id, edge_id);
            }
            ContactEditorAction::CreateContact {
                from_role,
                to_role,
                notes,
                draft,
            } => {
                self.create_relationship_contact(ctx, id, from_role, to_role, notes, draft);
            }
        }
    }

    fn handle_relationships_action(&mut self, ctx: &egui::Context, action: RelationshipsAction) {
        match action {
            RelationshipsAction::Create {
                from_kind,
                from_id,
                from_role,
                to_kind,
                to_id,
                to_role,
                notes,
            } => {
                self.create_relationship_request(
                    from_kind, from_id, from_role, to_kind, to_id, to_role, notes, None, ctx,
                );
            }
            RelationshipsAction::Delete(id) => {
                self.delete_relationship_request(id, None);
            }
            RelationshipsAction::OpenContact(id) => self.open_contact(&id),
            RelationshipsAction::Refresh => {
                self.party_catalog.clear();
                self.relationship_edges.clear();
                self.relationship_graph_loaded = false;
                self.party_catalog_request = None;
                self.relationship_form.error = None;
                self.ensure_party_catalog();
                self.notify(ctx, "Refreshing the relationship network…");
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn create_relationship(
        &mut self,
        ctx: &egui::Context,
        window_id: &str,
        from_role: String,
        to_kind: PartyKind,
        to_id: String,
        to_role: String,
        notes: String,
    ) {
        let Some(contact_id) = self
            .contact_windows
            .get(window_id)
            .map(|window| window.contact_id.clone())
        else {
            return;
        };
        self.create_relationship_request(
            PartyKind::Contact,
            contact_id,
            from_role,
            to_kind,
            to_id,
            to_role,
            notes,
            Some(window_id.to_owned()),
            ctx,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn create_relationship_request(
        &mut self,
        from_kind: PartyKind,
        from_id: String,
        from_role: String,
        to_kind: PartyKind,
        to_id: String,
        to_role: String,
        notes: String,
        window_id: Option<String>,
        ctx: &egui::Context,
    ) {
        let Some(token) = self.access_token() else {
            if let Some(window_id) = &window_id {
                if let Some(window) = self.contact_windows.get_mut(window_id) {
                    window.relationship_failed("Sign in to link relationships.".to_owned());
                }
            } else {
                self.relationship_form.error = Some("Sign in to link relationships.".to_owned());
                self.relationship_form.pending = false;
            }
            return;
        };

        let notes = notes.trim().to_owned();
        let payload = ApiRelationshipWrite {
            from_kind,
            from_id,
            from_role,
            to_kind,
            to_id,
            to_role,
            start_date: None,
            end_date: None,
            notes: (!notes.is_empty()).then_some(notes),
        };

        let request = self.next_relationship_request();
        self.relationship_create_requests
            .insert(request, window_id.clone());
        match &self.remote {
            Some(remote) => {
                remote.create_relationship(request, self.server.base_url.clone(), token, payload)
            }
            None => {
                self.relationship_create_requests.remove(&request);
                if let Some(window_id) = &window_id {
                    if let Some(window) = self.contact_windows.get_mut(window_id) {
                        window.relationship_failed(
                            "The server connection is not available.".to_owned(),
                        );
                    }
                } else {
                    self.relationship_form.error =
                        Some("The server connection is not available.".to_owned());
                    self.relationship_form.pending = false;
                }
            }
        }
        let _ = ctx;
    }

    fn delete_relationship(&mut self, window_id: &str, edge_id: String) {
        self.delete_relationship_request(edge_id, Some(window_id.to_owned()));
    }

    fn delete_relationship_request(&mut self, edge_id: String, window_id: Option<String>) {
        let Some(token) = self.access_token() else {
            if let Some(window_id) = &window_id {
                if let Some(window) = self.contact_windows.get_mut(window_id) {
                    window.relationship_failed("Sign in to edit relationships.".to_owned());
                }
            } else {
                self.relationship_form.error = Some("Sign in to edit relationships.".to_owned());
                self.relationship_form.pending = false;
            }
            return;
        };
        let request = self.next_relationship_request();
        self.relationship_delete_requests
            .insert(request, window_id.clone());
        match &self.remote {
            Some(remote) => {
                remote.delete_relationship(request, self.server.base_url.clone(), token, edge_id)
            }
            None => {
                self.relationship_delete_requests.remove(&request);
                if let Some(window_id) = &window_id {
                    if let Some(window) = self.contact_windows.get_mut(window_id) {
                        window.relationship_failed(
                            "The server connection is not available.".to_owned(),
                        );
                    }
                } else {
                    self.relationship_form.error =
                        Some("The server connection is not available.".to_owned());
                    self.relationship_form.pending = false;
                }
            }
        }
    }

    fn create_relationship_contact(
        &mut self,
        ctx: &egui::Context,
        window_id: &str,
        from_role: String,
        to_role: String,
        notes: String,
        draft: NewRelationshipContact,
    ) {
        let Some(token) = self.access_token() else {
            if let Some(window) = self.contact_windows.get_mut(window_id) {
                window.relationship_failed("Sign in to create a contact record.".to_owned());
            }
            return;
        };

        let payload = ApiContactWrite {
            display_name: draft.display_name.trim().to_owned(),
            given_name: optional_str(&draft.given_name),
            family_name: optional_str(&draft.family_name),
            preferred_name: None,
            pronouns: None,
            date_of_birth: None,
            email: optional_str(&draft.email),
            phone: optional_str(&draft.phone),
            notes: None,
            details: Value::Object(Default::default()),
            is_active: true,
            custom_fields: BTreeMap::new(),
        };

        let request = self.next_relationship_request();
        self.relationship_contact_creates.insert(
            request,
            PendingInlineRelationship {
                window_id: window_id.to_owned(),
                from_role,
                to_role,
                notes,
            },
        );

        match &self.remote {
            Some(remote) => {
                remote.create_contact(request, self.server.base_url.clone(), token, payload)
            }
            None => {
                self.relationship_contact_creates.remove(&request);
                if let Some(window) = self.contact_windows.get_mut(window_id) {
                    window
                        .relationship_failed("The server connection is not available.".to_owned());
                }
            }
        }
        let _ = ctx;
    }

    /// Insert a newly created contact into the local list snapshots so it can be
    /// selected from a relationship lookup immediately.
    fn lookup_parties(&self) -> Vec<ApiGraphNode> {
        let mut nodes = self.party_catalog.clone();
        let loaded = self.contacts.rows.load_full();
        for row in loaded.iter().chain(self.known_contacts.values()) {
            if !nodes
                .iter()
                .any(|node| node.kind == PartyKind::Contact && node.id == row.id)
            {
                nodes.push(ApiGraphNode {
                    kind: PartyKind::Contact,
                    id: row.id.clone(),
                    label: row.name.clone(),
                    degree: 0,
                    depth: None,
                });
            }
        }
        nodes
    }

    fn remember_contact(&mut self, detail: &ApiContactDetail) {
        let row = row_from_api(&detail.contact);
        let kind = row.kind;
        self.known_contacts.insert(row.id.clone(), row.clone());
        if !self
            .party_catalog
            .iter()
            .any(|node| node.kind == PartyKind::Contact && node.id == row.id)
        {
            self.party_catalog.push(ApiGraphNode {
                kind: PartyKind::Contact,
                id: row.id.clone(),
                label: row.name.clone(),
                degree: 0,
                depth: None,
            });
        }

        let state = &self.contacts;
        let matches = match state.filter {
            ContactFilter::All => true,
            ContactFilter::Clients => matches!(kind, ContactKind::Client | ContactKind::Dual),
            ContactFilter::Workers => matches!(kind, ContactKind::Worker | ContactKind::Dual),
            ContactFilter::Other => kind == ContactKind::Other,
        };

        if matches {
            let mut rows = (*state.rows.load_full()).clone();
            if !rows.iter().any(|existing| existing.id == row.id) {
                rows.insert(0, row.clone());
            }
            state.rows.store(Arc::new(rows));
        }
    }

    fn render_intake_window(&mut self, ctx: &egui::Context) {
        let visible = self.intake.view == IntakeView::Wizard && self.intake.popped_out;

        if visible {
            let title = self
                .intake
                .draft
                .as_ref()
                .map(|draft| format!("Intake — {}", draft.display_name()))
                .unwrap_or_else(|| "Intake — CareLibre".to_owned());
            let builder = egui::ViewportBuilder::default()
                .with_title(title)
                .with_app_id("carelibre-intake")
                .with_decorations(!self.config.client_side_decorations)
                .with_inner_size([1000.0, 820.0])
                .with_min_inner_size([720.0, 560.0]);

            let client_side = self.config.client_side_decorations;
            let action = ctx.show_viewport_immediate(intake_viewport_id(), builder, |ui, _| {
                pages::intake::show_wizard_viewport(ui, &mut self.intake, client_side)
            });

            if let Some(action) = action {
                self.handle_intake_action(ctx, action);
            }

            if !self.intake.popped_out || self.intake.view != IntakeView::Wizard {
                ctx.send_viewport_cmd_to(intake_viewport_id(), egui::ViewportCommand::Close);
            }
        } else if self.intake.popped_out {
            self.intake.popped_out = false;
            ctx.send_viewport_cmd_to(intake_viewport_id(), egui::ViewportCommand::Close);
        }
    }

    /// True while the intake wizard is detached into a native window.
    pub fn intake_popped_out(&self) -> bool {
        self.intake.popped_out
    }

    /// Open the first loaded contact. Used by screenshot previews.
    pub fn open_first_contact(&mut self) -> bool {
        let rows = self.contacts.rows.load_full();
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

    /// True once the relationship graph (nodes and edges) has loaded.
    pub fn relationship_graph_ready(&self) -> bool {
        self.relationship_graph_loaded
    }

    /// Number of roster shifts loaded in the rostering workspace.
    pub fn roster_shift_count(&self) -> usize {
        self.rostering.shifts.len()
    }

    /// True once a selected shift's detail has loaded.
    pub fn roster_detail_ready(&self) -> bool {
        self.rostering.detail.is_some()
    }

    /// True once suitability has been scored for the selected shift.
    pub fn roster_suitability_ready(&self) -> bool {
        self.rostering.suitability.is_some()
    }

    /// Score workers for the selected shift. Used by screenshot previews.
    pub fn find_workers_for_selected_shift(&mut self) -> bool {
        let Some(id) = self.rostering.selected.clone() else {
            return false;
        };
        self.load_roster_suitability(id);
        true
    }

    /// Open the first loaded shift. Used by screenshot previews.
    pub fn open_first_shift(&mut self) -> bool {
        let Some(shift) = self.rostering.shifts.first() else {
            return false;
        };
        let id = shift.id.clone();
        self.rostering.selected = Some(id.clone());
        self.load_roster_detail(id);
        true
    }

    /// Number of server relationships loaded across open record editors.
    pub fn relationship_count(&self) -> usize {
        self.contact_windows
            .values()
            .map(|window| window.relationships.len())
            .max()
            .unwrap_or(0)
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

    /// True while at least one contact editor is detached into a native window.
    pub fn contact_editor_popped_out(&self) -> bool {
        self.contact_windows
            .values()
            .any(|window| window.popped_out)
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
                    let state = &mut self.contacts;
                    if request_id != state.generation || filter != state.filter {
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
                    let current = self
                        .contact_windows
                        .get(&id)
                        .is_some_and(|window| window.request_id == Some(request_id));
                    if !current {
                        continue;
                    }
                    match result {
                        Ok(detail) => {
                            if let Some(window) = self.contact_windows.get_mut(&id) {
                                window.apply(detail);
                            }
                            self.refresh_contact_lists();
                            self.send_contacts_query(false);
                            self.notify(ctx, "Contact saved to the server.");
                        }
                        Err(error) => {
                            if let Some(window) = self.contact_windows.get_mut(&id) {
                                window.saving = false;
                                window.error = Some(error);
                            }
                        }
                    }
                }
                RemoteEvent::ContactCreated { request_id, result } => {
                    self.handle_contact_created(ctx, request_id, result);
                }
                RemoteEvent::RelationshipsListed {
                    request_id,
                    contact_id,
                    organisation_id,
                    result,
                } => {
                    self.handle_relationships_listed(
                        request_id,
                        contact_id,
                        organisation_id,
                        result,
                    );
                }
                RemoteEvent::RelationshipGraphLoaded { request_id, result } => {
                    self.handle_relationship_graph(ctx, request_id, result);
                }
                RemoteEvent::RelationshipCreated { request_id, result } => {
                    self.handle_relationship_created(ctx, request_id, result);
                }
                RemoteEvent::RelationshipDeleted {
                    request_id,
                    id,
                    result,
                } => {
                    self.handle_relationship_deleted(ctx, request_id, id, result);
                }
                RemoteEvent::SitesListed { request_id, result } => {
                    self.handle_sites_listed(request_id, result);
                }
                RemoteEvent::SiteCreated { request_id, result } => {
                    self.handle_site_created(ctx, request_id, result);
                }
                RemoteEvent::ShiftsListed { request_id, result } => {
                    self.handle_shifts_listed(request_id, result);
                }
                RemoteEvent::ShiftLoaded {
                    request_id,
                    id,
                    result,
                } => {
                    self.handle_shift_loaded(request_id, id, result);
                }
                RemoteEvent::RosterFiltersLoaded { request_id, result } => {
                    self.handle_roster_filters(request_id, result);
                }
                RemoteEvent::ShiftsCreated { request_id, result } => {
                    self.handle_shifts_created(ctx, request_id, result);
                }
                RemoteEvent::ShiftCopied { request_id, result } => {
                    self.handle_shift_copied(ctx, request_id, result);
                }
                RemoteEvent::ShiftDeleted {
                    request_id,
                    id,
                    result,
                } => {
                    self.handle_shift_deleted(ctx, request_id, id, result);
                }
                RemoteEvent::ShiftCreated { request_id, result } => {
                    self.handle_shift_created(ctx, request_id, result);
                }
                RemoteEvent::ShiftUpdated {
                    request_id,
                    id,
                    result,
                } => {
                    self.handle_shift_updated(ctx, request_id, id, result);
                }
                RemoteEvent::AssignmentCreated {
                    request_id,
                    shift_id,
                    result,
                } => {
                    self.handle_assignment_created(ctx, request_id, shift_id, result);
                }
                RemoteEvent::AssignmentUpdated {
                    request_id,
                    id,
                    result,
                } => {
                    self.handle_assignment_updated(request_id, id, result);
                }
                RemoteEvent::AssignmentDeleted {
                    request_id,
                    id,
                    result,
                } => {
                    self.handle_assignment_deleted(request_id, id, result);
                }
                RemoteEvent::SuitabilityLoaded {
                    request_id,
                    shift_id,
                    result,
                } => {
                    self.handle_suitability_loaded(request_id, shift_id, result);
                }
            }
        }
    }

    fn handle_sites_listed(
        &mut self,
        request_id: u64,
        result: Result<Vec<crate::rostering::ApiSite>, String>,
    ) {
        self.rostering_requests.remove(&request_id);
        match result {
            Ok(sites) => self.rostering.sites = sites,
            Err(error) => self.rostering.error = Some(error),
        }
    }

    fn handle_site_created(
        &mut self,
        ctx: &egui::Context,
        request_id: u64,
        result: Result<crate::rostering::ApiSite, String>,
    ) {
        self.rostering_requests.remove(&request_id);
        match result {
            Ok(site) => {
                self.rostering.sites.push(site.clone());
                self.rostering.sites.sort_by(|a, b| a.name.cmp(&b.name));
                self.rostering.site_form = None;
                self.notify(ctx, format!("Site “{}” added.", site.name));
            }
            Err(error) => {
                if let Some(form) = &mut self.rostering.site_form {
                    form.saving = false;
                    form.error = Some(error);
                }
            }
        }
    }

    fn handle_shifts_listed(
        &mut self,
        request_id: u64,
        result: Result<crate::rostering::ApiShiftList, String>,
    ) {
        self.rostering_requests.remove(&request_id);
        self.rostering.loading_shifts = false;
        self.rostering.requested = true;
        match result {
            Ok(list) => {
                self.rostering.shifts = list.items;
                self.rostering.error = None;
            }
            Err(error) => self.rostering.error = Some(error),
        }
    }

    fn handle_shift_loaded(
        &mut self,
        request_id: u64,
        _id: String,
        result: Result<crate::rostering::ApiShiftDetail, String>,
    ) {
        self.rostering_requests.remove(&request_id);
        self.rostering.loading_detail = false;
        match result {
            Ok(detail) => {
                self.rostering.selected = Some(detail.shift.id.clone());
                self.rostering.detail = Some(detail);
                self.rostering.error = None;
            }
            Err(error) => self.rostering.error = Some(error),
        }
    }

    fn handle_roster_filters(
        &mut self,
        request_id: u64,
        result: crate::remote::RosterFiltersResult,
    ) {
        self.rostering_requests.remove(&request_id);
        match result {
            Ok((divisions, services, cost_centres)) => {
                self.rostering.divisions = divisions;
                self.rostering.services = services;
                self.rostering.cost_centres = cost_centres;
            }
            Err(error) => self.rostering.error = Some(error),
        }
    }

    fn handle_shifts_created(
        &mut self,
        ctx: &egui::Context,
        request_id: u64,
        result: Result<crate::rostering::ApiShiftList, String>,
    ) {
        self.rostering_requests.remove(&request_id);
        match result {
            Ok(list) => {
                self.rostering.editor = None;
                self.rostering.suitability = None;
                self.rostering.suitability_error = None;
                let count = list.items.len();
                if let Some(first) = list.items.first() {
                    self.rostering.selected = Some(first.id.clone());
                }
                self.notify(
                    ctx,
                    if count == 1 {
                        "Shift created.".to_owned()
                    } else {
                        format!("{count} shifts created.")
                    },
                );
                self.load_roster_shifts();
                if let Some(first) = list.items.first() {
                    self.load_roster_detail(first.id.clone());
                }
            }
            Err(error) => {
                if let Some(editor) = &mut self.rostering.editor {
                    editor.saving = false;
                    editor.error = Some(error);
                } else {
                    self.rostering.error = Some(error);
                }
            }
        }
    }

    fn handle_shift_copied(
        &mut self,
        ctx: &egui::Context,
        request_id: u64,
        result: Result<crate::rostering::ApiShiftDetail, String>,
    ) {
        self.rostering_requests.remove(&request_id);
        match result {
            Ok(detail) => {
                let id = detail.shift.id.clone();
                self.rostering.paste = None;
                self.rostering.suitability = None;
                self.rostering.suitability_error = None;
                self.rostering.selected = Some(id);
                self.rostering.detail = Some(detail);
                self.notify(ctx, "Shift pasted.");
                self.load_roster_shifts();
            }
            Err(error) => {
                if let Some(paste) = &mut self.rostering.paste {
                    paste.saving = false;
                    paste.error = Some(error);
                } else {
                    self.rostering.error = Some(error);
                }
            }
        }
    }

    fn handle_shift_deleted(
        &mut self,
        ctx: &egui::Context,
        request_id: u64,
        id: String,
        result: Result<(), String>,
    ) {
        self.rostering_requests.remove(&request_id);
        match result {
            Ok(()) => {
                if self.rostering.selected.as_deref() == Some(id.as_str()) {
                    self.rostering.selected = None;
                    self.rostering.detail = None;
                    self.rostering.suitability = None;
                }
                self.notify(ctx, "Shift deleted.");
                self.load_roster_shifts();
            }
            Err(error) => self.rostering.error = Some(error),
        }
    }

    fn handle_shift_created(
        &mut self,
        ctx: &egui::Context,
        request_id: u64,
        result: Result<crate::rostering::ApiShift, String>,
    ) {
        self.rostering_requests.remove(&request_id);
        match result {
            Ok(shift) => {
                self.rostering.editor = None;
                self.notify(ctx, "Shift created.");
                self.load_roster_shifts();
                self.load_roster_detail(shift.id);
            }
            Err(error) => {
                if let Some(editor) = &mut self.rostering.editor {
                    editor.saving = false;
                    editor.error = Some(error);
                }
            }
        }
    }

    fn handle_shift_updated(
        &mut self,
        ctx: &egui::Context,
        request_id: u64,
        id: String,
        result: Result<crate::rostering::ApiShift, String>,
    ) {
        self.rostering_requests.remove(&request_id);
        match result {
            Ok(shift) => {
                if let Some(existing) = self
                    .rostering
                    .shifts
                    .iter_mut()
                    .find(|existing| existing.id == id)
                {
                    *existing = shift.clone();
                }
                if let Some(detail) = &mut self.rostering.detail
                    && detail.shift.id == id
                {
                    detail.shift = shift;
                }
                self.rostering.editor = None;
                self.notify(ctx, "Shift updated.");
            }
            Err(error) => {
                if let Some(editor) = &mut self.rostering.editor {
                    editor.saving = false;
                    editor.error = Some(error);
                }
            }
        }
    }

    fn handle_assignment_created(
        &mut self,
        ctx: &egui::Context,
        request_id: u64,
        _shift_id: String,
        result: Result<crate::rostering::ApiAssignment, String>,
    ) {
        self.rostering_requests.remove(&request_id);
        self.rostering.assignment_pending = false;
        match result {
            Ok(_) => {
                self.rostering.assignment_target = None;
                self.rostering.assignment_error = None;
                self.notify(ctx, "Assignment added.");
                self.refresh_roster_after_assignment();
            }
            Err(error) => self.rostering.assignment_error = Some(error),
        }
    }

    fn handle_assignment_updated(
        &mut self,
        request_id: u64,
        _id: String,
        result: Result<crate::rostering::ApiAssignment, String>,
    ) {
        self.rostering_requests.remove(&request_id);
        match result {
            Ok(_) => self.refresh_roster_after_assignment(),
            Err(error) => self.rostering.error = Some(error),
        }
    }

    fn handle_assignment_deleted(
        &mut self,
        request_id: u64,
        _id: String,
        result: Result<(), String>,
    ) {
        self.rostering_requests.remove(&request_id);
        match result {
            Ok(()) => self.refresh_roster_after_assignment(),
            Err(error) => self.rostering.error = Some(error),
        }
    }

    fn handle_suitability_loaded(
        &mut self,
        request_id: u64,
        _shift_id: String,
        result: Result<crate::rostering::ApiSuitability, String>,
    ) {
        self.rostering_requests.remove(&request_id);
        self.rostering.loading_suitability = false;
        match result {
            Ok(suitability) => {
                self.rostering.suitability = Some(suitability);
                self.rostering.suitability_error = None;
            }
            Err(error) => self.rostering.suitability_error = Some(error),
        }
    }

    fn handle_relationships_listed(
        &mut self,
        request_id: u64,
        contact_id: Option<String>,
        organisation_id: Option<String>,
        result: Result<crate::contact::ApiRelationshipList, String>,
    ) {
        self.relationship_list_requests.remove(&request_id);
        let _ = organisation_id;
        let Some(contact_id) = contact_id else {
            return;
        };
        let Some(window) = self.contact_windows.get_mut(&contact_id) else {
            return;
        };
        if window.relationship_request_id != Some(request_id) {
            return;
        }
        match result {
            Ok(list) => window.apply_relationships(list.items),
            Err(error) => window.relationships_failed(error),
        }
    }

    fn handle_relationship_graph(
        &mut self,
        ctx: &egui::Context,
        request_id: u64,
        result: Result<crate::contact::ApiRelationshipGraph, String>,
    ) {
        if self.party_catalog_request != Some(request_id) {
            return;
        }
        self.party_catalog_request = None;
        match result {
            Ok(graph) => {
                self.party_catalog = graph.nodes;
                self.relationship_edges = graph.edges;
                self.relationship_graph_loaded = true;
                self.relationship_form.error = None;
                self.relationship_form.pending = false;
            }
            Err(error) => {
                self.relationship_form.error = Some(error.clone());
                self.relationship_form.pending = false;
                self.notify(
                    ctx,
                    format!("Could not load the relationship directory: {error}"),
                );
            }
        }
    }

    /// Make a new intake contact a client or worker of the signed-in
    /// organisation by linking them to it.
    fn link_intake_to_organisation(&mut self, kind: IntakeKind, contact_id: &str, name: &str) {
        let (from_role, to_role) = match kind {
            IntakeKind::Client => pages::contact_editor::CLIENT_ROLES,
            IntakeKind::Worker => pages::contact_editor::WORKER_ROLES,
            IntakeKind::Other => return,
        };
        let Some(session) = &self.server.session else {
            return;
        };
        let organisation_id = session.auth.organisation.id.clone();
        let token = session.auth.access_token.clone();
        if self.remote.is_none() {
            return;
        }
        let request_id = self.next_relationship_request();
        self.intake_role_links.insert(request_id, name.to_owned());
        let Some(remote) = &self.remote else {
            return;
        };
        remote.create_relationship(
            request_id,
            self.server.base_url.clone(),
            token,
            ApiRelationshipWrite {
                from_kind: PartyKind::Contact,
                from_id: contact_id.to_owned(),
                from_role: from_role.to_owned(),
                to_kind: PartyKind::Organisation,
                to_id: organisation_id,
                to_role: to_role.to_owned(),
                start_date: None,
                end_date: None,
                notes: None,
            },
        );
    }

    fn handle_relationship_created(
        &mut self,
        ctx: &egui::Context,
        request_id: u64,
        result: Result<ApiRelationship, String>,
    ) {
        if let Some(name) = self.intake_role_links.remove(&request_id) {
            match result {
                Ok(edge) => {
                    self.relationship_edges.push(edge);
                    self.refresh_contact_lists();
                    self.send_contacts_query(false);
                }
                Err(error) => self.notify(
                    ctx,
                    format!(
                        "{name} was saved, but linking them to the organisation failed: {error}"
                    ),
                ),
            }
            return;
        }
        let Some(window_id) = self.relationship_create_requests.remove(&request_id) else {
            return;
        };
        match result {
            Ok(edge) => {
                if !self
                    .relationship_edges
                    .iter()
                    .any(|existing| existing.id == edge.id)
                {
                    self.relationship_edges.push(edge.clone());
                }
                let mut targets: Vec<String> = window_id.iter().cloned().collect();
                for (id, window) in &self.contact_windows {
                    let touches = (edge.from_kind == PartyKind::Contact
                        && edge.from_id == window.contact_id)
                        || (edge.to_kind == PartyKind::Contact && edge.to_id == window.contact_id);
                    if touches && !targets.contains(id) {
                        targets.push(id.clone());
                    }
                }
                for id in targets {
                    if let Some(window) = self.contact_windows.get_mut(&id) {
                        window.relationship_linked(edge.clone());
                    }
                }
                self.relationship_form.pending = false;
                self.relationship_form.error = None;
                self.relationship_form.notes.clear();
                self.notify(ctx, "Relationship linked.");
            }
            Err(error) => {
                if let Some(window_id) = &window_id {
                    if let Some(window) = self.contact_windows.get_mut(window_id) {
                        window.relationship_failed(error);
                    }
                } else {
                    self.relationship_form.error = Some(error);
                    self.relationship_form.pending = false;
                }
            }
        }
    }

    fn handle_relationship_deleted(
        &mut self,
        ctx: &egui::Context,
        request_id: u64,
        id: String,
        result: Result<(), String>,
    ) {
        let Some(window_id) = self.relationship_delete_requests.remove(&request_id) else {
            return;
        };
        match result {
            Ok(()) => {
                self.relationship_edges.retain(|edge| edge.id != id);
                for window in self.contact_windows.values_mut() {
                    window.relationship_removed(&id);
                }
                self.relationship_form.pending = false;
                self.relationship_form.error = None;
                self.notify(ctx, "Relationship removed.");
            }
            Err(error) => {
                if let Some(window_id) = &window_id {
                    if let Some(window) = self.contact_windows.get_mut(window_id) {
                        window.relationship_failed(error);
                    }
                } else {
                    self.relationship_form.error = Some(error);
                    self.relationship_form.pending = false;
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
        if let Some(pending) = self.relationship_contact_creates.remove(&request_id) {
            match result {
                Ok(detail) => {
                    let contact_id = detail.contact.id.clone();
                    let name = detail.contact.display_name.clone();
                    self.remember_contact(&detail);
                    self.create_relationship(
                        ctx,
                        &pending.window_id,
                        pending.from_role,
                        PartyKind::Contact,
                        contact_id,
                        pending.to_role,
                        pending.notes,
                    );
                    self.notify(ctx, format!("{name} created; linking it now…"));
                }
                Err(error) => {
                    if let Some(window) = self.contact_windows.get_mut(&pending.window_id) {
                        window.relationship_failed(error);
                    }
                }
            }
            return;
        }

        if self.intake.create_request != Some(request_id) {
            return;
        }
        self.intake.create_request = None;
        self.intake.saving = false;
        match result {
            Ok(detail) => {
                self.remember_contact(&detail);
                let name = detail.contact.display_name.clone();
                let contact_id = detail.contact.id.clone();
                let kind = self
                    .intake
                    .draft
                    .as_ref()
                    .map_or(IntakeKind::Other, |draft| draft.kind);
                self.link_intake_to_organisation(kind, &contact_id, &name);
                self.intake.completed = Some(crate::intake::CompletedIntake {
                    contact_id,
                    name: name.clone(),
                    kind,
                });
                self.intake.view = IntakeView::Completed;
                self.intake.dirty = false;
                self.intake.error = None;
                if self.intake.popped_out
                    && let Some(ctx) = &self.app_ctx
                {
                    ctx.send_viewport_cmd_to(intake_viewport_id(), egui::ViewportCommand::Close);
                }
                self.intake.popped_out = false;
                self.refresh_contact_lists();
                self.send_contacts_query(false);

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
                        self.restoring_session = false;
                        self.settings.on_signed_in();
                        if self.config.remember_login {
                            if let Err(error) = self.remember_current_session() {
                                self.settings.error =
                                    Some(format!("Could not save the login securely: {error}"));
                            }
                        } else if self.config.remembered_email.is_some() {
                            self.forget_saved_login();
                        }
                        self.refresh_contact_lists();
                        self.send_contacts_query(false);
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
                let was_restoring = self.restoring_session;
                self.restoring_session = false;
                match result {
                    Ok(auth) => {
                        let organisation = auth.organisation.name.clone();
                        self.server.session = Some(crate::server::Session {
                            expires_at: Instant::now()
                                + Duration::from_secs(auth.expires_in.max(60)),
                            auth,
                        });
                        if self.config.remember_login
                            && let Err(error) = self.remember_current_session()
                        {
                            self.settings.error =
                                Some(format!("Could not refresh the saved login: {error}"));
                        }
                        if was_restoring {
                            self.settings.on_signed_in();
                            self.refresh_contact_lists();
                            self.send_contacts_query(false);
                            self.notify(ctx, format!("Welcome back to {organisation}."));
                        }
                    }
                    Err(_) => {
                        self.server.session = None;
                        self.forget_saved_login();
                        self.close_all_contact_windows();
                        self.refresh_contact_lists();
                        self.cache_counts = ContactCounts::default();
                        self.settings.error = Some(if was_restoring {
                            "Saved login expired. Please sign in again.".to_owned()
                        } else {
                            "Your session expired. Please sign in again.".to_owned()
                        });
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
            Action::SetTheme(mode) => {
                self.config.theme = mode;
                self.settings.theme = mode;
                match self.config.save() {
                    Ok(()) => {
                        theme::sync(ctx, mode);
                        self.settings.error = None;
                        self.settings.notice = Some(format!("Appearance set to {}.", mode.label()));
                        ctx.request_repaint();
                    }
                    Err(error) => self.settings.error = Some(error),
                }
            }
            Action::SetClientSideDecorations(enabled) => {
                self.config.client_side_decorations = enabled;
                match self.config.save() {
                    Ok(()) => {
                        self.apply_window_decorations(ctx, enabled);
                        self.settings.error = None;
                        self.settings.notice = Some(if enabled {
                            "Client-side window decorations enabled.".to_owned()
                        } else {
                            "Native window decorations enabled.".to_owned()
                        });
                    }
                    Err(error) => self.settings.error = Some(error),
                }
            }
            Action::SetRememberLogin(enabled) => {
                self.config.remember_login = enabled;
                if enabled {
                    if let Err(error) = self.remember_current_session() {
                        self.settings.error =
                            Some(format!("Could not save the login securely: {error}"));
                    } else {
                        self.settings.error = None;
                        self.settings.notice =
                            Some("Login will be remembered on this device.".to_owned());
                    }
                } else {
                    self.forget_saved_login();
                    self.settings.error = None;
                    self.settings.notice = Some("Saved login removed.".to_owned());
                }
                if let Err(error) = self.config.save() {
                    self.settings.error = Some(error);
                }
            }
            Action::Logout => {
                if let Some(session) = self.server.session.take() {
                    if let Some(runtime) = &self.runtime {
                        runtime.logout(session.auth.refresh_token.clone());
                    }
                    self.server.refresh_in_flight = false;
                    self.forget_saved_login();
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
        if server_changed {
            self.forget_saved_login();
        }
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
                    .fill(theme::bg())
                    .inner_margin(Margin {
                        left: 20,
                        right: 20,
                        top: 0,
                        bottom: 0,
                    })
                    .stroke(Stroke::new(1.0, theme::border())),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Brand
                    let (logo, _) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
                    ui.painter()
                        .rect_filled(logo, CornerRadius::same(9), theme::accent());
                    ui.painter().text(
                        logo.center(),
                        Align2::CENTER_CENTER,
                        "C",
                        FontId::proportional(19.0),
                        theme::on_accent(),
                    );
                    ui.add_space(2.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("CareLibre").font(theme::strong_font(16.0)));
                        ui.label(
                            RichText::new("CARE PLATFORM")
                                .color(theme::faint())
                                .size(8.5),
                        );
                    });

                    ui.add_space(14.0);
                    let (separator, _) = ui.allocate_exact_size(vec2(1.0, 26.0), Sense::hover());
                    ui.painter()
                        .rect_filled(separator, CornerRadius::ZERO, theme::border());
                    ui.add_space(14.0);

                    ui.label(
                        RichText::new(format!("{}  {}", self.page.icon(), self.page.label()))
                            .color(theme::muted())
                            .size(13.0),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        match &account {
                            Some(account) => {
                                let response = theme::avatar(
                                    ui,
                                    &account.initials,
                                    32.0,
                                    theme::accent_blue(),
                                );
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
                                    .color(theme::on_accent())
                                    .font(theme::strong_font(13.0)),
                            )
                            .fill(theme::accent())
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
                                .hint_text("Search contacts, shifts…")
                                .margin(Margin::symmetric(10, 8)),
                        );
                        if search.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter))
                            && !self.search.trim().is_empty()
                        {
                            let query = self.search.trim().to_owned();
                            self.page = Page::Contacts;
                            self.show_contacts(ContactFilter::All, Some(query));
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
                    .fill(theme::sidebar())
                    .inner_margin(Margin::symmetric(12, 16))
                    .stroke(Stroke::new(1.0, theme::border())),
            )
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                group_label(ui, "WORKSPACE");
                for page in [
                    Page::Home,
                    Page::Contacts,
                    Page::Rostering,
                    Page::Relationships,
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
                theme::faint(),
                format!("Server URL: {}", self.server.base_url),
            ),
            ConnectionState::Connecting => (
                "Connecting to server…".to_owned(),
                theme::warning(),
                format!("Contacting {}", self.server.base_url),
            ),
            ConnectionState::Online {
                version,
                database_up: true,
                ..
            } => (
                format!("Server online · v{version}"),
                theme::success(),
                format!("{} · database up", self.server.base_url),
            ),
            ConnectionState::Online {
                database_up: false,
                ready_error,
                ..
            } => (
                "Server online · database down".to_owned(),
                theme::warning(),
                format!(
                    "{} · database down: {}",
                    self.server.base_url,
                    ready_error.as_deref().unwrap_or("readiness check failed")
                ),
            ),
            ConnectionState::Offline { error } => (
                "Server offline".to_owned(),
                theme::danger(),
                format!("{} · {error}", self.server.base_url),
            ),
        };

        egui::Panel::bottom("status_bar")
            .exact_size(30.0)
            .show_separator_line(false)
            .frame(
                Frame::new()
                    .fill(theme::sidebar())
                    .inner_margin(Margin::symmetric(20, 0))
                    .stroke(Stroke::new(1.0, theme::border())),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("CareLibre Desktop v0.1.0 · Prototype")
                            .color(theme::faint())
                            .size(10.5),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new("Community Edition")
                                .color(theme::faint())
                                .size(10.5),
                        );
                        dot(ui, theme::border());
                        status_segment(ui, "wgpu", theme::faint(), None);
                        dot(ui, theme::border());
                        status_segment(ui, &server_label, server_colour, Some(&server_tooltip));
                        dot(ui, theme::border());
                        ui.label(
                            RichText::new("Rust · native")
                                .color(theme::faint())
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
        let exit = (remaining.min(0.45) / 0.45).clamp(0.0, 1.0);
        let enter = (((now - notice.created_at) / 0.18) as f32).clamp(0.0, 1.0);
        let alpha = enter * exit;
        if enter < 1.0 {
            ctx.request_repaint();
        }
        let slide = (1.0 - theme::ease_out(enter)) * 10.0;

        egui::Area::new(egui::Id::unique("notice_toast"))
            .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -46.0 + slide))
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                Frame::new()
                    .fill(theme::surface_raised().gamma_multiply(alpha))
                    .stroke(Stroke::new(1.0, theme::accent().gamma_multiply(alpha)))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::symmetric(16, 10))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(text)
                                .color(theme::text())
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
            .color(theme::faint())
            .font(theme::strong_font(9.5)),
    );
    ui.add_space(4.0);
}

fn nav_item(ui: &mut Ui, selected: bool, page: Page) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::click());

    let hover =
        ui.ctx()
            .animate_bool_with_time(response.id.with("nav_hover"), response.hovered(), 0.14);
    if selected {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(9), theme::accent_soft(24));
        let bar = egui::Rect::from_min_size(rect.left_center() + vec2(7.0, -9.0), vec2(3.0, 18.0));
        ui.painter()
            .rect_filled(bar, CornerRadius::same(2), theme::accent());
    } else if hover > 0.0 {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(9),
            theme::lerp_color(Color32::TRANSPARENT, theme::surface_raised(), hover),
        );
    }

    let text_colour = if selected {
        theme::text()
    } else {
        theme::muted()
    };
    let icon_colour = if selected {
        theme::accent()
    } else {
        theme::faint()
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
            let response = theme::avatar(ui, &account.initials, 28.0, theme::accent_blue());
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
                    .color(theme::faint())
                    .size(10.0),
                );
            });
        }
        None => {
            theme::avatar(ui, "?", 28.0, theme::surface_raised());
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("Not signed in")
                        .color(theme::muted())
                        .font(theme::strong_font(12.0)),
                );
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Connect to server  →")
                                .color(theme::accent())
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
        .fill(theme::bg())
        .stroke(Stroke::new(1.0, theme::border()))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            // The sidebar lays its footer out bottom-up; force normal flow here.
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                    ui.painter()
                        .circle_filled(rect.center(), 4.0, theme::success());
                    ui.label(
                        RichText::new(title)
                            .color(theme::muted())
                            .font(theme::strong_font(9.5)),
                    );
                });
                ui.add_space(3.0);
                ui.label(
                    RichText::new("Self-hosted · free forever")
                        .color(theme::faint())
                        .size(10.5),
                );
                ui.add_space(8.0);

                let (colour, text) = if server_error {
                    (theme::danger(), "Server offline".to_owned())
                } else if server_ready {
                    (
                        theme::success(),
                        if server_contacts == 0 {
                            "Server connected".to_owned()
                        } else {
                            format!("Server · {} contacts", format_count(server_contacts))
                        },
                    )
                } else {
                    (theme::warning(), "Sign in to load contacts".to_owned())
                };

                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(7.0, 7.0), Sense::hover());
                    ui.painter().circle_filled(rect.center(), 3.5, colour);
                    ui.label(RichText::new(text).color(theme::muted()).size(9.5));
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

fn intake_viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("carelibre-intake")
}

fn contact_viewport_id(id: &str) -> egui::ViewportId {
    egui::ViewportId::from_hash_of(("carelibre-contact", id))
}

fn page_viewport_id(page: Page) -> egui::ViewportId {
    egui::ViewportId::from_hash_of(("carelibre-page", page.label()))
}

/// A draggable tab in the record dock. Returns the tab response and whether
/// its close button was clicked.
fn dock_tab(ui: &mut Ui, id: &str, name: &str, active: bool) -> (egui::Response, bool) {
    let font = theme::strong_font(11.5);
    let label = if name.chars().count() > 26 {
        let mut short: String = name.chars().take(25).collect();
        short.push('…');
        short
    } else {
        name.to_owned()
    };
    let text_width = ui
        .painter()
        .layout_no_wrap(label.clone(), font.clone(), theme::text())
        .size()
        .x;
    let width = (text_width + 44.0).clamp(96.0, 200.0);
    let stable_id = ui.scope_id().with(("dock_tab", id));
    let (rect, _) = ui.allocate_exact_size(vec2(width, 30.0), Sense::hover());
    let response = ui.interact(rect, stable_id, Sense::click_and_drag());

    let hover = ui
        .ctx()
        .animate_bool_with_time(stable_id.with("hover"), response.hovered(), 0.12);
    let selected = ui
        .ctx()
        .animate_bool_with_time(stable_id.with("selected"), active, 0.16);

    let base = if active {
        theme::surface_raised()
    } else {
        theme::sidebar()
    };
    let fill = theme::lerp_color(base, theme::hover(), hover * 0.8);
    ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
    if selected > 0.0 {
        let underline = egui::Rect::from_min_size(
            egui::pos2(rect.left() + 8.0, rect.bottom() - 2.0),
            vec2((rect.width() - 16.0) * selected, 2.0),
        );
        ui.painter()
            .rect_filled(underline, CornerRadius::same(1), theme::accent());
    }

    let ink = theme::lerp_color(theme::muted(), theme::text(), selected.max(hover * 0.5));
    ui.painter().text(
        egui::pos2(rect.left() + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        font,
        ink,
    );

    let close_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 13.0, rect.center().y),
        vec2(18.0, 18.0),
    );
    let close = ui.interact(close_rect, stable_id.with("close"), Sense::click());
    if close.hovered() {
        ui.painter()
            .circle_filled(close_rect.center(), 8.0, theme::hover());
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    ui.painter().text(
        close_rect.center(),
        Align2::CENTER_CENTER,
        "×",
        FontId::proportional(13.0),
        if close.hovered() {
            theme::text()
        } else {
            theme::faint()
        },
    );

    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    (response, close.clicked())
}

fn dot(ui: &mut Ui, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(6.0, 6.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(3), colour);
}
