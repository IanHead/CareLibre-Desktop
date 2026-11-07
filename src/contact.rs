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
    pub notes: Option<String>,
    pub updated_at: String,
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
