//! Full contact records, including profile data and custom-field metadata.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::store::{ContactKind, ContactRow};

/// Contact summary as returned by the server.
#[derive(Clone, Debug, Deserialize)]
pub struct ApiContact {
    pub id: String,
    pub display_name: String,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub preferred_name: Option<String>,
    pub pronouns: Option<String>,
    pub date_of_birth: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub notes: Option<String>,
    pub is_active: bool,
    pub details: Value,
    pub role: String,
    pub updated_at: String,
    /// Set when a search matched one of this contact's relationship roles.
    #[serde(default)]
    pub relationship_match: Option<ApiRelationshipMatch>,
}

/// The relationship role a search matched, and who it is held for.
#[derive(Clone, Debug, Deserialize)]
pub struct ApiRelationshipMatch {
    pub role: String,
    pub with: Vec<String>,
    pub total: usize,
}

impl ApiRelationshipMatch {
    /// "Support Coordinator for Ian Client, Mia Client +3".
    pub fn summary(&self) -> String {
        let mut text = format!(
            "{} for {}",
            relationship_role_label(&self.role),
            self.with.join(", ")
        );
        let more = self.total.saturating_sub(self.with.len());
        if more > 0 {
            text.push_str(&format!(" +{more}"));
        }
        text
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub struct ApiCounts {
    pub all: usize,
    pub clients: usize,
    pub workers: usize,
    pub dual: usize,
    pub other: usize,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiCursor {
    pub name: String,
    pub id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiContactList {
    pub items: Vec<ApiContact>,
    pub total: usize,
    pub counts: ApiCounts,
    pub next: Option<ApiCursor>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiCustomField {
    pub id: String,
    pub key: String,
    pub label: String,
    pub help_text: Option<String>,
    pub data_type: String,
    pub is_required: bool,
    pub options: Option<Vec<String>>,
    pub sort_order: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiContactDetail {
    pub contact: ApiContact,
    pub custom_fields: Vec<ApiCustomField>,
    pub custom_values: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ApiContactWrite {
    pub display_name: String,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub preferred_name: Option<String>,
    pub pronouns: Option<String>,
    pub date_of_birth: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub notes: Option<String>,
    pub details: Value,
    pub is_active: bool,
    pub custom_fields: BTreeMap<String, Value>,
}

pub fn kind_from_role(role: &str) -> ContactKind {
    match role {
        "client" => ContactKind::Client,
        "worker" => ContactKind::Worker,
        "dual" => ContactKind::Dual,
        _ => ContactKind::Other,
    }
}

pub fn row_from_api(contact: &ApiContact) -> ContactRow {
    let division = contact
        .details
        .get("division")
        .and_then(Value::as_str)
        .or_else(|| contact.details.get("funding").and_then(Value::as_str))
        .map(|value| {
            let mut chars = value.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        });

    let status = if !contact.is_active {
        "Inactive".to_owned()
    } else {
        contact
            .details
            .get("contact_status")
            .and_then(Value::as_str)
            .unwrap_or("Active")
            .to_owned()
    };

    ContactRow {
        id: contact.id.clone(),
        name: contact.display_name.clone(),
        kind: kind_from_role(&contact.role),
        email: contact.email.clone(),
        phone: contact.phone.clone(),
        division,
        status,
        relationship: contact
            .relationship_match
            .as_ref()
            .map(ApiRelationshipMatch::summary),
    }
}

/// Local editor model retained for screenshots/tests.
#[derive(Clone, Debug)]
pub struct ContactRecord {
    pub summary: ContactRow,
    pub profile: BTreeMap<String, String>,
    pub custom_fields: Vec<CustomFieldDef>,
    /// Values keyed by custom field id.
    pub custom_values: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct CustomFieldDef {
    pub id: String,
    pub key: String,
    pub label: String,
    pub field_type: String,
    pub options: Vec<String>,
    pub section: Option<String>,
    pub required: bool,
    pub sort_order: i64,
}

#[derive(Clone, Debug)]
pub struct ContactUpdate {
    pub id: String,
    pub name: String,
    pub kind: ContactKind,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub division: Option<String>,
    pub status: String,
    pub profile: BTreeMap<String, String>,
    /// Values keyed by custom field id.
    pub custom_values: BTreeMap<String, String>,
}

impl ContactRecord {
    pub fn value(&self, key: &str) -> &str {
        self.profile.get(key).map(String::as_str).unwrap_or("")
    }

    pub fn custom_value(&self, field_id: &str) -> &str {
        self.custom_values
            .get(field_id)
            .map(String::as_str)
            .unwrap_or("")
    }
}

// Relationship graph API ---------------------------------------------------

/// The kind of party at one end of a relationship.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PartyKind {
    Contact,
    Organisation,
}

impl PartyKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Contact => "Contact",
            Self::Organisation => "Organisation",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Contact => "contact",
            Self::Organisation => "organisation",
        }
    }
}

/// One two-sided relationship, with a role for each endpoint.
#[derive(Clone, Debug, Deserialize)]
pub struct ApiRelationship {
    pub id: String,
    pub from_kind: PartyKind,
    pub from_id: String,
    pub from_name: String,
    pub from_role: String,
    pub to_kind: PartyKind,
    pub to_id: String,
    pub to_name: String,
    pub to_role: String,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    /// Why the relationship ends; set whenever `end_date` is.
    #[serde(default)]
    pub end_reason: Option<String>,
    /// `current`, `upcoming` or `ended`.
    #[serde(default = "current_status")]
    pub status: String,
    /// The relationship this one replaced through a transfer or role change.
    #[serde(default)]
    pub previous_id: Option<String>,
    pub notes: Option<String>,
    pub updated_at: String,
}

fn current_status() -> String {
    "current".to_owned()
}

impl ApiRelationship {
    pub fn is_ended(&self) -> bool {
        self.status == "ended"
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiRelationshipList {
    pub items: Vec<ApiRelationship>,
    pub total: usize,
}

/// A node in the relationship graph, used as the party lookup catalogue.
#[derive(Clone, Debug, Deserialize)]
pub struct ApiGraphNode {
    pub kind: PartyKind,
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub degree: usize,
    #[serde(default)]
    pub depth: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiRelationshipGraph {
    pub nodes: Vec<ApiGraphNode>,
    pub edges: Vec<ApiRelationship>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ApiRelationshipWrite {
    pub from_kind: PartyKind,
    pub from_id: String,
    pub from_role: String,
    pub to_kind: PartyKind,
    pub to_id: String,
    pub to_role: String,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub notes: Option<String>,
}

/// Render a stored role (`emergency_contact`) for the UI.
pub fn relationship_role_label(role: &str) -> String {
    role.split(['_', '-', ' '])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// Relationship lifecycle -----------------------------------------------------

/// Why a relationship ends, as `(server key, label)`. Required with any end date.
pub const END_REASONS: [(&str, &str); 8] = [
    ("offboarded", "Offboarded or resigned"),
    ("moved_away", "Moved away or left the area"),
    ("plan_ended", "Plan or funding ended"),
    ("deceased", "Deceased"),
    ("changed_provider", "Changed provider"),
    ("circumstances_changed", "Circumstances changed"),
    ("incident", "Incident"),
    ("other", "Other"),
];

pub fn end_reason_label(key: &str) -> &str {
    END_REASONS
        .iter()
        .find(|(value, _)| *value == key)
        .map_or(key, |(_, label)| label)
}

/// One change to a relationship, sent to the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelationshipChange {
    /// Set an end date (past, today or future) with a reason.
    End {
        end_date: String,
        reason: String,
        note: String,
    },
    /// Hand one side of the relationship to a different party from a date.
    Transfer {
        effective_date: String,
        /// `from` or `to`: the side being replaced.
        replace: String,
        new_kind: PartyKind,
        new_id: String,
        reason: String,
        note: String,
    },
    /// Continue the relationship in new roles from a date.
    ChangeRole {
        effective_date: String,
        from_role: String,
        to_role: String,
        reason: String,
        note: String,
    },
    /// Fix a mistake in place. `note` explains what was wrong.
    Correct {
        relationship: ApiRelationshipCorrection,
        note: String,
    },
    /// Hide a relationship recorded in error.
    Remove { reason: String },
}

/// The full set of corrected values for a relationship.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ApiRelationshipCorrection {
    pub from_kind: PartyKind,
    pub from_id: String,
    pub from_role: String,
    pub to_kind: PartyKind,
    pub to_id: String,
    pub to_role: String,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub end_reason: Option<String>,
    pub notes: Option<String>,
}

/// One entry in a relationship's history.
#[derive(Clone, Debug, Deserialize)]
pub struct ApiRelationshipEvent {
    pub id: String,
    pub relationship_id: String,
    pub action: String,
    pub effective_date: Option<String>,
    pub reason: Option<String>,
    pub note: Option<String>,
    #[serde(default)]
    pub note_hidden: bool,
    pub actor_name: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiRelationshipHistory {
    pub items: Vec<ApiRelationshipEvent>,
}

/// Past-tense label for a history action.
pub fn history_action_label(action: &str) -> &str {
    match action {
        "added" => "Added",
        "ended" => "Ended",
        "transferred" => "Transferred",
        "role_changed" => "Role changed",
        "corrected" => "Corrected",
        "removed" => "Removed",
        other => other,
    }
}

// Relationship type catalogue -------------------------------------------------

/// An allowed role pair from the organisation's catalogue.
#[derive(Clone, Debug, Deserialize)]
pub struct ApiRelationshipType {
    pub id: String,
    pub from_role: String,
    pub to_role: String,
    pub category: String,
    pub is_active: bool,
    #[serde(default)]
    pub in_use: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiRelationshipTypeList {
    pub items: Vec<ApiRelationshipType>,
}

/// One way to read a catalogue pair: this record holds `subject_role` and the
/// linked record holds `linked_role`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleOption {
    pub subject_role: String,
    pub linked_role: String,
    pub category: String,
}

impl RoleOption {
    pub fn new(subject_role: &str, linked_role: &str, category: &str) -> Self {
        Self {
            subject_role: subject_role.to_owned(),
            linked_role: linked_role.to_owned(),
            category: category.to_owned(),
        }
    }

    /// The other record's role, which is what people look for in a list:
    /// "General Practitioner".
    pub fn label(&self) -> String {
        relationship_role_label(&self.linked_role)
    }

    /// "General Practitioner (this record: Patient)".
    pub fn describe(&self) -> String {
        format!(
            "{} (this record: {})",
            relationship_role_label(&self.linked_role),
            relationship_role_label(&self.subject_role)
        )
    }

    pub fn matches(&self, search: &str) -> bool {
        let search = search.trim().to_lowercase();
        search.is_empty()
            || self.describe().to_lowercase().contains(&search)
            || self.subject_role.contains(&search.replace(' ', "_"))
            || self.linked_role.contains(&search.replace(' ', "_"))
    }
}

/// Every active pair, read both ways round, grouped by category and sorted by
/// the other record's role.
pub fn role_options(types: &[ApiRelationshipType]) -> Vec<RoleOption> {
    let mut options = Vec::new();
    for item in types.iter().filter(|item| item.is_active) {
        options.push(RoleOption::new(
            &item.from_role,
            &item.to_role,
            &item.category,
        ));
        if item.from_role != item.to_role {
            options.push(RoleOption::new(
                &item.to_role,
                &item.from_role,
                &item.category,
            ));
        }
    }
    options.sort_by_key(|option| {
        (
            category_rank(&option.category),
            option.label(),
            relationship_role_label(&option.subject_role),
        )
    });
    options
}

fn category_rank(category: &str) -> usize {
    ["care", "family", "health", "services", "work"]
        .iter()
        .position(|key| *key == category)
        .unwrap_or(5)
}

pub fn category_label(category: &str) -> &'static str {
    match category {
        "care" => "Care and support",
        "family" => "Family",
        "health" => "Health",
        "services" => "Services",
        "work" => "Work",
        _ => "Other",
    }
}

#[cfg(test)]
mod role_option_tests {
    use super::{ApiRelationshipType, role_options};

    fn pair(from: &str, to: &str, category: &str, active: bool) -> ApiRelationshipType {
        ApiRelationshipType {
            id: format!("{from}-{to}"),
            from_role: from.to_owned(),
            to_role: to.to_owned(),
            category: category.to_owned(),
            is_active: active,
            in_use: 0,
        }
    }

    #[test]
    fn each_pair_reads_both_ways_and_retired_pairs_are_hidden() {
        let options = role_options(&[
            pair("patient", "general_practitioner", "health", true),
            pair("cousin", "cousin", "family", true),
            pair("patient", "dentist", "health", false),
        ]);
        let described: Vec<String> = options.iter().map(|option| option.describe()).collect();
        assert_eq!(
            described,
            [
                "Cousin (this record: Cousin)",
                "General Practitioner (this record: Patient)",
                "Patient (this record: General Practitioner)",
            ],
            "family before health; a symmetric pair appears once; retired pairs are left out"
        );
        assert!(!options[1].matches("gp") && options[1].matches("general"));
        assert!(options[2].matches("Patient"));
    }
}
