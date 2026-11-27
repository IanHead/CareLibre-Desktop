//! The relationship constellation shown on a contact record.
//!
//! The whole-tenant relationship graph is already loaded for the party lookup,
//! so the constellation is built client-side: a breadth-first walk from the
//! open contact, capped at the number of steps the user has chosen.

use std::collections::{BTreeMap, VecDeque};

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Stroke, StrokeKind,
    Ui, Vec2, pos2, vec2,
};

use crate::config::{CONSTELLATION_MAX_STEPS, CONSTELLATION_MIN_STEPS, RelationshipViewPreference};
use crate::contact::{ApiGraphNode, ApiRelationship, PartyKind, relationship_role_label};
use crate::theme;

/// Total nodes drawn. Beyond this the graph becomes unreadable, so we stop and
/// point the user at the relationship list instead.
pub const MAX_NODES: usize = 60;
const CANVAS_HEIGHT: f32 = 300.0;
const NARROW_CANVAS_HEIGHT: f32 = 230.0;

/// A snapshot of the relationship edges used to build constellations.
#[derive(Debug)]
pub struct RelationshipNetwork {
    pub edges: Vec<ApiRelationship>,
    /// Bumped whenever the graph changes so views rebuild exactly once.
    pub revision: u64,
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

/// What the section wants the record to do after this frame.
#[derive(Default)]
pub struct SectionResult {
    pub preference_changed: bool,
    pub open_contact: Option<String>,
    pub reload: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewKey {
    steps: u8,
    width: u32,
    height: u32,
    revision: u64,
    focus: String,
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
    pub truncated: bool,
}

/// Draw the collapsible constellation section inside the Relationships card.
pub fn section(
    ui: &mut Ui,
    source: &ConstellationSource<'_>,
    focus_id: &str,
    focus_label: &str,
    preference: &mut RelationshipViewPreference,
    cache: &mut Option<ConstellationView>,
) -> SectionResult {
    let mut result = SectionResult::default();

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
            "Show this contact's relationship network"
        } else {
            "Hide this contact's relationship network"
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
        return result;
    }

    ui.add_space(4.0);
    ui.horizontal(|ui| {
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
            RichText::new("away from this contact")
                .color(theme::faint())
                .size(10.0),
        );
    });

    ui.label(
        RichText::new(
            "Each ring is one step from this contact. Click a contact to open their record.",
        )
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
    let height = if width < 360.0 {
        NARROW_CANVAS_HEIGHT
    } else {
        CANVAS_HEIGHT
    };
    let key = ViewKey {
        steps: preference.steps,
        width: width.round() as u32,
        height: height.round() as u32,
        revision: source.network.revision,
        focus: focus_id.to_owned(),
    };
    if cache.as_ref().map(|view| &view.key) != Some(&key) {
        let mut view = build_view(
            source.nodes(),
            &source.network.edges,
            focus_id,
            focus_label,
            preference.steps,
            vec2(width, height),
        );
        view.key = key.clone();
        *cache = Some(view);
    }
    let Some(view) = cache.as_ref() else {
        return result;
    };

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

    if let Some(contact_id) = draw_canvas(ui, view, focus_id, vec2(width, height)) {
        result.open_contact = Some(contact_id);
    }

    ui.add_space(3.0);
    legend(ui, preference.steps);

    result
}

fn legend(ui: &mut Ui, steps: u8) {
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
            RichText::new(format!("Focus contact · rings show up to {steps} steps"))
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
    focus_id: &str,
    size: Vec2,
) -> Option<String> {
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

    let pointer = response.hover_pos().filter(|pos| rect.contains(*pos));
    let hovered = pointer.and_then(|pos| {
        view.nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                let centre = rect.min + node.pos.to_vec2();
                let distance = centre.distance(pos);
                (distance <= node.radius + 7.0).then_some((index, distance))
            })
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .map(|(index, _)| index)
    });

    let hovered_node = hovered.map(|index| &view.nodes[index]);

    for edge in &view.edges {
        let from = &view.nodes[edge.from];
        let to = &view.nodes[edge.to];
        let highlighted = hovered.is_some_and(|index| index == edge.from || index == edge.to);
        let colour = if highlighted {
            theme::accent()
        } else {
            Color32::from_rgba_unmultiplied(
                theme::muted().r(),
                theme::muted().g(),
                theme::muted().b(),
                80,
            )
        };
        painter.line_segment(
            [rect.min + from.pos.to_vec2(), rect.min + to.pos.to_vec2()],
            Stroke::new(if highlighted { 2.2 } else { 1.0 }, colour),
        );
    }

    let labels_everywhere = view.nodes.len() <= 10;
    let label_first_hop = view.nodes.iter().filter(|node| node.hop == 1).count() <= 14;
    let mut placed_labels: Vec<Rect> = Vec::new();
    for (index, node) in view.nodes.iter().enumerate() {
        let centre = rect.min + node.pos.to_vec2();
        let is_focus = node.hop == 0;
        let is_hovered = hovered == Some(index);
        let colour = if is_focus {
            theme::accent()
        } else if node.kind == PartyKind::Organisation {
            theme::warning()
        } else {
            theme::accent_blue()
        };
        let fill = Color32::from_rgba_unmultiplied(colour.r(), colour.g(), colour.b(), 55);

        if is_focus {
            painter.circle_stroke(
                centre,
                node.radius + 4.0,
                Stroke::new(1.5, theme::accent_soft(160)),
            );
        }

        match node.kind {
            PartyKind::Contact => {
                painter.circle_filled(centre, node.radius, fill);
                painter.circle_stroke(
                    centre,
                    node.radius,
                    Stroke::new(if is_hovered { 2.2 } else { 1.4 }, colour),
                );
            }
            PartyKind::Organisation => {
                let box_rect = Rect::from_center_size(centre, Vec2::splat(node.radius * 1.85));
                painter.rect_filled(box_rect, CornerRadius::same(3), fill);
                painter.rect_stroke(
                    box_rect,
                    CornerRadius::same(3),
                    Stroke::new(if is_hovered { 2.2 } else { 1.4 }, colour),
                    StrokeKind::Inside,
                );
            }
        }

        painter.text(
            centre,
            Align2::CENTER_CENTER,
            initials(&node.label),
            FontId::proportional((node.radius * 0.75).clamp(5.5, 11.0)),
            colour,
        );

        if is_focus
            || is_hovered
            || (node.hop == 1 && label_first_hop)
            || (labels_everywhere && node.hop > 1)
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
        if node.kind == PartyKind::Contact && node.id != focus_id {
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
                    "{} · {} step{} from this contact",
                    node.kind.label(),
                    node.hop,
                    if node.hop == 1 { "" } else { "s" }
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
            if node.kind == PartyKind::Contact && node.id != focus_id {
                ui.label(
                    RichText::new("Click to open this record.")
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
                "Relationship constellation: {} records within {} steps",
                view.nodes.len().saturating_sub(1),
                view.key.steps
            ),
        )
    });

    if response.clicked()
        && let Some(node) = hovered_node
        && node.kind == PartyKind::Contact
        && node.id != focus_id
    {
        return Some(node.id.clone());
    }

    None
}

#[allow(clippy::too_many_arguments)]
fn draw_node_label(
    painter: &egui::Painter,
    rect: Rect,
    focus_centre: Pos2,
    centre: Pos2,
    node: &ViewNode,
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
pub fn build_view(
    catalog: &[ApiGraphNode],
    edges: &[ApiRelationship],
    focus_id: &str,
    focus_label: &str,
    steps: u8,
    size: Vec2,
) -> ConstellationView {
    let focus_key = node_key(PartyKind::Contact, focus_id);
    let active: Vec<&ApiRelationship> = edges.iter().filter(|edge| !edge.is_ended()).collect();

    let mut labels: BTreeMap<String, (PartyKind, String, String)> = BTreeMap::new();
    for node in catalog {
        labels.insert(
            node_key(node.kind, &node.id),
            (node.kind, node.id.clone(), node.label.clone()),
        );
    }
    for edge in &active {
        labels
            .entry(node_key(edge.from_kind, &edge.from_id))
            .or_insert_with(|| (edge.from_kind, edge.from_id.clone(), edge.from_name.clone()));
        labels
            .entry(node_key(edge.to_kind, &edge.to_id))
            .or_insert_with(|| (edge.to_kind, edge.to_id.clone(), edge.to_name.clone()));
    }

    labels.entry(focus_key.clone()).or_insert_with(|| {
        (
            PartyKind::Contact,
            focus_id.to_owned(),
            if focus_label.trim().is_empty() {
                "This contact".to_owned()
            } else {
                focus_label.to_owned()
            },
        )
    });

    let mut adjacency: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, edge) in active.iter().enumerate() {
        adjacency
            .entry(node_key(edge.from_kind, &edge.from_id))
            .or_default()
            .push(index);
        adjacency
            .entry(node_key(edge.to_kind, &edge.to_id))
            .or_default()
            .push(index);
    }

    let mut hops: BTreeMap<String, usize> = BTreeMap::new();
    let mut roles: BTreeMap<String, String> = BTreeMap::new();
    let mut order: Vec<String> = vec![focus_key.clone()];
    let mut queue: VecDeque<String> = VecDeque::from([focus_key.clone()]);
    hops.insert(focus_key.clone(), 0);
    let mut truncated = false;

    while let Some(current) = queue.pop_front() {
        let hop = hops[&current];
        if hop >= steps as usize {
            continue;
        }

        let mut neighbours: Vec<(String, String)> = Vec::new();
        if let Some(indices) = adjacency.get(&current) {
            for &index in indices {
                let edge = active[index];
                let from = node_key(edge.from_kind, &edge.from_id);
                let to = node_key(edge.to_kind, &edge.to_id);
                let (other, role) = if from == current {
                    (to, edge.to_role.clone())
                } else {
                    (from, edge.from_role.clone())
                };
                if !hops.contains_key(&other)
                    && !neighbours.iter().any(|(existing, _)| existing == &other)
                {
                    neighbours.push((other, role));
                }
            }
        }
        neighbours.sort();

        for (neighbour, role) in neighbours {
            if hops.len() >= MAX_NODES {
                truncated = true;
                break;
            }
            hops.insert(neighbour.clone(), hop + 1);
            roles.insert(neighbour.clone(), role);
            order.push(neighbour.clone());
            queue.push_back(neighbour);
        }
        if truncated {
            break;
        }
    }

    let mut nodes = Vec::with_capacity(order.len());
    let mut index_by_key: BTreeMap<String, usize> = BTreeMap::new();

    let mut push_node = |key: &str, radius: f32, pos: Pos2| {
        if let Some((kind, id, label)) = labels.get(key) {
            index_by_key.insert(key.to_owned(), nodes.len());
            nodes.push(ViewNode {
                kind: *kind,
                id: id.clone(),
                label: if key == focus_key.as_str() && !focus_label.trim().is_empty() {
                    focus_label.to_owned()
                } else {
                    label.clone()
                },
                hop: hops.get(key).copied().unwrap_or(0),
                role: roles.get(key).cloned().unwrap_or_default(),
                pos,
                radius,
            });
        }
    };

    let centre = pos2(size.x / 2.0, size.y / 2.0);
    push_node(&focus_key, 17.0, centre);

    for hop in 1..=steps as usize {
        let mut keys: Vec<&String> = hops
            .iter()
            .filter_map(|(key, depth)| (*depth == hop).then_some(key))
            .collect();
        keys.sort_by(|left, right| {
            let left_label = labels.get(*left).map_or("", |(_, _, label)| label.as_str());
            let right_label = labels
                .get(*right)
                .map_or("", |(_, _, label)| label.as_str());
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
    for edge in &active {
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

    ConstellationView {
        key: ViewKey {
            steps,
            width: size.x.round() as u32,
            height: size.y.round() as u32,
            revision: 0,
            focus: focus_id.to_owned(),
        },
        nodes,
        edges: view_edges,
        truncated,
    }
}

fn ring_radius(size: Vec2, steps: u8, hop: u8) -> f32 {
    let max_radius = (size.x.min(size.y) / 2.0 - 30.0).max(50.0);
    let inner = (max_radius * 0.35).min(46.0);
    let fraction = hop as f32 / steps.max(1) as f32;
    inner + (max_radius - inner) * fraction
}

fn node_key(kind: PartyKind, id: &str) -> String {
    format!("{}:{id}", kind.as_str())
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

        let one = build_view(&catalog, &edges, "a", "Alice", 1, vec2(400.0, 300.0));
        assert_eq!(one.nodes.len(), 2);
        assert!(one.nodes.iter().any(|node| node.id == "b"));
        assert!(!one.nodes.iter().any(|node| node.id == "c"));

        let two = build_view(&catalog, &edges, "a", "Alice", 2, vec2(400.0, 300.0));
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

        let view = build_view(&catalog, &[ended], "a", "Alice", 4, vec2(400.0, 300.0));
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

        let view = build_view(&catalog, &edges, "a", "Alice", 2, vec2(400.0, 300.0));
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

        let first = build_view(&catalog, &edges, "a", "Alice", 2, vec2(400.0, 300.0));
        let second = build_view(&catalog, &edges, "a", "Alice", 2, vec2(400.0, 300.0));
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

        let view = build_view(&catalog, &[org_edge], "a", "Alice", 1, vec2(400.0, 300.0));
        let org = view
            .nodes
            .iter()
            .find(|node| node.kind == PartyKind::Organisation)
            .expect("the organisation is part of the network");
        assert_eq!(org.label, "Care Co");
    }

    #[test]
    fn the_section_toggles_and_changes_steps() {
        use egui_kittest::{Harness, kittest::Queryable};

        struct TestState {
            catalog: Vec<ApiGraphNode>,
            network: RelationshipNetwork,
            preference: RelationshipViewPreference,
            cache: Option<ConstellationView>,
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
                        "a",
                        "Alice",
                        &mut state.preference,
                        &mut state.cache,
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

        let view = build_view(&catalog, &edges, "focus", "Focus", 1, vec2(400.0, 300.0));
        assert_eq!(view.nodes.len(), MAX_NODES);
        assert!(view.truncated);
    }
}
