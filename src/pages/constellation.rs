//! The relationship constellation shown on a contact record.
//!
//! The whole-tenant relationship graph is already loaded for the party lookup,
//! so the constellation is built client-side: a breadth-first walk from the
//! open contact, capped at the number of steps the user has chosen.

use std::collections::{BTreeMap, BTreeSet};

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Stroke, StrokeKind,
    Ui, Vec2, pos2, vec2,
};

use crate::config::{
    CONSTELLATION_DEFAULT_HEIGHT, CONSTELLATION_MAX_HEIGHT, CONSTELLATION_MAX_STEPS,
    CONSTELLATION_MIN_HEIGHT, CONSTELLATION_MIN_STEPS, RelationshipViewPreference,
};
use crate::contact::{ApiGraphNode, ApiRelationship, PartyKind, relationship_role_label};
use crate::graph::{
    FocusTrace, GraphFilters, HUB_BADGE_MIN, HUB_DEGREE, RelationshipGraph, WalkPolicy, node_key,
    transition_factor,
};
use crate::theme;

pub use crate::graph::RelationshipNetwork;

/// Total nodes drawn. Beyond this the graph becomes unreadable, so we stop and
/// point the user at the relationship list instead.
pub const MAX_NODES: usize = 60;
/// Height of the drag strip under the canvas.
const RESIZE_HANDLE_HEIGHT: f32 = 12.0;

/// Interpolated positions for the constellation, so changing the step count or
/// resizing the record glides nodes to their new rings instead of snapping.
#[derive(Default)]
pub struct ConstellationAnimation {
    displayed: BTreeMap<String, AnimatedNode>,
    last_frame: f64,
}

#[derive(Clone, Debug)]
pub struct AnimatedNode {
    kind: PartyKind,
    label: String,
    hop: usize,
    pos: Pos2,
    radius: f32,
    alpha: f32,
}

impl ConstellationAnimation {
    pub fn clear(&mut self) {
        self.displayed.clear();
        self.last_frame = 0.0;
    }

    fn get(&self, kind: PartyKind, id: &str) -> Option<&AnimatedNode> {
        self.displayed.get(&node_key(kind, id))
    }

    fn update(&mut self, view: &ConstellationView, ctx: &egui::Context, now: f64) -> bool {
        let dt = if self.last_frame <= 0.0 {
            0.0
        } else {
            (now - self.last_frame).clamp(0.0, 0.12) as f32
        };
        self.last_frame = now;
        let factor = transition_factor(dt);
        let mut moving = false;
        let mut targets = BTreeSet::new();

        for node in &view.nodes {
            let key = node_key(node.kind, &node.id);
            targets.insert(key.clone());
            match self.displayed.get_mut(&key) {
                Some(display) => {
                    display.label = node.label.clone();
                    display.hop = node.hop;
                    let dx = node.pos.x - display.pos.x;
                    let dy = node.pos.y - display.pos.y;
                    let dr = node.radius - display.radius;
                    let da = 1.0 - display.alpha;
                    if dx.abs() + dy.abs() > 0.05 || dr.abs() > 0.05 || da.abs() > 0.005 {
                        moving = true;
                    }
                    display.pos.x += dx * factor;
                    display.pos.y += dy * factor;
                    display.radius += dr * factor;
                    display.alpha += da * factor;
                }
                None => {
                    self.displayed.insert(
                        key,
                        AnimatedNode {
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

        let ghosts: Vec<String> = self
            .displayed
            .keys()
            .filter(|key| !targets.contains(*key))
            .cloned()
            .collect();
        for key in ghosts {
            if let Some(display) = self.displayed.get_mut(&key) {
                display.alpha -= display.alpha * factor;
                if display.alpha < 0.02 {
                    self.displayed.remove(&key);
                } else {
                    moving = true;
                }
            }
        }

        if moving {
            ctx.request_repaint();
        }
        moving
    }
}

/// Everything the constellation needs from the application shell.
pub struct ConstellationSource<'a> {
    /// Party lookup nodes, including locally known contacts that the graph does
    /// not contain yet.
    pub nodes: &'a [ApiGraphNode],
    pub network: &'a RelationshipNetwork,
    pub loading: bool,
    pub error: Option<&'a str>,
}

impl ConstellationSource<'_> {
    pub fn nodes(&self) -> &[ApiGraphNode] {
        self.nodes
    }
}

/// One party the user asked to open from the constellation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartyTarget {
    pub kind: PartyKind,
    pub id: String,
}

/// What the section wants the record to do after this frame.
#[derive(Default)]
pub struct SectionResult {
    pub preference_changed: bool,
    /// A party double-clicked in the graph. Contacts open their record and
    /// organisations focus their editor, decided by the caller.
    pub open_party: Option<PartyTarget>,
    pub reload: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewKey {
    steps: u8,
    width: u32,
    height: u32,
    revision: u64,
    focus: String,
    focus_kind: PartyKind,
    skip_hubs: bool,
}

#[derive(Clone, Debug)]
pub struct ViewNode {
    pub kind: PartyKind,
    pub id: String,
    pub label: String,
    pub hop: usize,
    /// The role this node holds on the edge it was reached through.
    pub role: String,
    /// Position inside the canvas, before the canvas origin is added.
    pub pos: Pos2,
    pub radius: f32,
}

#[derive(Clone, Debug)]
pub struct ViewEdge {
    pub from: usize,
    pub to: usize,
    pub from_role: String,
    pub to_role: String,
}

#[derive(Clone, Debug)]
pub struct ConstellationView {
    pub key: ViewKey,
    pub nodes: Vec<ViewNode>,
    pub edges: Vec<ViewEdge>,
    /// Suppressed parties and their direct link counts.
    pub hubs: BTreeMap<String, usize>,
    pub truncated: bool,
}

/// Draw the collapsible constellation section inside the Relationships card.
#[allow(clippy::too_many_arguments)]
pub fn section(
    ui: &mut Ui,
    source: &ConstellationSource<'_>,
    focus_kind: PartyKind,
    focus_id: &str,
    focus_label: &str,
    preference: &mut RelationshipViewPreference,
    cache: &mut Option<ConstellationView>,
    animation: &mut ConstellationAnimation,
    selection: &mut Option<String>,
) -> SectionResult {
    let mut result = SectionResult::default();
    let focus_noun = focus_noun(focus_kind);

    ui.horizontal(|ui| {
        let arrow = if preference.collapsed { "▶" } else { "▼" };
        let response = ui.add(
            egui::Button::new(
                RichText::new(format!("{arrow}  Relationship constellation"))
                    .font(theme::strong_font(12.5))
                    .color(theme::text()),
            )
            .frame(false),
        );
        if response.clicked() {
            preference.collapsed = !preference.collapsed;
            result.preference_changed = true;
        }
        response.on_hover_text(if preference.collapsed {
            format!("Show this {focus_noun}'s relationship network")
        } else {
            format!("Hide this {focus_noun}'s relationship network")
        });

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(format!(
                    "{} step{}",
                    preference.steps,
                    if preference.steps == 1 { "" } else { "s" }
                ))
                .color(theme::faint())
                .size(10.0),
            );
        });
    });

    if preference.collapsed {
        animation.clear();
        return result;
    }

    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Show up to").color(theme::muted()).size(10.5));
        let decrease = ui.add_enabled(
            preference.steps > CONSTELLATION_MIN_STEPS,
            egui::Button::new(RichText::new("−").size(13.0)).min_size(vec2(24.0, 22.0)),
        );
        if decrease.clicked() {
            preference.steps = preference
                .steps
                .saturating_sub(1)
                .max(CONSTELLATION_MIN_STEPS);
            result.preference_changed = true;
        }
        decrease.on_hover_text("Show fewer steps");

        ui.label(
            RichText::new(format!(
                "{} step{}",
                preference.steps,
                if preference.steps == 1 { "" } else { "s" }
            ))
            .font(theme::strong_font(11.0))
            .color(theme::text()),
        );

        let increase = ui.add_enabled(
            preference.steps < CONSTELLATION_MAX_STEPS,
            egui::Button::new(RichText::new("+").size(13.0)).min_size(vec2(24.0, 22.0)),
        );
        if increase.clicked() {
            preference.steps = (preference.steps + 1).min(CONSTELLATION_MAX_STEPS);
            result.preference_changed = true;
        }
        increase.on_hover_text("Show more steps");

        ui.label(
            RichText::new(format!("away from this {focus_noun}"))
                .color(theme::faint())
                .size(10.0),
        );

        ui.add_space(4.0);
        if ui
            .checkbox(&mut preference.skip_hubs, "Skip hubs")
            .on_hover_text(
                "Show highly connected parties, such as the organisation everyone belongs to, without expanding all of their links",
            )
            .changed()
        {
            result.preference_changed = true;
        }
    });

    ui.label(
        RichText::new(format!(
            "Each ring is one step from this {focus_noun}. Click a party to inspect it, or double-click a contact or organisation to open it."
        ))
        .color(theme::faint())
        .size(10.0),
    );
    ui.add_space(4.0);

    if let Some(error) = source.error {
        ui.label(
            RichText::new(format!("Could not load the relationship network: {error}"))
                .color(theme::danger())
                .size(10.5),
        );
        if ui.small_button("Try again").clicked() {
            result.reload = true;
        }
        ui.add_space(4.0);
    }

    if source.loading && source.network.edges.is_empty() {
        ui.label(
            RichText::new("Loading the relationship network…")
                .color(theme::muted())
                .size(10.5),
        );
        return result;
    }

    let width = ui.available_width().max(180.0);
    let height = preference
        .height
        .clamp(CONSTELLATION_MIN_HEIGHT, CONSTELLATION_MAX_HEIGHT);
    let key = ViewKey {
        steps: preference.steps,
        width: width.round() as u32,
        height: height.round() as u32,
        revision: source.network.revision,
        focus: focus_id.to_owned(),
        focus_kind,
        skip_hubs: preference.skip_hubs,
    };
    if cache.as_ref().map(|view| &view.key) != Some(&key) {
        let mut view = build_view(
            source.nodes(),
            &source.network.edges,
            focus_kind,
            focus_id,
            focus_label,
            preference.steps,
            vec2(width, height),
            preference.skip_hubs,
        );
        view.key = key.clone();
        *cache = Some(view);
    }
    let Some(view) = cache.as_ref() else {
        return result;
    };

    if selection.as_ref().is_some_and(|key| {
        !view
            .nodes
            .iter()
            .any(|node| &node_key(node.kind, &node.id) == key)
    }) {
        *selection = None;
    }

    let now = ui.ctx().input(|input| input.time);
    animation.update(view, ui.ctx(), now);

    if view.nodes.len() <= 1 {
        ui.label(
            RichText::new("No active relationships to visualise yet.")
                .color(theme::faint())
                .size(10.5),
        );
        return result;
    }

    if view.truncated {
        ui.label(
            RichText::new(format!(
                "Showing the first {MAX_NODES} connections. Reduce the steps to see fewer people, or use the relationship list below."
            ))
            .color(theme::warning())
            .size(10.0),
        );
        ui.add_space(2.0);
    }

    if let Some(target) = draw_canvas(
        ui,
        view,
        focus_kind,
        focus_id,
        vec2(width, height),
        animation,
        selection,
    ) {
        result.open_party = Some(target);
    }

    if resize_handle(ui, preference) {
        result.preference_changed = true;
    }

    ui.add_space(3.0);
    legend(ui, preference.steps, focus_kind);
    if !view.hubs.is_empty() {
        ui.label(
            RichText::new(
                "Highly connected parties are shown but not expanded. Clear Skip hubs to include their links.",
            )
            .color(theme::faint())
            .size(9.5),
        );
    }

    result
}

/// The lower-case noun for the party a constellation is centred on.
fn focus_noun(kind: PartyKind) -> &'static str {
    match kind {
        PartyKind::Contact => "contact",
        PartyKind::Organisation => "organisation",
    }
}

/// A drag strip under the canvas. Dragging changes the height continuously;
/// releasing or double-clicking persists it, so a drag writes the account
/// preference once rather than every frame. Arrow keys resize when focused.
fn resize_handle(ui: &mut Ui, preference: &mut RelationshipViewPreference) -> bool {
    let mut persist = false;
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), RESIZE_HANDLE_HEIGHT),
        Sense::click_and_drag(),
    );
    let hovered = response.hovered() || response.dragged();
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }

    if response.clicked() {
        response.request_focus();
    }

    if response.dragged() {
        preference.height = (preference.height + response.drag_delta().y)
            .clamp(CONSTELLATION_MIN_HEIGHT, CONSTELLATION_MAX_HEIGHT);
    }
    if response.drag_stopped() {
        persist = true;
    }

    let mut nudge = 0.0;
    if response.has_focus() {
        let step = if ui.input(|input| input.modifiers.shift) {
            40.0
        } else {
            12.0
        };
        if ui.input(|input| input.key_pressed(egui::Key::ArrowDown)) {
            nudge -= step;
        }
        if ui.input(|input| input.key_pressed(egui::Key::ArrowUp)) {
            nudge += step;
        }
        if ui.input(|input| input.key_pressed(egui::Key::Home)) {
            preference.height = CONSTELLATION_DEFAULT_HEIGHT;
            persist = true;
        }
    }
    if nudge != 0.0 {
        preference.height =
            (preference.height + nudge).clamp(CONSTELLATION_MIN_HEIGHT, CONSTELLATION_MAX_HEIGHT);
        persist = true;
    }
    if response.double_clicked() {
        preference.height = CONSTELLATION_DEFAULT_HEIGHT;
        persist = true;
    }

    let grip = Rect::from_center_size(rect.center(), vec2(46.0, 3.0));
    let colour = if hovered {
        theme::accent()
    } else {
        theme::border_strong()
    };
    ui.painter()
        .rect_filled(grip, CornerRadius::same(2), colour);

    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Splitter,
            true,
            "Resize relationship constellation",
        )
    });
    response.on_hover_text(
        "Drag to resize the constellation. Double-click or press Home to reset the height.",
    );

    persist
}

fn legend(ui: &mut Ui, steps: u8, focus_kind: PartyKind) {
    ui.horizontal(|ui| {
        legend_swatch(ui, true, theme::accent_blue());
        ui.label(RichText::new("Contact").color(theme::muted()).size(9.5));
        ui.add_space(8.0);
        legend_swatch(ui, false, theme::warning());
        ui.label(
            RichText::new("Organisation")
                .color(theme::muted())
                .size(9.5),
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new(format!(
                "Focus {} · rings show up to {steps} step{}",
                focus_noun(focus_kind),
                if steps == 1 { "" } else { "s" }
            ))
            .color(theme::faint())
            .size(9.5),
        );
    });
}

fn legend_swatch(ui: &mut Ui, circle: bool, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(9.0, 9.0), Sense::hover());
    let painter = ui.painter();
    if circle {
        painter.circle_filled(rect.center(), 4.0, colour);
    } else {
        painter.rect_filled(rect.shrink(0.5), CornerRadius::same(2), colour);
    }
}

fn draw_canvas(
    ui: &mut Ui,
    view: &ConstellationView,
    focus_kind: PartyKind,
    focus_id: &str,
    size: Vec2,
    animation: &ConstellationAnimation,
    selection: &mut Option<String>,
) -> Option<PartyTarget> {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect.shrink(0.5), CornerRadius::same(10), theme::bg());
    painter.rect_stroke(
        rect.shrink(0.5),
        CornerRadius::same(10),
        Stroke::new(1.0, theme::border()),
        StrokeKind::Inside,
    );

    let focus = view.nodes.iter().find(|node| node.hop == 0);
    let focus_centre = focus.map_or(rect.center(), |node| rect.min + node.pos.to_vec2());

    for hop in 1..=CONSTELLATION_MAX_STEPS {
        if hop as usize > view.key.steps as usize {
            break;
        }
        let radius = ring_radius(size, view.key.steps, hop);
        painter.circle_stroke(
            focus_centre,
            radius,
            Stroke::new(
                1.0,
                Color32::from_rgba_unmultiplied(
                    theme::border().r(),
                    theme::border().g(),
                    theme::border().b(),
                    70,
                ),
            ),
        );
    }

    let target_keys: BTreeSet<String> = view
        .nodes
        .iter()
        .map(|node| node_key(node.kind, &node.id))
        .collect();
    let pointer = response.hover_pos().filter(|pos| rect.contains(*pos));
    let hovered_key = pointer.and_then(|pos| {
        animation
            .displayed
            .iter()
            .filter(|(key, node)| target_keys.contains(*key) && node.alpha > 0.35)
            .filter_map(|(key, node)| {
                let centre = rect.min + node.pos.to_vec2();
                let distance = centre.distance(pos);
                (distance <= node.radius + 7.0).then_some((key, distance))
            })
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .map(|(key, _)| key.clone())
    });
    let hovered = hovered_key.as_ref().and_then(|key| {
        view.nodes
            .iter()
            .position(|node| &node_key(node.kind, &node.id) == key)
    });
    let hovered_node = hovered.map(|index| &view.nodes[index]);
    let focus_index = view.nodes.iter().position(|node| node.hop == 0);
    let highlight_index = hovered.or_else(|| {
        selection.as_deref().and_then(|key| {
            view.nodes
                .iter()
                .position(|node| node_key(node.kind, &node.id) == key)
        })
    });
    let trace = highlight_index.zip(focus_index).map(|(start, focus)| {
        FocusTrace::build(
            view.nodes.len(),
            view.edges.iter().map(|edge| (edge.from, edge.to)),
            start,
            focus,
        )
    });
    let trace_colour = trace.as_ref().map(|trace| {
        if trace.path_length() == 1 {
            theme::accent()
        } else {
            theme::relationship_path()
        }
    });

    for edge in &view.edges {
        let from_node = &view.nodes[edge.from];
        let to_node = &view.nodes[edge.to];
        let (Some(from), Some(to)) = (
            animation.get(from_node.kind, &from_node.id),
            animation.get(to_node.kind, &to_node.id),
        ) else {
            continue;
        };
        let alpha = from.alpha.min(to.alpha);
        if alpha < 0.02 {
            continue;
        }
        let on_path = trace
            .as_ref()
            .is_some_and(|trace| trace.edge_on_path(edge.from, edge.to));
        let direct = trace
            .as_ref()
            .is_some_and(|trace| trace.touches_start(edge.from, edge.to));
        let (colour, width) = if on_path {
            (
                with_alpha(trace_colour.unwrap_or_else(theme::accent), alpha),
                2.6,
            )
        } else if direct {
            (with_alpha(theme::accent(), alpha), 2.2)
        } else {
            let base = if trace.is_some() { 26.0 } else { 80.0 };
            (
                Color32::from_rgba_unmultiplied(
                    theme::muted().r(),
                    theme::muted().g(),
                    theme::muted().b(),
                    (base * alpha) as u8,
                ),
                1.0,
            )
        };
        painter.line_segment(
            [rect.min + from.pos.to_vec2(), rect.min + to.pos.to_vec2()],
            Stroke::new(width, colour),
        );
    }

    let labels_everywhere = view.nodes.len() <= 10;
    let label_first_hop = view.nodes.iter().filter(|node| node.hop == 1).count() <= 14;
    let mut placed_labels: Vec<Rect> = Vec::new();
    let mut displays: Vec<(&String, &AnimatedNode)> = animation.displayed.iter().collect();
    displays.sort_by_key(|(_, node)| node.alpha < 0.5);
    for (key, node) in displays {
        let view_index = view
            .nodes
            .iter()
            .position(|view_node| &node_key(view_node.kind, &view_node.id) == key);
        let on_path = view_index
            .is_some_and(|index| trace.as_ref().is_some_and(|trace| trace.is_on_path(index)));
        let direct = view_index
            .is_some_and(|index| trace.as_ref().is_some_and(|trace| trace.is_direct(index)));
        let dimmed = trace.is_some() && !on_path && !direct;
        let alpha = node.alpha.clamp(0.0, 1.0) * if dimmed { 0.4 } else { 1.0 };
        if alpha < 0.02 {
            continue;
        }
        let centre = rect.min + node.pos.to_vec2();
        let is_focus = node.hop == 0;
        let is_hovered = hovered_key.as_deref() == Some(key.as_str());
        let is_selected = selection.as_deref() == Some(key.as_str());
        let is_target = target_keys.contains(key);
        let colour = if is_focus {
            theme::accent()
        } else if node.kind == PartyKind::Organisation {
            theme::warning()
        } else {
            theme::accent_blue()
        };
        let fill = Color32::from_rgba_unmultiplied(
            colour.r(),
            colour.g(),
            colour.b(),
            (55.0 * alpha) as u8,
        );

        if is_focus {
            painter.circle_stroke(
                centre,
                node.radius + 4.0,
                Stroke::new(1.5, theme::accent_soft((160.0 * alpha) as u8)),
            );
        }
        if is_selected && !is_focus {
            painter.circle_stroke(
                centre,
                node.radius + 3.0,
                Stroke::new(1.5, with_alpha(theme::text(), alpha)),
            );
        }

        let stroke = Stroke::new(
            if is_hovered { 2.2 } else { 1.4 },
            with_alpha(colour, alpha),
        );
        match node.kind {
            PartyKind::Contact => {
                painter.circle_filled(centre, node.radius, fill);
                painter.circle_stroke(centre, node.radius, stroke);
            }
            PartyKind::Organisation => {
                let box_rect = Rect::from_center_size(centre, Vec2::splat(node.radius * 1.85));
                painter.rect_filled(box_rect, CornerRadius::same(3), fill);
                painter.rect_stroke(box_rect, CornerRadius::same(3), stroke, StrokeKind::Inside);
            }
        }

        painter.text(
            centre,
            Align2::CENTER_CENTER,
            initials(&node.label),
            FontId::proportional((node.radius * 0.75).clamp(5.5, 11.0)),
            with_alpha(colour, alpha),
        );

        if on_path {
            let colour = with_alpha(trace_colour.unwrap_or_else(theme::accent), alpha);
            match node.kind {
                PartyKind::Contact => {
                    painter.circle_stroke(centre, node.radius + 3.5, Stroke::new(2.0, colour));
                }
                PartyKind::Organisation => {
                    let ring =
                        Rect::from_center_size(centre, Vec2::splat(node.radius * 1.85 + 7.0));
                    painter.rect_stroke(
                        ring,
                        CornerRadius::same(5),
                        Stroke::new(2.0, colour),
                        StrokeKind::Inside,
                    );
                }
            }
        }

        if is_target
            && alpha > 0.55
            && let Some(links) = view.hubs.get(key)
            && *links >= HUB_BADGE_MIN
        {
            let galley = painter.layout_no_wrap(
                format!("{links}"),
                FontId::proportional(8.5),
                with_alpha(theme::warning(), alpha),
            );
            let badge = Rect::from_min_size(
                centre + vec2(node.radius + 2.0, -node.radius - 9.0),
                galley.size() + vec2(6.0, 2.0),
            );
            painter.rect_filled(badge, CornerRadius::same(3), theme::bg());
            painter.rect_stroke(
                badge,
                CornerRadius::same(3),
                Stroke::new(1.0, with_alpha(theme::border(), alpha)),
                StrokeKind::Inside,
            );
            painter.galley(
                badge.min + vec2(3.0, 1.0),
                galley,
                with_alpha(theme::warning(), alpha),
            );
        }

        if is_target
            && !dimmed
            && alpha > 0.55
            && (is_focus
                || is_hovered
                || on_path
                || (node.hop == 1 && label_first_hop)
                || (labels_everywhere && node.hop > 1))
        {
            draw_node_label(
                &painter,
                rect,
                focus_centre,
                centre,
                node,
                is_hovered,
                &mut placed_labels,
            );
        }
    }

    if let Some(index) = hovered {
        let node = &view.nodes[index];
        let is_focus = node.kind == focus_kind && node.id == focus_id;
        if !is_focus {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        let mut roles: Vec<String> = Vec::new();
        if node.hop > 0 {
            for edge in &view.edges {
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
        }

        let tooltip_id =
            response
                .id
                .with(("constellation-node", node.kind.as_str(), node.id.as_str()));
        egui::Tooltip::always_open(
            ui.ctx().clone(),
            ui.layer_id(),
            tooltip_id,
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
                    "{} · {} step{} from this {}",
                    node.kind.label(),
                    node.hop,
                    if node.hop == 1 { "" } else { "s" },
                    focus_noun(focus_kind)
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
            if roles.len() > 3 {
                ui.label(
                    RichText::new(format!("…and {} more", roles.len() - 3))
                        .color(theme::faint())
                        .size(10.0),
                );
            }
            if let Some(links) = view.hubs.get(&node_key(node.kind, &node.id)) {
                ui.label(
                    RichText::new(format!("{links} links · neighbours not expanded"))
                        .color(theme::warning())
                        .size(10.0),
                );
            }
            if node.kind != focus_kind || node.id != focus_id {
                ui.label(
                    RichText::new(match node.kind {
                        PartyKind::Contact => "Click to inspect · double-click to open this record",
                        PartyKind::Organisation => {
                            "Click to inspect · double-click to open this organisation"
                        }
                    })
                    .color(theme::accent())
                    .size(10.0),
                );
            }
        });
    }

    if let Some(trace) = &trace
        && trace.path_length() >= 1
        && highlight_index != focus_index
    {
        let anchor = rect.left_top() + vec2(10.0, 8.0);
        painter.circle_filled(
            anchor + vec2(4.0, 5.0),
            4.0,
            trace_colour.unwrap_or_else(theme::accent),
        );
        painter.text(
            anchor + vec2(13.0, 0.0),
            Align2::LEFT_TOP,
            if trace.path_length() == 1 {
                "Direct link to focus"
            } else {
                "Path to focus"
            },
            FontId::proportional(9.0),
            theme::faint(),
        );
    }

    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Image,
            true,
            format!(
                "Relationship constellation: {} records within {} steps",
                view.nodes.len().saturating_sub(1),
                view.key.steps
            ),
        )
    });

    if response.clicked() {
        *selection = hovered_key.clone();
    }

    if response.double_clicked()
        && let Some(node) = hovered_node
        && (node.kind != focus_kind || node.id != focus_id)
    {
        return Some(PartyTarget {
            kind: node.kind,
            id: node.id.clone(),
        });
    }

    None
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
fn draw_node_label(
    painter: &egui::Painter,
    rect: Rect,
    focus_centre: Pos2,
    centre: Pos2,
    node: &AnimatedNode,
    emphasised: bool,
    placed: &mut Vec<Rect>,
) {
    let text = truncate(&node.label, 20);
    let galley = painter.layout_no_wrap(
        text,
        FontId::proportional(if emphasised { 10.0 } else { 9.0 }),
        if emphasised {
            theme::text()
        } else {
            theme::muted()
        },
    );
    let mut direction = if node.hop == 0 {
        vec2(0.0, 1.0)
    } else {
        let offset = centre - focus_centre;
        if offset.length_sq() > f32::EPSILON {
            offset.normalized()
        } else {
            vec2(0.0, 1.0)
        }
    };
    let half = galley.size() / 2.0;
    if node.hop > 0 {
        // Keep labels inside the canvas. Outer-ring labels near an edge flip
        // inward instead of being clamped on top of a neighbour's label.
        let anchor = centre + direction * (node.radius + 10.0);
        let outside = anchor.x - half.x < rect.left() + 4.0
            || anchor.x + half.x > rect.right() - 4.0
            || anchor.y - half.y < rect.top() + 4.0
            || anchor.y + half.y > rect.bottom() - 4.0;
        if outside {
            direction = -direction;
        }
    }
    let anchor = centre + direction * (node.radius + 10.0);
    let mut position = pos2(
        anchor
            .x
            .clamp(rect.left() + half.x + 4.0, rect.right() - half.x - 4.0),
        anchor
            .y
            .clamp(rect.top() + half.y + 4.0, rect.bottom() - half.y - 4.0),
    );
    let mut background = Rect::from_center_size(position, galley.size() + vec2(6.0, 2.0));
    // Nudge overlapping labels away from each other. This matters most for a
    // cluster of neighbours near the top of a ring.
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

/// Build a depth-limited view and place its nodes on concentric rings.
#[allow(clippy::too_many_arguments)]
pub fn build_view(
    catalog: &[ApiGraphNode],
    edges: &[ApiRelationship],
    focus_kind: PartyKind,
    focus_id: &str,
    focus_label: &str,
    steps: u8,
    size: Vec2,
    skip_hubs: bool,
) -> ConstellationView {
    let focus_key = node_key(focus_kind, focus_id);
    let mut graph =
        RelationshipGraph::build(catalog, edges, &GraphFilters::default(), Some(&focus_key));
    graph.ensure_label(
        focus_kind,
        focus_id,
        if focus_label.trim().is_empty() {
            match focus_kind {
                PartyKind::Contact => "This contact",
                PartyKind::Organisation => "This organisation",
            }
        } else {
            focus_label
        },
    );
    let policy = WalkPolicy {
        max_nodes: MAX_NODES,
        skip_hubs,
        hub_degree: HUB_DEGREE,
        expanded: &BTreeSet::new(),
    };
    let walk = graph.walk_with(&focus_key, steps, &policy);

    let mut nodes: Vec<ViewNode> = Vec::with_capacity(walk.hops.len());
    let mut index_by_key: BTreeMap<String, usize> = BTreeMap::new();

    let mut push_node = |key: &str, radius: f32, pos: Pos2| {
        if let Some(node) = graph.label(key) {
            index_by_key.insert(key.to_owned(), nodes.len());
            nodes.push(ViewNode {
                kind: node.kind,
                id: node.id.clone(),
                label: if key == focus_key.as_str() && !focus_label.trim().is_empty() {
                    focus_label.to_owned()
                } else {
                    node.label.clone()
                },
                hop: walk.hops.get(key).copied().unwrap_or(0),
                role: walk.roles.get(key).cloned().unwrap_or_default(),
                pos,
                radius,
            });
        }
    };

    let centre = pos2(size.x / 2.0, size.y / 2.0);
    push_node(&focus_key, 17.0, centre);

    for hop in 1..=steps as usize {
        let mut keys: Vec<&String> = walk
            .hops
            .iter()
            .filter_map(|(key, depth)| (*depth == hop).then_some(key))
            .collect();
        keys.sort_by(|left, right| {
            let left_label = graph.label(left).map_or("", |node| node.label.as_str());
            let right_label = graph.label(right).map_or("", |node| node.label.as_str());
            left_label
                .to_lowercase()
                .cmp(&right_label.to_lowercase())
                .then_with(|| left.cmp(right))
        });

        let count = keys.len();
        if count == 0 {
            continue;
        }
        let radius = ring_radius(size, steps, hop as u8);
        let step_angle = std::f32::consts::TAU / count as f32;
        let offset = -std::f32::consts::FRAC_PI_2 + hop as f32 * 0.37;
        for (index, key) in keys.into_iter().enumerate() {
            let angle = offset + step_angle * index as f32;
            let position = pos2(
                (centre.x + angle.cos() * radius).clamp(18.0, size.x - 18.0),
                (centre.y + angle.sin() * radius).clamp(18.0, size.y - 18.0),
            );
            // Crowded rings shrink their nodes rather than overlapping. The
            // hover target is padded in draw_canvas, so even small nodes remain
            // easy to inspect.
            let base_radius: f32 = match hop {
                1 => 11.0,
                2 => 9.0,
                _ => 8.0,
            };
            let spacing = std::f32::consts::TAU * radius / count as f32;
            let node_radius = base_radius.min((spacing * 0.32).max(4.0));
            push_node(key, node_radius, position);
        }
    }

    drop(push_node);

    let mut view_edges = Vec::new();
    for edge in graph.edges() {
        let from = node_key(edge.from_kind, &edge.from_id);
        let to = node_key(edge.to_kind, &edge.to_id);
        if let (Some(&from), Some(&to)) = (index_by_key.get(&from), index_by_key.get(&to)) {
            view_edges.push(ViewEdge {
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
            let key = node_key(node.kind, &node.id);
            if !graph.expands_through(&key, &focus_key, &policy) {
                let links = graph.degree(&key);
                if links > 0 {
                    hubs.insert(key, links);
                }
            }
        }
    }

    ConstellationView {
        key: ViewKey {
            steps,
            width: size.x.round() as u32,
            height: size.y.round() as u32,
            revision: 0,
            focus: focus_id.to_owned(),
            focus_kind,
            skip_hubs,
        },
        nodes,
        edges: view_edges,
        hubs,
        truncated: walk.truncated,
    }
}

fn ring_radius(size: Vec2, steps: u8, hop: u8) -> f32 {
    let max_radius = (size.x.min(size.y) / 2.0 - 30.0).max(50.0);
    let inner = (max_radius * 0.35).min(46.0);
    let fraction = hop as f32 / steps.max(1) as f32;
    inner + (max_radius - inner) * fraction
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

    fn edge(id: &str, from_id: &str, to_id: &str) -> ApiRelationship {
        ApiRelationship {
            id: id.to_owned(),
            from_kind: PartyKind::Contact,
            from_id: from_id.to_owned(),
            from_name: format!("Contact {from_id}"),
            from_role: "care_recipient".to_owned(),
            to_kind: PartyKind::Contact,
            to_id: to_id.to_owned(),
            to_name: format!("Contact {to_id}"),
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
    fn walk_respects_the_step_limit() {
        let catalog = vec![
            node(PartyKind::Contact, "a", "Alice"),
            node(PartyKind::Contact, "b", "Bob"),
            node(PartyKind::Contact, "c", "Carol"),
            node(PartyKind::Contact, "d", "Dave"),
        ];
        let edges = vec![
            edge("1", "a", "b"),
            edge("2", "b", "c"),
            edge("3", "c", "d"),
        ];

        let one = build_view(
            &catalog,
            &edges,
            PartyKind::Contact,
            "a",
            "Alice",
            1,
            vec2(400.0, 300.0),
            false,
        );
        assert_eq!(one.nodes.len(), 2);
        assert!(one.nodes.iter().any(|node| node.id == "b"));
        assert!(!one.nodes.iter().any(|node| node.id == "c"));

        let two = build_view(
            &catalog,
            &edges,
            PartyKind::Contact,
            "a",
            "Alice",
            2,
            vec2(400.0, 300.0),
            false,
        );
        assert_eq!(two.nodes.len(), 3);
        assert!(two.nodes.iter().any(|node| node.id == "c"));
    }

    #[test]
    fn ended_relationships_are_not_walked() {
        let catalog = vec![
            node(PartyKind::Contact, "a", "Alice"),
            node(PartyKind::Contact, "b", "Bob"),
        ];
        let mut ended = edge("1", "a", "b");
        ended.status = "ended".to_owned();

        let view = build_view(
            &catalog,
            &[ended],
            PartyKind::Contact,
            "a",
            "Alice",
            4,
            vec2(400.0, 300.0),
            false,
        );
        assert_eq!(view.nodes.len(), 1);
        assert!(view.edges.is_empty());
    }

    #[test]
    fn cycles_and_duplicate_edges_do_not_duplicate_nodes() {
        let catalog = vec![
            node(PartyKind::Contact, "a", "Alice"),
            node(PartyKind::Contact, "b", "Bob"),
        ];
        let edges = vec![
            edge("1", "a", "b"),
            edge("2", "b", "a"),
            edge("3", "a", "b"),
        ];

        let view = build_view(
            &catalog,
            &edges,
            PartyKind::Contact,
            "a",
            "Alice",
            2,
            vec2(400.0, 300.0),
            false,
        );
        assert_eq!(view.nodes.len(), 2);
    }

    #[test]
    fn layout_is_deterministic_and_inside_the_canvas() {
        let catalog = vec![
            node(PartyKind::Contact, "a", "Alice"),
            node(PartyKind::Contact, "b", "Bob"),
            node(PartyKind::Contact, "c", "Carol"),
            node(PartyKind::Contact, "d", "Dave"),
        ];
        let edges = vec![
            edge("1", "a", "b"),
            edge("2", "b", "c"),
            edge("3", "a", "d"),
        ];

        let first = build_view(
            &catalog,
            &edges,
            PartyKind::Contact,
            "a",
            "Alice",
            2,
            vec2(400.0, 300.0),
            false,
        );
        let second = build_view(
            &catalog,
            &edges,
            PartyKind::Contact,
            "a",
            "Alice",
            2,
            vec2(400.0, 300.0),
            false,
        );
        assert_eq!(first.nodes.len(), second.nodes.len());
        for (left, right) in first.nodes.iter().zip(&second.nodes) {
            assert_eq!(left.pos, right.pos);
            assert!(left.pos.x >= 0.0 && left.pos.x <= 400.0);
            assert!(left.pos.y >= 0.0 && left.pos.y <= 300.0);
        }
    }

    #[test]
    fn organisation_nodes_are_kept_and_labelled() {
        let catalog = vec![
            node(PartyKind::Contact, "a", "Alice"),
            node(PartyKind::Organisation, "org", "Care Co"),
        ];
        let mut org_edge = edge("1", "a", "org");
        org_edge.to_kind = PartyKind::Organisation;

        let view = build_view(
            &catalog,
            &[org_edge],
            PartyKind::Contact,
            "a",
            "Alice",
            1,
            vec2(400.0, 300.0),
            false,
        );
        let org = view
            .nodes
            .iter()
            .find(|node| node.kind == PartyKind::Organisation)
            .expect("the organisation is part of the network");
        assert_eq!(org.label, "Care Co");
    }

    #[test]
    fn the_constellation_animates_between_step_counts() {
        let catalog = vec![
            node(PartyKind::Contact, "a", "Alice"),
            node(PartyKind::Contact, "b", "Bob"),
            node(PartyKind::Contact, "c", "Carol"),
        ];
        let edges = vec![edge("1", "a", "b"), edge("2", "b", "c")];
        let ctx = egui::Context::default();
        let size = vec2(400.0, 300.0);

        let one = build_view(
            &catalog,
            &edges,
            PartyKind::Contact,
            "a",
            "Alice",
            1,
            size,
            false,
        );
        let mut animation = ConstellationAnimation::default();
        animation.update(&one, &ctx, 0.0);
        for step in 1..=30 {
            animation.update(&one, &ctx, step as f64 * 0.016);
        }
        let focus = animation
            .get(PartyKind::Contact, "a")
            .expect("the focus is drawn");
        assert!(focus.alpha > 0.95);

        let two = build_view(
            &catalog,
            &edges,
            PartyKind::Contact,
            "a",
            "Alice",
            2,
            size,
            false,
        );
        animation.update(&two, &ctx, 0.5);
        let carol = animation
            .get(PartyKind::Contact, "c")
            .expect("the new party fades in");
        assert!(carol.alpha < 1.0, "a new node should fade rather than pop");
        for step in 1..=60 {
            animation.update(&two, &ctx, 0.5 + step as f64 * 0.016);
        }
        let carol = animation.get(PartyKind::Contact, "c").expect("Carol");
        assert!(carol.alpha > 0.95);
    }

    #[test]
    fn collapsing_clears_the_animation() {
        let mut animation = ConstellationAnimation::default();
        animation.displayed.insert(
            "contact:a".to_owned(),
            AnimatedNode {
                kind: PartyKind::Contact,
                label: "Alice".to_owned(),
                hop: 0,
                pos: pos2(10.0, 10.0),
                radius: 10.0,
                alpha: 1.0,
            },
        );
        animation.clear();
        assert!(animation.displayed.is_empty());
    }

    #[test]
    fn hubs_are_shown_without_expanding_their_links() {
        let mut catalog = vec![
            node(PartyKind::Contact, "a", "Alice"),
            node(PartyKind::Organisation, "org", "Care Co"),
        ];
        let mut to_org = edge("0", "a", "org");
        to_org.to_kind = PartyKind::Organisation;
        to_org.to_name = "Care Co".to_owned();
        let mut edges = vec![to_org];
        for index in 0..12 {
            let id = format!("n{index}");
            catalog.push(node(PartyKind::Contact, &id, &format!("Node {index}")));
            let mut link = edge(&format!("e{index}"), "org", &id);
            link.from_kind = PartyKind::Organisation;
            link.from_name = "Care Co".to_owned();
            edges.push(link);
        }

        let size = vec2(400.0, 300.0);
        let hidden = build_view(
            &catalog,
            &edges,
            PartyKind::Contact,
            "a",
            "Alice",
            2,
            size,
            true,
        );
        assert!(hidden.nodes.iter().any(|node| node.id == "org"));
        assert!(!hidden.nodes.iter().any(|node| node.id == "n0"));
        assert_eq!(
            hidden.hubs.get(&node_key(PartyKind::Organisation, "org")),
            Some(&13)
        );

        let shown = build_view(
            &catalog,
            &edges,
            PartyKind::Contact,
            "a",
            "Alice",
            2,
            size,
            false,
        );
        assert!(shown.nodes.iter().any(|node| node.id == "n0"));
        assert!(shown.hubs.is_empty());
    }

    #[test]
    fn an_organisation_can_be_the_focus_of_the_walk() {
        let catalog = vec![
            node(PartyKind::Organisation, "org", "Care Co"),
            node(PartyKind::Contact, "a", "Alice"),
            node(PartyKind::Contact, "b", "Bob"),
        ];
        let mut to_alice = edge("1", "org", "a");
        to_alice.from_kind = PartyKind::Organisation;
        to_alice.from_name = "Care Co".to_owned();
        let mut to_bob = edge("2", "org", "b");
        to_bob.from_kind = PartyKind::Organisation;
        to_bob.from_name = "Care Co".to_owned();

        let view = build_view(
            &catalog,
            &[to_alice, to_bob],
            PartyKind::Organisation,
            "org",
            "Care Co",
            1,
            vec2(400.0, 300.0),
            false,
        );

        let focus = view
            .nodes
            .iter()
            .find(|node| node.hop == 0)
            .expect("the organisation is the centre");
        assert_eq!(focus.kind, PartyKind::Organisation);
        assert_eq!(focus.label, "Care Co");
        assert_eq!(view.key.focus_kind, PartyKind::Organisation);
        assert!(view.nodes.iter().any(|node| node.id == "a"));
    }

    #[test]
    fn the_section_toggles_and_changes_steps() {
        use egui_kittest::{Harness, kittest::Queryable};

        struct TestState {
            catalog: Vec<ApiGraphNode>,
            network: RelationshipNetwork,
            preference: RelationshipViewPreference,
            cache: Option<ConstellationView>,
            animation: ConstellationAnimation,
            selection: Option<String>,
        }

        let catalog = vec![
            node(PartyKind::Contact, "a", "Alice"),
            node(PartyKind::Contact, "b", "Bob"),
            node(PartyKind::Contact, "c", "Carol"),
        ];
        let edges = vec![edge("1", "a", "b"), edge("2", "b", "c")];
        let state = TestState {
            catalog,
            network: RelationshipNetwork { edges, revision: 1 },
            preference: RelationshipViewPreference::default(),
            cache: None,
            animation: ConstellationAnimation::default(),
            selection: None,
        };

        let mut harness = Harness::builder()
            .with_size(egui::vec2(520.0, 720.0))
            .with_max_steps(20)
            .with_context_setup(|ctx| {
                crate::theme::apply(ctx, crate::theme::ThemeMode::Dark);
            })
            .build_ui_state(
                move |ui, state: &mut TestState| {
                    let source = ConstellationSource {
                        nodes: &state.catalog,
                        network: &state.network,
                        loading: false,
                        error: None,
                    };
                    section(
                        ui,
                        &source,
                        PartyKind::Contact,
                        "a",
                        "Alice",
                        &mut state.preference,
                        &mut state.cache,
                        &mut state.animation,
                        &mut state.selection,
                    );
                },
                state,
            );

        harness.run();
        assert!(harness.state().preference.collapsed);

        harness
            .get_by_label_contains("Relationship constellation")
            .click();
        harness.run();
        assert!(!harness.state().preference.collapsed);
        assert!(harness.state().cache.is_some());

        harness
            .get_by_role_and_label(egui::Role::Button, "+")
            .click();
        harness.run();
        assert_eq!(
            harness.state().preference.steps,
            crate::config::CONSTELLATION_DEFAULT_STEPS + 1
        );

        harness
            .get_by_role_and_label(egui::Role::Button, "+")
            .click();
        harness.run();
        assert_eq!(
            harness.state().preference.steps,
            crate::config::CONSTELLATION_DEFAULT_STEPS + 2
        );

        harness
            .get_by_role_and_label(egui::Role::Button, "+")
            .click();
        harness.run();
        assert_eq!(
            harness.state().preference.steps,
            crate::config::CONSTELLATION_MAX_STEPS
        );

        harness
            .get_by_role_and_label(egui::Role::Button, "−")
            .click();
        harness.run();
        assert_eq!(
            harness.state().preference.steps,
            crate::config::CONSTELLATION_MAX_STEPS - 1
        );
    }

    #[test]
    fn the_view_is_capped() {
        let mut catalog = vec![node(PartyKind::Contact, "focus", "Focus")];
        let mut edges = Vec::new();
        for index in 0..(MAX_NODES + 10) {
            let id = format!("n{index}");
            catalog.push(node(PartyKind::Contact, &id, &format!("Node {index}")));
            edges.push(edge(&format!("e{index}"), "focus", &id));
        }

        let view = build_view(
            &catalog,
            &edges,
            PartyKind::Contact,
            "focus",
            "Focus",
            1,
            vec2(400.0, 300.0),
            false,
        );
        assert_eq!(view.nodes.len(), MAX_NODES);
        assert!(view.truncated);
    }
}
