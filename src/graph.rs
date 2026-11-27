//! A shared model of the tenant relationship graph.
//!
//! The server returns every party and relationship for the signed-in
//! organisation. Both the on-record constellation and the relationship
//! explorer filter that snapshot and walk it from a focus party, so the
//! traversal lives here rather than in either view.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::contact::{ApiGraphNode, ApiRelationship, PartyKind};

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

    /// Breadth-first walk from `focus`, stopping at `levels` and `max_nodes`.
    pub fn walk(&self, focus: &str, levels: u8, max_nodes: usize) -> GraphWalk {
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
                if hops.len() >= max_nodes {
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
