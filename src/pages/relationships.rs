//! The full-window relationship explorer.
//!
//! The on-record constellation is a glance: it is capped at a handful of steps
//! and opening a party replaces the record. The explorer keeps the graph as the
//! workspace instead. It draws the selected number of levels around a focus
//! party, lets the user pan and zoom, and shows a party's direct relationships
//! without navigating away. The relationship graph is already loaded for the
//! party lookup, so navigation is client-side.

use std::collections::{BTreeMap, BTreeSet};

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Layout, Margin, Pos2, Rect, RichText,
    Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};

use crate::config::{
    EXPLORER_DEFAULT_LEVELS, EXPLORER_MAX_LEVELS, EXPLORER_MIN_LEVELS, ExplorerPreference,
};
use crate::contact::{ApiGraphNode, ApiRelationship, PartyKind, relationship_role_label};
use crate::graph::{
    GraphFilters, HUB_BADGE_MIN, HUB_DEGREE, RelationshipGraph, RelationshipNetwork, WalkPolicy,
    node_key, transition_factor,
};
use crate::theme;

/// Total parties drawn. A larger cap than the record constellation would be
/// unreadable without zooming, but dense tenants still need a ceiling.
pub const MAX_NODES: usize = 240;
const FOCUS_RADIUS: f32 = 18.0;
const MIN_RING_STEP: f32 = 150.0;
const MIN_NODE_SPACING: f32 = 56.0;
const PANEL_WIDTH: f32 = 326.0;
const RESPONSIVE_BREAKPOINT: f32 = 760.0;
const MAX_HISTORY: usize = 24;

/// One visited party, with the view that was current before leaving it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Focus {
    pub kind: PartyKind,
    pub id: String,
    pub label: String,
    pub key: String,
}

impl Focus {
    pub fn new(kind: PartyKind, id: &str, label: &str) -> Self {
        Self {
            kind,
            id: id.to_owned(),
            label: if label.trim().is_empty() {
                format!("This {}", kind.as_str())
            } else {
                label.to_owned()
            },
            key: node_key(kind, id),
        }
    }
}

#[derive(Clone, Debug)]
struct Visited {
    focus: Focus,
    levels: u8,
    pan: Vec2,
    zoom: f32,
}

/// What one direct-relationship row asked for.
enum DirectRowAction {
    OpenContact(String),
    Select(String),
}

/// What the page wants the shell to do after this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum ExplorerAction {
    OpenContact(String),
    Reload,
    /// Levels or filters changed; persist the account preference.
    PreferenceChanged,
}

/// Everything the explorer needs from the application shell.
pub struct ExplorerSource<'a> {
    pub nodes: &'a [ApiGraphNode],
    pub network: &'a RelationshipNetwork,
    pub loading: bool,
    pub error: Option<&'a str>,
}

/// The explorer page state, owned by the application shell.
pub struct ExplorerState {
    focus: Option<Focus>,
    selection: Option<String>,
    history: Vec<Visited>,
    levels: u8,
    filters: GraphFilters,
    /// Show highly connected parties without expanding their neighbours.
    skip_hubs: bool,
    /// Hubs the user explicitly expanded for this visit.
    expanded: BTreeSet<String>,
    search: String,
    picking: bool,
    pan: Vec2,
    zoom: f32,
    /// Camera goals; the rendered pan and zoom ease toward these.
    target_pan: Vec2,
    target_zoom: f32,
    layout: Option<ExplorerLayout>,
    canvas_size: Vec2,
    auto_fit: bool,
    hovered: Option<String>,
    /// Interpolated node positions, keyed by party. Removed parties stay for a
    /// few frames so they can fade out instead of disappearing abruptly.
    displayed: BTreeMap<String, NodeDisplay>,
    last_frame: f64,
}

impl Default for ExplorerState {
    fn default() -> Self {
        Self {
            focus: None,
            selection: None,
            history: Vec::new(),
            levels: EXPLORER_DEFAULT_LEVELS,
            filters: GraphFilters::default(),
            skip_hubs: true,
            expanded: BTreeSet::new(),
            search: String::new(),
            picking: true,
            pan: Vec2::ZERO,
            zoom: 1.0,
            target_pan: Vec2::ZERO,
            target_zoom: 1.0,
            layout: None,
            canvas_size: Vec2::ZERO,
            auto_fit: true,
            hovered: None,
            displayed: BTreeMap::new(),
            last_frame: 0.0,
        }
    }
}

impl ExplorerState {
    /// Open the explorer on a party, keeping the previous focus on the back
    /// stack. Used when a record's "Explore network" control is pressed.
    pub fn focus_on(&mut self, kind: PartyKind, id: &str, label: &str) {
        let focus = Focus::new(kind, id, label);
        if self
            .focus
            .as_ref()
            .is_some_and(|current| current.key == focus.key)
        {
            self.selection = None;
            self.picking = false;
            return;
        }
        if let Some(current) = &self.focus {
            self.history.push(Visited {
                focus: current.clone(),
                levels: self.levels,
                pan: self.pan,
                zoom: self.zoom,
            });
            if self.history.len() > MAX_HISTORY {
                self.history.remove(0);
            }
        }
        self.focus = Some(focus);
        self.selection = None;
        self.search.clear();
        self.picking = false;
        self.levels = self.levels.clamp(EXPLORER_MIN_LEVELS, EXPLORER_MAX_LEVELS);
        self.expanded.clear();
        self.auto_fit = true;
        self.layout = None;
    }

    pub fn current_focus(&self) -> Option<&Focus> {
        self.focus.as_ref()
    }

    pub fn levels(&self) -> u8 {
        self.levels
    }

    /// The account-scoped settings this explorer should persist.
    pub fn preference(&self) -> ExplorerPreference {
        ExplorerPreference {
            levels: self.levels,
            show_ended: self.filters.show_ended,
            show_contacts: self.filters.show_contacts,
            show_organisations: self.filters.show_organisations,
            skip_hubs: self.skip_hubs,
        }
        .normalised()
    }

    pub fn apply_preference(&mut self, preference: ExplorerPreference) {
        let preference = preference.normalised();
        let levels_changed = preference.levels != self.levels;
        let filters_changed = preference.show_ended != self.filters.show_ended
            || preference.show_contacts != self.filters.show_contacts
            || preference.show_organisations != self.filters.show_organisations;
        let hubs_changed = preference.skip_hubs != self.skip_hubs;
        self.levels = preference.levels;
        self.filters.show_ended = preference.show_ended;
        self.filters.show_contacts = preference.show_contacts;
        self.filters.show_organisations = preference.show_organisations;
        self.skip_hubs = preference.skip_hubs;
        // A periodic token refresh re-applies the saved preference; only
        // rebuild and re-fit when the settings actually moved.
        if levels_changed || filters_changed || hubs_changed {
            self.invalidate();
        }
    }

    fn focus_key(&self) -> Option<String> {
        self.focus.as_ref().map(|focus| focus.key.clone())
    }

    fn show_picker(&mut self) {
        self.picking = true;
        self.search.clear();
        self.selection = None;
    }

    fn set_levels(&mut self, levels: u8) -> bool {
        let levels = levels.clamp(EXPLORER_MIN_LEVELS, EXPLORER_MAX_LEVELS);
        if levels == self.levels {
            return false;
        }
        self.levels = levels;
        self.auto_fit = true;
        self.layout = None;
        true
    }

    fn invalidate(&mut self) {
        self.layout = None;
        self.auto_fit = true;
    }

    fn go_back(&mut self) -> bool {
        let Some(visited) = self.history.pop() else {
            return false;
        };
        self.restore(visited);
        true
    }

    fn jump_to(&mut self, index: usize) -> bool {
        let Some(visited) = self.history.get(index).cloned() else {
            return false;
        };
        self.history.truncate(index);
        self.restore(visited);
        true
    }

    fn restore(&mut self, visited: Visited) {
        self.focus = Some(visited.focus);
        self.levels = visited.levels;
        self.expanded.clear();
        // Keep the current camera so returning to a party glides back to the
        // view it was left in rather than snapping.
        self.target_pan = visited.pan;
        self.target_zoom = visited.zoom;
        self.selection = None;
        self.picking = false;
        self.auto_fit = false;
        self.layout = None;
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Ring {
    hop: u8,
    radius: f32,
}

/// One node's interpolated position while the graph transitions.
#[derive(Clone, Debug)]
struct NodeDisplay {
    kind: PartyKind,
    label: String,
    hop: usize,
    pos: Pos2,
    radius: f32,
    alpha: f32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LayoutKey {
    revision: u64,
    focus: String,
    levels: u8,
    filters: GraphFilters,
    skip_hubs: bool,
    expanded: BTreeSet<String>,
}

pub struct ExplorerNode {
    pub key: String,
    pub kind: PartyKind,
    pub id: String,
    pub label: String,
    pub hop: usize,
    pub role: String,
    /// Position in world coordinates, with the focus at the origin.
    pub pos: Pos2,
    pub radius: f32,
}

pub struct ExplorerEdge {
    pub from: usize,
    pub to: usize,
    pub from_role: String,
    pub to_role: String,
}

struct ExplorerLayout {
    key: LayoutKey,
    graph: RelationshipGraph,
    nodes: Vec<ExplorerNode>,
    edges: Vec<ExplorerEdge>,
    rings: Vec<Ring>,
    roles: BTreeSet<String>,
    /// Suppressed parties and their direct link counts, for badges and the
    /// panel's expand control.
    hubs: BTreeMap<String, usize>,
    radius: f32,
    truncated: bool,
}

impl ExplorerLayout {
    fn node_index(&self, key: &str) -> Option<usize> {
        self.nodes.iter().position(|node| node.key == key)
    }
}

/// Draw the relationship explorer page.
pub fn show(
    ui: &mut Ui,
    state: &mut ExplorerState,
    source: &ExplorerSource<'_>,
) -> Option<ExplorerAction> {
    header(ui);
    ui.add_space(10.0);

    let mut action = None;

    if let Some(error) = source.error {
        error_card(ui, error);
        if ui
            .add_enabled(!source.loading, theme::secondary_button("Try again"))
            .clicked()
        {
            action = Some(ExplorerAction::Reload);
        }
        ui.add_space(8.0);
    }

    if state.focus.is_none() {
        return picker(ui, state, source).or(action);
    }

    if source.loading && source.network.edges.is_empty() {
        loading_card(ui, "Loading the relationship network…");
        return action;
    }

    ensure_layout(state, source);

    if state.picking {
        return picker(ui, state, source).or(action);
    }

    let Some(layout) = state.layout.take() else {
        return action;
    };

    if let Some(next) = toolbar(ui, state, &layout) {
        action = Some(next);
    }
    ui.add_space(8.0);

    if layout.truncated {
        ui.label(
            RichText::new(format!(
                "Showing the first {MAX_NODES} parties. Lower the levels or filter the network to see the rest."
            ))
            .color(theme::warning())
            .size(10.5),
        );
        ui.add_space(4.0);
    }

    let body_height = (ui.available_height() - 6.0).max(220.0);
    let wide = ui.available_width() >= RESPONSIVE_BREAKPOINT;
    if wide {
        ui.horizontal_top(|ui| {
            let available = ui.available_width();
            let panel_width = PANEL_WIDTH.min((available * 0.4).max(260.0));
            let canvas_width = (available - panel_width - 12.0).max(220.0);

            let next = ui
                .allocate_ui_with_layout(
                    vec2(canvas_width, body_height),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.set_min_size(vec2(canvas_width, body_height));
                        canvas(ui, state, &layout, vec2(canvas_width, body_height))
                    },
                )
                .inner;
            if let Some(next) = next {
                action = Some(next);
            }
            ui.add_space(12.0);
            let next = ui
                .allocate_ui_with_layout(
                    vec2(panel_width, body_height),
                    Layout::top_down(Align::Min),
                    |ui| panel(ui, state, &layout, vec2(panel_width, body_height)),
                )
                .inner;
            if let Some(next) = next {
                action = Some(next);
            }
        });
    } else {
        let width = ui.available_width();
        let canvas_height = (body_height * 0.58).max(220.0);
        let next = ui
            .allocate_ui_with_layout(
                vec2(width, canvas_height),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_min_size(vec2(width, canvas_height));
                    canvas(ui, state, &layout, vec2(width, canvas_height))
                },
            )
            .inner;
        if let Some(next) = next {
            action = Some(next);
        }
        ui.add_space(8.0);
        let panel_height = (body_height - canvas_height - 8.0).max(150.0);
        let next = ui
            .allocate_ui_with_layout(
                vec2(width, panel_height),
                Layout::top_down(Align::Min),
                |ui| panel(ui, state, &layout, vec2(width, panel_height)),
            )
            .inner;
        if let Some(next) = next {
            action = Some(next);
        }
    }

    ui.add_space(4.0);
    ui.label(
        RichText::new(
            "Click a party to see its direct relationships · double-click to focus it · hubs show their links but are not expanded",
        )
        .color(theme::faint())
        .size(10.0),
    );

    if state.layout.is_none() {
        state.layout = Some(layout);
    }
    action
}

fn header(ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Relationships").font(theme::strong_font(26.0)));
        theme::pill(ui, "EXPLORER", theme::accent());
    });
    ui.label(
        RichText::new(
            "Walk the relationship graph, change how many levels you see, and inspect each party's direct links.",
        )
        .color(theme::muted())
        .size(12.5),
    );
}

fn error_card(ui: &mut Ui, error: &str) {
    theme::card().show(ui, |ui| {
        ui.label(
            RichText::new("Could not load the relationship network")
                .font(theme::strong_font(12.5))
                .color(theme::text()),
        );
        ui.label(RichText::new(error).color(theme::danger()).size(11.0));
    });
}

fn loading_card(ui: &mut Ui, message: &str) {
    theme::card().show(ui, |ui| {
        ui.label(RichText::new(message).color(theme::muted()).size(11.5));
    });
}

/// Ensure the world-space layout matches the data, focus, level and filter
/// state. Layouts are cached so panning and zooming never rebuild the graph.
fn ensure_layout(state: &mut ExplorerState, source: &ExplorerSource<'_>) {
    let Some(focus) = state.focus.clone() else {
        state.layout = None;
        return;
    };
    let key = LayoutKey {
        revision: source.network.revision,
        focus: focus.key.clone(),
        levels: state.levels,
        filters: state.filters.clone(),
        skip_hubs: state.skip_hubs,
        expanded: state.expanded.clone(),
    };
    if state.layout.as_ref().map(|layout| &layout.key) == Some(&key) {
        return;
    }
    state.layout = Some(build_layout(
        source.nodes,
        &source.network.edges,
        &focus,
        state.levels,
        &state.filters,
        source.network.revision,
        state.skip_hubs,
        &state.expanded,
    ));
    state.auto_fit = true;
}

#[allow(clippy::too_many_arguments)]
fn build_layout(
    catalog: &[ApiGraphNode],
    edges: &[ApiRelationship],
    focus: &Focus,
    levels: u8,
    filters: &GraphFilters,
    revision: u64,
    skip_hubs: bool,
    expanded: &BTreeSet<String>,
) -> ExplorerLayout {
    let mut graph = RelationshipGraph::build(catalog, edges, filters, Some(&focus.key));
    graph.ensure_label(focus.kind, &focus.id, &focus.label);
    let policy = WalkPolicy {
        max_nodes: MAX_NODES,
        skip_hubs,
        hub_degree: HUB_DEGREE,
        expanded,
    };

    let mut roles = BTreeSet::new();
    for edge in edges.iter().filter(|edge| !edge.is_ended()) {
        roles.insert(edge.from_role.clone());
        roles.insert(edge.to_role.clone());
    }

    let walk = graph.walk_with(&focus.key, levels, &policy);
    let mut nodes: Vec<ExplorerNode> = Vec::with_capacity(walk.hops.len());
    let mut index_by_key: BTreeMap<String, usize> = BTreeMap::new();

    let mut push_node = |key: &str, radius: f32, pos: Pos2| {
        if let Some(node) = graph.label(key) {
            index_by_key.insert(key.to_owned(), nodes.len());
            nodes.push(ExplorerNode {
                key: key.to_owned(),
                kind: node.kind,
                id: node.id.clone(),
                label: node.label.clone(),
                hop: walk.hops.get(key).copied().unwrap_or(0),
                role: walk.roles.get(key).cloned().unwrap_or_default(),
                pos,
                radius,
            });
        }
    };

    push_node(&focus.key, FOCUS_RADIUS, pos2(0.0, 0.0));

    let mut rings = Vec::new();
    let mut previous_radius = 0.0_f32;
    let mut angle_by_key: BTreeMap<String, f32> = BTreeMap::new();

    for hop in 1..=levels as usize {
        let mut keys: Vec<&String> = walk
            .hops
            .iter()
            .filter_map(|(key, depth)| (*depth == hop).then_some(key))
            .collect();
        keys.sort_by(|left, right| {
            let left_parent = walk
                .parents
                .get(*left)
                .and_then(|parent| angle_by_key.get(parent))
                .copied()
                .unwrap_or(0.0);
            let right_parent = walk
                .parents
                .get(*right)
                .and_then(|parent| angle_by_key.get(parent))
                .copied()
                .unwrap_or(0.0);
            left_parent
                .total_cmp(&right_parent)
                .then_with(|| {
                    let left_label = graph.label(left).map_or("", |node| node.label.as_str());
                    let right_label = graph.label(right).map_or("", |node| node.label.as_str());
                    left_label.to_lowercase().cmp(&right_label.to_lowercase())
                })
                .then_with(|| left.cmp(right))
        });

        if keys.is_empty() {
            continue;
        }
        let total = keys.len();
        let mut placed = 0;
        let mut radius = (previous_radius + MIN_RING_STEP).max(MIN_RING_STEP);

        while placed < total {
            // Crowded levels spread over several concentric sub-rings instead
            // of one huge circle. This keeps the first hop readable even when
            // a popular organisation pulls in hundreds of second-hop parties.
            let capacity = ((std::f32::consts::TAU * radius / MIN_NODE_SPACING).floor() as usize)
                .max(1)
                .min(total - placed);
            let step_angle = std::f32::consts::TAU / capacity as f32;
            let offset = -std::f32::consts::FRAC_PI_2
                + hop as f32 * 0.37
                + (placed as f32 / total as f32) * std::f32::consts::TAU;

            for index in 0..capacity {
                let angle = offset + step_angle * index as f32;
                let position = pos2(angle.cos() * radius, angle.sin() * radius);
                let base_radius: f32 = match hop {
                    1 => 12.0,
                    2 => 10.0,
                    _ => 8.5,
                };
                let spacing = std::f32::consts::TAU * radius / capacity as f32;
                let node_radius = base_radius.min((spacing * 0.32).max(4.0));
                let key = keys[placed + index].clone();
                push_node(&key, node_radius, position);
                angle_by_key.insert(key, angle);
            }

            rings.push(Ring {
                hop: hop as u8,
                radius,
            });
            placed += capacity;
            previous_radius = radius;
            radius += MIN_RING_STEP;
        }
    }

    drop(push_node);

    let mut view_edges = Vec::new();
    for edge in graph.edges() {
        let from = node_key(edge.from_kind, &edge.from_id);
        let to = node_key(edge.to_kind, &edge.to_id);
        if let (Some(&from), Some(&to)) = (index_by_key.get(&from), index_by_key.get(&to)) {
            view_edges.push(ExplorerEdge {
                from,
                to,
                from_role: edge.from_role.clone(),
                to_role: edge.to_role.clone(),
            });
        }
    }

    let mut hubs = BTreeMap::new();
    if skip_hubs {
        for node in &nodes {
            if !graph.expands_through(&node.key, &focus.key, &policy) {
                let links = graph.degree(&node.key);
                if links > 0 {
                    hubs.insert(node.key.clone(), links);
                }
            }
        }
    }

    ExplorerLayout {
        key: LayoutKey {
            revision,
            focus: focus.key.clone(),
            levels,
            filters: filters.clone(),
            skip_hubs,
            expanded: expanded.clone(),
        },
        graph,
        nodes,
        edges: view_edges,
        rings,
        roles,
        hubs,
        radius: (previous_radius + 90.0).max(160.0),
        truncated: walk.truncated,
    }
}

fn toolbar(
    ui: &mut Ui,
    state: &mut ExplorerState,
    layout: &ExplorerLayout,
) -> Option<ExplorerAction> {
    let mut action = None;
    let mut changed = false;

    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(
                !state.history.is_empty(),
                egui::Button::new(RichText::new("← Back").color(theme::text()).size(11.5)),
            )
            .on_hover_text("Return to the previous party")
            .clicked()
            && state.go_back()
        {
            changed = true;
        }

        let mut jump = None;
        for index in 0..state.history.len() {
            let entry = &state.history[index];
            if ui
                .add(
                    egui::Button::new(
                        RichText::new(truncate(&entry.focus.label, 22))
                            .color(theme::muted())
                            .size(10.5),
                    )
                    .frame(false),
                )
                .on_hover_text("Go back to this party")
                .clicked()
            {
                jump = Some(index);
            }
            ui.label(RichText::new("›").color(theme::faint()).size(10.5));
        }
        if let Some(index) = jump
            && state.jump_to(index)
        {
            changed = true;
        }
        if let Some(focus) = &state.focus {
            ui.label(
                RichText::new(truncate(&focus.label, 30))
                    .font(theme::strong_font(11.5))
                    .color(theme::text()),
            );
            theme::pill(ui, focus.kind.label(), focus_pill_colour(focus.kind));
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add(egui::Button::new(
                    RichText::new("Change focus")
                        .color(theme::text())
                        .size(10.5),
                ))
                .clicked()
            {
                state.show_picker();
            }
            if ui
                .add(egui::Button::new(
                    RichText::new("Fit").color(theme::text()).size(10.5),
                ))
                .on_hover_text("Fit the whole network in view")
                .clicked()
            {
                state.auto_fit = true;
            }
        });
    });

    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Levels").color(theme::faint()).size(10.5));
        if ui
            .add_enabled(
                state.levels > EXPLORER_MIN_LEVELS,
                egui::Button::new(RichText::new("−").size(13.0)).min_size(vec2(24.0, 22.0)),
            )
            .on_hover_text("Show one fewer level")
            .clicked()
        {
            changed |= state.set_levels(state.levels - 1);
        }
        ui.label(
            RichText::new(state.levels.to_string())
                .font(theme::strong_font(11.5))
                .color(theme::text()),
        );
        if ui
            .add_enabled(
                state.levels < EXPLORER_MAX_LEVELS,
                egui::Button::new(RichText::new("+").size(13.0)).min_size(vec2(24.0, 22.0)),
            )
            .on_hover_text("Show one more level")
            .clicked()
        {
            changed |= state.set_levels(state.levels + 1);
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        let contacts_only = state.filters.show_contacts && !state.filters.show_organisations;
        let organisations_only = state.filters.show_organisations && !state.filters.show_contacts;
        if ui
            .add_enabled(
                !contacts_only,
                egui::Button::new(RichText::new("Contacts").size(10.5))
                    .selected(state.filters.show_contacts),
            )
            .on_hover_text("Show or hide contact parties")
            .clicked()
        {
            state.filters.show_contacts = !state.filters.show_contacts;
            state.invalidate();
            changed = true;
        }
        if ui
            .add_enabled(
                !organisations_only,
                egui::Button::new(RichText::new("Organisations").size(10.5))
                    .selected(state.filters.show_organisations),
            )
            .on_hover_text("Show or hide organisations")
            .clicked()
        {
            state.filters.show_organisations = !state.filters.show_organisations;
            state.invalidate();
            changed = true;
        }
        if ui
            .checkbox(&mut state.filters.show_ended, "Show ended")
            .on_hover_text("Include relationships that have ended")
            .changed()
        {
            state.invalidate();
            changed = true;
        }
        if ui
            .checkbox(&mut state.skip_hubs, "Skip hubs")
            .on_hover_text(
                "Show highly connected parties, such as the organisation everyone belongs to, without expanding all of their clients. Select a party to expand it.",
            )
            .changed()
        {
            state.invalidate();
            changed = true;
        }

        let before = state.filters.role.clone();
        let selected = state
            .filters
            .role
            .as_deref()
            .map(relationship_role_label)
            .unwrap_or_else(|| "All roles".to_owned());
        egui::ComboBox::from_id_salt("explorer-role")
            .selected_text(RichText::new(selected).size(10.5))
            .width(150.0)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(state.filters.role.is_none(), "All roles")
                    .clicked()
                {
                    state.filters.role = None;
                }
                for role in &layout.roles {
                    if ui
                        .selectable_label(
                            state.filters.role.as_deref() == Some(role.as_str()),
                            relationship_role_label(role),
                        )
                        .clicked()
                    {
                        state.filters.role = Some(role.clone());
                    }
                }
            })
            .response
            .on_hover_text("Only show relationships with this role");
        if before != state.filters.role {
            state.invalidate();
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!(
                    "{} parties · {} links",
                    layout.nodes.len().saturating_sub(1),
                    layout.edges.len()
                ))
                .color(theme::faint())
                .size(10.0),
            );
        });
    });

    if changed {
        action = Some(ExplorerAction::PreferenceChanged);
    }
    action
}

fn picker(
    ui: &mut Ui,
    state: &mut ExplorerState,
    source: &ExplorerSource<'_>,
) -> Option<ExplorerAction> {
    let mut chosen: Option<Focus> = None;
    let height = (ui.available_height() - 8.0).max(200.0);

    theme::card().show(ui, |ui| {
        ui.set_min_height(height);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Choose a starting point")
                    .font(theme::strong_font(15.0))
                    .color(theme::text()),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if state.current_focus().is_some()
                    && ui
                        .add(egui::Button::new(
                            RichText::new("Back to graph")
                                .color(theme::muted())
                                .size(10.5),
                        ))
                        .clicked()
                {
                    state.picking = false;
                }
            });
        });
        ui.label(
            RichText::new(
                "Start from a contact or organisation, then walk outwards one level at a time.",
            )
            .color(theme::muted())
            .size(10.5),
        );
        ui.add_space(8.0);

        let width = ui.available_width().min(420.0);
        let response = ui.add_sized(
            [width, 34.0],
            egui::TextEdit::singleline(&mut state.search)
                .hint_text("Search contacts and organisations…")
                .margin(Margin::symmetric(10, 8)),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Search parties")
        });
        if !state.search.is_empty() && ui.small_button("Clear").clicked() {
            state.search.clear();
        }
        ui.add_space(4.0);

        let query = state.search.trim().to_lowercase();
        let mut parties: Vec<&ApiGraphNode> = source
            .nodes
            .iter()
            .filter(|node| query.is_empty() || node.label.to_lowercase().contains(&query))
            .collect();
        parties.sort_by(|left, right| {
            left.label
                .to_lowercase()
                .cmp(&right.label.to_lowercase())
                .then_with(|| left.kind.label().cmp(right.kind.label()))
                .then_with(|| left.id.cmp(&right.id))
        });

        ui.label(
            RichText::new(format!(
                "{} part{}",
                parties.len(),
                if parties.len() == 1 { "y" } else { "ies" }
            ))
            .color(theme::faint())
            .size(10.0),
        );

        if parties.is_empty() && source.loading {
            ui.label(
                RichText::new("Loading the relationship directory…")
                    .color(theme::muted())
                    .size(11.0),
            );
        } else if parties.is_empty() {
            ui.add_space(8.0);
            ui.label(
                RichText::new(if query.is_empty() {
                    "No parties are in the relationship directory yet."
                } else {
                    "No party matches this search."
                })
                .color(theme::faint())
                .size(11.0),
            );
        } else {
            let row_height = 42.0;
            let list_height = (ui.available_height() - 8.0).max(120.0);
            egui::ScrollArea::vertical()
                .id_salt("explorer-picker")
                .max_height(list_height)
                .auto_shrink([false, false])
                .show_rows(ui, row_height, parties.len(), |ui, range| {
                    for index in range {
                        let node = parties[index];
                        let (rect, response) = ui.allocate_exact_size(
                            vec2(ui.available_width(), row_height - 4.0),
                            Sense::click(),
                        );
                        let painter = ui.painter();
                        if response.hovered() {
                            painter.rect_filled(rect, CornerRadius::same(8), theme::hover());
                        }
                        painter.text(
                            rect.left_center() + vec2(10.0, 0.0),
                            Align2::LEFT_CENTER,
                            truncate(&node.label, 70),
                            FontId::proportional(12.0),
                            theme::text(),
                        );
                        painter.text(
                            rect.right_center() - vec2(10.0, 0.0),
                            Align2::RIGHT_CENTER,
                            node.kind.label(),
                            FontId::proportional(9.5),
                            theme::faint(),
                        );
                        painter.line_segment(
                            [
                                rect.left_bottom() + vec2(10.0, 1.0),
                                rect.right_bottom() - vec2(10.0, -1.0),
                            ],
                            Stroke::new(1.0, theme::border()),
                        );
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::Role::Button,
                                true,
                                format!("{} · {}", node.label, node.kind.label()),
                            )
                        });
                        if response.clicked() {
                            chosen = Some(Focus::new(node.kind, &node.id, &node.label));
                        }
                    }
                });
        }
    });

    if let Some(focus) = chosen {
        state.focus_on(focus.kind, &focus.id, &focus.label);
    }
    None
}

fn canvas(
    ui: &mut Ui,
    state: &mut ExplorerState,
    layout: &ExplorerLayout,
    size: Vec2,
) -> Option<ExplorerAction> {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());

    let now = ui.ctx().input(|input| input.time);
    let dt = if state.last_frame <= 0.0 {
        1.0 / 60.0
    } else {
        (now - state.last_frame).clamp(0.0, 0.12) as f32
    };
    state.last_frame = now;

    let first_draw = state.canvas_size == Vec2::ZERO;
    if state.canvas_size != size {
        state.canvas_size = size;
        if first_draw {
            response.request_focus();
        }
    }
    if state.auto_fit {
        fit(state, layout, size);
    }
    if first_draw {
        // Land on the fitted view instead of zooming in from a default
        // camera. Later focus changes animate.
        state.pan = state.target_pan;
        state.zoom = state.target_zoom;
    }

    if response.dragged() {
        state.pan += response.drag_delta();
        state.target_pan = state.pan;
        state.auto_fit = false;
    }

    let pointer = response.hover_pos().filter(|pos| rect.contains(*pos));
    if response.hovered()
        && let Some(pointer) = pointer
    {
        let scroll = ui.ctx().input(|input| input.smooth_scroll_delta.y);
        if scroll.abs() > 0.1 {
            zoom_around(state, scroll, pointer, rect);
            state.auto_fit = false;
        }
    }

    let nodes_moving = update_displayed(state, layout, dt);
    let camera_moving = update_camera(state, dt);
    if nodes_moving || camera_moving {
        ui.ctx().request_repaint();
    }

    let hovered = pointer.and_then(|pos| hit_test(state, layout, rect, pos));
    state.hovered = hovered.map(|index| layout.nodes[index].key.clone());

    if response.clicked() {
        response.request_focus();
        state.selection = hovered.map(|index| layout.nodes[index].key.clone());
    }

    if response.double_clicked()
        && let Some(index) = hovered
    {
        let node = &layout.nodes[index];
        if node.hop > 0 {
            state.focus_on(node.kind, &node.id, &node.label);
        }
    }

    let action = handle_keys(ui, state, &response, layout);

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect.shrink(0.5), CornerRadius::same(12), theme::bg());
    painter.rect_stroke(
        rect.shrink(0.5),
        CornerRadius::same(12),
        Stroke::new(1.0, theme::border()),
        StrokeKind::Inside,
    );

    let centre = rect.center() + state.pan;
    let selected = state.selection.clone();
    let hovered_key = state.hovered.clone();
    let active_key = selected.as_deref().or(hovered_key.as_deref());

    let mut ring_hop = 0;
    for ring in &layout.rings {
        painter.circle_stroke(
            centre,
            ring.radius * state.zoom,
            Stroke::new(
                1.0,
                Color32::from_rgba_unmultiplied(
                    theme::border().r(),
                    theme::border().g(),
                    theme::border().b(),
                    if ring.hop == ring_hop { 40 } else { 90 },
                ),
            ),
        );
        if ring.hop != ring_hop {
            ring_hop = ring.hop;
            if state.zoom > 0.3 {
                painter.text(
                    centre + vec2(0.0, -ring.radius * state.zoom - 5.0),
                    Align2::CENTER_BOTTOM,
                    format!("{}", ring.hop),
                    FontId::proportional(9.0),
                    theme::faint(),
                );
            }
        }
    }

    for edge in &layout.edges {
        let from_key = &layout.nodes[edge.from].key;
        let to_key = &layout.nodes[edge.to].key;
        let (Some(from), Some(to)) = (state.displayed.get(from_key), state.displayed.get(to_key))
        else {
            continue;
        };
        let alpha = from.alpha.min(to.alpha);
        if alpha < 0.02 {
            continue;
        }
        let highlighted = active_key.is_some_and(|key| key == from_key || key == to_key);
        let colour = if highlighted {
            with_alpha(theme::accent(), alpha)
        } else {
            Color32::from_rgba_unmultiplied(
                theme::muted().r(),
                theme::muted().g(),
                theme::muted().b(),
                (85.0 * alpha) as u8,
            )
        };
        painter.line_segment(
            [
                centre + from.pos.to_vec2() * state.zoom,
                centre + to.pos.to_vec2() * state.zoom,
            ],
            Stroke::new(
                if highlighted {
                    2.0
                } else {
                    (1.0 * state.zoom).clamp(0.6, 1.6)
                },
                colour,
            ),
        );
    }

    let labels_everywhere = state.zoom >= 0.8 && layout.nodes.len() <= 30;
    let mut placed_labels: Vec<Rect> = Vec::new();
    // Draw faded-out parties first so the new graph sits on top.
    let mut displays: Vec<(&String, &NodeDisplay)> = state.displayed.iter().collect();
    displays.sort_by_key(|(_, node)| node.alpha < 0.5);
    for (key, node) in displays {
        let Some(index) = layout.node_index(key) else {
            if node.alpha > 0.02 {
                draw_node(
                    &painter, rect, centre, state.zoom, node, false, false, false, None,
                );
            }
            continue;
        };
        let is_focus = node.hop == 0;
        let is_selected = selected.as_deref() == Some(key.as_str());
        let is_hovered = hovered == Some(index);
        let emphasis = is_focus || is_selected || is_hovered;
        draw_node(
            &painter,
            rect,
            centre,
            state.zoom,
            node,
            is_focus,
            is_selected,
            emphasis,
            layout
                .hubs
                .get(key)
                .copied()
                .filter(|links| *links >= HUB_BADGE_MIN),
        );

        if node.alpha > 0.55
            && (emphasis || labels_everywhere || (node.hop == 1 && layout.nodes.len() <= 18))
        {
            draw_label(
                &painter,
                rect,
                centre + node.pos.to_vec2() * state.zoom,
                (node.radius * state.zoom).clamp(3.0, 26.0),
                &node.label,
                emphasis,
                &mut placed_labels,
            );
        }
    }

    painter.text(
        rect.left_bottom() + vec2(12.0, -10.0),
        Align2::LEFT_BOTTOM,
        "● Contact   ■ Organisation",
        FontId::proportional(9.5),
        theme::faint(),
    );
    painter.text(
        rect.right_bottom() + vec2(-12.0, -10.0),
        Align2::RIGHT_BOTTOM,
        format!("{:.0}%", state.zoom * 100.0),
        FontId::proportional(9.5),
        theme::faint(),
    );

    if hovered.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    } else if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }

    if let Some(index) = hovered {
        let node = &layout.nodes[index];
        let mut roles: Vec<String> = Vec::new();
        for edge in &layout.edges {
            if edge.from != index && edge.to != index {
                continue;
            }
            let label = format!(
                "{} ↔ {}",
                relationship_role_label(&edge.from_role),
                relationship_role_label(&edge.to_role)
            );
            if !roles.contains(&label) {
                roles.push(label);
            }
        }
        if roles.is_empty() && !node.role.is_empty() {
            roles.push(relationship_role_label(&node.role));
        }

        egui::Tooltip::always_open(
            ui.ctx().clone(),
            ui.layer_id(),
            response.id.with(("explorer-node", &node.key)),
            egui::PopupAnchor::Pointer,
        )
        .gap(14.0)
        .show(|ui| {
            ui.label(
                RichText::new(&node.label)
                    .font(theme::strong_font(12.0))
                    .color(theme::text()),
            );
            ui.label(
                RichText::new(format!(
                    "{} · {} level{} from {}",
                    node.kind.label(),
                    node.hop,
                    if node.hop == 1 { "" } else { "s" },
                    state
                        .focus
                        .as_ref()
                        .map(|focus| focus.label.as_str())
                        .unwrap_or("the focus")
                ))
                .color(theme::muted())
                .size(10.5),
            );
            for role in roles.iter().take(3) {
                ui.label(
                    RichText::new(format!("Relationship: {role}"))
                        .color(theme::faint())
                        .size(10.0),
                );
            }
            if let Some(links) = layout.hubs.get(&node.key) {
                ui.label(
                    RichText::new(format!("{links} links · neighbours not expanded"))
                        .color(theme::warning())
                        .size(10.0),
                );
            }
            if node.hop > 0 {
                ui.label(
                    RichText::new("Click to select · double-click to focus here")
                        .color(theme::accent())
                        .size(10.0),
                );
            }
        });
    }

    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Image,
            true,
            format!(
                "Relationship explorer: {} parties within {} levels",
                layout.nodes.len().saturating_sub(1),
                layout.key.levels
            ),
        )
    });

    action
}

fn with_alpha(colour: Color32, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        colour.r(),
        colour.g(),
        colour.b(),
        (colour.a() as f32 * alpha.clamp(0.0, 1.0)) as u8,
    )
}

#[allow(clippy::too_many_arguments)]
fn draw_node(
    painter: &egui::Painter,
    rect: Rect,
    centre: Pos2,
    zoom: f32,
    node: &NodeDisplay,
    is_focus: bool,
    is_selected: bool,
    emphasis: bool,
    hub_links: Option<usize>,
) {
    let position = centre + node.pos.to_vec2() * zoom;
    if !rect.expand(30.0).contains(position) {
        return;
    }
    let alpha = node.alpha.clamp(0.0, 1.0);
    if alpha < 0.02 {
        return;
    }
    let colour = if is_focus {
        theme::accent()
    } else if node.kind == PartyKind::Organisation {
        theme::warning()
    } else {
        theme::accent_blue()
    };
    let floor = if is_focus {
        9.0
    } else if node.hop == 1 {
        5.0
    } else {
        3.0
    };
    let radius = (node.radius * zoom).clamp(floor, 26.0);
    let fill =
        Color32::from_rgba_unmultiplied(colour.r(), colour.g(), colour.b(), (55.0 * alpha) as u8);

    if is_focus {
        painter.circle_stroke(
            position,
            radius + 5.0,
            Stroke::new(1.5, theme::accent_soft((150.0 * alpha) as u8)),
        );
    }
    if is_selected {
        painter.circle_stroke(
            position,
            radius + 3.5,
            Stroke::new(1.5, with_alpha(theme::text(), alpha)),
        );
    }

    let stroke = Stroke::new(if emphasis { 2.2 } else { 1.4 }, with_alpha(colour, alpha));
    match node.kind {
        PartyKind::Contact => {
            painter.circle_filled(position, radius, fill);
            painter.circle_stroke(position, radius, stroke);
        }
        PartyKind::Organisation => {
            let box_rect = Rect::from_center_size(position, Vec2::splat(radius * 1.85));
            painter.rect_filled(box_rect, CornerRadius::same(3), fill);
            painter.rect_stroke(box_rect, CornerRadius::same(3), stroke, StrokeKind::Inside);
        }
    }

    if radius >= 7.0 {
        painter.text(
            position,
            Align2::CENTER_CENTER,
            initials(&node.label),
            FontId::proportional((radius * 0.72).clamp(5.0, 10.5)),
            with_alpha(colour, alpha),
        );
    }

    if let Some(links) = hub_links
        && zoom > 0.28
        && alpha > 0.55
    {
        let galley = painter.layout_no_wrap(
            format!("{links}"),
            FontId::proportional(8.5),
            with_alpha(theme::warning(), alpha),
        );
        let size = galley.size() + vec2(9.0, 3.0);
        let anchor = position + vec2(radius * 0.65, -radius - size.y * 0.5);
        let badge = Rect::from_min_size(anchor, size);
        painter.rect_filled(badge, CornerRadius::same(4), theme::bg());
        painter.rect_stroke(
            badge,
            CornerRadius::same(4),
            Stroke::new(1.0, with_alpha(theme::border(), alpha)),
            StrokeKind::Inside,
        );
        painter.galley(
            badge.min + vec2(4.5, 1.5),
            galley,
            with_alpha(theme::warning(), alpha),
        );
    }
}

fn update_displayed(state: &mut ExplorerState, layout: &ExplorerLayout, dt: f32) -> bool {
    let k = transition_factor(dt);
    let mut moving = false;
    let target: BTreeSet<&str> = layout.nodes.iter().map(|node| node.key.as_str()).collect();

    for node in &layout.nodes {
        match state.displayed.get_mut(&node.key) {
            Some(display) => {
                display.kind = node.kind;
                if display.label != node.label {
                    display.label.clone_from(&node.label);
                }
                display.hop = node.hop;
                let dx = node.pos.x - display.pos.x;
                let dy = node.pos.y - display.pos.y;
                let dr = node.radius - display.radius;
                let da = 1.0 - display.alpha;
                if dx.abs() + dy.abs() > 0.05 || dr.abs() > 0.05 || da.abs() > 0.005 {
                    moving = true;
                }
                display.pos.x += dx * k;
                display.pos.y += dy * k;
                display.radius += dr * k;
                display.alpha += da * k;
            }
            None => {
                state.displayed.insert(
                    node.key.clone(),
                    NodeDisplay {
                        kind: node.kind,
                        label: node.label.clone(),
                        hop: node.hop,
                        pos: node.pos,
                        radius: node.radius * 0.6,
                        alpha: 0.0,
                    },
                );
                moving = true;
            }
        }
    }

    let ghosts: Vec<String> = state
        .displayed
        .keys()
        .filter(|key| !target.contains(key.as_str()))
        .cloned()
        .collect();
    for key in ghosts {
        if let Some(display) = state.displayed.get_mut(&key) {
            display.alpha -= display.alpha * k;
            if display.alpha < 0.02 {
                state.displayed.remove(&key);
            } else {
                moving = true;
            }
        }
    }
    moving
}

fn update_camera(state: &mut ExplorerState, dt: f32) -> bool {
    let k = transition_factor(dt);
    let dz = state.target_zoom - state.zoom;
    let dp = state.target_pan - state.pan;
    if dz.abs() < 0.0005 && dp.length() < 0.05 {
        state.zoom = state.target_zoom;
        state.pan = state.target_pan;
        return false;
    }
    state.zoom += dz * k;
    state.pan += dp * k;
    true
}

fn handle_keys(
    ui: &Ui,
    state: &mut ExplorerState,
    response: &egui::Response,
    layout: &ExplorerLayout,
) -> Option<ExplorerAction> {
    if !response.has_focus() {
        return None;
    }
    let mut levels_changed = false;
    let mut refit = false;
    let mut go_back = false;

    ui.ctx().input(|input| {
        if input.key_pressed(egui::Key::Plus) || input.key_pressed(egui::Key::Equals) {
            levels_changed |= state.set_levels(state.levels.saturating_add(1));
        }
        if input.key_pressed(egui::Key::Minus) {
            levels_changed |= state.set_levels(state.levels.saturating_sub(1));
        }
        if input.key_pressed(egui::Key::F) {
            refit = true;
        }
        if input.key_pressed(egui::Key::Escape) || input.key_pressed(egui::Key::Backspace) {
            if state.selection.is_some() {
                state.selection = None;
            } else {
                go_back = true;
            }
        }
        if input.key_pressed(egui::Key::Enter)
            && let Some(selected) = state.selection.clone()
            && let Some(index) = layout.node_index(&selected)
            && layout.nodes[index].hop > 0
        {
            let node = &layout.nodes[index];
            state.focus_on(node.kind, &node.id, &node.label);
        }
        if input.key_pressed(egui::Key::ArrowRight) || input.key_pressed(egui::Key::ArrowDown) {
            cycle_selection(state, layout, 1);
        }
        if input.key_pressed(egui::Key::ArrowLeft) || input.key_pressed(egui::Key::ArrowUp) {
            cycle_selection(state, layout, -1);
        }
    });

    if go_back && state.go_back() {
        // The restored view is drawn from the next frame's layout.
    }
    if refit {
        state.auto_fit = true;
    }

    if levels_changed {
        Some(ExplorerAction::PreferenceChanged)
    } else {
        None
    }
}

fn cycle_selection(state: &mut ExplorerState, layout: &ExplorerLayout, delta: isize) {
    if layout.nodes.is_empty() {
        return;
    }
    let current = state
        .selection
        .as_deref()
        .and_then(|key| layout.node_index(key));
    let next = match (current, delta.is_negative()) {
        (Some(index), false) => (index + 1) % layout.nodes.len(),
        (Some(0), true) => layout.nodes.len() - 1,
        (Some(index), true) => index - 1,
        (None, false) => 0,
        (None, true) => layout.nodes.len() - 1,
    };
    state.selection = Some(layout.nodes[next].key.clone());
}

fn panel(
    ui: &mut Ui,
    state: &mut ExplorerState,
    layout: &ExplorerLayout,
    size: Vec2,
) -> Option<ExplorerAction> {
    let mut action = None;
    let mut focus_here: Option<Focus> = None;

    egui::Frame::new()
        .fill(theme::surface())
        .stroke(Stroke::new(1.0, theme::border()))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_min_size(vec2(size.x - 24.0, size.y - 24.0));
            ui.set_max_width(size.x - 24.0);

            let focus_key = state.focus_key().unwrap_or_default();
            let selected_key = state
                .selection
                .clone()
                .filter(|key| layout.graph.label(key).is_some())
                .unwrap_or_else(|| focus_key.clone());
            let selected = layout.graph.label(&selected_key);
            let selected_is_focus = selected_key == focus_key;

            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Direct relationships")
                        .font(theme::strong_font(12.5))
                        .color(theme::text()),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if !selected_is_focus
                        && ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Back to focus")
                                        .color(theme::muted())
                                        .size(10.0),
                                )
                                .frame(false),
                            )
                            .clicked()
                    {
                        state.selection = None;
                    }
                });
            });

            let Some(selected) = selected else {
                ui.label(
                    RichText::new("Select a party in the graph to see their links.")
                        .color(theme::faint())
                        .size(10.5),
                );
                return;
            };

            ui.add_space(2.0);
            ui.label(
                RichText::new(&selected.label)
                    .font(theme::strong_font(15.0))
                    .color(theme::text()),
            );
            ui.horizontal(|ui| {
                theme::pill(ui, selected.kind.label(), focus_pill_colour(selected.kind));
                let hop = layout
                    .nodes
                    .iter()
                    .find(|node| node.key == selected_key)
                    .map(|node| node.hop);
                ui.label(
                    RichText::new(match hop {
                        Some(0) => "Focus party".to_owned(),
                        Some(hop) => format!(
                            "{hop} level{} from the focus",
                            if hop == 1 { "" } else { "s" }
                        ),
                        None => "Outside the current levels".to_owned(),
                    })
                    .color(theme::faint())
                    .size(10.0),
                );
            });

            if let Some(links) = layout.hubs.get(&selected_key).copied() {
                ui.add_space(3.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("{links} links · neighbours held back"))
                            .color(theme::warning())
                            .size(9.5),
                    );
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Expand").color(theme::accent()).size(10.0),
                            )
                            .frame(false),
                        )
                        .on_hover_text("Show this party's neighbours")
                        .clicked()
                    {
                        state.expanded.insert(selected_key.clone());
                        state.invalidate();
                    }
                });
            } else if state.expanded.contains(&selected_key) {
                ui.add_space(3.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new("Neighbours expanded")
                            .color(theme::success())
                            .size(9.5),
                    );
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Collapse").color(theme::muted()).size(10.0),
                            )
                            .frame(false),
                        )
                        .clicked()
                    {
                        state.expanded.remove(&selected_key);
                        state.invalidate();
                    }
                });
            }

            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if selected.kind == PartyKind::Contact
                    && !selected_is_focus
                    && ui.add(theme::secondary_button("Open record")).clicked()
                {
                    action = Some(ExplorerAction::OpenContact(selected.id.clone()));
                }
                if !selected_is_focus && ui.add(theme::primary_button("Focus here")).clicked() {
                    focus_here = Some(Focus::new(selected.kind, &selected.id, &selected.label));
                }
            });

            ui.add_space(6.0);
            ui.separator();

            let edges = layout.graph.direct_edges(&selected_key);
            ui.label(
                RichText::new(format!(
                    "{} direct link{}",
                    edges.len(),
                    if edges.len() == 1 { "" } else { "s" }
                ))
                .color(theme::faint())
                .size(10.0),
            );
            ui.add_space(2.0);

            if edges.is_empty() {
                ui.add_space(8.0);
                ui.label(
                    RichText::new(if layout.graph.edges().is_empty() {
                        "No active relationships to explore yet."
                    } else {
                        "No direct links match the current filters."
                    })
                    .color(theme::faint())
                    .size(10.5),
                );
            } else {
                let list_height = (size.y - 205.0).max(90.0);
                egui::ScrollArea::vertical()
                    .id_salt(("explorer-direct", &selected_key))
                    .max_height(list_height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for edge in edges {
                            match direct_row(ui, edge, &selected_key) {
                                Some(DirectRowAction::OpenContact(id)) => {
                                    action = Some(ExplorerAction::OpenContact(id));
                                }
                                Some(DirectRowAction::Select(key)) => {
                                    state.selection = Some(key);
                                }
                                None => {}
                            }
                        }
                    });
            }
        });

    if let Some(focus) = focus_here {
        state.focus_on(focus.kind, &focus.id, &focus.label);
    }
    action
}

fn direct_row(ui: &mut Ui, edge: &ApiRelationship, selected_key: &str) -> Option<DirectRowAction> {
    let from_key = node_key(edge.from_kind, &edge.from_id);
    let (other_kind, other_id, other_name, my_role, their_role) = if from_key == selected_key {
        (
            edge.to_kind,
            edge.to_id.as_str(),
            edge.to_name.as_str(),
            edge.from_role.as_str(),
            edge.to_role.as_str(),
        )
    } else {
        (
            edge.from_kind,
            edge.from_id.as_str(),
            edge.from_name.as_str(),
            edge.to_role.as_str(),
            edge.from_role.as_str(),
        )
    };

    let mut action = None;
    egui::Frame::new()
        .fill(theme::surface_raised())
        .stroke(Stroke::new(1.0, theme::border()))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin::same(9))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(other_name)
                            .font(theme::strong_font(11.5))
                            .color(theme::text()),
                    );
                    ui.horizontal_wrapped(|ui| {
                        theme::pill(ui, other_kind.label(), focus_pill_colour(other_kind));
                        let (status, colour) = status_label(edge);
                        theme::pill(ui, status, colour);
                    });
                });
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    if other_kind == PartyKind::Contact
                        && ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Open").color(theme::accent()).size(10.0),
                                )
                                .frame(false),
                            )
                            .on_hover_text("Open this party's record")
                            .clicked()
                    {
                        action = Some(DirectRowAction::OpenContact(other_id.to_owned()));
                    }

                    let other_key = node_key(other_kind, other_id);
                    let already_selected = other_key == selected_key;
                    let select = ui.add_enabled(
                        !already_selected,
                        egui::Button::new(
                            RichText::new(if already_selected {
                                "Selected"
                            } else {
                                "Select"
                            })
                            .color(if already_selected {
                                theme::faint()
                            } else {
                                theme::text()
                            })
                            .size(10.0),
                        )
                        .frame(false),
                    );
                    if select
                        .on_hover_text("Show this party's direct relationships in the side pane")
                        .clicked()
                    {
                        action = Some(DirectRowAction::Select(other_key));
                    }
                });
            });

            ui.label(
                RichText::new(format!(
                    "{} ↔ {}",
                    relationship_role_label(my_role),
                    relationship_role_label(their_role)
                ))
                .color(theme::muted())
                .size(10.0),
            );

            let mut detail = Vec::new();
            if let Some(start) = edge.start_date.as_deref() {
                detail.push(format!("since {start}"));
            }
            if let Some(end) = edge.end_date.as_deref() {
                detail.push(format!("ended {end}"));
            }
            if let Some(reason) = edge.end_reason.as_deref()
                && !reason.trim().is_empty()
            {
                detail.push(relationship_role_label(reason));
            }
            if !detail.is_empty() {
                ui.label(
                    RichText::new(detail.join(" · "))
                        .color(theme::faint())
                        .size(9.5),
                );
            }
            if let Some(notes) = edge.notes.as_deref()
                && !notes.trim().is_empty()
            {
                ui.label(
                    RichText::new(truncate(notes.trim(), 110))
                        .color(theme::faint())
                        .size(9.5),
                );
            }
        });

    action
}

fn fit(state: &mut ExplorerState, layout: &ExplorerLayout, size: Vec2) {
    let usable = (size.x.min(size.y) - 40.0).max(80.0);
    state.target_zoom = (usable / (2.0 * layout.radius.max(1.0))).clamp(0.12, 1.8);
    state.target_pan = Vec2::ZERO;
}

fn zoom_around(state: &mut ExplorerState, scroll: f32, pointer: Pos2, rect: Rect) {
    let old = state.zoom;
    let next = (old * (scroll * 0.0015).exp()).clamp(0.12, 3.0);
    if (next - old).abs() < f32::EPSILON {
        return;
    }
    let centre = rect.center() + state.pan;
    let world = (pointer - centre) / old;
    state.pan = pointer - rect.center() - world * next;
    state.zoom = next;
    state.target_pan = state.pan;
    state.target_zoom = state.zoom;
}

fn hit_test(
    state: &ExplorerState,
    layout: &ExplorerLayout,
    rect: Rect,
    pointer: Pos2,
) -> Option<usize> {
    let world = (pointer - rect.center() - state.pan) / state.zoom;
    layout
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            let display = state.displayed.get(&node.key)?;
            if display.alpha < 0.35 {
                return None;
            }
            let distance = (display.pos.to_vec2() - world).length();
            let target = display.radius + 9.0 / state.zoom.max(0.12);
            (distance <= target).then_some((index, distance))
        })
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(index, _)| index)
}

fn draw_label(
    painter: &egui::Painter,
    rect: Rect,
    position: Pos2,
    radius: f32,
    label: &str,
    emphasised: bool,
    placed: &mut Vec<Rect>,
) {
    let galley = painter.layout_no_wrap(
        truncate(label, 24),
        FontId::proportional(if emphasised { 10.0 } else { 9.0 }),
        if emphasised {
            theme::text()
        } else {
            theme::muted()
        },
    );
    let half = galley.size() / 2.0;
    let anchor = position + vec2(0.0, radius.max(6.0) + 9.0);
    let mut position = pos2(
        anchor
            .x
            .clamp(rect.left() + half.x + 4.0, rect.right() - half.x - 4.0),
        anchor
            .y
            .clamp(rect.top() + half.y + 4.0, rect.bottom() - half.y - 4.0),
    );
    let mut background = Rect::from_center_size(position, galley.size() + vec2(6.0, 2.0));
    for shift in [12.0_f32, -12.0, 24.0, -24.0] {
        if !placed.iter().any(|other| background.intersects(*other)) {
            break;
        }
        position.y =
            (position.y + shift).clamp(rect.top() + half.y + 4.0, rect.bottom() - half.y - 4.0);
        background = Rect::from_center_size(position, galley.size() + vec2(6.0, 2.0));
    }
    placed.push(background);
    painter.rect_filled(background, CornerRadius::same(3), theme::bg());
    painter.galley(position - half, galley, theme::text());
}

fn focus_pill_colour(kind: PartyKind) -> Color32 {
    match kind {
        PartyKind::Contact => theme::accent_blue(),
        PartyKind::Organisation => theme::warning(),
    }
}

fn status_label(edge: &ApiRelationship) -> (&'static str, Color32) {
    if edge.is_ended() {
        ("Ended", theme::danger())
    } else if edge.status == "upcoming" {
        ("Upcoming", theme::warning())
    } else {
        ("Current", theme::success())
    }
}

fn initials(label: &str) -> String {
    let mut initials = label
        .split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(2)
        .collect::<String>();
    if initials.is_empty() {
        initials.push('?');
    }
    initials.make_ascii_uppercase();
    initials
}

fn truncate(value: &str, max_chars: usize) -> String {
    let collected: String = value.chars().take(max_chars + 1).collect();
    if collected.chars().count() > max_chars {
        let mut shortened: String = collected
            .chars()
            .take(max_chars.saturating_sub(1))
            .collect();
        shortened.push('…');
        shortened
    } else {
        collected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(kind: PartyKind, id: &str, label: &str) -> ApiGraphNode {
        ApiGraphNode {
            kind,
            id: id.to_owned(),
            label: label.to_owned(),
            degree: 0,
            depth: None,
        }
    }

    fn edge(id: &str, from: &str, to: &str) -> ApiRelationship {
        ApiRelationship {
            id: id.to_owned(),
            from_kind: PartyKind::Contact,
            from_id: from.to_owned(),
            from_name: format!("Contact {from}"),
            from_role: "care_recipient".to_owned(),
            to_kind: PartyKind::Contact,
            to_id: to.to_owned(),
            to_name: format!("Contact {to}"),
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

    fn catalog() -> Vec<ApiGraphNode> {
        vec![
            node(PartyKind::Contact, "a", "Alice"),
            node(PartyKind::Contact, "b", "Bob"),
            node(PartyKind::Contact, "c", "Carol"),
            node(PartyKind::Contact, "d", "Dave"),
        ]
    }

    fn layout(levels: u8) -> ExplorerLayout {
        build_layout(
            &catalog(),
            &[
                edge("1", "a", "b"),
                edge("2", "b", "c"),
                edge("3", "c", "d"),
            ],
            &Focus::new(PartyKind::Contact, "a", "Alice"),
            levels,
            &GraphFilters::default(),
            1,
            true,
            &BTreeSet::new(),
        )
    }

    #[test]
    fn layout_respects_the_level_count() {
        assert_eq!(layout(1).nodes.len(), 2);
        assert_eq!(layout(2).nodes.len(), 3);
        assert_eq!(layout(3).nodes.len(), 4);
    }

    #[test]
    fn layout_is_deterministic_and_centred_on_the_focus() {
        let first = layout(3);
        let second = layout(3);
        assert_eq!(first.nodes.len(), second.nodes.len());
        for (left, right) in first.nodes.iter().zip(&second.nodes) {
            assert_eq!(left.pos, right.pos);
        }
        let focus = &first.nodes[0];
        assert_eq!(focus.hop, 0);
        assert_eq!(focus.pos, pos2(0.0, 0.0));
        assert!(first.radius > 100.0);
        for node in &first.nodes {
            assert!(node.pos.distance(pos2(0.0, 0.0)) <= first.radius);
        }
    }

    #[test]
    fn level_changes_are_clamped() {
        let mut state = ExplorerState::default();
        assert!(state.set_levels(EXPLORER_MAX_LEVELS + 9));
        assert_eq!(state.levels, EXPLORER_MAX_LEVELS);
        assert!(state.set_levels(0));
        assert_eq!(state.levels, EXPLORER_MIN_LEVELS);
        assert!(!state.set_levels(EXPLORER_MIN_LEVELS));
    }

    #[test]
    fn back_restores_the_previous_focus_and_view() {
        let mut state = ExplorerState::default();
        state.focus_on(PartyKind::Contact, "a", "Alice");
        state.levels = 3;
        state.pan = vec2(12.0, -8.0);
        state.zoom = 1.4;

        state.focus_on(PartyKind::Contact, "b", "Bob");
        assert_eq!(state.levels(), 3);
        assert_eq!(state.history.len(), 1);

        assert!(state.go_back());
        assert_eq!(
            state.current_focus().map(|focus| focus.id.as_str()),
            Some("a")
        );
        assert_eq!(state.levels, 3);
        assert_eq!(state.pan, vec2(12.0, -8.0));
        assert!((state.zoom - 1.4).abs() < f32::EPSILON);
        assert!(!state.go_back());
    }

    #[test]
    fn history_can_be_jumped_to() {
        let mut state = ExplorerState::default();
        state.focus_on(PartyKind::Contact, "a", "Alice");
        state.focus_on(PartyKind::Contact, "b", "Bob");
        state.focus_on(PartyKind::Contact, "c", "Carol");
        assert_eq!(state.history.len(), 2);

        assert!(state.jump_to(0));
        assert_eq!(
            state.current_focus().map(|focus| focus.id.as_str()),
            Some("a")
        );
        assert!(state.history.is_empty());
    }

    #[test]
    fn preferences_round_trip_and_keep_one_kind_visible() {
        let mut state = ExplorerState::default();
        state.apply_preference(ExplorerPreference {
            levels: 4,
            show_ended: true,
            show_contacts: false,
            show_organisations: false,
            skip_hubs: true,
        });
        assert_eq!(state.levels, 4);
        assert!(state.filters.show_ended);
        assert!(state.filters.show_contacts);
        assert_eq!(state.preference().levels, 4);
    }

    #[test]
    fn the_layout_is_capped() {
        let mut catalog = vec![node(PartyKind::Contact, "focus", "Focus")];
        let mut edges = Vec::new();
        for index in 0..(MAX_NODES + 20) {
            let id = format!("n{index}");
            catalog.push(node(PartyKind::Contact, &id, &format!("Node {index}")));
            edges.push(edge(&format!("e{index}"), "focus", &id));
        }
        let layout = build_layout(
            &catalog,
            &edges,
            &Focus::new(PartyKind::Contact, "focus", "Focus"),
            1,
            &GraphFilters::default(),
            1,
            true,
            &BTreeSet::new(),
        );
        assert_eq!(layout.nodes.len(), MAX_NODES);
        assert!(layout.truncated);
    }

    #[test]
    fn hubs_are_badged_and_can_be_expanded() {
        let mut catalog = vec![
            node(PartyKind::Contact, "a", "Alice"),
            node(PartyKind::Organisation, "org", "Care Co"),
        ];
        let mut org_edge = edge("0", "a", "org");
        org_edge.to_kind = PartyKind::Organisation;
        org_edge.to_name = "Care Co".to_owned();
        let mut edges = vec![org_edge];
        for index in 0..20 {
            let id = format!("n{index}");
            catalog.push(node(PartyKind::Contact, &id, &format!("Node {index}")));
            let mut link = edge(&format!("e{index}"), "org", &id);
            link.from_kind = PartyKind::Organisation;
            link.from_name = "Care Co".to_owned();
            edges.push(link);
        }
        let focus = Focus::new(PartyKind::Contact, "a", "Alice");
        let org_key = node_key(PartyKind::Organisation, "org");

        let layout = build_layout(
            &catalog,
            &edges,
            &focus,
            2,
            &GraphFilters::default(),
            1,
            true,
            &BTreeSet::new(),
        );
        assert!(layout.nodes.iter().any(|node| node.id == "org"));
        assert!(!layout.nodes.iter().any(|node| node.id == "n0"));
        assert_eq!(layout.hubs.get(&org_key), Some(&21));

        let mut expanded = BTreeSet::new();
        expanded.insert(org_key.clone());
        let layout = build_layout(
            &catalog,
            &edges,
            &focus,
            2,
            &GraphFilters::default(),
            1,
            true,
            &expanded,
        );
        assert!(layout.nodes.iter().any(|node| node.id == "n0"));
        assert!(
            layout.hubs.is_empty(),
            "an expanded hub is no longer held back"
        );

        let layout = build_layout(
            &catalog,
            &edges,
            &focus,
            2,
            &GraphFilters::default(),
            1,
            false,
            &BTreeSet::new(),
        );
        assert!(layout.nodes.iter().any(|node| node.id == "n0"));
        assert!(layout.hubs.is_empty());
    }

    #[test]
    fn filters_apply_to_the_layout_and_the_direct_list() {
        let mut catalog = catalog();
        catalog.push(node(PartyKind::Organisation, "org", "Care Co"));
        let mut org_edge = edge("4", "a", "org");
        org_edge.to_kind = PartyKind::Organisation;
        org_edge.to_name = "Care Co".to_owned();
        org_edge.to_role = "provider".to_owned();

        let filters = GraphFilters {
            show_contacts: false,
            ..GraphFilters::default()
        };
        let layout = build_layout(
            &catalog,
            &[edge("1", "a", "b"), org_edge],
            &Focus::new(PartyKind::Contact, "a", "Alice"),
            2,
            &filters,
            1,
            true,
            &BTreeSet::new(),
        );
        assert_eq!(layout.nodes.len(), 2);
        assert!(layout.nodes.iter().any(|node| node.id == "org"));
        assert!(
            layout
                .graph
                .direct_edges("contact:a")
                .iter()
                .all(|edge| edge.to_kind == PartyKind::Organisation)
        );
    }

    #[test]
    fn changing_focus_glides_nodes_to_their_new_positions() {
        let mut state = ExplorerState::default();
        state.focus_on(PartyKind::Contact, "a", "Alice");
        let first = layout(1);
        update_displayed(&mut state, &first, 0.05);
        for _ in 0..20 {
            update_displayed(&mut state, &first, 0.016);
        }

        let second = build_layout(
            &catalog(),
            &[edge("1", "a", "b"), edge("2", "b", "c")],
            &Focus::new(PartyKind::Contact, "b", "Bob"),
            2,
            &GraphFilters::default(),
            1,
            true,
            &BTreeSet::new(),
        );
        for _ in 0..60 {
            update_displayed(&mut state, &second, 0.016);
        }

        let focus = state
            .displayed
            .get(&node_key(PartyKind::Contact, "b"))
            .expect("the new focus is drawn");
        assert!(focus.pos.to_vec2().length() < 0.5);
        assert!(focus.alpha > 0.95);
        let carol = state
            .displayed
            .get(&node_key(PartyKind::Contact, "c"))
            .expect("a new party fades in");
        assert!(carol.alpha > 0.95);
    }

    #[test]
    fn removed_parties_fade_out() {
        let mut state = ExplorerState::default();
        state.focus_on(PartyKind::Contact, "a", "Alice");
        let two_levels = layout(2);
        for _ in 0..30 {
            update_displayed(&mut state, &two_levels, 0.016);
        }
        assert!(
            state
                .displayed
                .contains_key(&node_key(PartyKind::Contact, "c"))
        );

        let one_level = layout(1);
        update_displayed(&mut state, &one_level, 0.016);
        let carol = state
            .displayed
            .get(&node_key(PartyKind::Contact, "c"))
            .expect("the old node lingers while it fades");
        assert!(carol.alpha < 1.0);
        for _ in 0..80 {
            update_displayed(&mut state, &one_level, 0.016);
        }
        assert!(
            !state
                .displayed
                .contains_key(&node_key(PartyKind::Contact, "c"))
        );
    }

    #[test]
    fn the_camera_eases_towards_the_fitted_view() {
        let mut state = ExplorerState::default();
        let layout = layout(2);
        fit(&mut state, &layout, vec2(640.0, 420.0));
        assert!(state.zoom == 1.0, "fitting only sets the camera goal");
        assert!(update_camera(&mut state, 0.016));
        assert!(state.zoom < 1.0 && state.zoom > state.target_zoom);
        assert!(state.zoom > state.target_zoom);
        for _ in 0..120 {
            update_camera(&mut state, 0.016);
        }
        assert!((state.zoom - state.target_zoom).abs() < 0.001);
        assert!(!update_camera(&mut state, 0.016));
    }

    #[test]
    fn selecting_a_party_shows_their_direct_relationships() {
        use egui_kittest::{Harness, kittest::Queryable};

        struct PageState {
            explorer: ExplorerState,
            catalog: Vec<ApiGraphNode>,
            network: RelationshipNetwork,
        }

        let mut explorer = ExplorerState::default();
        explorer.focus_on(PartyKind::Contact, "a", "Alice");
        explorer.selection = Some(node_key(PartyKind::Contact, "b"));

        let page = PageState {
            explorer,
            catalog: catalog(),
            network: RelationshipNetwork {
                edges: vec![edge("1", "a", "b"), edge("2", "b", "c")],
                revision: 1,
            },
        };

        let mut harness = Harness::builder()
            .with_size(egui::vec2(1100.0, 760.0))
            .with_max_steps(20)
            .with_context_setup(|ctx| theme::apply(ctx, theme::ThemeMode::Dark))
            .build_ui_state(
                move |ui, page: &mut PageState| {
                    let source = ExplorerSource {
                        nodes: &page.catalog,
                        network: &page.network,
                        loading: false,
                        error: None,
                    };
                    show(ui, &mut page.explorer, &source);
                },
                page,
            );

        harness.run();
        assert!(
            harness
                .query_all_by_label_contains("Direct relationships")
                .next()
                .is_some()
        );
        assert!(
            harness.query_all_by_label_contains("Bob").next().is_some(),
            "the selected party's name is shown"
        );
        assert!(
            harness
                .query_all_by_label_contains("Emergency Contact")
                .next()
                .is_some()
        );

        harness.get_by_label("Focus here").click();
        harness.run();
        assert_eq!(
            harness
                .state()
                .explorer
                .current_focus()
                .map(|focus| focus.id.as_str()),
            Some("b")
        );

        harness.get_by_label("← Back").click();
        harness.run();
        assert_eq!(
            harness
                .state()
                .explorer
                .current_focus()
                .map(|focus| focus.id.as_str()),
            Some("a")
        );

        harness.get_by_label("Select").click();
        harness.run();
        assert_eq!(
            harness.state().explorer.selection.as_deref(),
            Some("contact:b")
        );
        assert!(
            harness
                .query_all_by_label_contains("Alice")
                .next()
                .is_some(),
            "the side pane now lists Bob's relationships"
        );
    }

    #[test]
    fn a_hub_can_be_expanded_from_the_panel() {
        use egui_kittest::{Harness, kittest::Queryable};

        struct PageState {
            explorer: ExplorerState,
            catalog: Vec<ApiGraphNode>,
            network: RelationshipNetwork,
        }

        let mut catalog = vec![
            node(PartyKind::Contact, "a", "Alice"),
            node(PartyKind::Organisation, "org", "Care Co"),
        ];
        let mut org_edge = edge("0", "a", "org");
        org_edge.to_kind = PartyKind::Organisation;
        org_edge.to_name = "Care Co".to_owned();
        let mut edges = vec![org_edge];
        for index in 0..20 {
            let id = format!("n{index}");
            catalog.push(node(PartyKind::Contact, &id, &format!("Node {index}")));
            let mut link = edge(&format!("e{index}"), "org", &id);
            link.from_kind = PartyKind::Organisation;
            link.from_name = "Care Co".to_owned();
            edges.push(link);
        }

        let mut explorer = ExplorerState::default();
        explorer.focus_on(PartyKind::Contact, "a", "Alice");
        explorer.selection = Some(node_key(PartyKind::Organisation, "org"));

        let page = PageState {
            explorer,
            catalog,
            network: RelationshipNetwork { edges, revision: 1 },
        };

        let mut harness = Harness::builder()
            .with_size(egui::vec2(1100.0, 760.0))
            .with_max_steps(20)
            .with_context_setup(|ctx| theme::apply(ctx, theme::ThemeMode::Dark))
            .build_ui_state(
                move |ui, page: &mut PageState| {
                    let source = ExplorerSource {
                        nodes: &page.catalog,
                        network: &page.network,
                        loading: false,
                        error: None,
                    };
                    show(ui, &mut page.explorer, &source);
                },
                page,
            );

        harness.run();
        assert!(
            harness
                .query_all_by_label_contains("neighbours held back")
                .next()
                .is_some(),
            "a hub shows that its links are held back"
        );

        harness.get_by_label("Expand").click();
        harness.run();
        assert!(
            harness
                .state()
                .explorer
                .expanded
                .contains(&node_key(PartyKind::Organisation, "org"))
        );
        assert!(
            harness
                .query_all_by_label_contains("Neighbours expanded")
                .next()
                .is_some()
        );
    }
}
