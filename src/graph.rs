//! A shared model of the tenant relationship graph.
//!
//! The server returns every party and relationship for the signed-in
//! organisation. Both the on-record constellation and the relationship
//! explorer filter that snapshot and walk it from a focus party, so the
//! traversal lives here rather than in either view.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::contact::{ApiGraphNode, ApiRelationship, PartyKind};

/// Frame-rate independent smoothing factor for short graph transitions.
/// Harnesses sometimes report no elapsed wall-clock time between frames, so a
/// small visible step is used rather than freezing mid-transition.
pub fn transition_factor(dt_seconds: f32) -> f32 {
    if dt_seconds <= 0.0 {
        0.3
    } else {
        1.0 - (-dt_seconds.clamp(0.0, 0.12) * 10.0).exp()
    }
}

/// Parties above this many links are treated as hubs. Personal relationships
/// are rarely this wide; organisations and a few administrative contacts are.
pub const HUB_DEGREE: usize = 30;

/// Suppressed parties at or above this many links get a count badge.
pub const HUB_BADGE_MIN: usize = 10;

/// How a walk treats highly connected parties.
#[derive(Clone, Copy, Debug)]
pub struct WalkPolicy<'a> {
    pub max_nodes: usize,
    /// Show hubs but do not walk through them unless they are focused or in
    /// `expanded`.
    pub skip_hubs: bool,
    pub hub_degree: usize,
    pub expanded: &'a BTreeSet<String>,
}

static NO_EXPANSIONS: BTreeSet<String> = BTreeSet::new();

impl WalkPolicy<'_> {
    /// Walk every reachable party. Used by tests and by views that have not
    /// opted into hub suppression.
    pub fn simple(max_nodes: usize) -> Self {
        Self {
            max_nodes,
            skip_hubs: false,
            hub_degree: HUB_DEGREE,
            expanded: &NO_EXPANSIONS,
        }
    }
}

/// A stable key for one party, shared by the catalogue and every edge.
pub fn node_key(kind: PartyKind, id: &str) -> String {
    format!("{}:{id}", kind.as_str())
}

/// One party's display information.
#[derive(Clone, Debug)]
pub struct NodeLabel {
    pub kind: PartyKind,
    pub id: String,
    pub label: String,
}

/// The subset of relationships a view wants to consider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphFilters {
    pub show_ended: bool,
    pub show_contacts: bool,
    pub show_organisations: bool,
    /// Only keep edges where either side holds this role.
    pub role: Option<String>,
}

impl Default for GraphFilters {
    fn default() -> Self {
        Self {
            show_ended: false,
            show_contacts: true,
            show_organisations: true,
            role: None,
        }
    }
}

impl GraphFilters {
    pub fn allows_kind(&self, kind: PartyKind) -> bool {
        match kind {
            PartyKind::Contact => self.show_contacts,
            PartyKind::Organisation => self.show_organisations,
        }
    }

    /// The focus party is always kept, even when its kind is filtered out, so
    /// the explorer never loses the centre of the view.
    fn keeps(&self, edge: &ApiRelationship, focus: Option<&str>) -> bool {
        if !self.show_ended && edge.is_ended() {
            return false;
        }
        if let Some(role) = &self.role
            && edge.from_role != *role
            && edge.to_role != *role
        {
            return false;
        }
        let from = node_key(edge.from_kind, &edge.from_id);
        let to = node_key(edge.to_kind, &edge.to_id);
        let allowed = |key: &str, kind| focus == Some(key) || self.allows_kind(kind);
        allowed(&from, edge.from_kind) && allowed(&to, edge.to_kind)
    }
}

/// A revisioned snapshot of every relationship edge, shared by views so a
/// rebuild only happens when the data changes.
#[derive(Debug)]
pub struct RelationshipNetwork {
    pub edges: Vec<ApiRelationship>,
    /// Bumped whenever the graph changes so views rebuild exactly once.
    pub revision: u64,
}

/// A filtered, indexed snapshot of the relationship graph.
pub struct RelationshipGraph {
    /// Every known party, including parties whose edges are filtered out. The
    /// explorer uses this to label a selected party and its counterparts.
    pub labels: BTreeMap<String, NodeLabel>,
    edges: Vec<ApiRelationship>,
    adjacency: BTreeMap<String, Vec<usize>>,
}

impl RelationshipGraph {
    pub fn build(
        catalog: &[ApiGraphNode],
        edges: &[ApiRelationship],
        filters: &GraphFilters,
        focus: Option<&str>,
    ) -> Self {
        let mut labels = BTreeMap::new();
        for node in catalog {
            labels.insert(
                node_key(node.kind, &node.id),
                NodeLabel {
                    kind: node.kind,
                    id: node.id.clone(),
                    label: node.label.clone(),
                },
            );
        }
        for edge in edges {
            labels
                .entry(node_key(edge.from_kind, &edge.from_id))
                .or_insert_with(|| NodeLabel {
                    kind: edge.from_kind,
                    id: edge.from_id.clone(),
                    label: edge.from_name.clone(),
                });
            labels
                .entry(node_key(edge.to_kind, &edge.to_id))
                .or_insert_with(|| NodeLabel {
                    kind: edge.to_kind,
                    id: edge.to_id.clone(),
                    label: edge.to_name.clone(),
                });
        }

        let edges: Vec<ApiRelationship> = edges
            .iter()
            .filter(|edge| filters.keeps(edge, focus))
            .cloned()
            .collect();

        let mut adjacency: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (index, edge) in edges.iter().enumerate() {
            let from = node_key(edge.from_kind, &edge.from_id);
            let to = node_key(edge.to_kind, &edge.to_id);
            if from == to {
                adjacency.entry(from).or_default().push(index);
            } else {
                adjacency.entry(from).or_default().push(index);
                adjacency.entry(to).or_default().push(index);
            }
        }

        Self {
            labels,
            edges,
            adjacency,
        }
    }

    /// Add a label for a party the snapshot does not know yet, such as the
    /// contact whose record is currently open.
    pub fn ensure_label(&mut self, kind: PartyKind, id: &str, label: &str) {
        self.labels
            .entry(node_key(kind, id))
            .or_insert_with(|| NodeLabel {
                kind,
                id: id.to_owned(),
                label: label.to_owned(),
            });
    }

    pub fn edges(&self) -> &[ApiRelationship] {
        &self.edges
    }

    pub fn label(&self, key: &str) -> Option<&NodeLabel> {
        self.labels.get(key)
    }

    /// Every role that appears on a kept edge, for the explorer's role filter.
    pub fn roles(&self) -> BTreeSet<String> {
        let mut roles = BTreeSet::new();
        for edge in &self.edges {
            roles.insert(edge.from_role.clone());
            roles.insert(edge.to_role.clone());
        }
        roles
    }

    /// The parties directly linked to `key`, using the same filters as the
    /// graph. Ended edges follow the snapshot's `show_ended` setting.
    pub fn direct_edges(&self, key: &str) -> Vec<&ApiRelationship> {
        let Some(indices) = self.adjacency.get(key) else {
            return Vec::new();
        };
        indices.iter().map(|index| &self.edges[*index]).collect()
    }

    /// The number of links attached to one party.
    pub fn degree(&self, key: &str) -> usize {
        self.adjacency.get(key).map_or(0, Vec::len)
    }

    /// Whether a party's neighbours should be walked through under this
    /// policy. Views use it to badge parties whose links are held back.
    pub fn expands_through(&self, key: &str, focus: &str, policy: &WalkPolicy<'_>) -> bool {
        if key == focus || !policy.skip_hubs || policy.expanded.contains(key) {
            return true;
        }
        // An organisation is an affiliation: every client of the same provider
        // is not a meaningful indirect relationship.
        if self
            .label(key)
            .is_some_and(|label| label.kind == PartyKind::Organisation)
        {
            return false;
        }
        self.degree(key) <= policy.hub_degree
    }

    /// Breadth-first walk from `focus`, stopping at `levels` and `max_nodes`.
    pub fn walk(&self, focus: &str, levels: u8, max_nodes: usize) -> GraphWalk {
        self.walk_with(focus, levels, &WalkPolicy::simple(max_nodes))
    }

    /// Breadth-first walk with a hub policy. Hubs are still included as nodes;
    /// only their neighbours are held back, so the user can expand them from
    /// the side panel.
    pub fn walk_with(&self, focus: &str, levels: u8, policy: &WalkPolicy<'_>) -> GraphWalk {
        let mut hops: BTreeMap<String, usize> = BTreeMap::new();
        let mut roles: BTreeMap<String, String> = BTreeMap::new();
        let mut parents: BTreeMap<String, String> = BTreeMap::new();
        let mut queue: VecDeque<String> = VecDeque::from([focus.to_owned()]);
        hops.insert(focus.to_owned(), 0);
        let mut truncated = false;

        while let Some(current) = queue.pop_front() {
            let hop = hops[&current];
            if hop >= levels as usize {
                continue;
            }
            if !self.expands_through(&current, focus, policy) {
                continue;
            }

            let mut neighbours: Vec<(String, String)> = Vec::new();
            if let Some(indices) = self.adjacency.get(&current) {
                for &index in indices {
                    let edge = &self.edges[index];
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
                if hops.len() >= policy.max_nodes {
                    truncated = true;
                    break;
                }
                hops.insert(neighbour.clone(), hop + 1);
                roles.insert(neighbour.clone(), role);
                parents.insert(neighbour.clone(), current.clone());
                queue.push_back(neighbour);
            }
            if truncated {
                break;
            }
        }

        GraphWalk {
            hops,
            roles,
            parents,
            truncated,
        }
    }
}

/// Breadth-first distance in edges from `start` over a small view graph.
///
/// The returned vector is indexed like the view's node list; unreachable nodes
/// are `usize::MAX`. Used to colour a hovered or selected party's neighbourhood.
pub fn neighbourhood_depths(
    node_count: usize,
    edges: impl IntoIterator<Item = (usize, usize)>,
    start: usize,
) -> Vec<usize> {
    let mut adjacency = vec![Vec::new(); node_count];
    for (from, to) in edges {
        if from >= node_count || to >= node_count {
            continue;
        }
        adjacency[from].push(to);
        if from != to {
            adjacency[to].push(from);
        }
    }

    let mut depths = vec![usize::MAX; node_count];
    if start >= node_count {
        return depths;
    }
    depths[start] = 0;
    let mut queue = VecDeque::from([start]);
    while let Some(current) = queue.pop_front() {
        let next_depth = depths[current] + 1;
        for &next in &adjacency[current] {
            if depths[next] == usize::MAX {
                depths[next] = next_depth;
                queue.push_back(next);
            }
        }
    }
    depths
}

/// The shortest path(s) from a hovered party back to the graph's focus.
///
/// Every edge that lies on an equally short route is kept, so two family
/// members leading to the same focus are both shown. Depths are measured from
/// the hovered party: its direct links are depth 1, the next step is depth 2,
/// and the focus sits at the path length.
#[derive(Clone, Debug)]
pub struct FocusTrace {
    start: usize,
    depths: Vec<usize>,
    to_focus: Vec<usize>,
    path_length: usize,
    on_path: Vec<bool>,
}

impl FocusTrace {
    pub fn build(
        node_count: usize,
        edges: impl IntoIterator<Item = (usize, usize)> + Clone,
        start: usize,
        focus: usize,
    ) -> Self {
        let depths = neighbourhood_depths(node_count, edges.clone(), start);
        let to_focus = neighbourhood_depths(node_count, edges, focus);
        let path_length = depths.get(focus).copied().unwrap_or(usize::MAX);
        let on_path = (0..node_count)
            .map(|index| {
                depths[index] != usize::MAX
                    && to_focus[index] != usize::MAX
                    && depths[index] + to_focus[index] == path_length
            })
            .collect();
        Self {
            start,
            depths,
            to_focus,
            path_length,
            on_path,
        }
    }

    pub fn path_length(&self) -> usize {
        self.path_length
    }

    pub fn is_on_path(&self, index: usize) -> bool {
        self.on_path.get(index).copied().unwrap_or(false)
    }

    /// A direct link of the hovered party, whether or not it leads back to the
    /// focus.
    pub fn is_direct(&self, index: usize) -> bool {
        self.depths.get(index).copied() == Some(1)
    }

    pub fn edge_on_path(&self, from: usize, to: usize) -> bool {
        if self.path_length == 0 || self.path_length == usize::MAX {
            return false;
        }
        let Some(&from_depth) = self.depths.get(from) else {
            return false;
        };
        let Some(&to_depth) = self.depths.get(to) else {
            return false;
        };
        if from_depth == usize::MAX || to_depth == usize::MAX {
            return false;
        }
        let from_to_focus = self.distance_to_focus(from);
        let to_to_focus = self.distance_to_focus(to);
        if from_to_focus == usize::MAX || to_to_focus == usize::MAX {
            return false;
        }
        // Walk the edge in whichever direction moves away from the hovered
        // party, then check the remaining distance to the focus.
        from_depth + 1 + to_to_focus == self.path_length
            || to_depth + 1 + from_to_focus == self.path_length
    }

    pub fn touches_start(&self, from: usize, to: usize) -> bool {
        from == self.start || to == self.start
    }

    fn distance_to_focus(&self, index: usize) -> usize {
        // The on-path calculation stores the backwards distance in this vector
        // through `on_path`; keep a compact accessor for clarity.
        self.to_focus.get(index).copied().unwrap_or(usize::MAX)
    }
}

/// The parties reached by a walk, with the hop distance and the role that
/// pointed at each one.
#[derive(Clone, Debug, Default)]
pub struct GraphWalk {
    pub hops: BTreeMap<String, usize>,
    pub roles: BTreeMap<String, String>,
    /// The party each node was first reached through. The explorer groups a
    /// level's nodes by parent so related branches stay together.
    pub parents: BTreeMap<String, String>,
    pub truncated: bool,
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

    fn key(id: &str) -> String {
        node_key(PartyKind::Contact, id)
    }

    fn graph(edges: &[ApiRelationship], filters: &GraphFilters, focus: &str) -> RelationshipGraph {
        let catalog: Vec<ApiGraphNode> = {
            let mut ids: BTreeSet<&str> = BTreeSet::new();
            for edge in edges {
                ids.insert(&edge.from_id);
                ids.insert(&edge.to_id);
            }
            ids.into_iter()
                .map(|id| node(PartyKind::Contact, id, &format!("Contact {id}")))
                .collect()
        };
        let focus_key = key(focus);
        RelationshipGraph::build(&catalog, edges, filters, Some(&focus_key))
    }

    #[test]
    fn walk_stops_at_the_level_limit() {
        let edges = vec![edge("1", "a", "b"), edge("2", "b", "c")];
        let graph = graph(&edges, &GraphFilters::default(), "a");

        let one = graph.walk(&key("a"), 1, 60);
        assert!(one.hops.contains_key(&key("b")));
        assert!(!one.hops.contains_key(&key("c")));

        let two = graph.walk(&key("a"), 2, 60);
        assert!(two.hops.contains_key(&key("c")));
        assert_eq!(two.hops[&key("c")], 2);
    }

    #[test]
    fn ended_relationships_are_optional() {
        let mut ended = edge("1", "a", "b");
        ended.status = "ended".to_owned();
        let edges = vec![ended];

        let hidden = graph(&edges, &GraphFilters::default(), "a");
        assert_eq!(hidden.walk(&key("a"), 4, 60).hops.len(), 1);

        let shown = GraphFilters {
            show_ended: true,
            ..GraphFilters::default()
        };
        let shown = graph(&edges, &shown, "a");
        assert_eq!(shown.walk(&key("a"), 4, 60).hops.len(), 2);
    }

    #[test]
    fn kind_and_role_filters_apply_to_edges() {
        let mut org_edge = edge("1", "a", "org");
        org_edge.to_kind = PartyKind::Organisation;
        org_edge.to_role = "provider".to_owned();
        let edges = vec![edge("2", "a", "b"), org_edge];

        let no_contacts = GraphFilters {
            show_contacts: false,
            ..GraphFilters::default()
        };
        let graph_no_contacts = graph(&edges, &no_contacts, "a");
        assert!(
            graph_no_contacts
                .direct_edges(&node_key(PartyKind::Contact, "a"))
                .iter()
                .all(|edge| edge.to_kind == PartyKind::Organisation)
        );

        let role = GraphFilters {
            role: Some("provider".to_owned()),
            ..GraphFilters::default()
        };
        let graph_role = graph(&edges, &role, "a");
        assert_eq!(
            graph_role
                .direct_edges(&node_key(PartyKind::Contact, "a"))
                .len(),
            1
        );
    }

    #[test]
    fn a_self_relationship_is_listed_once() {
        let edges = vec![edge("1", "a", "a")];
        let graph = graph(&edges, &GraphFilters::default(), "a");
        assert_eq!(
            graph.direct_edges(&node_key(PartyKind::Contact, "a")).len(),
            1
        );
    }

    #[test]
    fn duplicate_edges_do_not_duplicate_parties() {
        let edges = vec![
            edge("1", "a", "b"),
            edge("2", "a", "b"),
            edge("3", "b", "a"),
        ];
        let graph = graph(&edges, &GraphFilters::default(), "a");
        let walk = graph.walk(&key("a"), 2, 60);
        assert_eq!(walk.hops.len(), 2);
    }

    #[test]
    fn the_walk_is_capped() {
        let mut edges = Vec::new();
        for index in 0..70 {
            edges.push(edge(&format!("{index}"), "focus", &format!("n{index}")));
        }
        let graph = graph(&edges, &GraphFilters::default(), "focus");
        let walk = graph.walk(&key("focus"), 1, 60);
        assert_eq!(walk.hops.len(), 60);
        assert!(walk.truncated);
    }

    #[test]
    fn organisations_are_shown_but_not_expanded() {
        let catalog = vec![
            node(PartyKind::Contact, "a", "Alice"),
            node(PartyKind::Contact, "b", "Bob"),
            node(PartyKind::Organisation, "org", "Care Co"),
            node(PartyKind::Contact, "c", "Carol"),
        ];
        let mut to_org = edge("1", "a", "org");
        to_org.to_kind = PartyKind::Organisation;
        to_org.to_name = "Care Co".to_owned();
        let mut from_org = edge("2", "org", "c");
        from_org.from_kind = PartyKind::Organisation;
        from_org.from_name = "Care Co".to_owned();
        let edges = vec![to_org, from_org, edge("3", "a", "b")];
        let graph =
            RelationshipGraph::build(&catalog, &edges, &GraphFilters::default(), Some(&key("a")));

        let empty = BTreeSet::new();
        let policy = WalkPolicy {
            max_nodes: 60,
            skip_hubs: true,
            hub_degree: HUB_DEGREE,
            expanded: &empty,
        };
        let walk = graph.walk_with(&key("a"), 2, &policy);
        assert!(
            walk.hops
                .contains_key(&node_key(PartyKind::Organisation, "org"))
        );
        assert!(!walk.hops.contains_key(&key("c")));

        // Explicit expansion, and focusing the organisation, both walk through.
        let mut expanded = BTreeSet::new();
        expanded.insert(node_key(PartyKind::Organisation, "org"));
        let policy = WalkPolicy {
            expanded: &expanded,
            ..policy
        };
        assert!(
            graph
                .walk_with(&key("a"), 2, &policy)
                .hops
                .contains_key(&key("c"))
        );

        let empty = BTreeSet::new();
        let policy = WalkPolicy {
            expanded: &empty,
            ..policy
        };
        assert!(
            graph
                .walk_with(&node_key(PartyKind::Organisation, "org"), 1, &policy)
                .hops
                .contains_key(&key("c"))
        );
    }

    #[test]
    fn a_well_connected_contact_is_not_expanded() {
        let mut catalog = vec![node(PartyKind::Contact, "hub", "Hub")];
        let mut edges = vec![edge("0", "a", "hub")];
        for index in 0..40 {
            let id = format!("n{index}");
            catalog.push(node(PartyKind::Contact, &id, &format!("Node {index}")));
            edges.push(edge(&format!("{index}"), "hub", &id));
        }
        let graph =
            RelationshipGraph::build(&catalog, &edges, &GraphFilters::default(), Some(&key("a")));

        let empty = BTreeSet::new();
        let policy = WalkPolicy {
            max_nodes: 60,
            skip_hubs: true,
            hub_degree: HUB_DEGREE,
            expanded: &empty,
        };
        let walk = graph.walk_with(&key("a"), 3, &policy);
        assert!(walk.hops.contains_key(&key("hub")));
        assert!(!walk.hops.contains_key(&key("n0")));
        assert_eq!(graph.degree(&key("hub")), 41);
    }

    #[test]
    fn neighbourhood_depths_follow_a_chain() {
        let depths = neighbourhood_depths(4, [(0, 1), (1, 2)], 0);
        assert_eq!(depths[0], 0);
        assert_eq!(depths[1], 1);
        assert_eq!(depths[2], 2);
        assert_eq!(depths[3], usize::MAX);
    }

    #[test]
    fn a_focus_trace_marks_only_the_route_back() {
        let edges = [(0, 1), (1, 2), (2, 3), (1, 4)];
        let trace = FocusTrace::build(5, edges, 3, 0);
        assert_eq!(trace.path_length(), 3);
        assert!(trace.is_on_path(3));
        assert!(trace.is_on_path(2));
        assert!(trace.is_on_path(1));
        assert!(trace.is_on_path(0));
        assert!(!trace.is_on_path(4));

        assert!(trace.edge_on_path(3, 2));
        assert!(trace.edge_on_path(2, 1));
        assert!(trace.edge_on_path(1, 0));
        assert!(!trace.edge_on_path(1, 4));

        assert!(trace.is_direct(2), "the first step is a direct link");
        assert!(!trace.is_direct(1));
        assert!(
            !trace.is_direct(4),
            "a branch beyond the first hop is not direct"
        );
        assert!(trace.touches_start(3, 2));
        assert!(!trace.touches_start(2, 1));
    }

    #[test]
    fn a_focus_trace_keeps_equally_short_routes() {
        // 3 -> 2 -> 0 and 3 -> 1 -> 0 are both two steps.
        let edges = [(3, 2), (2, 0), (3, 1), (1, 0), (2, 4)];
        let trace = FocusTrace::build(5, edges, 3, 0);
        assert_eq!(trace.path_length(), 2);
        assert!(trace.edge_on_path(3, 2));
        assert!(trace.edge_on_path(3, 1));
        assert!(trace.edge_on_path(2, 0));
        assert!(trace.edge_on_path(1, 0));
        assert!(!trace.edge_on_path(2, 4));
        assert!(!trace.is_on_path(4));
        assert!(!trace.is_direct(4));
    }

    #[test]
    fn a_focus_trace_from_the_focus_has_no_route() {
        let edges = [(0, 1), (1, 2)];
        let trace = FocusTrace::build(3, edges, 0, 0);
        assert_eq!(trace.path_length(), 0);
        assert!(trace.is_on_path(0));
        assert!(!trace.edge_on_path(0, 1));
        assert!(trace.touches_start(0, 1));
        assert!(trace.is_direct(1));
    }

    #[test]
    fn direct_edges_report_both_sides() {
        let edges = vec![edge("1", "a", "b")];
        let graph = graph(&edges, &GraphFilters::default(), "a");
        let from_a = graph.direct_edges(&key("a"));
        let from_b = graph.direct_edges(&key("b"));
        assert_eq!(from_a.len(), 1);
        assert_eq!(from_b.len(), 1);
        assert_eq!(from_a[0].id, from_b[0].id);
    }
}
