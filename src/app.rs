//! Application shell: top bar, navigation, status bar and routing.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::{Duration, Instant},
};

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Frame, Layout, Margin, RichText, Sense,
    Stroke, StrokeKind, Ui, vec2,
};
use serde_json::{Value, json};

use crate::{
    api, chrome,
    config::{Config, ExplorerPreference, RelationshipViewPreference, relationship_view_key},
    contact::{
        ApiAgreementLineWrite, ApiAgreementWrite, ApiClientRequirementWrite, ApiContactDetail,
        ApiContactWrite, ApiFundingPackageWrite, ApiGraphNode, ApiRelationship,
        ApiRelationshipType, ApiRelationshipWrite, ApiWorkerQualityValueWrite, PartyKind,
        RelationshipChange, RoleOption, relationship_role_label, role_options, row_from_api,
    },
    credentials,
    intake::{IntakeKind, IntakeRelationship},
    organisation::ApiOrganisation,
    pages::{
        self, Action, Page,
        constellation::{ConstellationSource, RelationshipNetwork},
        contact_editor::{ContactEditorAction, ContactWindowState, NewRelationshipContact},
        contacts::{ContactsAction, ContactsState, LOAD_MORE_SIZE, PAGE_SIZE},
        intake::{IntakeAction, IntakeState, IntakeView},
        organisations::{
            OrganisationEditorAction, OrganisationEditorState, OrganisationsAction,
            OrganisationsState,
        },
        relationships::{ExplorerAction, ExplorerSource, ExplorerState},
        rostering::{RosteringAction, RosteringState},
        settings::SettingsForm,
    },
    perf::{PerfHud, PerfInfo},
    remote::{RemoteEvent, RemoteHandle},
    server::{ConnectionState, Event, ServerRuntime, ServerState},
    store::{self, ContactCounts, ContactFilter, ContactKind, ContactRow, StoreEvent, StoreHandle},
    theme,
};

const MAX_RECORD_COLUMNS: usize = 5;
const COLUMN_MIN_WIDTH: f32 = 240.0;
const COLUMN_STRIP_PADDING: f32 = 10.0;
const COLUMN_STRIP_TOP_PADDING: f32 = 10.0;
const COLUMN_STRIP_BOTTOM_PADDING: f32 = 10.0;
const SIDEBAR_WIDTH: f32 = 232.0;
const SIDEBAR_RAIL_WIDTH: f32 = 64.0;
const CENTERED_MENU_WIDTH: f32 = 440.0;
const CENTERED_MENU_HEIGHT: f32 = 710.0;
const COLUMN_ANIM_DURATION: f64 = 0.32;
const SIDEBAR_ANIM_DURATION: f64 = 0.36;
const PAGE_ANIM_DURATION: f64 = 0.24;
const TOP_BAR_ANIM_DURATION: f64 = 0.22;
const NOTICE_ENTER_DURATION: f32 = 0.24;

#[derive(Clone, Debug, PartialEq, Eq)]
enum ColumnKind {
    Page(Page),
    Contact(String),
    /// A `new` or organisation-id key for an open editor column.
    Organisation(String),
    IntakeCase(String),
}

#[derive(Clone, Debug)]
struct ColumnState {
    kind: ColumnKind,
    /// Target width after fitting the available space.
    width: f32,
    /// Relative share of the available width. New columns start at 1.0 so
    /// every column receives an equal percentage of the window.
    weight: f32,
    /// Animated width used for layout.
    display_width: f32,
    anim_from: f32,
    anim_to: f32,
    anim_start: f64,
    /// Animated horizontal position inside the column strip.
    position_x: f32,
    pos_from: f32,
    pos_to: f32,
    pos_start: f64,
    /// A closing column stays in the strip while it shrinks to zero width.
    closing: bool,
    close_start: f64,
}

impl ColumnKind {
    fn key(&self) -> String {
        match self {
            Self::Page(page) => format!("__page_{}__", page.label()),
            Self::Contact(id) => id.clone(),
            Self::Organisation(key) => format!("__organisation_{key}__"),
            Self::IntakeCase(id) => format!("__intake_case_{id}__"),
        }
    }
}

pub struct CareLibreApp {
    page: Page,
    search: String,
    notice: Option<Notice>,
    config: Config,
    settings: SettingsForm,
    server: ServerState,
    runtime: Option<ServerRuntime>,
    contacts: ContactsState,
    organisations: OrganisationsState,
    organisation_request_seq: u64,
    organisation_list_request: Option<u64>,
    /// Save request id -> (editor key, existing organisation id).
    organisation_save_requests: BTreeMap<u64, (String, Option<String>)>,
    /// Delete request id -> (organisation id, editor key when deleted from a column).
    organisation_delete_requests: BTreeMap<u64, (String, Option<String>)>,
    organisation_query_started: Option<Instant>,
    intake: IntakeState,
    intake_board: pages::intake::board::IntakeBoardState,
    intake_case_columns: BTreeMap<String, pages::intake::board::IntakeCaseColumnState>,
    discard_intake_case: Option<String>,
    discard_intake_request: Option<u64>,
    discard_intake_error: Option<String>,
    contact_windows: BTreeMap<String, ContactWindowState>,
    organisation_editors: BTreeMap<String, OrganisationEditorState>,
    columns: Vec<ColumnState>,
    columns_initialised: bool,
    column_rects: Vec<(usize, egui::Rect)>,
    last_column_order: Vec<usize>,
    max_total_columns: usize,
    active_contact: Option<String>,
    drag_column: Option<String>,
    sidebar_width: f32,
    sidebar_target: f32,
    sidebar_anim_from: f32,
    sidebar_anim_start: f64,
    sidebar_initialised: bool,
    /// 0..1 presence of the top bar's **Close all** button.
    close_all_anim: f32,
    close_all_anim_from: f32,
    close_all_target: f32,
    close_all_anim_start: f64,
    close_all_initialised: bool,
    relationship_request_seq: u64,
    relationship_list_requests: BTreeMap<u64, String>,
    relationship_create_requests: BTreeMap<u64, Option<String>>,
    /// Client/worker links to the organisation requested after an intake,
    /// keyed by request id, holding the new contact's name.
    intake_role_links: BTreeMap<u64, String>,
    /// Contacts created from an intake relationship slot, keyed by request id.
    intake_contact_creates: BTreeMap<u64, PendingIntakeContact>,
    /// Support-network relationships requested after an intake, keyed by
    /// request id, holding a human-readable label for the notice.
    intake_relationship_links: BTreeMap<u64, String>,
    /// Lifecycle changes in flight, keyed by request id, holding the editor window id.
    relationship_change_requests: BTreeMap<u64, String>,
    relationship_history_requests: BTreeMap<u64, String>,
    /// The organisation's relationship type catalogue and the picker options
    /// built from it, loaded once per signed-in organisation.
    relationship_types: Vec<ApiRelationshipType>,
    role_options: Vec<RoleOption>,
    relationship_types_request: Option<u64>,
    relationship_types_org: Option<String>,
    relationship_contact_creates: BTreeMap<u64, PendingInlineRelationship>,
    party_catalog: Vec<ApiGraphNode>,
    party_catalog_request: Option<u64>,
    relationship_edges: Vec<ApiRelationship>,
    relationship_graph_loaded: bool,
    relationship_graph_error: Option<String>,
    /// Bumped whenever the graph changes; constellation layouts rebuild once.
    relationship_graph_revision: u64,
    relationship_network: Option<Arc<RelationshipNetwork>>,
    /// The full-window relationship explorer's navigation and layout state.
    relationship_explorer: ExplorerState,
    rostering: RosteringState,
    rostering_request_seq: u64,
    rostering_requests: BTreeMap<u64, RosterRequest>,
    known_contacts: BTreeMap<String, ContactRow>,
    last_page: Page,
    page_changed_at: f64,
    frame_time: f64,
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

struct PendingIntakeContact {
    slot: String,
    role: RoleOption,
    notes: String,
}

struct AccountBadge {
    initials: String,
    name: String,
    organisation: String,
    role: String,
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
            page: Page::Contacts,
            search: String::new(),
            notice: None,
            config,
            settings,
            server,
            runtime: None,
            contacts: ContactsState::new(ContactFilter::All),
            organisations: OrganisationsState::new(),
            // Organisation request ids live in their own range.
            organisation_request_seq: 1 << 44,
            organisation_list_request: None,
            organisation_save_requests: BTreeMap::new(),
            organisation_delete_requests: BTreeMap::new(),
            organisation_query_started: None,
            intake: IntakeState::new(),
            intake_board: pages::intake::board::IntakeBoardState::default(),
            intake_case_columns: BTreeMap::new(),
            discard_intake_case: None,
            discard_intake_request: None,
            discard_intake_error: None,
            contact_windows: BTreeMap::new(),
            organisation_editors: BTreeMap::new(),
            columns: Vec::new(),
            columns_initialised: false,
            column_rects: Vec::new(),
            last_column_order: Vec::new(),
            max_total_columns: 4,
            active_contact: None,
            drag_column: None,
            sidebar_width: 0.0,
            sidebar_target: 0.0,
            sidebar_anim_from: 0.0,
            sidebar_anim_start: 0.0,
            sidebar_initialised: false,
            close_all_anim: 0.0,
            close_all_anim_from: 0.0,
            close_all_target: 0.0,
            close_all_anim_start: 0.0,
            close_all_initialised: false,
            // Keep relationship request ids in a separate range so they can
            // never collide with intake creation request ids.
            relationship_request_seq: 1 << 32,
            relationship_list_requests: BTreeMap::new(),
            relationship_create_requests: BTreeMap::new(),
            intake_role_links: BTreeMap::new(),
            intake_contact_creates: BTreeMap::new(),
            intake_relationship_links: BTreeMap::new(),
            relationship_change_requests: BTreeMap::new(),
            relationship_history_requests: BTreeMap::new(),
            relationship_types: Vec::new(),
            role_options: Vec::new(),
            relationship_types_request: None,
            relationship_types_org: None,
            relationship_contact_creates: BTreeMap::new(),
            party_catalog: Vec::new(),
            party_catalog_request: None,
            relationship_edges: Vec::new(),
            relationship_graph_loaded: false,
            relationship_graph_error: None,
            relationship_graph_revision: 0,
            relationship_network: None,
            relationship_explorer: ExplorerState::default(),
            rostering: RosteringState::default(),
            // Rostering request ids live in their own range.
            rostering_request_seq: 1 << 40,
            rostering_requests: BTreeMap::new(),
            known_contacts: BTreeMap::new(),
            last_page: Page::Contacts,
            page_changed_at: f64::NEG_INFINITY,
            frame_time: 0.0,
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

    /// Navigate to a page. Useful for deep links and headless previews. This
    /// replaces any other page columns, unlike the sidebar which opens each
    /// page as its own column.
    pub fn set_page(&mut self, page: Page) {
        self.columns.retain(|column| match &column.kind {
            ColumnKind::Page(open) => *open == page,
            ColumnKind::Contact(_) | ColumnKind::Organisation(_) | ColumnKind::IntakeCase(_) => {
                true
            }
        });
        self.open_page(page);
        self.reset_column_weights();
        self.last_page = page;
    }

    /// Show a page and make sure it has a column in the work area.
    fn open_page(&mut self, page: Page) {
        if let Some(index) = self
            .columns
            .iter()
            .position(|column| matches!(column.kind, ColumnKind::Page(open) if open == page))
        {
            self.page = page;
            if self.columns[index].closing {
                self.cancel_column_close(index);
            }
            return;
        }
        if self.visible_column_count() >= self.max_total_columns {
            if let Some(ctx) = self.app_ctx.clone() {
                self.notify(
                    &ctx,
                    format!(
                        "Close a column before opening another. Up to {} columns can be open at once in this window.",
                        self.max_total_columns
                    ),
                );
            }
            return;
        }
        self.page = page;
        self.columns.insert(
            0,
            ColumnState {
                kind: ColumnKind::Page(page),
                width: 760.0,
                weight: 1.0,
                display_width: 0.0,
                anim_from: 0.0,
                anim_to: 760.0,
                anim_start: 0.0,
                position_x: -1.0,
                pos_from: 0.0,
                pos_to: 0.0,
                pos_start: 0.0,
                closing: false,
                close_start: 0.0,
            },
        );
        self.reset_column_weights();
    }

    /// Open an additional page column. Used by previews and tests.
    pub fn open_page_column(&mut self, page: Page) {
        self.open_page(page);
    }

    /// Give every open column the same share of the window. Called whenever a
    /// column is added or removed so the remaining columns redistribute evenly.
    fn reset_column_weights(&mut self) {
        for column in &mut self.columns {
            column.weight = 1.0;
        }
    }

    fn begin_column_close(&mut self, index: usize, now: f64) {
        let column = &mut self.columns[index];
        if column.closing {
            return;
        }
        column.closing = true;
        column.close_start = now;
        column.anim_from = column.display_width;
        column.anim_to = 0.0;
        column.anim_start = now;
    }

    fn close_all_columns(&mut self, now: f64) {
        let ids: Vec<String> = self.contact_windows.keys().cloned().collect();
        for id in ids {
            self.close_contact_window(&id);
        }
        let organisations: Vec<String> = self.organisation_editors.keys().cloned().collect();
        for key in organisations {
            self.close_organisation_editor(&key);
        }
        let intake_cases: Vec<String> = self.intake_case_columns.keys().cloned().collect();
        for id in intake_cases {
            self.close_intake_case(&id);
        }
        let pages: Vec<Page> = self
            .columns
            .iter()
            .filter_map(|column| match &column.kind {
                ColumnKind::Page(page) => Some(*page),
                ColumnKind::Contact(_)
                | ColumnKind::Organisation(_)
                | ColumnKind::IntakeCase(_) => None,
            })
            .collect();
        for page in pages {
            if let Some(index) = self
                .columns
                .iter()
                .position(|column| matches!(column.kind, ColumnKind::Page(open) if open == page))
            {
                self.begin_column_close(index, now);
            }
        }
    }

    /// Drop every workspace surface and cached account snapshot when the
    /// session ends or moves to another server. The sign-in shell must not keep
    /// records, pages, intake drafts, roster data or relationship data in memory
    /// or leave detached windows open.
    fn clear_authenticated_state(&mut self, ctx: &egui::Context) {
        self.server.session = None;
        self.server.refresh_in_flight = false;
        self.server.auth_in_flight = false;
        self.restoring_session = false;

        for id in self.contact_windows.keys() {
            ctx.send_viewport_cmd_to(contact_viewport_id(id), egui::ViewportCommand::Close);
        }
        for page in &self.popped_pages {
            ctx.send_viewport_cmd_to(page_viewport_id(*page), egui::ViewportCommand::Close);
        }
        if self.intake.popped_out {
            ctx.send_viewport_cmd_to(intake_viewport_id(), egui::ViewportCommand::Close);
        }

        self.contact_windows.clear();
        self.organisation_editors.clear();
        self.columns.clear();
        self.popped_pages.clear();
        self.active_contact = None;
        self.drag_column = None;
        self.column_rects.clear();
        self.last_column_order.clear();
        self.columns_initialised = false;

        self.intake = IntakeState::new();
        self.intake_board.reset();
        self.intake_case_columns.clear();
        self.discard_intake_case = None;
        self.discard_intake_request = None;
        self.discard_intake_error = None;
        self.contacts = ContactsState::new(ContactFilter::All);
        self.organisations = OrganisationsState::new();
        self.rostering = RosteringState::default();
        self.known_contacts.clear();
        self.search.clear();

        self.relationship_list_requests.clear();
        self.relationship_create_requests.clear();
        self.relationship_change_requests.clear();
        self.relationship_history_requests.clear();
        self.intake_role_links.clear();
        self.intake_contact_creates.clear();
        self.intake_relationship_links.clear();
        self.relationship_contact_creates.clear();
        self.relationship_types.clear();
        self.role_options.clear();
        self.relationship_types_request = None;
        self.relationship_types_org = None;
        self.party_catalog.clear();
        self.party_catalog_request = None;
        self.relationship_edges.clear();
        self.relationship_graph_loaded = false;
        self.relationship_graph_revision = 0;
        self.relationship_graph_error = None;
        self.note_relationship_network_changed();
        self.relationship_network = None;
        self.relationship_explorer = ExplorerState::default();

        self.organisation_list_request = None;
        self.organisation_save_requests.clear();
        self.organisation_delete_requests.clear();
        self.organisation_query_started = None;
        self.rostering_requests.clear();

        self.cache_counts = ContactCounts::default();
        self.cache_shifts = None;
        self.page = Page::Contacts;
        self.last_page = Page::Contacts;
        self.notice = None;
        self.settings.notice = None;
    }

    fn cancel_column_close(&mut self, index: usize) {
        let column = &mut self.columns[index];
        column.closing = false;
        column.close_start = 0.0;
        column.anim_from = column.display_width;
        column.anim_to = column.width.max(1.0);
        column.anim_start = 0.0;
    }

    pub fn docked_contact_ids(&self) -> Vec<String> {
        self.columns
            .iter()
            .enumerate()
            .filter_map(|(index, column)| match &column.kind {
                ColumnKind::Contact(id)
                    if !self.columns[index].closing
                        && self
                            .contact_windows
                            .get(id)
                            .is_some_and(|window| !window.popped_out) =>
                {
                    Some(id.clone())
                }
                _ => None,
            })
            .collect()
    }

    pub fn page(&self) -> Page {
        self.page
    }

    /// Whether the current page currently has a column in the work area.
    pub fn page_column_open(&self) -> bool {
        self.columns.iter().any(|column| {
            matches!(column.kind, ColumnKind::Page(open) if open == self.page) && !column.closing
        })
    }

    fn visible_column_count(&self) -> usize {
        self.columns
            .iter()
            .filter(|column| {
                !column.closing
                    && match &column.kind {
                        ColumnKind::Page(_)
                        | ColumnKind::Organisation(_)
                        | ColumnKind::IntakeCase(_) => true,
                        ColumnKind::Contact(id) => self
                            .contact_windows
                            .get(id)
                            .is_some_and(|window| !window.popped_out),
                    }
            })
            .count()
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

    /// Install an authenticated session without contacting the server. Used by
    /// headless previews and UI tests that exercise the signed-in workspace.
    pub fn use_session(&mut self, auth: crate::api::AuthSession) {
        self.server.auth_in_flight = false;
        self.server.refresh_in_flight = false;
        self.restoring_session = false;
        self.server.session = Some(crate::server::Session {
            expires_at: Instant::now() + Duration::from_secs(auth.expires_in.max(60)),
            auth,
        });
        self.settings.on_signed_in();
        self.apply_relationship_preferences();
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

    /// Collapse or expand the docked main menu for previews/tests without
    /// persisting the preference.
    pub fn set_menu_collapsed(&mut self, collapsed: bool) {
        self.config.menu_collapsed = collapsed;
    }

    /// Whether the docked main menu is collapsed to its icon rail.
    pub fn menu_collapsed(&self) -> bool {
        self.config.menu_collapsed
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
        self.open_page(Page::Intake);
        if self.intake.view == IntakeView::Completed {
            self.intake.open_picker();
        }
        if self.intake.view == IntakeView::Picker {
            self.intake.list_requested = false;
        }
    }

    /// Begin a scenario directly. Useful for deep links and previews.
    pub fn start_intake(&mut self, kind: IntakeKind) {
        self.open_page(Page::Intake);
        self.intake.start(kind);
    }

    /// Number of contacts currently materialised in the active list.
    pub fn contacts_loaded(&self) -> usize {
        self.contacts.loaded()
    }

    /// Whether the intake board has loaded, failed, or has no flows to show.
    pub fn intake_board_ready(&self) -> bool {
        self.intake_board.board.is_some()
            || self.intake_board.flows_error.is_some()
            || self.intake_board.board_error.is_some()
            || (self.intake_board.flows_requested && !self.intake_board.flows_loading)
    }

    /// Number of cards currently rendered across all intake columns.
    pub fn intake_cards_loaded(&self) -> usize {
        self.intake_board
            .board
            .as_ref()
            .map(|board| board.columns.iter().map(|column| column.cards.len()).sum())
            .unwrap_or(0)
    }

    /// Open the first intake card as a column. Used by previews and tests.
    pub fn open_first_intake_case(&mut self) -> bool {
        let Some(card) = self
            .intake_board
            .board
            .as_ref()
            .and_then(|board| board.columns.iter().find_map(|column| column.cards.first()))
        else {
            return false;
        };
        self.open_intake_case(card.id.clone());
        true
    }

    /// Whether at least one open intake case column has loaded.
    pub fn intake_case_column_ready(&self) -> bool {
        self.intake_case_columns
            .values()
            .any(|state| state.case.is_some() || state.error.is_some())
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
        self.frame_time = now;
        self.perf.begin_frame(ui.ctx());
        self.poll_server(ui.ctx());
        self.poll_store(ui.ctx());
        self.poll_remote(ui.ctx());

        // Nothing in the workspace is reachable without a session. The shell
        // still runs the workers and renders the sign-in form so the user can
        // authenticate, restore a remembered login, or see why it expired.
        if !self.signed_in() {
            self.signed_out_shell(ui);
            return;
        }

        if !self.columns.is_empty()
            && !ui.ctx().egui_wants_keyboard_input()
            && ui
                .ctx()
                .input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::B))
        {
            self.toggle_menu_collapsed(ui.ctx());
        }

        if self.page != self.last_page {
            self.last_page = self.page;
            self.page_changed_at = now;
        }
        let page_progress =
            ((now - self.page_changed_at) / PAGE_ANIM_DURATION).clamp(0.0, 1.0) as f32;
        if page_progress < 1.0 {
            ui.ctx().request_repaint();
        }
        self.tick_contact_search();
        self.tick_intake_autosave();
        self.ensure_contacts_loaded();
        self.ensure_organisations_loaded();
        self.tick_organisation_search();
        self.ensure_relationship_types();
        if self.intake.view == IntakeView::Wizard {
            self.ensure_party_catalog();
        }
        self.ensure_intake_board_loaded();
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
        self.status_bar(ui);
        self.sidebar(ui);

        let mut contacts_action: Option<ContactsAction> = None;
        let mut organisations_action: Option<OrganisationsAction> = None;
        let mut intake_action: Option<IntakeAction> = None;
        let mut rostering_action: Option<RosteringAction> = None;
        let action = egui::CentralPanel::default()
            .frame(Frame::new().fill(theme::bg()).inner_margin(Margin::ZERO))
            .show(ui, |ui| {
                self.render_column_strip(
                    ui,
                    &mut contacts_action,
                    &mut organisations_action,
                    &mut intake_action,
                    &mut rostering_action,
                )
            })
            .inner;

        if let Some(contacts_action) = contacts_action {
            self.handle_contacts_action(contacts_action);
        }
        if let Some(organisations_action) = organisations_action {
            self.handle_organisations_action(organisations_action);
        }
        if let Some(intake_action) = intake_action {
            self.handle_intake_action(ui.ctx(), intake_action);
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
        self.render_discard_intake_modal(ui.ctx());

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

    /// The signed-out application shell: window chrome, brand bar and status
    /// only. The main menu becomes a sign-in form, and no workspace, page,
    /// record or data surface is rendered.
    fn signed_out_shell(&mut self, ui: &mut Ui) {
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
        self.status_bar(ui);
        self.sidebar(ui);

        egui::CentralPanel::default()
            .frame(Frame::new().fill(theme::bg()).inner_margin(Margin::ZERO))
            .show(ui, |_| {});

        if self.config.client_side_decorations {
            chrome::resize_handles(ui.ctx());
        }

        self.show_notice(ui.ctx());
    }

    fn page_contents(
        &mut self,
        ui: &mut Ui,
        page: Page,
        contacts_action: &mut Option<ContactsAction>,
        organisations_action: &mut Option<OrganisationsAction>,
        intake_action: &mut Option<IntakeAction>,
        rostering_action: &mut Option<RosteringAction>,
    ) -> Option<Action> {
        match page {
            Page::Settings => pages::settings::show(ui, &mut self.settings, &self.server),
            Page::Intake => {
                let signed_in = self.access_token().is_some();
                if self.intake.view == IntakeView::Wizard
                    || self.intake.view == IntakeView::Completed
                {
                    let store_ready = self.store_ready;
                    let catalog = self.lookup_parties();
                    let roles = self.role_options.clone();
                    if let Some(action) = pages::intake::show(
                        ui,
                        &mut self.intake,
                        store_ready,
                        signed_in,
                        &catalog,
                        &roles,
                    ) {
                        *intake_action = Some(action);
                    }
                } else if let Some(action) =
                    pages::intake::board::show(ui, &mut self.intake_board, signed_in)
                {
                    *intake_action = Some(IntakeAction::Board(action));
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
            Page::Relationships => {
                self.ensure_party_catalog();
                let catalog = self.lookup_parties();
                let network = self.relationship_network();
                let graph_error = self.relationship_graph_error.clone();
                let source = ExplorerSource {
                    nodes: &catalog,
                    network: &network,
                    loading: self.party_catalog_request.is_some(),
                    error: graph_error.as_deref(),
                };
                if let Some(action) =
                    pages::relationships::show(ui, &mut self.relationship_explorer, &source)
                {
                    self.handle_explorer_action(ui.ctx(), action);
                }
                None
            }
            Page::Organisations => {
                let signed_in = self.access_token().is_some();
                if let Some(action) =
                    pages::organisations::show(ui, &mut self.organisations, signed_in)
                {
                    *organisations_action = Some(action);
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
            other => pages::placeholder::show(ui, other),
        }
    }

    fn account_badge(&self) -> Option<AccountBadge> {
        let session = self.server.session.as_ref()?;
        Some(AccountBadge {
            initials: session.initials(),
            name: session.auth.user.full_name.clone(),
            organisation: session.auth.organisation.name.clone(),
            role: session.role().to_owned(),
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

        let signed_in = self.signed_in();
        for event in events {
            // Local drafts and cache counts belong to the signed-in account.
            // Keep the store worker's readiness events so a later sign-in still
            // sees a working cache, but drop data events while signed out.
            if !signed_in && !matches!(event, StoreEvent::Ready { .. } | StoreEvent::Failed { .. })
            {
                continue;
            }
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
            ContactsAction::GoToSettings => self.open_page(Page::Settings),
            ContactsAction::StartIntake => self.open_intake(),
        }
    }

    fn next_organisation_request(&mut self) -> u64 {
        self.organisation_request_seq += 1;
        self.organisation_request_seq
    }

    fn ensure_organisations_loaded(&mut self) {
        if self.page != Page::Organisations || self.access_token().is_none() {
            return;
        }
        if !self.organisations.attempted
            && !self.organisations.loading
            && self.organisations.search_due.is_none()
        {
            self.send_organisations_query();
        }
    }

    fn tick_organisation_search(&mut self) {
        if let Some(due) = self.organisations.search_due
            && Instant::now() >= due
        {
            self.organisations.search_due = None;
            self.send_organisations_query();
        }
    }

    fn send_organisations_query(&mut self) {
        let Some(token) = self.access_token() else {
            self.organisations.loading = false;
            self.organisations.error =
                Some("Sign in to load organisations from the server.".to_owned());
            return;
        };
        if self.remote.is_none() {
            self.organisations.loading = false;
            self.organisations.error = Some("The server connection is not available.".to_owned());
            return;
        }

        let request = self.next_organisation_request();
        self.organisation_list_request = Some(request);
        self.organisation_query_started = Some(Instant::now());
        self.organisations.attempted = true;
        let search = self.organisations.search.clone();
        let status = self.organisations.status.param();
        self.organisations.loading = true;
        self.organisations.error = None;
        if let Some(remote) = &self.remote {
            remote.list_organisations(
                request,
                self.server.base_url.clone(),
                token,
                search,
                status,
                500,
                0,
            );
        }
    }

    fn handle_organisations_action(&mut self, action: OrganisationsAction) {
        match action {
            OrganisationsAction::SearchChanged => {
                self.organisations.loading = true;
            }
            OrganisationsAction::FilterChanged | OrganisationsAction::Reload => {
                self.organisations.search_due = None;
                self.send_organisations_query();
            }
            OrganisationsAction::New => self.open_organisation_editor(None),
            OrganisationsAction::Edit(id) => self.open_organisation_editor(Some(id)),
        }
    }

    fn handle_organisation_editor_action(&mut self, key: &str, action: OrganisationEditorAction) {
        match action {
            OrganisationEditorAction::Save => self.save_organisation(key),
            OrganisationEditorAction::Cancel => self.close_organisation_editor(key),
            OrganisationEditorAction::Delete => self.delete_organisation_from_editor(key),
            OrganisationEditorAction::Explore { id, label } => {
                self.open_relationship_explorer(PartyKind::Organisation, &id, &label);
            }
        }
    }

    fn save_organisation(&mut self, key: &str) {
        let Some(editor) = self.organisation_editors.get(key) else {
            return;
        };
        let Ok(payload) = editor.form.write() else {
            return;
        };
        let id = editor.record_id.clone();
        let Some(token) = self.access_token() else {
            if let Some(editor) = self.organisation_editors.get_mut(key) {
                editor.save_failed("Sign in to save organisations.".to_owned());
            }
            return;
        };

        let request = self.next_organisation_request();
        self.organisation_save_requests
            .insert(request, (key.to_owned(), id.clone()));
        if let Some(editor) = self.organisation_editors.get_mut(key) {
            editor.pending = true;
            editor.error = None;
        }

        if let Some(remote) = &self.remote {
            if let Some(id) = &id {
                remote.update_organisation(
                    request,
                    self.server.base_url.clone(),
                    token,
                    id.clone(),
                    payload,
                );
            } else {
                remote.create_organisation(request, self.server.base_url.clone(), token, payload);
            }
        } else {
            self.organisation_save_requests.remove(&request);
            if let Some(editor) = self.organisation_editors.get_mut(key) {
                editor.save_failed("The server connection is not available.".to_owned());
            }
        }
    }

    fn delete_organisation_from_editor(&mut self, key: &str) {
        let Some(editor) = self.organisation_editors.get(key) else {
            return;
        };
        let Some(id) = editor.record_id.clone() else {
            return;
        };
        let Some(token) = self.access_token() else {
            if let Some(editor) = self.organisation_editors.get_mut(key) {
                editor.delete_failed("Sign in to delete organisations.".to_owned());
            }
            return;
        };

        let request = self.next_organisation_request();
        self.organisation_delete_requests
            .insert(request, (id.clone(), Some(key.to_owned())));
        if let Some(editor) = self.organisation_editors.get_mut(key) {
            editor.delete_pending = true;
            editor.error = None;
        }
        if let Some(remote) = &self.remote {
            remote.delete_organisation(request, self.server.base_url.clone(), token, id);
        } else {
            self.organisation_delete_requests.remove(&request);
            if let Some(editor) = self.organisation_editors.get_mut(key) {
                editor.delete_failed("The server connection is not available.".to_owned());
            }
        }
    }

    fn update_organisation_catalog(&mut self, organisation: &crate::organisation::ApiOrganisation) {
        if let Some(node) = self
            .party_catalog
            .iter_mut()
            .find(|node| node.kind == PartyKind::Organisation && node.id == organisation.id)
        {
            node.label = organisation.name.clone();
        } else {
            self.party_catalog.push(ApiGraphNode {
                kind: PartyKind::Organisation,
                id: organisation.id.clone(),
                label: organisation.name.clone(),
                degree: organisation.relationship_count,
                depth: None,
            });
        }
        self.note_relationship_network_changed();
    }

    fn ensure_intake_board_loaded(&mut self) {
        if self.page != Page::Intake
            || self.intake.view != IntakeView::Picker
            || self.access_token().is_none()
            || self.remote.is_none()
        {
            return;
        }

        if self.intake_board.flows.is_empty()
            && !self.intake_board.flows_loading
            && !self.intake_board.flows_requested
        {
            self.request_intake_flows();
            return;
        }

        if self.intake_board.fields.is_empty()
            && !self.intake_board.fields_loading
            && !self.intake_board.fields_requested
        {
            self.request_intake_fields();
        }

        let known: BTreeSet<String> = self
            .intake_board
            .flows
            .iter()
            .map(|flow| flow.id.clone())
            .collect();
        self.intake_board
            .selected_flows
            .retain(|flow_id| known.contains(flow_id));

        if self.intake_board.board.is_none()
            && !self.intake_board.board_loading
            && !self.intake_board.board_requested
        {
            self.request_intake_board();
        }
    }

    fn request_intake_flows(&mut self) {
        let Some(token) = self.access_token() else {
            return;
        };
        let Some(remote) = &self.remote else {
            return;
        };
        let request_id = self.intake_board.next_request();
        self.intake_board.flows_loading = true;
        self.intake_board.flows_requested = true;
        self.intake_board.flows_error = None;
        self.intake_board.flows_request_id = Some(request_id);
        remote.load_intake_flows(request_id, self.server.base_url.clone(), token);
    }

    fn request_intake_fields(&mut self) {
        let Some(token) = self.access_token() else {
            return;
        };
        let Some(remote) = &self.remote else {
            return;
        };
        let request_id = self.intake_board.next_request();
        self.intake_board.fields_loading = true;
        self.intake_board.fields_requested = true;
        self.intake_board.fields_request_id = Some(request_id);
        self.intake_board.fields_error = None;
        remote.load_intake_fields(request_id, self.server.base_url.clone(), token);
    }

    fn request_intake_board(&mut self) {
        let Some(token) = self.access_token() else {
            return;
        };
        let Some(remote) = &self.remote else {
            return;
        };
        let request_id = self.intake_board.next_request();
        self.intake_board.board_loading = true;
        self.intake_board.board_requested = true;
        self.intake_board.board_error = None;
        self.intake_board.board_request_id = Some(request_id);
        remote.load_intake_board(
            request_id,
            self.server.base_url.clone(),
            token,
            self.intake_board.flow_filter_ids(),
            self.intake_board.subject_kind.clone(),
            self.intake_board.assignee_filter.clone(),
            self.intake_board.search.clone(),
        );
    }

    fn open_intake_case(&mut self, case_id: String) {
        if let Some(index) = self.columns.iter().position(
            |column| matches!(&column.kind, ColumnKind::IntakeCase(open) if open == &case_id),
        ) {
            if self.columns[index].closing {
                self.cancel_column_close(index);
            }
            return;
        }
        if self.visible_column_count() >= self.max_total_columns {
            if let Some(ctx) = self.app_ctx.clone() {
                self.notify(
                    &ctx,
                    format!(
                        "Close a column before opening another. Up to {} columns can be open at once in this window; pop a record out for more.",
                        self.max_total_columns
                    ),
                );
            }
            return;
        }
        self.intake_case_columns
            .entry(case_id.clone())
            .or_insert_with(|| pages::intake::board::IntakeCaseColumnState::new(case_id.clone()));
        self.columns.push(ColumnState {
            kind: ColumnKind::IntakeCase(case_id.clone()),
            width: 520.0,
            weight: 1.0,
            display_width: 0.0,
            anim_from: 0.0,
            anim_to: 520.0,
            anim_start: 0.0,
            position_x: -1.0,
            pos_from: 0.0,
            pos_to: 0.0,
            pos_start: 0.0,
            closing: false,
            close_start: 0.0,
        });
        self.reset_column_weights();
        self.request_intake_case(case_id);
    }

    fn close_intake_case(&mut self, case_id: &str) {
        if self
            .intake_case_columns
            .get(case_id)
            .and_then(|state| state.case.as_ref())
            .is_some_and(|case| case.case.is_draft)
        {
            self.discard_intake_case = Some(case_id.to_owned());
            self.discard_intake_error = None;
            return;
        }
        if let Some(index) = self.columns.iter().position(
            |column| matches!(&column.kind, ColumnKind::IntakeCase(open) if open == case_id),
        ) {
            self.begin_column_close(index, self.frame_time);
        }
    }

    fn force_close_intake_case(&mut self, case_id: &str) {
        if let Some(index) = self.columns.iter().position(
            |column| matches!(&column.kind, ColumnKind::IntakeCase(open) if open == case_id),
        ) {
            self.begin_column_close(index, self.frame_time);
        }
    }

    fn request_delete_intake_case(&mut self, case_id: String) {
        let Some(token) = self.access_token() else {
            return;
        };
        let Some(remote) = &self.remote else {
            return;
        };
        let request_id = self.intake_board.next_request();
        self.discard_intake_request = Some(request_id);
        self.discard_intake_error = None;
        remote.delete_intake_case(request_id, self.server.base_url.clone(), token, case_id);
    }

    fn render_discard_intake_modal(&mut self, ctx: &egui::Context) {
        let Some(case_id) = self.discard_intake_case.clone() else {
            return;
        };
        if !self.intake_case_columns.contains_key(&case_id) {
            self.discard_intake_case = None;
            self.discard_intake_error = None;
            return;
        }
        let mut delete = false;
        let mut keep_editing = false;
        let saving = self.discard_intake_request.is_some();
        let response = egui::Modal::new(egui::Id::unique("discard-intake-modal"))
            .backdrop_color(Color32::from_black_alpha(170))
            .frame(theme::card())
            .show(ctx, |ui| {
                ui.set_width(460.0);
                ui.label(
                    RichText::new("Discard this intake?")
                        .font(theme::strong_font(18.0)),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "This intake has not been saved. If you close it, the record will be deleted.",
                    )
                    .color(theme::muted())
                    .size(12.0),
                );
                if let Some(error) = &self.discard_intake_error {
                    ui.add_space(6.0);
                    ui.colored_label(theme::danger(), error);
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!saving, |ui| {
                        if ui
                            .add(egui::Button::new(
                                RichText::new("Delete intake").color(theme::danger()),
                            ))
                            .clicked()
                        {
                            delete = true;
                        }
                    });
                    if ui.add_enabled(!saving, egui::Button::new("Keep editing")).clicked() {
                        keep_editing = true;
                    }
                    if saving {
                        ui.spinner();
                    }
                });
            });
        if response.should_close() {
            keep_editing = true;
        }
        if keep_editing {
            self.discard_intake_case = None;
            self.discard_intake_error = None;
        }
        if delete {
            self.request_delete_intake_case(case_id);
        }
    }

    fn request_intake_case(&mut self, case_id: String) {
        let Some(token) = self.access_token() else {
            return;
        };
        let Some(remote) = &self.remote else {
            return;
        };
        let request_id = self.intake_board.next_request();
        if let Some(state) = self.intake_case_columns.get_mut(&case_id) {
            state.loading = true;
            state.requested = true;
            state.request_id = Some(request_id);
            state.error = None;
            state.transition_error = None;
        }
        remote.load_intake_case(request_id, self.server.base_url.clone(), token, case_id);
    }

    fn request_intake_transition(&mut self, case_id: String, stage_id: String, version: i32) {
        let Some(token) = self.access_token() else {
            return;
        };
        let Some(remote) = &self.remote else {
            return;
        };
        let request_id = self.intake_board.next_request();
        if let Some(state) = self.intake_case_columns.get_mut(&case_id) {
            state.transitioning = true;
            state.transition_request_id = Some(request_id);
            state.transition_error = None;
        }
        remote.transition_intake_case(
            request_id,
            self.server.base_url.clone(),
            token,
            case_id,
            crate::intake_board::ApiTransitionWrite {
                to_stage_id: stage_id,
                version,
            },
        );
    }

    fn request_intake_contacts(&mut self) {
        let Some(token) = self.access_token() else {
            return;
        };
        let Some(remote) = &self.remote else {
            return;
        };
        let search = self.intake_board.new_search.trim().to_owned();
        if search.is_empty() {
            self.intake_board.new_error =
                Some("Enter a name, email or phone to search.".to_owned());
            return;
        }
        let request_id = self.intake_board.next_request();
        self.intake_board.contacts_request_id = Some(request_id);
        self.intake_board.new_candidates.clear();
        self.intake_board.new_error = None;
        remote.match_intake_contacts(request_id, self.server.base_url.clone(), token, search);
    }

    fn request_create_intake(&mut self) {
        let Some(token) = self.access_token() else {
            return;
        };
        let Some(remote) = &self.remote else {
            return;
        };
        let Some(flow_id) = self.intake_board.new_flow_id.clone() else {
            self.intake_board.new_error = Some("Choose an intake path.".to_owned());
            return;
        };
        let request_id = self.intake_board.next_request();
        self.intake_board.new_saving = true;
        self.intake_board.new_request_id = Some(request_id);
        self.intake_board.new_error = None;
        remote.create_intake_case(
            request_id,
            self.server.base_url.clone(),
            token,
            crate::intake_board::ApiIntakeCaseCreate {
                flow_id,
                contact_id: None,
                contact: None,
                display_name: None,
                source: Some("enquiry".to_owned()),
                data: json!({}),
                allow_duplicate_contact: false,
                duplicate_reason: None,
                draft: true,
            },
        );
    }

    fn request_intake_case_update(&mut self, case_id: String) {
        let Some(token) = self.access_token() else {
            return;
        };
        let Some(remote) = &self.remote else {
            return;
        };
        let Some(state) = self.intake_case_columns.get_mut(&case_id) else {
            return;
        };
        let Some(case) = &state.case else {
            return;
        };
        let data = state.build_patch(&self.intake_board.fields);
        let request_id = self.intake_board.next_request();
        let version = case.case.version;
        state.saving = true;
        state.save_request_id = Some(request_id);
        state.save_error = None;
        remote.update_intake_case(
            request_id,
            self.server.base_url.clone(),
            token,
            case_id,
            crate::intake_board::ApiIntakeCaseUpdate {
                version,
                data,
                force: false,
            },
        );
    }

    fn request_intake_checklist(&mut self, case_id: String, item_id: String, completed: bool) {
        let Some(token) = self.access_token() else {
            return;
        };
        let Some(remote) = &self.remote else {
            return;
        };
        let request_id = self.intake_board.next_request();
        if let Some(state) = self.intake_case_columns.get_mut(&case_id) {
            state.checklist_pending = true;
            state.checklist_request_id = Some(request_id);
            state.checklist_error = None;
        }
        remote.toggle_intake_checklist(
            request_id,
            self.server.base_url.clone(),
            token,
            case_id,
            item_id,
            crate::intake_board::ApiChecklistToggleWrite {
                completed,
                note: None,
            },
        );
    }

    fn request_intake_cancel(&mut self, case_id: String, reason: String) {
        let Some(token) = self.access_token() else {
            return;
        };
        let Some(remote) = &self.remote else {
            return;
        };
        let Some(state) = self.intake_case_columns.get_mut(&case_id) else {
            return;
        };
        let Some(case) = &state.case else {
            return;
        };
        let request_id = self.intake_board.next_request();
        let version = case.case.version;
        state.cancel_saving = true;
        state.cancel_request_id = Some(request_id);
        state.cancel_error = None;
        remote.cancel_intake_case(
            request_id,
            self.server.base_url.clone(),
            token,
            case_id,
            crate::intake_board::ApiIntakeCancelWrite { version, reason },
        );
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
            IntakeAction::Board(action) => self.handle_intake_board_action(action),
            IntakeAction::Start(kind) => {
                self.intake.start(kind);
                self.save_intake_now();
            }
            IntakeAction::Resume(id) => {
                let request_id = self.intake.next_request();
                self.intake.load_request = Some(request_id);
                self.intake.loading_draft = true;
                self.intake.error = None;
                self.intake.reset_relationships();
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
            IntakeAction::CreateRelationshipContact {
                slot,
                role,
                notes,
                draft,
            } => self.create_intake_relationship_contact(ctx, slot, role, notes, *draft),
            IntakeAction::GoToSettings => self.open_page(Page::Settings),
            IntakeAction::ViewContact { filter, name } => {
                self.open_page(Page::Contacts);
                self.show_contacts(filter, Some(name));
                let _ = ctx;
            }
        }
    }

    fn handle_intake_board_action(&mut self, action: pages::intake::board::IntakeBoardAction) {
        use pages::intake::board::IntakeBoardAction;
        match action {
            IntakeBoardAction::ReloadFlows => {
                self.intake_board.reset();
                self.request_intake_flows();
            }
            IntakeBoardAction::Search => {
                self.intake_board.reset_board();
                self.request_intake_board();
            }
            IntakeBoardAction::ReloadBoard => {
                self.intake_board.reset_board();
                self.request_intake_board();
            }
            IntakeBoardAction::OpenCase(case_id) => {
                self.open_intake_case(case_id);
            }
            IntakeBoardAction::OpenNewIntake => {
                self.intake_board.new_open = true;
                self.intake_board.new_error = None;
                if self.intake_board.new_flow_id.is_none() {
                    self.intake_board.new_flow_id = self
                        .intake_board
                        .flows
                        .iter()
                        .find(|flow| flow.is_active)
                        .or_else(|| self.intake_board.flows.first())
                        .map(|flow| flow.id.clone());
                }
                if self.intake_board.fields.is_empty()
                    && !self.intake_board.fields_loading
                    && !self.intake_board.fields_requested
                {
                    self.request_intake_fields();
                }
            }
            IntakeBoardAction::CloseNewIntake => {
                self.intake_board.new_open = false;
                self.intake_board.new_error = None;
                self.intake_board.new_saving = false;
                self.intake_board.new_request_id = None;
            }
            IntakeBoardAction::SearchNewContacts => self.request_intake_contacts(),
            IntakeBoardAction::CreateIntake => self.request_create_intake(),
        }
    }

    fn handle_intake_case_action(
        &mut self,
        case_id: &str,
        action: pages::intake::board::IntakeCaseAction,
    ) {
        use pages::intake::board::IntakeCaseAction;
        match action {
            IntakeCaseAction::Reload => self.request_intake_case(case_id.to_owned()),
            IntakeCaseAction::SaveFields => self.request_intake_case_update(case_id.to_owned()),
            IntakeCaseAction::ToggleChecklist { item_id, completed } => {
                self.request_intake_checklist(case_id.to_owned(), item_id, completed);
            }
            IntakeCaseAction::Transition {
                case_id,
                stage_id,
                version,
            } => self.request_intake_transition(case_id, stage_id, version),
            IntakeCaseAction::Cancel { reason } => {
                self.request_intake_cancel(case_id.to_owned(), reason);
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

    /// Search the contact list. Used by screenshot previews.
    pub fn search_contacts(&mut self, query: &str) {
        self.open_page(Page::Contacts);
        self.last_page = Page::Contacts;
        let filter = self.contacts.filter;
        self.show_contacts(filter, Some(query.to_owned()));
    }

    /// Show the contact list narrowed to a relationship. Used by screenshot previews.
    pub fn set_contact_filter(&mut self, filter: ContactFilter) {
        self.open_page(Page::Contacts);
        self.last_page = Page::Contacts;
        self.contacts.filter = filter;
        self.refresh_contact_lists();
    }

    fn open_organisation_editor(&mut self, id: Option<String>) {
        if let Some((key, _)) = self
            .organisation_editors
            .iter()
            .find(|(_, editor)| editor.record_id == id)
        {
            let key = key.clone();
            if let Some(index) = self.columns.iter().position(
                |column| matches!(&column.kind, ColumnKind::Organisation(open) if open == &key),
            ) && self.columns[index].closing
            {
                self.cancel_column_close(index);
            }
            return;
        }

        let editor = match &id {
            Some(id) => {
                let Some(organisation) = self
                    .organisations
                    .items
                    .iter()
                    .find(|organisation| &organisation.id == id)
                else {
                    return;
                };
                OrganisationEditorState::edit(organisation)
            }
            None => OrganisationEditorState::new(),
        };

        if self.visible_column_count() >= self.max_total_columns {
            if let Some(ctx) = self.app_ctx.clone() {
                self.notify(
                    &ctx,
                    format!(
                        "Close a column before opening another. Up to {} columns can be open at once in this window.",
                        self.max_total_columns
                    ),
                );
            }
            return;
        }

        let key = id.unwrap_or_else(|| "new".to_owned());
        self.organisation_editors.insert(key.clone(), editor);
        self.columns.push(ColumnState {
            kind: ColumnKind::Organisation(key),
            width: 520.0,
            weight: 1.0,
            display_width: 0.0,
            anim_from: 0.0,
            anim_to: 520.0,
            anim_start: 0.0,
            position_x: -1.0,
            pos_from: 0.0,
            pos_to: 0.0,
            pos_start: 0.0,
            closing: false,
            close_start: 0.0,
        });
        self.reset_column_weights();
    }

    fn close_organisation_editor(&mut self, key: &str) {
        let now = self.frame_time;
        if let Some(index) = self.columns.iter().position(
            |column| matches!(&column.kind, ColumnKind::Organisation(open) if open == key),
        ) {
            self.begin_column_close(index, now);
            return;
        }
        self.organisation_editors.remove(key);
    }

    pub fn open_contact(&mut self, id: &str) {
        if let Some(index) = self
            .columns
            .iter()
            .position(|column| matches!(&column.kind, ColumnKind::Contact(open) if open == id))
            && self.columns[index].closing
        {
            self.cancel_column_close(index);
        }
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

        let mut state = ContactWindowState::new(id.to_owned());
        state.constellation = self.constellation_preference();
        let request_id = state.request_id.unwrap_or(1);
        if self.visible_column_count() >= self.max_total_columns {
            if let Some(ctx) = self.app_ctx.clone() {
                self.notify(
                    &ctx,
                    format!(
                        "Close a column before opening another. Up to {} columns can be open at once in this window; pop a record out for more.",
                        self.max_total_columns
                    ),
                );
            }
            return;
        }
        self.contact_windows.insert(id.to_owned(), state);
        self.columns.push(ColumnState {
            kind: ColumnKind::Contact(id.to_owned()),
            width: 520.0,
            weight: 1.0,
            display_width: 0.0,
            anim_from: 0.0,
            anim_to: 520.0,
            anim_start: 0.0,
            position_x: -1.0,
            pos_from: 0.0,
            pos_to: 0.0,
            pos_start: 0.0,
            closing: false,
            close_start: 0.0,
        });
        self.reset_column_weights();
        self.active_contact = Some(id.to_owned());
        self.load_contact_window(id, request_id);
        self.ensure_party_catalog();
        self.load_contact_relationships(id);
    }

    fn next_relationship_request(&mut self) -> u64 {
        self.relationship_request_seq += 1;
        self.relationship_request_seq
    }

    /// Load the relationship type catalogue for the signed-in organisation.
    fn ensure_relationship_types(&mut self) {
        let Some(session) = &self.server.session else {
            return;
        };
        let organisation = session.auth.organisation.id.clone();
        if self.relationship_types_org.as_deref() == Some(organisation.as_str())
            || self.relationship_types_request.is_some()
        {
            return;
        }
        let token = session.auth.access_token.clone();
        let request = self.next_relationship_request();
        if let Some(remote) = &self.remote {
            self.relationship_types_request = Some(request);
            remote.relationship_types(request, self.server.base_url.clone(), token);
        }
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
        self.relationship_graph_error = None;
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

    fn set_contact_quality(&mut self, id: &str, payload: ApiWorkerQualityValueWrite) {
        let Some(token) = self.access_token() else {
            if let Some(window) = self.contact_windows.get_mut(id) {
                window.quality_pending = false;
                window.quality_error = Some("Sign in to save skills.".to_owned());
            }
            return;
        };
        let request = self.next_relationship_request();
        let Some(remote) = &self.remote else {
            if let Some(window) = self.contact_windows.get_mut(id) {
                window.quality_pending = false;
                window.quality_error = Some("The server connection is not available.".to_owned());
            }
            return;
        };
        if let Some(window) = self.contact_windows.get_mut(id) {
            window.quality_pending = true;
            window.quality_error = None;
        }
        remote.set_contact_quality(
            request,
            self.server.base_url.clone(),
            token,
            id.to_owned(),
            payload,
        );
    }

    fn delete_contact_quality(&mut self, id: &str, quality_id: String) {
        let Some(token) = self.access_token() else {
            return;
        };
        let request = self.next_relationship_request();
        let Some(remote) = &self.remote else {
            return;
        };
        if let Some(window) = self.contact_windows.get_mut(id) {
            window.quality_pending = true;
            window.quality_error = None;
        }
        remote.delete_contact_quality(
            request,
            self.server.base_url.clone(),
            token,
            id.to_owned(),
            quality_id,
        );
    }

    fn set_contact_requirement(&mut self, id: &str, payload: ApiClientRequirementWrite) {
        let Some(token) = self.access_token() else {
            if let Some(window) = self.contact_windows.get_mut(id) {
                window.requirement_pending = false;
                window.requirement_error = Some("Sign in to save requirements.".to_owned());
            }
            return;
        };
        let request = self.next_relationship_request();
        let Some(remote) = &self.remote else {
            if let Some(window) = self.contact_windows.get_mut(id) {
                window.requirement_pending = false;
                window.requirement_error =
                    Some("The server connection is not available.".to_owned());
            }
            return;
        };
        if let Some(window) = self.contact_windows.get_mut(id) {
            window.requirement_pending = true;
            window.requirement_error = None;
        }
        remote.set_contact_requirement(
            request,
            self.server.base_url.clone(),
            token,
            id.to_owned(),
            payload,
        );
    }

    fn delete_contact_requirement(&mut self, id: &str, requirement_id: String) {
        let Some(token) = self.access_token() else {
            return;
        };
        let request = self.next_relationship_request();
        let Some(remote) = &self.remote else {
            return;
        };
        if let Some(window) = self.contact_windows.get_mut(id) {
            window.requirement_pending = true;
            window.requirement_error = None;
        }
        remote.delete_contact_requirement(
            request,
            self.server.base_url.clone(),
            token,
            id.to_owned(),
            requirement_id,
        );
    }

    fn add_contact_funding_package(&mut self, id: &str, payload: ApiFundingPackageWrite) {
        let Some(token) = self.access_token() else {
            if let Some(window) = self.contact_windows.get_mut(id) {
                window.funding_pending = false;
                window.funding_error = Some("Sign in to save funding.".to_owned());
            }
            return;
        };
        let request = self.next_relationship_request();
        let Some(remote) = &self.remote else {
            if let Some(window) = self.contact_windows.get_mut(id) {
                window.funding_pending = false;
                window.funding_error = Some("The server connection is not available.".to_owned());
            }
            return;
        };
        if let Some(window) = self.contact_windows.get_mut(id) {
            window.funding_pending = true;
            window.funding_error = None;
        }
        remote.create_funding_package(
            request,
            self.server.base_url.clone(),
            token,
            id.to_owned(),
            payload,
        );
    }

    fn add_contact_agreement(&mut self, id: &str, payload: ApiAgreementWrite) {
        let Some(token) = self.access_token() else {
            if let Some(window) = self.contact_windows.get_mut(id) {
                window.funding_pending = false;
                window.funding_error = Some("Sign in to save agreements.".to_owned());
            }
            return;
        };
        let request = self.next_relationship_request();
        let Some(remote) = &self.remote else {
            if let Some(window) = self.contact_windows.get_mut(id) {
                window.funding_pending = false;
                window.funding_error = Some("The server connection is not available.".to_owned());
            }
            return;
        };
        if let Some(window) = self.contact_windows.get_mut(id) {
            window.funding_pending = true;
            window.funding_error = None;
        }
        remote.create_agreement(
            request,
            self.server.base_url.clone(),
            token,
            id.to_owned(),
            payload,
        );
    }

    fn add_contact_agreement_line(
        &mut self,
        id: &str,
        agreement_id: String,
        payload: ApiAgreementLineWrite,
    ) {
        let Some(token) = self.access_token() else {
            return;
        };
        let request = self.next_relationship_request();
        let Some(remote) = &self.remote else {
            return;
        };
        if let Some(window) = self.contact_windows.get_mut(id) {
            window.funding_pending = true;
            window.funding_error = None;
        }
        remote.add_agreement_line(
            request,
            self.server.base_url.clone(),
            token,
            id.to_owned(),
            agreement_id,
            payload,
        );
    }

    fn delete_contact_agreement_line(&mut self, id: &str, agreement_id: String, line_id: String) {
        let Some(token) = self.access_token() else {
            return;
        };
        let request = self.next_relationship_request();
        let Some(remote) = &self.remote else {
            return;
        };
        if let Some(window) = self.contact_windows.get_mut(id) {
            window.funding_pending = true;
            window.funding_error = None;
        }
        remote.delete_agreement_line(
            request,
            self.server.base_url.clone(),
            token,
            id.to_owned(),
            agreement_id,
            line_id,
        );
    }

    fn finalise_contact_agreement(&mut self, id: &str, agreement_id: String) {
        let Some(token) = self.access_token() else {
            return;
        };
        let request = self.next_relationship_request();
        let Some(remote) = &self.remote else {
            return;
        };
        if let Some(window) = self.contact_windows.get_mut(id) {
            window.funding_pending = true;
            window.funding_error = None;
        }
        remote.finalise_agreement(
            request,
            self.server.base_url.clone(),
            token,
            id.to_owned(),
            agreement_id,
        );
    }

    fn close_contact_window(&mut self, id: &str) {
        let now = self.frame_time;
        if let Some(index) = self
            .columns
            .iter()
            .position(|column| matches!(&column.kind, ColumnKind::Contact(open) if open == id))
        {
            let popped_out = self
                .contact_windows
                .get(id)
                .is_some_and(|window| window.popped_out);
            if !popped_out {
                self.begin_column_close(index, now);
                return;
            }
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
        self.columns
            .retain(|column| !matches!(&column.kind, ColumnKind::Contact(open) if open == id));
        self.relationship_list_requests
            .retain(|_, contact_id| contact_id != id);
        self.relationship_create_requests
            .retain(|_, window_id| window_id.as_deref() != Some(id));
        self.relationship_contact_creates
            .retain(|_, pending| pending.window_id != id);
        if self.active_contact.as_deref() == Some(id) {
            self.active_contact = self.docked_contact_ids().first().cloned();
        }
        if popped_out && let Some(ctx) = &self.app_ctx {
            ctx.send_viewport_cmd_to(contact_viewport_id(id), egui::ViewportCommand::Close);
        }
    }

    /// Placeholder shown in the main window when the current page lives in its
    /// own native window.
    fn detached_page_placeholder(&mut self, ui: &mut Ui, page: Page) {
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
            let mut organisations_action: Option<OrganisationsAction> = None;
            let mut intake_action: Option<IntakeAction> = None;
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
                            &mut organisations_action,
                            &mut intake_action,
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
            if let Some(action) = organisations_action {
                self.handle_organisations_action(action);
            }
            if let Some(action) = intake_action {
                self.handle_intake_action(ctx, action);
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
            .columns
            .iter()
            .filter_map(|column| match &column.kind {
                ColumnKind::Contact(id)
                    if self
                        .contact_windows
                        .get(id)
                        .is_some_and(|window| window.popped_out) =>
                {
                    Some(id.clone())
                }
                _ => None,
            })
            .collect();
        let catalog = self.lookup_parties();
        let network = self.relationship_network();
        let graph_error = self.relationship_graph_error.clone();
        let source = ConstellationSource {
            nodes: &catalog,
            network: &network,
            loading: self.party_catalog_request.is_some(),
            error: graph_error.as_deref(),
        };
        let roles = self.role_options.clone();
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
                pages::contact_editor::show_viewport(ui, window, client_side, &source, &roles)
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
        let open_ids: BTreeSet<String> = self.contact_windows.keys().cloned().collect();
        self.columns.retain(|column| match &column.kind {
            ColumnKind::Page(_) | ColumnKind::Organisation(_) | ColumnKind::IntakeCase(_) => true,
            ColumnKind::Contact(id) => open_ids.contains(id),
        });
    }

    /// The main work area: the open page and every docked record as resizable,
    /// reorderable columns. The page itself is simply the first column.
    fn render_column_strip(
        &mut self,
        ui: &mut Ui,
        contacts_action: &mut Option<ContactsAction>,
        organisations_action: &mut Option<OrganisationsAction>,
        intake_action: &mut Option<IntakeAction>,
        rostering_action: &mut Option<RosteringAction>,
    ) -> Option<Action> {
        let mut page_action = None;
        let mut close: Option<String> = None;
        let mut close_organisation: Option<String> = None;
        let mut close_intake_case: Option<String> = None;
        let mut close_page: Option<Page> = None;
        let mut resize: Option<(usize, usize, f32)> = None;
        let mut editor_actions: Vec<(String, ContactEditorAction)> = Vec::new();
        let mut organisation_editor_actions: Vec<(String, OrganisationEditorAction)> = Vec::new();
        let mut intake_case_actions: Vec<(String, pages::intake::board::IntakeCaseAction)> =
            Vec::new();

        let visible: Vec<usize> = self
            .columns
            .iter()
            .enumerate()
            .filter_map(|(index, column)| match &column.kind {
                ColumnKind::Page(_) => Some(index),
                ColumnKind::Contact(id) => (column.closing
                    || self
                        .contact_windows
                        .get(id)
                        .is_some_and(|window| !window.popped_out))
                .then_some(index),
                ColumnKind::Organisation(_) => Some(index),
                ColumnKind::IntakeCase(_) => Some(index),
            })
            .collect();
        if visible.is_empty() {
            return page_action;
        }
        ui.add_space(COLUMN_STRIP_TOP_PADDING);
        let viewport_width = ui.available_width();
        let strip_width = (viewport_width - COLUMN_STRIP_PADDING * 2.0).max(1.0);
        let now = ui.ctx().input(|input| input.time);
        let max_total_columns = (((strip_width + 7.0) / (COLUMN_MIN_WIDTH + 7.0)).floor() as usize)
            .clamp(1, MAX_RECORD_COLUMNS + 1);
        self.max_total_columns = max_total_columns;
        let content_width = self.normalise_column_widths(&visible, strip_width, now);
        let handle_width = 7.0;

        let height = (ui.available_height() - COLUMN_STRIP_BOTTOM_PADDING).max(120.0);

        let catalog = self.lookup_parties();
        let network = self.relationship_network();
        let graph_error = self.relationship_graph_error.clone();
        let source = ConstellationSource {
            nodes: &catalog,
            network: &network,
            loading: self.party_catalog_request.is_some(),
            error: graph_error.as_deref(),
        };
        let roles = self.role_options.clone();

        if ui.ctx().input(|input| input.key_pressed(egui::Key::Escape))
            && self.drag_column.is_some()
        {
            self.drag_column = None;
            ui.ctx().request_repaint();
        }

        let order = self.column_order_for_drag(&visible, ui);
        let mut new_column_rects: Vec<(usize, egui::Rect)> = Vec::new();
        let mut rendered_columns: Vec<(usize, f32, f32)> = Vec::new();

        // Target positions in the preview order, based on animated widths.
        let mut target_x = COLUMN_STRIP_PADDING;
        let mut target_positions: std::collections::HashMap<usize, f32> =
            std::collections::HashMap::new();
        for index in &order {
            target_positions.insert(*index, target_x);
            target_x += self.columns[*index].display_width.max(1.0) + handle_width;
        }
        let order_changed = self.last_column_order != order;

        for &index in &order {
            let target = *target_positions.get(&index).unwrap_or(&0.0);
            let column = &mut self.columns[index];
            if column.position_x < 0.0 {
                column.position_x = target;
                column.pos_from = target;
                column.pos_to = target;
                column.pos_start = now;
            } else if order_changed || column.pos_start <= 0.0 {
                column.pos_from = column.position_x;
                column.pos_to = target;
                column.pos_start = now;
            } else {
                column.pos_to = target;
            }
        }

        {
            let (strip_rect, _) =
                ui.allocate_exact_size(vec2(viewport_width, height), Sense::hover());

            for index in order.iter().copied() {
                let (kind, display_width, target_x, closing, close_progress) = {
                    let column = &self.columns[index];
                    let close_progress = if column.closing {
                        ((now - column.close_start) / COLUMN_ANIM_DURATION).clamp(0.0, 1.0) as f32
                    } else {
                        0.0
                    };
                    (
                        column.kind.clone(),
                        column.display_width.max(1.0),
                        *target_positions.get(&index).unwrap_or(&0.0),
                        column.closing,
                        close_progress,
                    )
                };
                let column = &mut self.columns[index];
                let t = ((now - column.pos_start) / COLUMN_ANIM_DURATION).clamp(0.0, 1.0);
                let eased = theme::ease_in_out(t as f32);
                let x = if t >= 1.0 {
                    column.pos_to
                } else {
                    column.pos_from + (column.pos_to - column.pos_from) * eased
                };
                column.position_x = x;
                rendered_columns.push((index, x, display_width));

                let rect = egui::Rect::from_min_size(
                    strip_rect.min + vec2(x, 0.0),
                    vec2(display_width, height),
                );
                let mut column_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt(("column", kind.key()))
                        .max_rect(rect)
                        .layout(Layout::top_down(Align::Min)),
                );
                column_ui.set_clip_rect(rect.expand(1.0));
                if closing {
                    column_ui.multiply_opacity(1.0 - theme::ease_in_out(close_progress));
                }
                let is_dragging = self.drag_column.as_deref() == Some(kind.key().as_str());
                Frame::new()
                    .fill(if is_dragging {
                        theme::surface_raised()
                    } else {
                        theme::surface()
                    })
                    .stroke(Stroke::NONE)
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(1))
                    .show(&mut column_ui, |ui| {
                        ui.set_min_height(height);
                        let column_page = match &kind {
                            ColumnKind::Page(open) => *open,
                            ColumnKind::Contact(_)
                            | ColumnKind::Organisation(_)
                            | ColumnKind::IntakeCase(_) => self.page,
                        };
                        let column_popped = self.page_popped_out(column_page);
                        let (drag, close_clicked, pop_out, toggle_page) = Self::column_header(
                            ui,
                            &kind,
                            column_popped,
                            &self.contact_windows,
                            &self.organisation_editors,
                            &self.intake_case_columns,
                        );
                        if !closing {
                            match &kind {
                                ColumnKind::Page(open) => {
                                    if close_clicked {
                                        close_page = Some(*open);
                                    }
                                }
                                ColumnKind::Contact(id) => {
                                    if close_clicked {
                                        close = Some(id.clone());
                                    }
                                    if pop_out {
                                        editor_actions
                                            .push((id.clone(), ContactEditorAction::PopOut));
                                    }
                                }
                                ColumnKind::Organisation(key) => {
                                    if close_clicked {
                                        close_organisation = Some(key.clone());
                                    }
                                }
                                ColumnKind::IntakeCase(id) => {
                                    if close_clicked {
                                        close_intake_case = Some(id.clone());
                                    }
                                }
                            }
                            if toggle_page {
                                self.toggle_page_popout(ui.ctx(), column_page);
                            }
                            if drag.drag_started() {
                                self.drag_column = Some(kind.key());
                            }
                            if drag.dragged() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                            }
                        }
                        ui.separator();

                        match &kind {
                            ColumnKind::Page(open_page) => {
                                Frame::new()
                                    .fill(theme::bg())
                                    .corner_radius(CornerRadius {
                                        nw: 0,
                                        ne: 0,
                                        sw: 9,
                                        se: 9,
                                    })
                                    .inner_margin(Margin::same(16))
                                    .show(ui, |ui| {
                                        ui.set_min_height((height - 40.0).max(120.0));
                                        if column_popped {
                                            self.detached_page_placeholder(ui, *open_page);
                                        } else {
                                            let now = ui.ctx().input(|input| input.time);
                                            let progress = if *open_page == self.page {
                                                ((now - self.page_changed_at) / PAGE_ANIM_DURATION)
                                                    .clamp(0.0, 1.0)
                                                    as f32
                                            } else {
                                                1.0
                                            };
                                            if progress < 1.0 {
                                                let enter = theme::ease_in_out(progress);
                                                ui.multiply_opacity(0.35 + 0.65 * enter);
                                                ui.add_space((1.0 - enter) * 8.0);
                                            }
                                            if let Some(action) = self.page_contents(
                                                ui,
                                                *open_page,
                                                contacts_action,
                                                organisations_action,
                                                intake_action,
                                                rostering_action,
                                            ) {
                                                page_action = Some(action);
                                            }
                                        }
                                    });
                            }
                            ColumnKind::Contact(id) => {
                                Frame::new()
                                        .fill(theme::bg())
                                        .corner_radius(CornerRadius {
                                            nw: 0,
                                            ne: 0,
                                            sw: 9,
                                            se: 9,
                                        })
                                        .inner_margin(Margin::same(12))
                                        .show(ui, |ui| {
                                            ui.set_min_height((height - 40.0).max(120.0));
                                            if let Some(window) = self.contact_windows.get_mut(id)
                                                && let Some(action) =
                                                    pages::contact_editor::show_column(
                                                        ui, window, &source, &roles,
                                                    )
                                            {
                                                editor_actions.push((id.clone(), action));
                                            }
                                        });
                            }
                            ColumnKind::Organisation(key) => {
                                Frame::new()
                                    .fill(theme::bg())
                                    .corner_radius(CornerRadius {
                                        nw: 0,
                                        ne: 0,
                                        sw: 9,
                                        se: 9,
                                    })
                                    .inner_margin(Margin::same(12))
                                    .show(ui, |ui| {
                                        ui.set_min_height((height - 40.0).max(120.0));
                                        if let Some(editor) = self.organisation_editors.get_mut(key)
                                            && let Some(action) =
                                                pages::organisations::show_editor(ui, editor)
                                        {
                                            organisation_editor_actions.push((key.clone(), action));
                                        }
                                    });
                            }
                            ColumnKind::IntakeCase(id) => {
                                Frame::new()
                                    .fill(theme::bg())
                                    .corner_radius(CornerRadius {
                                        nw: 0,
                                        ne: 0,
                                        sw: 9,
                                        se: 9,
                                    })
                                    .inner_margin(Margin::same(12))
                                    .show(ui, |ui| {
                                        ui.set_min_height((height - 40.0).max(120.0));
                                        if let Some(state) = self.intake_case_columns.get_mut(id)
                                            && let Some(action) =
                                                pages::intake::board::show_case_column(
                                                    ui,
                                                    state,
                                                    &self.intake_board.fields,
                                                )
                                        {
                                            intake_case_actions.push((id.clone(), action));
                                        }
                                    });
                            }
                        }
                    });

                column_ui.painter().rect_stroke(
                    rect.shrink(0.5),
                    CornerRadius::same(10),
                    if is_dragging {
                        Stroke::new(2.0, theme::accent())
                    } else {
                        Stroke::new(1.0, theme::border_strong())
                    },
                    StrokeKind::Inside,
                );

                new_column_rects.push((
                    index,
                    egui::Rect::from_min_size(
                        strip_rect.min + vec2(target_x, 0.0),
                        vec2(display_width, height),
                    ),
                ));
            }

            for (position, index) in order.iter().copied().enumerate() {
                if position + 1 == order.len() {
                    break;
                }
                let next_index = order[position + 1];
                let (left_x, left_width) = rendered_columns
                    .iter()
                    .find(|(column_index, _, _)| *column_index == index)
                    .map(|(_, x, width)| (*x, *width))
                    .unwrap_or((
                        *target_positions.get(&index).unwrap_or(&0.0),
                        self.columns[index].display_width.max(1.0),
                    ));
                let boundary = left_x + left_width;
                if boundary >= COLUMN_STRIP_PADDING + content_width - 4.0 {
                    continue;
                }
                let hit_width = 10.0;
                let handle_rect = egui::Rect::from_min_size(
                    strip_rect.min + vec2(boundary - (hit_width - handle_width) / 2.0, 0.0),
                    vec2(hit_width, height),
                );
                let left_key = self.columns[index].kind.key();
                let right_key = self.columns[next_index].kind.key();
                let handle = ui.interact(
                    handle_rect,
                    ui.scope_id().with(("column-resize", left_key, right_key)),
                    Sense::click_and_drag(),
                );
                if handle.hovered() || handle.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    ui.painter().vline(
                        strip_rect.min.x + boundary + handle_width / 2.0,
                        handle_rect.y_range(),
                        Stroke::new(2.0, theme::accent()),
                    );
                }
                if handle.dragged() {
                    resize = Some((index, next_index, handle.drag_delta().x));
                }
            }
            self.column_rects = new_column_rects;
            self.last_column_order = order.clone();
        }

        if let Some((left_index, right_index, delta)) = resize {
            self.apply_column_resize(left_index, right_index, delta, now);
        }

        if ui.ctx().input(|input| input.pointer.any_released()) && self.drag_column.take().is_some()
        {
            self.apply_column_order(&visible, &order);
        }

        if let Some(page) = close_page
            && let Some(index) = self
                .columns
                .iter()
                .position(|column| matches!(column.kind, ColumnKind::Page(open) if open == page))
        {
            self.begin_column_close(index, now);
        }
        if let Some(id) = close {
            self.close_contact_window(&id);
        }
        if let Some(key) = close_organisation {
            self.close_organisation_editor(&key);
        }
        if let Some(id) = close_intake_case {
            self.close_intake_case(&id);
        }
        for (id, action) in intake_case_actions {
            self.handle_intake_case_action(&id, action);
        }
        for (id, action) in editor_actions {
            let ctx = ui.ctx().clone();
            self.handle_editor_action(&ctx, &id, action);
        }
        for (key, action) in organisation_editor_actions {
            self.handle_organisation_editor_action(&key, action);
        }

        let finished: Vec<ColumnKind> = self
            .columns
            .iter()
            .filter(|column| column.closing && now - column.close_start >= COLUMN_ANIM_DURATION)
            .map(|column| column.kind.clone())
            .collect();
        let mut removed_any = false;
        for kind in finished {
            match kind {
                ColumnKind::Page(page) => {
                    self.dock_page(ui.ctx(), page);
                    let before = self.columns.len();
                    self.columns.retain(
                        |column| !matches!(column.kind, ColumnKind::Page(open) if open == page),
                    );
                    removed_any |= self.columns.len() != before;
                    if self.page == page {
                        self.page = self
                            .columns
                            .iter()
                            .find_map(|column| match &column.kind {
                                ColumnKind::Page(open) if !column.closing => Some(*open),
                                _ => None,
                            })
                            .unwrap_or(self.page);
                    }
                }
                ColumnKind::Contact(id) => {
                    let had_column = self.columns.iter().any(
                        |column| matches!(&column.kind, ColumnKind::Contact(open) if open == &id),
                    );
                    self.remove_contact_window(&id);
                    removed_any |= had_column;
                }
                ColumnKind::Organisation(key) => {
                    let before = self.columns.len();
                    self.columns.retain(
                        |column| !matches!(&column.kind, ColumnKind::Organisation(open) if open == &key),
                    );
                    self.organisation_editors.remove(&key);
                    removed_any |= self.columns.len() != before;
                }
                ColumnKind::IntakeCase(id) => {
                    let before = self.columns.len();
                    self.columns.retain(
                        |column| !matches!(&column.kind, ColumnKind::IntakeCase(open) if open == &id),
                    );
                    self.intake_case_columns.remove(&id);
                    removed_any |= self.columns.len() != before;
                }
            }
        }
        if removed_any {
            self.reset_column_weights();
        }

        let animating = self.columns.iter().any(|column| {
            column.closing
                || (now - column.anim_start) < COLUMN_ANIM_DURATION
                || (now - column.pos_start) < COLUMN_ANIM_DURATION
        });
        if animating {
            ui.ctx().request_repaint();
        }

        page_action
    }

    fn column_header(
        ui: &mut Ui,
        kind: &ColumnKind,
        page_popped: bool,
        windows: &BTreeMap<String, ContactWindowState>,
        organisations: &BTreeMap<String, OrganisationEditorState>,
        intake_cases: &BTreeMap<String, pages::intake::board::IntakeCaseColumnState>,
    ) -> (egui::Response, bool, bool, bool) {
        let mut close_clicked = false;
        let mut pop_out = false;
        let mut toggle_page = false;

        let frame = Frame::new()
            .fill(theme::sidebar())
            .corner_radius(CornerRadius {
                nw: 9,
                ne: 9,
                sw: 0,
                se: 0,
            })
            .inner_margin(Margin::symmetric(10, 6))
            .show(ui, |ui| {
                let width = ui.available_width();
                let (rect, _) = ui.allocate_exact_size(vec2(width, 34.0), Sense::hover());

                let mut control_rects: Vec<egui::Rect> = Vec::new();
                let mut controls = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt(("column-controls", kind.key()))
                        .max_rect(rect)
                        .layout(Layout::right_to_left(Align::Center)),
                );
                let close_response = column_icon_button(&mut controls, "×", "Close column");
                if close_response.clicked() {
                    close_clicked = true;
                }
                control_rects.push(close_response.rect);

                match kind {
                    ColumnKind::Page(_) => {
                        let (glyph, tooltip) = if page_popped {
                            ("↙", "Dock page")
                        } else {
                            ("↗", "Pop out page")
                        };
                        let response = column_icon_button(&mut controls, glyph, tooltip);
                        if response.clicked() {
                            toggle_page = true;
                        }
                        control_rects.push(response.rect);
                    }
                    ColumnKind::Contact(_) => {
                        let response = column_icon_button(&mut controls, "↗", "Pop out record");
                        if response.clicked() {
                            pop_out = true;
                        }
                        control_rects.push(response.rect);
                    }
                    ColumnKind::Organisation(_) | ColumnKind::IntakeCase(_) => {}
                }

                let controls_left = control_rects
                    .iter()
                    .map(|rect| rect.left())
                    .fold(rect.right(), f32::min);
                let title_rect = egui::Rect::from_min_max(
                    rect.min,
                    egui::pos2((controls_left - 8.0).max(rect.left()), rect.bottom()),
                );

                let (icon, title, subtitle) = match kind {
                    ColumnKind::Page(page) => {
                        (page.icon(), page.label().to_owned(), "Page".to_owned())
                    }
                    ColumnKind::Contact(id) => {
                        let (name, subtitle) = windows
                            .get(id)
                            .map(|window| {
                                let name = window.computed_display_name();
                                let name = if name.is_empty() {
                                    "Loading…".to_owned()
                                } else {
                                    name
                                };
                                (
                                    name,
                                    format!("{} · {}", window.role.to_uppercase(), window.status),
                                )
                            })
                            .unwrap_or_else(|| ("Contact".to_owned(), "CONTACT".to_owned()));
                        ("👤", name, subtitle)
                    }
                    ColumnKind::Organisation(key) => {
                        let (name, subtitle) = organisations
                            .get(key)
                            .map(|editor| (editor.title(), editor.subtitle()))
                            .unwrap_or_else(|| {
                                ("Organisation".to_owned(), "ORGANISATION".to_owned())
                            });
                        ("🏢", name, subtitle)
                    }
                    ColumnKind::IntakeCase(id) => {
                        let (name, subtitle) = intake_cases
                            .get(id)
                            .map(|state| (state.title(), state.subtitle()))
                            .unwrap_or_else(|| {
                                ("Intake case".to_owned(), "INTAKE CASE".to_owned())
                            });
                        ("📝", name, subtitle)
                    }
                };

                let mut title_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt(("column-title", kind.key()))
                        .max_rect(title_rect)
                        .layout(Layout::left_to_right(Align::Center)),
                );
                title_ui.add_space(4.0);
                title_ui.add(
                    egui::Label::new(
                        RichText::new(format!("{icon}  {title}"))
                            .color(theme::text())
                            .font(theme::strong_font(11.5)),
                    )
                    .truncate()
                    .selectable(false),
                );
                title_ui.add(
                    egui::Label::new(RichText::new(subtitle).color(theme::faint()).size(9.5))
                        .truncate()
                        .selectable(false),
                );
                let saving = match kind {
                    ColumnKind::Contact(id) => windows.get(id).is_some_and(|window| window.saving),
                    ColumnKind::IntakeCase(id) => intake_cases
                        .get(id)
                        .is_some_and(|state| state.loading || state.transitioning),
                    ColumnKind::Organisation(key) => {
                        organisations.get(key).is_some_and(|editor| editor.pending)
                    }
                    ColumnKind::Page(_) => false,
                };
                if saving {
                    title_ui.add(
                        egui::Label::new(
                            RichText::new("Saving…").color(theme::warning()).size(9.5),
                        )
                        .selectable(false),
                    );
                }

                let drag = ui.interact(
                    title_rect,
                    ui.scope_id().with(("column-drag", kind.key())),
                    Sense::click_and_drag(),
                );
                if drag.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
                drag.clone().on_hover_text("Drag to reorder this column");

                drag
            });
        (frame.inner, close_clicked, pop_out, toggle_page)
    }

    /// Build the order used while dragging: the dragged column is shown in its
    /// prospective landing slot, without committing the move yet.
    fn column_order_for_drag(&self, visible: &[usize], ui: &Ui) -> Vec<usize> {
        let mut order = visible.to_vec();
        let Some(dragged_key) = self.drag_column.as_ref() else {
            return order;
        };
        let Some(dragged_pos) = order
            .iter()
            .position(|index| self.columns[*index].kind.key() == *dragged_key)
        else {
            return order;
        };
        let Some(pointer_x) = ui
            .ctx()
            .input(|input| input.pointer.interact_pos())
            .map(|position| position.x)
        else {
            return order;
        };

        let mut target = order.len();
        for (position, index) in order.iter().enumerate() {
            if let Some((_, rect)) = self
                .column_rects
                .iter()
                .find(|(column_index, _)| column_index == index)
                && pointer_x < rect.center().x
            {
                target = position;
                break;
            }
        }

        let dragged = order.remove(dragged_pos);
        let insert = if target > dragged_pos {
            target - 1
        } else {
            target
        };
        order.insert(insert.min(order.len()), dragged);
        order
    }

    fn apply_column_order(&mut self, visible: &[usize], order: &[usize]) {
        if order.len() != visible.len() {
            return;
        }
        let ordered: Vec<ColumnState> = order
            .iter()
            .map(|index| self.columns[*index].clone())
            .collect();
        for (slot, column) in visible.iter().copied().zip(ordered) {
            self.columns[slot] = column;
        }
    }

    fn apply_column_resize(&mut self, left_index: usize, right_index: usize, delta: f32, now: f64) {
        let left = self.columns[left_index].width;
        let right = self.columns[right_index].width;
        let min_delta = (COLUMN_MIN_WIDTH - left).min(0.0);
        let max_delta = (right - COLUMN_MIN_WIDTH).max(0.0);
        let delta = delta.clamp(min_delta, max_delta);
        let left_width = left + delta;
        let right_width = right - delta;
        let pair_width = left + right;
        let pair_weight = self.columns[left_index].weight + self.columns[right_index].weight;
        let left_weight = if pair_width > 0.0 {
            pair_weight * left_width / pair_width
        } else {
            self.columns[left_index].weight
        };
        let right_weight = if pair_width > 0.0 {
            pair_weight * right_width / pair_width
        } else {
            self.columns[right_index].weight
        };
        for (index, width, weight) in [
            (left_index, left_width, left_weight),
            (right_index, right_width, right_weight),
        ] {
            let column = &mut self.columns[index];
            column.width = width;
            column.weight = weight;
            column.display_width = width;
            // Keep the animation target in sync, otherwise the next
            // normalise pass restores the pre-resize width.
            column.anim_from = width;
            column.anim_to = width;
            column.anim_start = now;
        }
    }

    /// Fit the visible columns into the viewport. Columns share the available
    /// width by weight, so a fresh set of columns is split evenly. Opening is
    /// capped so the workspace never needs horizontal scrolling.
    fn normalise_column_widths(&mut self, visible: &[usize], viewport_width: f32, now: f64) -> f32 {
        if visible.is_empty() {
            return viewport_width;
        }
        let handle_total = 7.0 * visible.len().saturating_sub(1) as f32;

        // While a column is collapsing, leave the remaining columns at their
        // current widths so nothing overlaps. They reflow once the close
        // animation has finished and the column is removed.
        if visible.iter().any(|index| self.columns[*index].closing) {
            for &index in visible {
                let column = &mut self.columns[index];
                if column.closing && column.anim_to > 0.0 {
                    column.anim_from = column.display_width;
                    column.anim_to = 0.0;
                    column.anim_start = now;
                }
                if column.anim_start <= 0.0 {
                    column.anim_start = now;
                }
                let t = ((now - column.anim_start) / COLUMN_ANIM_DURATION).clamp(0.0, 1.0);
                let eased = theme::ease_in_out(t as f32);
                column.display_width = if t >= 1.0 {
                    column.anim_to
                } else {
                    column.anim_from + (column.anim_to - column.anim_from) * eased
                };
            }
            return viewport_width;
        }

        let usable = (viewport_width - handle_total).max(1.0);
        let weight_total: f32 = visible
            .iter()
            .map(|index| self.columns[*index].weight.max(0.001))
            .sum();
        let mut targets: Vec<f32> = visible
            .iter()
            .map(|index| (usable * self.columns[*index].weight.max(0.001) / weight_total).max(1.0))
            .collect();
        let sum: f32 = targets.iter().sum();
        if (sum - usable).abs() > 0.5
            && let Some((slot, _)) = targets
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        {
            targets[slot] = (targets[slot] + (usable - sum)).max(1.0);
        }

        // Never let a column collapse to the point where its rounded border
        // disappears. If the window is too small for the minimums, keep them
        // and let the rightmost columns be clipped rather than shrinking.
        let min_total = COLUMN_MIN_WIDTH * visible.len() as f32;
        if usable < min_total {
            targets.fill(COLUMN_MIN_WIDTH);
        } else {
            for _ in 0..visible.len() {
                let deficit: f32 = targets
                    .iter()
                    .map(|target| (COLUMN_MIN_WIDTH - *target).max(0.0))
                    .sum();
                if deficit <= 0.5 {
                    break;
                }
                let flexible: f32 = targets
                    .iter()
                    .map(|target| (*target - COLUMN_MIN_WIDTH).max(0.0))
                    .sum();
                if flexible <= 0.5 {
                    break;
                }
                for target in &mut targets {
                    if *target < COLUMN_MIN_WIDTH {
                        *target = COLUMN_MIN_WIDTH;
                    } else {
                        *target -= deficit * (*target - COLUMN_MIN_WIDTH) / flexible;
                    }
                }
            }
        }

        let initialise = !self.columns_initialised;
        for (slot, &index) in visible.iter().enumerate() {
            let target = targets[slot];
            let column = &mut self.columns[index];
            if initialise {
                column.width = target;
                column.display_width = target;
                column.anim_from = target;
                column.anim_to = target;
                column.anim_start = now;
            } else if (column.width - target).abs() > 0.5 {
                column.width = target;
                column.anim_from = column.display_width;
                column.anim_to = target;
                column.anim_start = now;
            } else if column.anim_start <= 0.0 {
                column.anim_start = now;
                column.anim_from = column.display_width;
                column.anim_to = target;
            }
        }
        self.columns_initialised = true;

        const DURATION: f64 = COLUMN_ANIM_DURATION;
        for &index in visible {
            let column = &mut self.columns[index];
            if column.anim_start <= 0.0 {
                column.anim_start = now;
            }
            let t = ((now - column.anim_start) / DURATION).clamp(0.0, 1.0);
            let eased = theme::ease_in_out(t as f32);
            column.display_width = if t >= 1.0 {
                column.anim_to
            } else {
                column.anim_from + (column.anim_to - column.anim_from) * eased
            };
        }

        (targets.iter().sum::<f32>() + handle_total).max(viewport_width)
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
            RosteringAction::OpenContact(id) => {
                self.open_contact(&id);
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
                self.reset_column_weights();
                if self.active_contact.as_deref() == Some(id) {
                    self.active_contact = self.docked_contact_ids().first().cloned();
                }
            }
            ContactEditorAction::Dock => {
                if let Some(window) = self.contact_windows.get_mut(id) {
                    window.popped_out = false;
                    window.close_requested = false;
                }
                self.reset_column_weights();
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
            ContactEditorAction::ChangeRelationship {
                id: edge_id,
                change,
            } => {
                self.change_relationship(id, edge_id, change);
            }
            ContactEditorAction::LoadRelationshipHistory(edge_id) => {
                self.load_relationship_history(id, edge_id);
            }
            ContactEditorAction::OpenRelationshipExplorer { kind, id, label } => {
                self.open_relationship_explorer(kind, &id, &label);
            }
            ContactEditorAction::SetConstellationPreference(preference) => {
                let preference = preference.normalised();
                // The setting belongs to the signed-in user, so every open
                // record follows it rather than drifting apart.
                for window in self.contact_windows.values_mut() {
                    window.constellation = preference;
                }
                if let Err(error) = self.save_constellation_preference(id) {
                    self.notify(
                        ctx,
                        format!("The constellation setting could not be saved: {error}"),
                    );
                }
            }
            ContactEditorAction::ReloadConstellation => {
                if self.party_catalog_request.is_none() {
                    self.relationship_graph_loaded = false;
                    self.relationship_graph_error = None;
                    self.note_relationship_network_changed();
                    self.ensure_party_catalog();
                }
            }
            ContactEditorAction::CreateContact {
                from_role,
                to_role,
                notes,
                draft,
            } => {
                self.create_relationship_contact(ctx, id, from_role, to_role, notes, draft);
            }
            ContactEditorAction::SetQualityValue(payload) => {
                self.set_contact_quality(id, payload);
            }
            ContactEditorAction::DeleteQualityValue(quality_id) => {
                self.delete_contact_quality(id, quality_id);
            }
            ContactEditorAction::SetRequirement(payload) => {
                self.set_contact_requirement(id, payload);
            }
            ContactEditorAction::DeleteRequirement(requirement_id) => {
                self.delete_contact_requirement(id, requirement_id);
            }
            ContactEditorAction::AddFundingPackage(payload) => {
                self.add_contact_funding_package(id, payload);
            }
            ContactEditorAction::AddAgreement(payload) => {
                self.add_contact_agreement(id, payload);
            }
            ContactEditorAction::AddAgreementLine {
                agreement_id,
                payload,
            } => {
                self.add_contact_agreement_line(id, agreement_id, payload);
            }
            ContactEditorAction::DeleteAgreementLine {
                agreement_id,
                line_id,
            } => {
                self.delete_contact_agreement_line(id, agreement_id, line_id);
            }
            ContactEditorAction::FinaliseAgreement(agreement_id) => {
                self.finalise_contact_agreement(id, agreement_id);
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
            if let Some(window_id) = &window_id
                && let Some(window) = self.contact_windows.get_mut(window_id)
            {
                window.relationship_failed("Sign in to link relationships.".to_owned());
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
                if let Some(window_id) = &window_id
                    && let Some(window) = self.contact_windows.get_mut(window_id)
                {
                    window
                        .relationship_failed("The server connection is not available.".to_owned());
                }
            }
        }
        let _ = ctx;
    }

    fn change_relationship(
        &mut self,
        window_id: &str,
        edge_id: String,
        change: RelationshipChange,
    ) {
        let Some(token) = self.access_token() else {
            if let Some(window) = self.contact_windows.get_mut(window_id) {
                window.relationship_change_failed("Sign in to edit relationships.".to_owned());
            }
            return;
        };
        let request = self.next_relationship_request();
        match &self.remote {
            Some(remote) => {
                self.relationship_change_requests
                    .insert(request, window_id.to_owned());
                remote.change_relationship(
                    request,
                    self.server.base_url.clone(),
                    token,
                    edge_id,
                    change,
                );
            }
            None => {
                if let Some(window) = self.contact_windows.get_mut(window_id) {
                    window.relationship_change_failed(
                        "The server connection is not available.".to_owned(),
                    );
                }
            }
        }
    }

    fn load_relationship_history(&mut self, window_id: &str, edge_id: String) {
        let Some(token) = self.access_token() else {
            return;
        };
        let request = self.next_relationship_request();
        if let Some(remote) = &self.remote {
            self.relationship_history_requests
                .insert(request, window_id.to_owned());
            remote.relationship_history(request, self.server.base_url.clone(), token, edge_id);
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

        let payload = relationship_contact_write(&draft);

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

    fn create_intake_relationship_contact(
        &mut self,
        ctx: &egui::Context,
        slot: String,
        role: RoleOption,
        notes: String,
        draft: NewRelationshipContact,
    ) {
        let Some(token) = self.access_token() else {
            self.intake.relationship_pending = false;
            self.intake.relationship_error = Some("Sign in to create a contact record.".to_owned());
            return;
        };
        if self.remote.is_none() {
            self.intake.relationship_pending = false;
            self.intake.relationship_error =
                Some("The server connection is not available.".to_owned());
            return;
        }

        let payload = relationship_contact_write(&draft);
        let request = self.next_relationship_request();
        self.intake_contact_creates
            .insert(request, PendingIntakeContact { slot, role, notes });
        self.intake.relationship_pending = true;
        self.intake.relationship_error = None;
        if let Some(remote) = &self.remote {
            remote.create_contact(request, self.server.base_url.clone(), token, payload);
        }
        let _ = ctx;
    }

    /// The constellation's immutable snapshot of the relationship graph. It is
    /// rebuilt only when the revision changes, so rendering does not clone the
    /// full tenant graph every frame.
    fn relationship_network(&mut self) -> Arc<RelationshipNetwork> {
        if self
            .relationship_network
            .as_ref()
            .is_none_or(|network| network.revision != self.relationship_graph_revision)
        {
            self.relationship_network = Some(Arc::new(RelationshipNetwork {
                edges: self.relationship_edges.clone(),
                revision: self.relationship_graph_revision,
            }));
        }
        self.relationship_network
            .clone()
            .expect("the relationship network was just built")
    }

    fn note_relationship_network_changed(&mut self) {
        self.relationship_graph_revision = self.relationship_graph_revision.wrapping_add(1);
    }

    fn constellation_key(&self) -> Option<String> {
        let session = self.server.session.as_ref()?;
        Some(relationship_view_key(
            &self.server.base_url,
            &session.auth.organisation.id,
            &session.auth.user.id,
        ))
    }

    fn constellation_preference(&self) -> RelationshipViewPreference {
        self.constellation_key()
            .map(|key| self.config.relationship_view(&key))
            .unwrap_or_default()
    }

    fn explorer_preference(&self) -> ExplorerPreference {
        self.constellation_key()
            .map(|key| self.config.explorer_view(&key))
            .unwrap_or_default()
    }

    /// Load the signed-in account's saved relationship view settings into the
    /// open record and the explorer once the session is known.
    fn apply_relationship_preferences(&mut self) {
        let preference = self.constellation_preference();
        for window in self.contact_windows.values_mut() {
            window.constellation = preference;
        }
        let explorer = self.explorer_preference();
        self.relationship_explorer.apply_preference(explorer);
    }

    fn save_constellation_preference(&mut self, id: &str) -> Result<(), String> {
        let Some(key) = self.constellation_key() else {
            return Ok(());
        };
        let Some(window) = self.contact_windows.get(id) else {
            return Ok(());
        };
        self.config
            .set_relationship_view(key, window.constellation.normalised());
        self.config.save()
    }

    fn save_explorer_preference(&mut self) -> Result<(), String> {
        let Some(key) = self.constellation_key() else {
            return Ok(());
        };
        self.config
            .set_explorer_view(key, self.relationship_explorer.preference());
        self.config.save()
    }

    fn handle_explorer_action(&mut self, ctx: &egui::Context, action: ExplorerAction) {
        match action {
            ExplorerAction::OpenContact(id) => self.open_contact(&id),
            ExplorerAction::Reload => {
                if self.party_catalog_request.is_none() {
                    self.relationship_graph_loaded = false;
                    self.relationship_graph_error = None;
                    self.note_relationship_network_changed();
                    self.ensure_party_catalog();
                }
            }
            ExplorerAction::PreferenceChanged => {
                if let Err(error) = self.save_explorer_preference() {
                    self.notify(
                        ctx,
                        format!("The relationship explorer settings could not be saved: {error}"),
                    );
                }
            }
        }
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
            self.note_relationship_network_changed();
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
            let catalog = self.lookup_parties();
            let roles = self.role_options.clone();
            let action = ctx.show_viewport_immediate(intake_viewport_id(), builder, |ui, _| {
                pages::intake::show_wizard_viewport(
                    ui,
                    &mut self.intake,
                    client_side,
                    &catalog,
                    &roles,
                )
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

    /// Set the constellation's collapsed state and step count on every open
    /// record without writing the user's saved preference. Used by headless
    /// screenshot builds so previews are deterministic.
    pub fn preview_constellation(&mut self, collapsed: bool, steps: u8) {
        let preference = RelationshipViewPreference {
            collapsed,
            steps,
            skip_hubs: true,
        }
        .normalised();
        for window in self.contact_windows.values_mut() {
            window.constellation = preference;
        }
    }

    /// Open the relationship explorer on a party and bring the page forward.
    pub fn open_relationship_explorer(&mut self, kind: PartyKind, id: &str, label: &str) {
        self.relationship_explorer.focus_on(kind, id, label);
        self.ensure_party_catalog();
        self.open_page(Page::Relationships);
    }

    /// The party the explorer is focused on, for deep-link and UI tests.
    pub fn relationship_explorer_focus(&self) -> Option<(PartyKind, String)> {
        self.relationship_explorer
            .current_focus()
            .map(|focus| (focus.kind, focus.id.clone()))
    }

    /// Focus the first directory party whose name contains `query`. Used by
    /// headless screenshot runs, where the live ids are not known in advance.
    pub fn focus_explorer_by_label(&mut self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        let mut parties = self.lookup_parties();
        parties.sort_by_key(|party| party.label.to_lowercase());
        let Some(party) = parties
            .into_iter()
            .find(|party| party.label.to_lowercase().contains(&query))
        else {
            return false;
        };
        self.open_relationship_explorer(party.kind, &party.id, &party.label);
        true
    }

    /// Select a party in the explorer without moving the focus. Used by
    /// headless screenshot runs to preview the route back to the focus.
    pub fn preview_explorer_selection(&mut self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        let Some(party) = self
            .lookup_parties()
            .into_iter()
            .find(|party| party.label.to_lowercase().contains(&query))
        else {
            return false;
        };
        self.relationship_explorer
            .select_party(party.kind, &party.id);
        true
    }

    /// Set the explorer's levels without writing the saved preference. Used by
    /// headless screenshot builds so previews are deterministic.
    pub fn preview_explorer_levels(&mut self, levels: u8) {
        let mut preference = self.relationship_explorer.preference();
        preference.levels = levels;
        self.relationship_explorer.apply_preference(preference);
    }

    pub fn contact_editor_ready(&self) -> bool {
        self.contact_windows.values().any(|window| !window.loading)
    }

    /// True once the external organisation list has loaded.
    pub fn organisations_ready(&self) -> bool {
        self.organisations.loaded_once && !self.organisations.loading
    }

    /// Number of external organisations currently listed.
    pub fn organisation_count(&self) -> usize {
        self.organisations.items.len()
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

        if !self.signed_in() {
            // Discard in-flight responses instead of populating hidden state
            // after sign-out. A fresh query runs once the user signs in again.
            return;
        }

        for event in events {
            match event {
                RemoteEvent::IntakeFlowsLoaded { request_id, result } => {
                    if self.intake_board.flows_request_id != Some(request_id) {
                        continue;
                    }
                    self.intake_board.flows_request_id = None;
                    self.intake_board.flows_loading = false;
                    self.intake_board.flows_requested = false;
                    match result {
                        Ok(list) => {
                            self.intake_board.flows = list.items;
                            self.intake_board.flows_error = None;
                        }
                        Err(error) => {
                            self.intake_board.flows_error = Some(error);
                        }
                    }
                }
                RemoteEvent::IntakeFieldsLoaded { request_id, result } => {
                    if self.intake_board.fields_request_id != Some(request_id) {
                        continue;
                    }
                    self.intake_board.fields_request_id = None;
                    self.intake_board.fields_loading = false;
                    match result {
                        Ok(catalog) => {
                            self.intake_board.fields = catalog.fields;
                            self.intake_board.fields_error = None;
                            let fields = self.intake_board.fields.clone();
                            for state in self.intake_case_columns.values_mut() {
                                if !state.dirty {
                                    state.load_drafts(&fields);
                                }
                            }
                        }
                        Err(error) => {
                            self.intake_board.fields_error = Some(error);
                        }
                    }
                }
                RemoteEvent::IntakeContactsMatched { request_id, result } => {
                    if self.intake_board.contacts_request_id != Some(request_id) {
                        continue;
                    }
                    self.intake_board.contacts_request_id = None;
                    match result {
                        Ok(list) => {
                            self.intake_board.new_candidates = list.candidates;
                            self.intake_board.new_error = None;
                        }
                        Err(error) => {
                            self.intake_board.new_error = Some(error);
                        }
                    }
                }
                RemoteEvent::IntakeCaseCreated { request_id, result } => {
                    if self.intake_board.new_request_id != Some(request_id) {
                        continue;
                    }
                    self.intake_board.new_request_id = None;
                    self.intake_board.new_saving = false;
                    match result {
                        Ok(response) => {
                            let detail = response.case;
                            let case_id = detail.case.id.clone();
                            self.intake_board.new_open = false;
                            self.intake_board.new_error = None;
                            self.intake_board.new_candidates.clear();
                            self.intake_board.new_contact_id = None;
                            let fields = self.intake_board.fields.clone();
                            let mut state =
                                pages::intake::board::IntakeCaseColumnState::new(case_id.clone());
                            state.apply_case(detail, &fields);
                            self.intake_case_columns.insert(case_id.clone(), state);
                            self.open_intake_case(case_id);
                            self.intake_board.reset_board();
                            self.request_intake_board();
                        }
                        Err(error) => {
                            self.intake_board.new_error = Some(error);
                        }
                    }
                }
                RemoteEvent::IntakeCaseUpdated {
                    request_id,
                    case_id,
                    result,
                } => {
                    let matches = self
                        .intake_case_columns
                        .get(&case_id)
                        .is_some_and(|state| state.save_request_id == Some(request_id));
                    if !matches {
                        continue;
                    }
                    let fields = self.intake_board.fields.clone();
                    let case = self.intake_case_columns.get_mut(&case_id);
                    let Some(state) = case else {
                        continue;
                    };
                    state.saving = false;
                    state.save_request_id = None;
                    match result {
                        Ok(detail) => {
                            state.apply_case(detail, &fields);
                            self.intake_board.reset_board();
                            self.request_intake_board();
                        }
                        Err(error) => {
                            state.save_error = Some(error);
                        }
                    }
                }
                RemoteEvent::IntakeChecklistToggled {
                    request_id,
                    case_id,
                    result,
                } => {
                    let matches = self
                        .intake_case_columns
                        .get(&case_id)
                        .is_some_and(|state| state.checklist_request_id == Some(request_id));
                    if !matches {
                        continue;
                    }
                    let state = self.intake_case_columns.get_mut(&case_id);
                    let Some(state) = state else {
                        continue;
                    };
                    state.checklist_pending = false;
                    state.checklist_request_id = None;
                    match result {
                        Ok(response) => {
                            state.save_error = None;
                            state.checklist_error = None;
                            if let Some(case) = state.case.as_mut() {
                                let item_id = response.item.id.clone();
                                if let Some(item) =
                                    case.checklist.iter_mut().find(|item| item.id == item_id)
                                {
                                    *item = response.item;
                                }
                                case.case.checklist = response.checklist;
                            }
                            self.intake_board.reset_board();
                            self.request_intake_board();
                        }
                        Err(error) => {
                            state.checklist_error = Some(error);
                        }
                    }
                }
                RemoteEvent::IntakeCaseCancelled {
                    request_id,
                    case_id,
                    result,
                } => {
                    let matches = self
                        .intake_case_columns
                        .get(&case_id)
                        .is_some_and(|state| state.cancel_request_id == Some(request_id));
                    if !matches {
                        continue;
                    }
                    let fields = self.intake_board.fields.clone();
                    let state = self.intake_case_columns.get_mut(&case_id);
                    let Some(state) = state else {
                        continue;
                    };
                    state.cancel_saving = false;
                    state.cancel_request_id = None;
                    match result {
                        Ok(detail) => {
                            state.apply_case(detail, &fields);
                            self.intake_board.reset_board();
                            self.request_intake_board();
                            self.notify(ctx, "Intake case cancelled.");
                        }
                        Err(error) => {
                            state.cancel_error = Some(error);
                        }
                    }
                }
                RemoteEvent::IntakeCaseDeleted {
                    request_id,
                    case_id,
                    result,
                } => {
                    if self.discard_intake_request != Some(request_id) {
                        continue;
                    }
                    self.discard_intake_request = None;
                    match result {
                        Ok(()) => {
                            self.discard_intake_case = None;
                            self.discard_intake_error = None;
                            self.force_close_intake_case(&case_id);
                            self.intake_board.reset_board();
                            self.request_intake_board();
                            self.notify(ctx, "Draft intake deleted.");
                        }
                        Err(error) => {
                            self.discard_intake_error = Some(error);
                        }
                    }
                }
                RemoteEvent::IntakeBoardLoaded { request_id, result } => {
                    if self.intake_board.board_request_id != Some(request_id) {
                        continue;
                    }
                    self.intake_board.board_request_id = None;
                    self.intake_board.board_loading = false;
                    self.intake_board.board_requested = false;
                    match result {
                        Ok(board) => {
                            self.intake_board.board = Some(board);
                            self.intake_board.board_error = None;
                        }
                        Err(error) => {
                            self.intake_board.board_error = Some(error);
                        }
                    }
                }
                RemoteEvent::IntakeCaseLoaded {
                    request_id,
                    case_id,
                    result,
                } => {
                    let matches = self
                        .intake_case_columns
                        .get(&case_id)
                        .is_some_and(|state| state.request_id == Some(request_id));
                    if !matches {
                        continue;
                    }
                    if let Some(state) = self.intake_case_columns.get_mut(&case_id) {
                        state.request_id = None;
                        state.loading = false;
                        state.requested = false;
                    }
                    match result {
                        Ok(case) => {
                            let fields = self.intake_board.fields.clone();
                            if let Some(state) = self.intake_case_columns.get_mut(&case_id) {
                                state.apply_case(case, &fields);
                            }
                        }
                        Err(error) => {
                            if let Some(state) = self.intake_case_columns.get_mut(&case_id) {
                                state.error = Some(error);
                            }
                        }
                    }
                }
                RemoteEvent::IntakeCaseTransitioned {
                    request_id,
                    case_id,
                    result,
                } => {
                    let matches = self
                        .intake_case_columns
                        .get(&case_id)
                        .is_some_and(|state| state.transition_request_id == Some(request_id));
                    if !matches {
                        continue;
                    }
                    self.intake_board.reset_board();
                    match result {
                        Ok(case) => {
                            let fields = self.intake_board.fields.clone();
                            if let Some(state) = self.intake_case_columns.get_mut(&case_id) {
                                state.apply_case(case, &fields);
                            }
                            self.request_intake_board();
                        }
                        Err(error) => {
                            if let Some(state) = self.intake_case_columns.get_mut(&case_id) {
                                state.transition_error = Some(error);
                            }
                        }
                    }
                    if let Some(state) = self.intake_case_columns.get_mut(&case_id) {
                        state.transition_request_id = None;
                        state.transitioning = false;
                    }
                }
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
                RemoteEvent::ContactQualitySaved {
                    contact_id, result, ..
                } => {
                    if let Some(window) = self.contact_windows.get_mut(&contact_id) {
                        match result {
                            Ok(value) => window.quality_value_saved(value),
                            Err(error) => window.quality_value_failed(error),
                        }
                    }
                }
                RemoteEvent::ContactQualityDeleted {
                    contact_id,
                    quality_id,
                    result,
                    ..
                } => {
                    if let Some(window) = self.contact_windows.get_mut(&contact_id) {
                        match result {
                            Ok(()) => window.quality_value_deleted(&quality_id),
                            Err(error) => window.quality_value_failed(error),
                        }
                    }
                }
                RemoteEvent::ContactRequirementSaved {
                    contact_id, result, ..
                } => {
                    if let Some(window) = self.contact_windows.get_mut(&contact_id) {
                        match result {
                            Ok(requirement) => window.requirement_saved(requirement),
                            Err(error) => window.requirement_failed(error),
                        }
                    }
                }
                RemoteEvent::ContactRequirementDeleted {
                    contact_id,
                    requirement_id,
                    result,
                    ..
                } => {
                    if let Some(window) = self.contact_windows.get_mut(&contact_id) {
                        match result {
                            Ok(()) => window.requirement_deleted(&requirement_id),
                            Err(error) => window.requirement_failed(error),
                        }
                    }
                }
                RemoteEvent::FundingPackageSaved {
                    contact_id, result, ..
                } => {
                    if let Some(window) = self.contact_windows.get_mut(&contact_id) {
                        match result {
                            Ok(package) => window.funding_package_saved(package),
                            Err(error) => window.funding_failed(error),
                        }
                    }
                }
                RemoteEvent::AgreementSaved {
                    contact_id, result, ..
                } => {
                    if let Some(window) = self.contact_windows.get_mut(&contact_id) {
                        match result {
                            Ok(agreement) => window.agreement_saved(agreement),
                            Err(error) => window.funding_failed(error),
                        }
                    }
                }
                RemoteEvent::AgreementLineSaved {
                    contact_id,
                    agreement_id,
                    result,
                    ..
                } => {
                    if let Some(window) = self.contact_windows.get_mut(&contact_id) {
                        match result {
                            Ok(line) => window.agreement_line_saved(&agreement_id, line),
                            Err(error) => window.funding_failed(error),
                        }
                    }
                }
                RemoteEvent::AgreementLineDeleted {
                    contact_id,
                    agreement_id,
                    line_id,
                    result,
                    ..
                } => {
                    if let Some(window) = self.contact_windows.get_mut(&contact_id) {
                        match result {
                            Ok(()) => window.agreement_line_deleted(&agreement_id, &line_id),
                            Err(error) => window.funding_failed(error),
                        }
                    }
                }
                RemoteEvent::AgreementFinalised {
                    contact_id, result, ..
                } => {
                    if let Some(window) = self.contact_windows.get_mut(&contact_id) {
                        match result {
                            Ok(finalised) => {
                                window.agreement_finalised(
                                    finalised.agreement,
                                    finalised.generated_shifts,
                                );
                            }
                            Err(error) => window.funding_failed(error),
                        }
                    }
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
                RemoteEvent::OrganisationsListed { request_id, result } => {
                    self.handle_organisations_listed(request_id, result);
                }
                RemoteEvent::OrganisationSaved { request_id, result } => {
                    self.handle_organisation_saved(ctx, request_id, result);
                }
                RemoteEvent::OrganisationDeleted {
                    request_id,
                    id,
                    result,
                } => {
                    self.handle_organisation_deleted(ctx, request_id, id, result);
                }
                RemoteEvent::RelationshipCreated { request_id, result } => {
                    self.handle_relationship_created(ctx, request_id, result);
                }
                RemoteEvent::RelationshipChanged {
                    request_id,
                    id,
                    result,
                } => {
                    self.handle_relationship_changed(ctx, request_id, id, result);
                }
                RemoteEvent::RelationshipTypesLoaded { request_id, result } => {
                    if self.relationship_types_request == Some(request_id) {
                        self.relationship_types_request = None;
                        match result {
                            Ok(types) => {
                                self.role_options = role_options(&types);
                                self.relationship_types = types;
                                self.relationship_types_org = self
                                    .server
                                    .session
                                    .as_ref()
                                    .map(|session| session.auth.organisation.id.clone());
                            }
                            Err(_) => {
                                // Pickers stay empty; signing in again retries.
                                self.relationship_types_org = self
                                    .server
                                    .session
                                    .as_ref()
                                    .map(|session| session.auth.organisation.id.clone());
                            }
                        }
                    }
                }
                RemoteEvent::RelationshipHistoryLoaded {
                    request_id,
                    id,
                    result,
                } => {
                    if let Some(window_id) = self.relationship_history_requests.remove(&request_id)
                        && let Some(window) = self.contact_windows.get_mut(&window_id)
                    {
                        window.history_loaded(&id, result);
                    }
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

    fn handle_organisations_listed(
        &mut self,
        request_id: u64,
        result: Result<crate::organisation::ApiOrganisationList, String>,
    ) {
        if self.organisation_list_request != Some(request_id) {
            return;
        }
        self.organisation_list_request = None;
        if let Some(started) = self.organisation_query_started.take() {
            self.organisations.last_query_ms = Some(started.elapsed().as_secs_f32() * 1000.0);
        }
        match result {
            Ok(list) => self.organisations.apply_list(list.items, list.total),
            Err(error) => self.organisations.list_failed(error),
        }
    }

    fn handle_organisation_saved(
        &mut self,
        ctx: &egui::Context,
        request_id: u64,
        result: Result<ApiOrganisation, String>,
    ) {
        let Some((key, update_id)) = self.organisation_save_requests.remove(&request_id) else {
            return;
        };
        match result {
            Ok(organisation) => {
                let name = organisation.name.clone();
                self.update_organisation_catalog(&organisation);
                if let Some(editor) = self.organisation_editors.get_mut(&key) {
                    editor.apply_saved(organisation.clone());
                }
                self.organisations.saved(organisation);
                self.notify(
                    ctx,
                    if update_id.is_some() {
                        format!("{name} updated.")
                    } else {
                        format!("{name} created.")
                    },
                );
            }
            Err(error) => {
                if let Some(editor) = self.organisation_editors.get_mut(&key) {
                    editor.save_failed(error);
                }
            }
        }
    }

    fn handle_organisation_deleted(
        &mut self,
        ctx: &egui::Context,
        request_id: u64,
        _id: String,
        result: Result<(), String>,
    ) {
        let Some((id, editor_key)) = self.organisation_delete_requests.remove(&request_id) else {
            return;
        };
        match result {
            Ok(()) => {
                let remove = self.organisations.status
                    == crate::organisation::OrganisationStatusFilter::Active;
                self.organisations.deleted(&id, remove);
                let editors: Vec<String> = self
                    .organisation_editors
                    .iter()
                    .filter(|(_, editor)| editor.record_id.as_deref() == Some(id.as_str()))
                    .map(|(key, _)| key.clone())
                    .collect();
                for key in editors {
                    self.close_organisation_editor(&key);
                }
                self.notify(ctx, "Organisation deleted.");
            }
            Err(error) => {
                if let Some(key) = editor_key
                    && let Some(editor) = self.organisation_editors.get_mut(&key)
                {
                    editor.delete_failed(error);
                }
            }
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
                self.relationship_graph_error = None;
                self.note_relationship_network_changed();
            }
            Err(error) => {
                self.relationship_graph_error = Some(error.clone());
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

    fn link_intake_relationships(
        &mut self,
        contact_id: &str,
        relationships: BTreeMap<String, IntakeRelationship>,
    ) {
        let Some(session) = &self.server.session else {
            return;
        };
        let token = session.auth.access_token.clone();
        if self.remote.is_none() {
            return;
        }

        for relationship in relationships.into_values() {
            if relationship.kind == PartyKind::Contact && relationship.id == contact_id {
                continue;
            }
            if relationship.subject_role.is_empty() || relationship.linked_role.is_empty() {
                continue;
            }
            let request = self.next_relationship_request();
            self.intake_relationship_links.insert(
                request,
                format!(
                    "{} as {}",
                    relationship.name,
                    relationship_role_label(&relationship.linked_role)
                ),
            );
            if let Some(remote) = &self.remote {
                remote.create_relationship(
                    request,
                    self.server.base_url.clone(),
                    token.clone(),
                    ApiRelationshipWrite {
                        from_kind: PartyKind::Contact,
                        from_id: contact_id.to_owned(),
                        from_role: relationship.subject_role,
                        to_kind: relationship.kind,
                        to_id: relationship.id,
                        to_role: relationship.linked_role,
                        start_date: None,
                        end_date: None,
                        notes: (!relationship.notes.trim().is_empty())
                            .then(|| relationship.notes.trim().to_owned()),
                    },
                );
            }
        }
    }

    fn handle_relationship_created(
        &mut self,
        ctx: &egui::Context,
        request_id: u64,
        result: Result<ApiRelationship, String>,
    ) {
        if let Some(label) = self.intake_relationship_links.remove(&request_id) {
            match result {
                Ok(edge) => {
                    if !self
                        .relationship_edges
                        .iter()
                        .any(|existing| existing.id == edge.id)
                    {
                        self.relationship_edges.push(edge);
                        self.note_relationship_network_changed();
                    }
                    self.refresh_contact_lists();
                    self.send_contacts_query(false);
                    self.notify(ctx, format!("Linked {label}."));
                }
                Err(error) => self.notify(ctx, format!("Could not link {label}: {error}")),
            }
            return;
        }

        if let Some(name) = self.intake_role_links.remove(&request_id) {
            match result {
                Ok(edge) => {
                    self.relationship_edges.push(edge);
                    self.note_relationship_network_changed();
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
                    self.note_relationship_network_changed();
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
                self.notify(ctx, "Relationship linked.");
            }
            Err(error) => {
                if let Some(window_id) = &window_id
                    && let Some(window) = self.contact_windows.get_mut(window_id)
                {
                    window.relationship_failed(error);
                }
            }
        }
    }

    fn handle_relationship_changed(
        &mut self,
        ctx: &egui::Context,
        request_id: u64,
        id: String,
        result: Result<(), String>,
    ) {
        let Some(window_id) = self.relationship_change_requests.remove(&request_id) else {
            return;
        };
        match result {
            Ok(()) => {
                if let Some(window) = self.contact_windows.get_mut(&window_id) {
                    window.relationship_changed();
                }
                // Both ends of the relationship may be open, and the change can
                // alter who counts as a client or worker.
                let open: Vec<String> = self.contact_windows.keys().cloned().collect();
                for contact_id in open {
                    self.load_contact_relationships(&contact_id);
                }
                self.relationship_edges.retain(|edge| edge.id != id);
                self.note_relationship_network_changed();
                self.relationship_graph_loaded = false;
                self.party_catalog_request = None;
                self.ensure_party_catalog();
                self.refresh_contact_lists();
                self.send_contacts_query(false);
                self.notify(ctx, "Relationship updated.");
            }
            Err(error) => {
                if let Some(window) = self.contact_windows.get_mut(&window_id) {
                    window.relationship_change_failed(error);
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
        if let Some(pending) = self.intake_contact_creates.remove(&request_id) {
            self.intake.relationship_pending = false;
            match result {
                Ok(detail) => {
                    let contact_id = detail.contact.id.clone();
                    let name = detail.contact.display_name.clone();
                    self.remember_contact(&detail);
                    if let Some(draft) = &mut self.intake.draft {
                        draft.set_relationship(
                            &pending.slot,
                            IntakeRelationship {
                                subject_role: pending.role.subject_role,
                                linked_role: pending.role.linked_role,
                                kind: PartyKind::Contact,
                                id: contact_id,
                                name: name.clone(),
                                notes: pending.notes,
                            },
                        );
                    }
                    self.intake.new_relationship = None;
                    self.intake.relationship_error = None;
                    self.intake.mark_edited();
                    if self.intake.view != IntakeView::Wizard {
                        self.save_intake_now();
                    }
                    self.notify(
                        ctx,
                        format!("{name} created. It will be linked when the intake is completed."),
                    );
                }
                Err(error) => {
                    self.intake.relationship_error = Some(error);
                }
            }
            return;
        }

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
                let relationships = self
                    .intake
                    .draft
                    .as_ref()
                    .map(|draft| draft.relationships.clone())
                    .unwrap_or_default();
                self.link_intake_to_organisation(kind, &contact_id, &name);
                self.link_intake_relationships(&contact_id, relationships);
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
                        self.apply_relationship_preferences();
                        self.organisations.reset();
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
                        self.apply_relationship_preferences();
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
                        self.clear_authenticated_state(ctx);
                        self.settings.on_signed_out();
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
                self.clear_authenticated_state(ctx);
                self.settings.on_signed_out();
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
            Action::Navigate(page) => self.open_page(page),
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
                    self.clear_authenticated_state(ctx);
                    self.settings.on_signed_out();
                    self.notify(ctx, "Signed out.");
                }
            }
        }
    }

    /// Point this run at another server without saving it. Used by previews
    /// and tests; the saved setting is unchanged.
    pub fn use_server(&mut self, base_url: &str) -> Result<(), String> {
        let url = api::normalize_base_url(base_url)?;
        self.settings.base_url = url.clone();
        self.server.base_url = url;
        Ok(())
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
            self.clear_authenticated_state(ctx);
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
        let signed_in = self.signed_in();

        let top_bar = egui::Panel::top("top_bar")
            .exact_size(64.0)
            .show_separator_line(false)
            .frame(Frame::new().fill(theme::bg()).inner_margin(Margin {
                left: 20,
                right: 20,
                top: 0,
                bottom: 0,
            }))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
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
                    let (brand, _) = ui.allocate_exact_size(vec2(120.0, 34.0), Sense::hover());
                    let painter = ui.painter();
                    painter.text(
                        brand.left_center() + vec2(0.0, -8.0),
                        Align2::LEFT_CENTER,
                        "CareLibre",
                        theme::strong_font(16.0),
                        theme::text(),
                    );
                    painter.text(
                        brand.left_center() + vec2(0.0, 8.0),
                        Align2::LEFT_CENTER,
                        "CARE PLATFORM",
                        FontId::proportional(8.5),
                        theme::faint(),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // Search, intake, notifications and account controls are
                        // part of the signed-in workspace. A signed-out launch
                        // shows only the brand and the sign-in menu.
                        if !signed_in {
                            return;
                        }
                        match &account {
                            Some(account) => {
                                let response = theme::avatar(
                                    ui,
                                    &account.initials,
                                    32.0,
                                    theme::accent_blue(),
                                );
                                if response.clicked() {
                                    self.open_page(Page::Settings);
                                }
                                response.on_hover_text(format!(
                                    "{}\n{} · {}",
                                    account.name, account.organisation, account.role
                                ));
                            }
                            None => {
                                if ui.add(theme::secondary_button("Sign in")).clicked() {
                                    self.open_page(Page::Settings);
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

                        let close_all_target = if self.visible_column_count() > 1 {
                            1.0
                        } else {
                            0.0
                        };
                        let close_all_now = ui.ctx().input(|input| input.time);
                        if !self.close_all_initialised {
                            self.close_all_anim = close_all_target;
                            self.close_all_anim_from = close_all_target;
                            self.close_all_target = close_all_target;
                            self.close_all_anim_start = close_all_now;
                            self.close_all_initialised = true;
                        } else if (self.close_all_target - close_all_target).abs() > 0.001 {
                            self.close_all_anim_from = self.close_all_anim;
                            self.close_all_target = close_all_target;
                            self.close_all_anim_start = close_all_now;
                        }
                        let close_all_progress =
                            ((close_all_now - self.close_all_anim_start) / TOP_BAR_ANIM_DURATION)
                                .clamp(0.0, 1.0) as f32;
                        if close_all_progress < 1.0 {
                            ui.ctx().request_repaint();
                        }
                        let close_all_eased = theme::ease_in_out(close_all_progress);
                        self.close_all_anim = self.close_all_anim_from
                            + (self.close_all_target - self.close_all_anim_from) * close_all_eased;
                        let close_all = self.close_all_anim.clamp(0.0, 1.0);

                        if close_all > 0.001 {
                            ui.add_space(6.0 * close_all);

                            let label = "Close all";
                            let text_width = ui
                                .painter()
                                .layout_no_wrap(
                                    label.to_owned(),
                                    theme::strong_font(13.0),
                                    theme::text(),
                                )
                                .size()
                                .x;
                            let button_width = text_width + 24.0;
                            let (region, _) = ui.allocate_exact_size(
                                vec2(button_width * close_all, 34.0),
                                Sense::hover(),
                            );
                            let mut close_all_ui = ui.new_child(
                                egui::UiBuilder::new()
                                    .id_salt("close-all-button")
                                    .max_rect(region)
                                    .layout(Layout::right_to_left(Align::Center)),
                            );
                            close_all_ui.set_clip_rect(region);
                            close_all_ui.multiply_opacity(close_all);
                            let response = close_all_ui
                                .add_sized([button_width, 34.0], theme::secondary_button(label));
                            if response.clicked() {
                                self.close_all_columns(self.frame_time);
                            }
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
                            self.open_page(Page::Contacts);
                            self.show_contacts(ContactFilter::All, Some(query));
                        }
                    });
                });
            });

        ui.painter().hline(
            top_bar.response.rect.x_range(),
            top_bar.response.rect.bottom() - 0.5,
            Stroke::new(1.0, theme::border()),
        );
    }

    /// Collapse or expand the docked main menu and remember the choice.
    fn toggle_menu_collapsed(&mut self, ctx: &egui::Context) {
        self.config.menu_collapsed = !self.config.menu_collapsed;
        // Headless previews and UI tests never connect, so they must not write
        // to the developer's saved preferences.
        if self.app_ctx.is_some()
            && let Err(error) = self.config.save()
        {
            self.notify(ctx, format!("Could not save the menu preference: {error}"));
        }
        ctx.request_repaint();
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        if !self.signed_in() {
            self.sign_in_menu(ui);
            return;
        }

        let account = self.account_badge();
        let now = ui.ctx().input(|input| input.time);
        let full_width = ui.available_width().max(1.0);
        let docked = !self.columns.is_empty();
        let target = if !docked {
            full_width
        } else if self.config.menu_collapsed {
            SIDEBAR_RAIL_WIDTH.min(full_width)
        } else {
            SIDEBAR_WIDTH.min(full_width)
        };

        if !self.sidebar_initialised {
            self.sidebar_width = target;
            self.sidebar_target = target;
            self.sidebar_anim_from = target;
            self.sidebar_anim_start = now;
            self.sidebar_initialised = true;
        } else if (self.sidebar_target - target).abs() > 0.5 {
            self.sidebar_anim_from = self.sidebar_width;
            self.sidebar_target = target;
            self.sidebar_anim_start = now;
        }

        let progress =
            ((now - self.sidebar_anim_start) / SIDEBAR_ANIM_DURATION).clamp(0.0, 1.0) as f32;
        if progress < 1.0 {
            let eased = theme::ease_in_out(progress);
            self.sidebar_width =
                self.sidebar_anim_from + (self.sidebar_target - self.sidebar_anim_from) * eased;
            ui.ctx().request_repaint();
        } else {
            self.sidebar_width = self.sidebar_target;
        }

        let panel_width = self.sidebar_width.clamp(1.0, full_width);
        let centeredness =
            ((panel_width - SIDEBAR_WIDTH) / (full_width - SIDEBAR_WIDTH).max(1.0)).clamp(0.0, 1.0);
        // 0 at the icon rail, 1 at the full docked width.
        let rail = ((panel_width - SIDEBAR_RAIL_WIDTH)
            / (SIDEBAR_WIDTH - SIDEBAR_RAIL_WIDTH).max(1.0))
        .clamp(0.0, 1.0);
        let content_width = if panel_width < SIDEBAR_WIDTH {
            (panel_width - 24.0).max(24.0)
        } else {
            (SIDEBAR_WIDTH - 24.0) + (CENTERED_MENU_WIDTH - (SIDEBAR_WIDTH - 24.0)) * centeredness
        };
        let mut toggle_menu = false;

        egui::Panel::left("sidebar")
            .exact_size(panel_width)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(theme::sidebar()))
            .show(ui, |ui| {
                let panel_rect = ui.max_rect();
                ui.painter().vline(
                    panel_rect.right() - 0.5,
                    panel_rect.y_range(),
                    Stroke::new(1.0, theme::border()),
                );
                let panel_height = ui.available_height();
                let content_height = panel_height
                    + (CENTERED_MENU_HEIGHT.min(panel_height) - panel_height) * centeredness;
                let content_x = ((panel_width - content_width) / 2.0).max(0.0);
                let content_y = ((panel_height - content_height) / 2.0).max(0.0);
                let rect = egui::Rect::from_min_size(
                    ui.min_rect().min + vec2(content_x, content_y),
                    vec2(content_width, content_height),
                );
                let mut menu_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(rect)
                        .layout(Layout::top_down(Align::Min)),
                );
                menu_ui.set_clip_rect(rect);
                menu_ui.spacing_mut().item_spacing.y = 8.0 - 4.0 * centeredness;

                if docked {
                    menu_ui.add_space(12.0);
                    if menu_toggle(&mut menu_ui, self.config.menu_collapsed, rail).clicked() {
                        toggle_menu = true;
                    }
                }

                group_label(&mut menu_ui, "WORKSPACE", rail);
                if nav_button(
                    &mut menu_ui,
                    "📝",
                    "Intake",
                    "Enquiries, referrals and onboarding across configurable stages.",
                    rail,
                    rail,
                    38.0 + 8.0 * rail,
                )
                .clicked()
                {
                    self.open_intake();
                }
                for page in [
                    Page::Contacts,
                    Page::Relationships,
                    Page::Organisations,
                    Page::Rostering,
                ] {
                    if nav_item(&mut menu_ui, page, rail, rail).clicked() {
                        self.open_page(page);
                    }
                }

                menu_ui.add_space(14.0);
                group_label(&mut menu_ui, "OPERATIONS", rail);
                for page in [
                    Page::Services,
                    Page::Documents,
                    Page::Communications,
                    Page::Reports,
                ] {
                    if nav_item(&mut menu_ui, page, rail, rail).clicked() {
                        self.open_page(page);
                    }
                }

                if centeredness > 0.5 && nav_item(&mut menu_ui, Page::Settings, rail, 0.0).clicked()
                {
                    self.open_page(Page::Settings);
                }

                if centeredness < 0.98 {
                    menu_ui.scope(|ui| {
                        ui.multiply_opacity(1.0 - centeredness);
                        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                            ui.add_space(8.0);
                            if user_row(ui, account.as_ref(), rail) {
                                self.open_page(Page::Settings);
                            }
                            ui.add_space(8.0);
                            ui.separator();
                            ui.add_space(8.0);
                            if nav_item(ui, Page::Settings, rail, 0.0).clicked() {
                                self.open_page(Page::Settings);
                            }
                        });
                    });
                }
            });

        if toggle_menu {
            self.toggle_menu_collapsed(ui.ctx());
        }
    }

    /// Replace the main menu with the sign-in form while no account is
    /// signed in. The panel still fills the window, so the form is centred the
    /// same way the workspace menu is on launch.
    fn sign_in_menu(&mut self, ui: &mut Ui) {
        let full_width = ui.available_width().max(1.0);
        let mut action: Option<Action> = None;

        egui::Panel::left("sidebar")
            .exact_size(full_width)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(theme::sidebar()))
            .show(ui, |ui| {
                let panel_height = ui.available_height();
                let content_width = CENTERED_MENU_WIDTH.min((full_width - 48.0).max(280.0));
                let content_height = (panel_height - 32.0).max(200.0);
                let content_x = ((full_width - content_width) / 2.0).max(0.0);
                let content_y = ((panel_height - content_height) / 2.0).max(16.0);
                let rect = egui::Rect::from_min_size(
                    ui.min_rect().min + vec2(content_x, content_y),
                    vec2(content_width, content_height),
                );
                let mut menu_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt("sign-in-menu")
                        .max_rect(rect)
                        .layout(Layout::top_down(Align::Min)),
                );
                menu_ui.set_clip_rect(rect);

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(&mut menu_ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new("Sign in").font(theme::strong_font(26.0)));
                        ui.label(
                            RichText::new(
                                "Sign in to your CareLibre server to open the workspace.",
                            )
                            .color(theme::muted())
                            .size(13.0),
                        );
                        ui.add_space(16.0);

                        theme::card().show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            pages::settings::sign_in_form(
                                ui,
                                &mut self.settings,
                                &self.server,
                                &mut action,
                            );
                        });

                        ui.add_space(12.0);
                        self.server_menu(ui, &mut action);
                    });
            });

        if let Some(action) = action {
            self.handle_action(ui.ctx(), action);
        }
    }

    /// Server address controls for the signed-out sign-in screen. Without this
    /// the first-run user could not point the client at their own server,
    /// because settings is only available after signing in.
    fn server_menu(&mut self, ui: &mut Ui, action: &mut Option<Action>) {
        egui::CollapsingHeader::new(
            RichText::new(format!("Server · {}", self.server.base_url))
                .color(theme::muted())
                .size(10.5),
        )
        .id_salt("sign-in-server")
        .show(ui, |ui| {
            ui.label(
                RichText::new("CareLibre server URL")
                    .color(theme::muted())
                    .size(11.0),
            );
            ui.add_space(3.0);
            let edit = ui.add_sized(
                [ui.available_width(), 34.0],
                egui::TextEdit::singleline(&mut self.settings.base_url)
                    .hint_text("http://127.0.0.1:8484")
                    .margin(Margin::symmetric(10, 8)),
            );
            let pressed_enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add(theme::primary_button("Save & test")).clicked() || pressed_enter {
                    self.settings.error = None;
                    self.settings.notice = None;
                    *action = Some(Action::SaveServer {
                        base_url: self.settings.base_url.clone(),
                    });
                }
                if ui.add(theme::secondary_button("Test now")).clicked() {
                    *action = Some(Action::TestServer);
                }
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

        let status_bar = egui::Panel::bottom("status_bar")
            .exact_size(36.0)
            .show_separator_line(false)
            .frame(
                Frame::new()
                    .fill(theme::sidebar())
                    .inner_margin(Margin::symmetric(20, 4)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
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

        ui.painter().hline(
            status_bar.response.rect.x_range(),
            status_bar.response.rect.top() + 0.5,
            Stroke::new(1.0, theme::border()),
        );
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
        let exit = theme::ease_in_out((remaining.min(0.45) / 0.45).clamp(0.0, 1.0));
        let enter = theme::ease_in_out(
            (((now - notice.created_at) as f32) / NOTICE_ENTER_DURATION).clamp(0.0, 1.0),
        );
        let alpha = enter * exit;
        if enter < 1.0 || remaining < 0.45 {
            ctx.request_repaint();
        }
        let slide = (1.0 - enter) * 10.0;

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

fn column_icon_button(ui: &mut Ui, glyph: &str, tooltip: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
    let hovered = response.hovered();
    let fill = if hovered {
        theme::surface_raised()
    } else {
        theme::surface()
    };
    let stroke = if response.is_pointer_button_down_on() {
        theme::accent()
    } else {
        theme::border()
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(6),
        fill,
        Stroke::new(1.0, stroke),
        StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(12.5),
        if hovered {
            theme::text()
        } else {
            theme::muted()
        },
    );
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), tooltip));
    response.on_hover_text(tooltip)
}

fn group_label(ui: &mut Ui, text: &str, opacity: f32) {
    ui.add_space(4.0);
    ui.label(
        RichText::new(text)
            .color(theme::faint().gamma_multiply(opacity))
            .font(theme::strong_font(9.5)),
    );
    ui.add_space(4.0);
}

/// The chevron that collapses or expands the docked main menu.
fn menu_toggle(ui: &mut Ui, collapsed: bool, rail: f32) -> egui::Response {
    let label = if collapsed {
        "Expand menu"
    } else {
        "Collapse menu"
    };
    let glyph = if collapsed { "»" } else { "«" };
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    let x = rect.center().x + (rect.right() - 16.0 - rect.center().x) * rail;
    let hit = egui::Rect::from_center_size(egui::pos2(x, rect.center().y), vec2(32.0, 32.0));
    let response = ui.interact(hit, ui.scope_id().with("menu-toggle"), Sense::click());
    let hovered = response.hovered();

    ui.painter().rect(
        hit,
        CornerRadius::same(8),
        if hovered {
            theme::hover()
        } else {
            theme::surface_raised()
        },
        Stroke::new(
            1.0,
            if hovered {
                theme::accent()
            } else {
                theme::border()
            },
        ),
        StrokeKind::Inside,
    );

    ui.painter().text(
        hit.center(),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(16.0),
        theme::text(),
    );

    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    let shortcut = ui.ctx().format_shortcut(&egui::KeyboardShortcut::new(
        egui::Modifiers::COMMAND,
        egui::Key::B,
    ));
    bubble_tooltip(ui, &response, label, &format!("Shortcut: {shortcut}"), hit);
    response
}

/// An instant, bubble-styled tooltip anchored to `anchor`.
///
/// egui's built-in tooltip waits for the pointer to settle and points at the
/// widget's whole rect. The icon rail needs a tooltip that appears straight away
/// and points at the glyph, so draw one in the tooltip layer and add a small
/// tail on its left edge.
fn bubble_tooltip(
    ui: &Ui,
    response: &egui::Response,
    title: &str,
    detail: &str,
    anchor: egui::Rect,
) {
    if !response.hovered() {
        return;
    }

    let anchor = response
        .ctx
        .layer_transform_to_global(response.layer_id)
        .map_or(anchor, |transform| transform * anchor);

    let mut tooltip =
        egui::Tooltip::always_open(response.ctx.clone(), response.layer_id, response.id, anchor)
            .gap(6.0);
    tooltip.popup = tooltip
        .popup
        .align(egui::RectAlign::RIGHT)
        .align_alternatives(&[])
        .width(260.0)
        .frame(
            Frame::new()
                .fill(theme::surface())
                .stroke(Stroke::new(1.0, theme::border()))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::symmetric(12, 9))
                .shadow(egui::epaint::Shadow {
                    offset: [0, 2],
                    blur: 10,
                    spread: 0,
                    color: Color32::from_black_alpha(70),
                }),
        );

    let Some(shown) = tooltip.show(|ui| {
        ui.set_max_width(236.0);
        ui.label(
            RichText::new(title)
                .font(theme::strong_font(12.5))
                .color(theme::text()),
        );
        ui.add_space(1.0);
        ui.label(RichText::new(detail).color(theme::muted()).size(10.5));
    }) else {
        return;
    };

    let bubble = shown.response.rect;
    let painter = ui.ctx().layer_painter(shown.response.layer_id);
    let centre_y = bubble.center().y;
    let tip = egui::pos2(bubble.left() - 7.0, centre_y);
    let base_top = egui::pos2(bubble.left() + 1.0, centre_y - 6.0);
    let base_bottom = egui::pos2(bubble.left() + 1.0, centre_y + 6.0);
    painter.add(egui::Shape::convex_polygon(
        vec![tip, base_top, base_bottom],
        theme::surface(),
        Stroke::NONE,
    ));
    let edge = Stroke::new(1.0, theme::border());
    painter.line_segment([tip, base_top], edge);
    painter.line_segment([tip, base_bottom], edge);
}

fn nav_item(ui: &mut Ui, page: Page, rail: f32, detail: f32) -> egui::Response {
    let height = 38.0 + 8.0 * detail;
    nav_button(
        ui,
        page.icon(),
        page.label(),
        page.blurb(),
        rail,
        detail,
        height,
    )
}

fn nav_button(
    ui: &mut Ui,
    icon: &str,
    label: &str,
    tooltip: &str,
    rail: f32,
    detail: f32,
    height: f32,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());

    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(9), theme::surface_raised());
    }

    let icon_font = FontId::proportional(14.0);
    let icon_width = ui
        .painter()
        .layout_no_wrap(icon.to_owned(), icon_font.clone(), theme::faint())
        .size()
        .x;
    // Centre the glyph on the rail, and keep its left edge at 24 px when the
    // menu is expanded.
    let icon_x = rect.width() / 2.0 + (24.0 + icon_width / 2.0 - rect.width() / 2.0) * rail;
    let icon_center = rect.left_center() + vec2(icon_x, 0.0);

    ui.painter().text(
        icon_center,
        Align2::CENTER_CENTER,
        icon,
        icon_font,
        theme::faint(),
    );
    if rail > 0.01 {
        ui.painter().text(
            rect.left_center() + vec2(50.0, -5.0 * detail),
            Align2::LEFT_CENTER,
            label,
            if detail > 0.5 {
                theme::strong_font(13.0 + detail * 0.5)
            } else {
                FontId::proportional(13.0)
            },
            theme::muted().gamma_multiply(rail),
        );
    }
    if detail > 0.01 {
        ui.painter().text(
            rect.left_center() + vec2(50.0, 9.0),
            Align2::LEFT_CENTER,
            tooltip,
            FontId::proportional(10.0),
            theme::muted().gamma_multiply(detail),
        );
    }

    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));

    if rail < 0.5 {
        bubble_tooltip(
            ui,
            &response,
            label,
            tooltip,
            egui::Rect::from_center_size(icon_center, vec2(18.0, 18.0)),
        );
        response
    } else {
        response.on_hover_text(tooltip)
    }
}

fn user_row(ui: &mut Ui, account: Option<&AccountBadge>, rail: f32) -> bool {
    let mut clicked = false;
    let expanded = rail > 0.5;

    ui.horizontal(|ui| {
        if !expanded {
            ui.add_space(6.0);
        }
        match account {
            Some(account) => {
                let response = theme::avatar(ui, &account.initials, 28.0, theme::accent_blue());
                if response.clicked() {
                    clicked = true;
                }
                if expanded {
                    response.on_hover_text(format!(
                        "{}\n{} · {}",
                        account.name, account.organisation, account.role
                    ));
                } else {
                    bubble_tooltip(
                        ui,
                        &response,
                        &account.name,
                        &format!("{} · {}", capitalize(&account.role), account.organisation),
                        response.rect,
                    );
                }
                if expanded {
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
            }
            None => {
                let response = theme::avatar(ui, "?", 28.0, theme::surface_raised());
                if response.clicked() {
                    clicked = true;
                }
                if !expanded {
                    bubble_tooltip(
                        ui,
                        &response,
                        "Not signed in",
                        "Open settings to connect to your server.",
                        response.rect,
                    );
                }
                if expanded {
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
            }
        }
    });

    clicked
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

fn relationship_contact_write(draft: &NewRelationshipContact) -> ApiContactWrite {
    ApiContactWrite {
        given_name: optional_str(&draft.given_name),
        family_name: optional_str(&draft.family_name),
        preferred_name: None,
        pronouns: None,
        date_of_birth: None,
        gender: None,
        languages: Vec::new(),
        cultural_identity: None,
        email: optional_str(&draft.email),
        phone: optional_str(&draft.phone),
        notes: None,
        details: Value::Object(Default::default()),
        is_active: true,
        custom_fields: BTreeMap::new(),
    }
}

fn intake_write(draft: &crate::intake::IntakeDraft) -> ApiContactWrite {
    use serde_json::Value;

    let mut details = serde_json::Map::new();
    for (key, value) in &draft.fields {
        // Intrinsic profile data now has first-class contact fields.
        if matches!(key.as_str(), "gender" | "languages" | "cultural_identity") {
            continue;
        }
        details.insert(key.clone(), Value::String(value.clone()));
    }
    details.insert(
        "contact_status".to_owned(),
        Value::String("Onboarding".to_owned()),
    );

    let date_of_birth = chrono::NaiveDate::parse_from_str(draft.get("date_of_birth"), "%d/%m/%Y")
        .ok()
        .map(|date| date.format("%Y-%m-%d").to_string());

    let gender = match draft
        .get("gender")
        .trim()
        .to_ascii_lowercase()
        .replace(['-', ' '], "_")
        .as_str()
    {
        "female" => Some("female".to_owned()),
        "male" => Some("male".to_owned()),
        "non_binary" => Some("non_binary".to_owned()),
        "prefer_not_to_say" => Some("prefer_not_to_say".to_owned()),
        _ => None,
    };
    let languages = draft
        .get("languages")
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let cultural_identity = optional_str(draft.get("cultural_identity"));

    let notes = ["intake_notes", "worker_notes", "notes"]
        .iter()
        .map(|key| draft.get(key).trim())
        .find(|value| !value.is_empty())
        .map(str::to_owned);

    ApiContactWrite {
        given_name: optional_str(draft.get("first_name")),
        family_name: optional_str(draft.get("last_name")),
        preferred_name: optional_str(draft.get("preferred_name")),
        pronouns: optional_str(draft.get("pronouns")),
        date_of_birth,
        gender,
        languages,
        cultural_identity,
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

fn dot(ui: &mut Ui, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(6.0, 6.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(3), colour);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_auth() -> crate::api::AuthSession {
        crate::api::AuthSession {
            access_token: "test-token".to_owned(),
            refresh_token: "test-refresh".to_owned(),
            token_type: "Bearer".to_owned(),
            expires_in: 900,
            user: crate::api::UserProfile {
                id: "user-1".to_owned(),
                email: "owner@example.com".to_owned(),
                full_name: "Test Owner".to_owned(),
            },
            organisation: crate::api::OrganisationProfile {
                id: "org-test".to_owned(),
                slug: "test".to_owned(),
                name: "Test Organisation".to_owned(),
                edition: "community".to_owned(),
                role: "owner".to_owned(),
            },
        }
    }

    fn test_column(kind: ColumnKind, width: f32) -> ColumnState {
        ColumnState {
            kind,
            width,
            weight: 1.0,
            display_width: width,
            anim_from: width,
            anim_to: width,
            anim_start: 0.0,
            position_x: 0.0,
            pos_from: 0.0,
            pos_to: 0.0,
            pos_start: 0.0,
            closing: false,
            close_start: 0.0,
        }
    }

    #[test]
    fn resizing_columns_keeps_the_animation_target_in_sync() {
        let mut app = CareLibreApp::new();
        app.columns
            .push(test_column(ColumnKind::Page(Page::Contacts), 760.0));
        app.columns
            .push(test_column(ColumnKind::Contact("test".to_owned()), 420.0));
        app.columns_initialised = true;

        app.apply_column_resize(0, 1, 40.0, 1.0);

        assert_eq!(app.columns[0].width, 800.0);
        assert_eq!(app.columns[1].width, 380.0);
        assert_eq!(app.columns[0].anim_to, 800.0);
        assert_eq!(app.columns[1].anim_to, 380.0);

        // Simulate the next frame: the settled animation must not restore the
        // pre-resize widths.
        let viewport = app.columns[0].width + app.columns[1].width + 7.0;
        app.normalise_column_widths(&[0, 1], viewport, 2.0);

        assert!((app.columns[0].display_width - 800.0).abs() < 0.01);
        assert!((app.columns[1].display_width - 380.0).abs() < 0.01);
    }

    #[test]
    fn columns_share_the_window_evenly() {
        let mut app = CareLibreApp::new();
        app.columns
            .push(test_column(ColumnKind::Page(Page::Contacts), 760.0));
        app.columns
            .push(test_column(ColumnKind::Contact("test".to_owned()), 520.0));
        app.columns_initialised = true;

        let viewport = 1128.0;
        app.normalise_column_widths(&[0, 1], viewport, 1.0);

        let expected = (viewport - 7.0) / 2.0;
        assert!((app.columns[0].width - expected).abs() < 0.01);
        assert!((app.columns[1].width - expected).abs() < 0.01);
    }

    fn graph_node(id: &str, label: &str) -> ApiGraphNode {
        ApiGraphNode {
            kind: PartyKind::Contact,
            id: id.to_owned(),
            label: label.to_owned(),
            degree: 1,
            depth: None,
        }
    }

    fn graph_edge(id: &str, from: &str, to: &str) -> ApiRelationship {
        ApiRelationship {
            id: id.to_owned(),
            from_kind: PartyKind::Contact,
            from_id: from.to_owned(),
            from_name: from.to_owned(),
            from_role: "care_recipient".to_owned(),
            to_kind: PartyKind::Contact,
            to_id: to.to_owned(),
            to_name: to.to_owned(),
            to_role: "emergency_contact".to_owned(),
            start_date: None,
            end_date: None,
            end_reason: None,
            status: "current".to_owned(),
            previous_id: None,
            notes: None,
            updated_at: String::new(),
        }
    }

    #[test]
    fn organisation_editors_open_as_separate_columns() {
        let mut app = CareLibreApp::new();
        app.organisations.apply_list(
            vec![ApiOrganisation {
                id: "org-1".to_owned(),
                name: "Northside Care".to_owned(),
                kind: Some("provider".to_owned()),
                abn: None,
                email: None,
                phone: None,
                website: None,
                notes: None,
                is_active: true,
                relationship_count: 0,
                updated_at: String::new(),
            }],
            1,
        );

        app.open_organisation_editor(None);
        app.open_organisation_editor(Some("org-1".to_owned()));
        // Opening the same record again focuses its existing column.
        app.open_organisation_editor(Some("org-1".to_owned()));

        assert_eq!(app.organisation_editors.len(), 2);
        let organisation_columns = app
            .columns
            .iter()
            .filter(|column| matches!(column.kind, ColumnKind::Organisation(_)))
            .count();
        assert_eq!(organisation_columns, 2);
    }

    #[test]
    fn closing_an_organisation_column_removes_it_cleanly() {
        use egui_kittest::{Harness, kittest::Queryable};

        let mut app = CareLibreApp::new();
        app.use_session(test_auth());
        app.organisations.apply_list(
            vec![ApiOrganisation {
                id: "org-1".to_owned(),
                name: "Northside Care".to_owned(),
                kind: Some("provider".to_owned()),
                abn: None,
                email: None,
                phone: None,
                website: None,
                notes: None,
                is_active: true,
                relationship_count: 0,
                updated_at: String::new(),
            }],
            1,
        );
        app.open_page(Page::Organisations);
        app.open_organisation_editor(Some("org-1".to_owned()));

        let mut harness = Harness::builder()
            .with_size(egui::vec2(1200.0, 800.0))
            .with_max_steps(40)
            .with_context_setup(|ctx| theme::apply(ctx, theme::ThemeMode::Dark))
            .build_ui_state(|ui, app: &mut CareLibreApp| app.show(ui), app);
        harness.run();

        harness
            .get_all_by_label("Close column")
            .last()
            .expect("the organisation column")
            .click();
        harness.run_steps(40);

        assert!(harness.state().organisation_editors.is_empty());
        assert!(
            !harness
                .state()
                .columns
                .iter()
                .any(|column| matches!(column.kind, ColumnKind::Organisation(_)))
        );
    }

    #[test]
    fn clicking_anywhere_on_an_organisation_card_opens_the_editor() {
        use egui_kittest::{Harness, kittest::Queryable};

        let mut app = CareLibreApp::new();
        app.organisations.apply_list(
            vec![ApiOrganisation {
                id: "org-1".to_owned(),
                name: "Northside Care".to_owned(),
                kind: Some("provider".to_owned()),
                abn: None,
                email: None,
                phone: None,
                website: None,
                notes: Some("Preferred provider".to_owned()),
                is_active: true,
                relationship_count: 2,
                updated_at: String::new(),
            }],
            1,
        );
        app.open_page(Page::Organisations);
        app.use_session(test_auth());
        app.organisations.loaded_once = true;
        app.organisations.attempted = true;

        let mut harness = Harness::builder()
            .with_size(egui::vec2(1200.0, 800.0))
            .with_max_steps(40)
            .with_context_setup(|ctx| theme::apply(ctx, theme::ThemeMode::Dark))
            .build_ui_state(|ui, app: &mut CareLibreApp| app.show(ui), app);
        harness.run();

        harness.get_by_label("Northside Care").click();
        harness.run();

        assert!(harness.state().organisation_editors.contains_key("org-1"));
    }

    #[test]
    fn a_contact_record_shows_the_relationship_constellation() {
        use egui_kittest::{Harness, kittest::Queryable};

        let mut app = CareLibreApp::new();
        app.use_session(test_auth());
        app.party_catalog = vec![
            graph_node("alice", "Alice Example"),
            graph_node("bob", "Bob Example"),
        ];
        app.relationship_edges = vec![graph_edge("edge-1", "alice", "bob")];
        app.relationship_graph_loaded = true;
        app.note_relationship_network_changed();

        let mut window = ContactWindowState::new("alice".to_owned());
        window.loading = false;
        window.given_name = "Alice".to_owned();
        window.family_name = "Example".to_owned();
        window.role = "client".to_owned();
        app.contact_windows.insert("alice".to_owned(), window);
        app.columns
            .push(test_column(ColumnKind::Contact("alice".to_owned()), 560.0));
        app.columns_initialised = true;
        app.active_contact = Some("alice".to_owned());

        let mut harness = Harness::builder()
            .with_size(egui::vec2(900.0, 800.0))
            .with_max_steps(30)
            .with_context_setup(|ctx| theme::apply(ctx, theme::ThemeMode::Dark))
            .build_ui_state(|ui, app: &mut CareLibreApp| app.show(ui), app);

        harness.run();
        let toggle = harness.get_by_label_contains("Relationship constellation");
        toggle.scroll_to_me();
        harness.run();
        harness
            .get_by_label_contains("Relationship constellation")
            .click();
        harness.run();

        assert!(
            !harness
                .state()
                .contact_windows
                .get("alice")
                .expect("the record stays open")
                .constellation
                .collapsed
        );
    }

    #[test]
    fn signing_out_clears_every_workspace_surface() {
        let mut app = CareLibreApp::new();
        app.use_session(test_auth());
        app.set_page(Page::Contacts);
        app.open_contact("alice");
        app.start_intake(IntakeKind::Client);
        app.party_catalog.push(graph_node("alice", "Alice Example"));
        app.relationship_edges
            .push(graph_edge("edge-1", "alice", "bob"));
        app.search = "alice".to_owned();

        app.server.session = None;
        app.clear_authenticated_state(&egui::Context::default());

        assert!(app.columns.is_empty());
        assert!(app.contact_windows.is_empty());
        assert!(app.organisation_editors.is_empty());
        assert!(app.party_catalog.is_empty());
        assert!(app.relationship_edges.is_empty());
        assert!(app.known_contacts.is_empty());
        assert!(app.rostering.shifts.is_empty());
        assert!(app.intake.draft.is_none());
        assert!(app.search.is_empty());
        assert_eq!(app.page, Page::Contacts);
    }

    #[test]
    fn the_relationship_explorer_opens_on_the_requested_party() {
        let mut app = CareLibreApp::new();
        app.use_session(test_auth());

        app.open_relationship_explorer(PartyKind::Contact, "alice", "Alice Example");

        assert_eq!(app.page(), Page::Relationships);
        assert_eq!(
            app.relationship_explorer_focus(),
            Some((PartyKind::Contact, "alice".to_owned()))
        );
    }

    #[test]
    fn a_contact_record_can_open_the_relationship_explorer() {
        use egui_kittest::{Harness, kittest::Queryable};

        let mut app = CareLibreApp::new();
        app.use_session(test_auth());
        app.party_catalog = vec![
            graph_node("alice", "Alice Example"),
            graph_node("bob", "Bob Example"),
        ];
        app.relationship_edges = vec![graph_edge("edge-1", "alice", "bob")];
        app.relationship_graph_loaded = true;
        app.note_relationship_network_changed();

        let mut window = ContactWindowState::new("alice".to_owned());
        window.loading = false;
        window.given_name = "Alice".to_owned();
        window.family_name = "Example".to_owned();
        window.role = "client".to_owned();
        app.contact_windows.insert("alice".to_owned(), window);
        app.columns
            .push(test_column(ColumnKind::Contact("alice".to_owned()), 560.0));
        app.columns_initialised = true;
        app.active_contact = Some("alice".to_owned());

        let mut harness = Harness::builder()
            .with_size(egui::vec2(1000.0, 800.0))
            .with_max_steps(30)
            .with_context_setup(|ctx| theme::apply(ctx, theme::ThemeMode::Dark))
            .build_ui_state(|ui, app: &mut CareLibreApp| app.show(ui), app);

        harness.run();
        let button = harness
            .get_all_by_label_contains("Explore network")
            .next()
            .expect("the record's explorer entry point");
        button.scroll_to_me();
        harness.run();
        harness
            .get_all_by_label_contains("Explore network")
            .next()
            .expect("the record's explorer entry point")
            .click();
        harness.run();

        assert_eq!(harness.state().page(), Page::Relationships);
        assert_eq!(
            harness.state().relationship_explorer_focus(),
            Some((PartyKind::Contact, "alice".to_owned()))
        );
    }

    #[test]
    fn pages_open_as_separate_columns() {
        let mut app = CareLibreApp::new();
        app.open_page(Page::Rostering);

        let page_columns = app
            .columns
            .iter()
            .filter(|column| matches!(column.kind, ColumnKind::Page(_)))
            .count();
        assert_eq!(page_columns, 1);
        assert_eq!(app.page(), Page::Rostering);

        app.open_page(Page::Contacts);
        let page_columns = app
            .columns
            .iter()
            .filter(|column| matches!(column.kind, ColumnKind::Page(_)))
            .count();
        assert_eq!(page_columns, 2);
        assert_eq!(app.page(), Page::Contacts);

        // Selecting an already-open page focuses it rather than duplicating it.
        app.open_page(Page::Rostering);
        let page_columns = app
            .columns
            .iter()
            .filter(|column| matches!(column.kind, ColumnKind::Page(_)))
            .count();
        assert_eq!(page_columns, 2);
        assert_eq!(app.page(), Page::Rostering);
    }
}
