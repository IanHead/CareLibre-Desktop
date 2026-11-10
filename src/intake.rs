//! Intake draft model. The wizard UI and the local cache share these types.

use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{contact::PartyKind, store::ContactKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IntakeKind {
    Client,
    Worker,
    Other,
}

impl IntakeKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Client => "Client intake",
            Self::Worker => "Worker intake",
            Self::Other => "Other contact intake",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::Client => "Client",
            Self::Worker => "Worker",
            Self::Other => "Other contact",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Client => "👤",
            Self::Worker => "👷",
            Self::Other => "👥",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Client => {
                "Onboard a participant: personal details, health needs, support network and risks."
            }
            Self::Worker => {
                "Onboard a support worker: contact details, position, employment terms and compliance."
            }
            Self::Other => {
                "Add a support coordinator, family member, GP or other contact to the directory."
            }
        }
    }

    pub fn contact_kind(self) -> ContactKind {
        match self {
            Self::Client => ContactKind::Client,
            Self::Worker => ContactKind::Worker,
            Self::Other => ContactKind::Other,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IntakeStatus {
    Draft,
    Completed,
}

/// A relationship selected during intake. The intake contact does not exist on
/// the server until the wizard is completed, so the target is held here and
/// linked once the contact has been created.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntakeRelationship {
    pub subject_role: String,
    pub linked_role: String,
    pub kind: PartyKind,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub notes: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntakeDraft {
    pub id: String,
    pub kind: IntakeKind,
    pub status: IntakeStatus,
    pub current_step: usize,
    pub fields: BTreeMap<String, String>,
    #[serde(default)]
    pub relationships: BTreeMap<String, IntakeRelationship>,
    pub created_at: i64,
    pub updated_at: i64,
    pub contact_id: Option<String>,
}

impl IntakeDraft {
    pub fn new(kind: IntakeKind) -> Self {
        let now = now_ms();
        Self {
            id: uuid::Uuid::now_v7().to_string(),
            kind,
            status: IntakeStatus::Draft,
            current_step: 0,
            fields: BTreeMap::new(),
            relationships: BTreeMap::new(),
            created_at: now,
            updated_at: now,
            contact_id: None,
        }
    }

    pub fn get(&self, key: &str) -> &str {
        self.fields.get(key).map(String::as_str).unwrap_or("")
    }

    pub fn set(&mut self, key: &str, value: String) {
        self.fields.insert(key.to_owned(), value);
        self.updated_at = now_ms();
    }

    pub fn is_checked(&self, key: &str) -> bool {
        self.get(key) == "true"
    }

    pub fn relationship(&self, key: &str) -> Option<&IntakeRelationship> {
        self.relationships.get(key)
    }

    pub fn set_relationship(&mut self, key: &str, relationship: IntakeRelationship) {
        self.relationships.insert(key.to_owned(), relationship);
        self.updated_at = now_ms();
    }

    pub fn remove_relationship(&mut self, key: &str) -> Option<IntakeRelationship> {
        let removed = self.relationships.remove(key);
        if removed.is_some() {
            self.updated_at = now_ms();
        }
        removed
    }

    pub fn display_name(&self) -> String {
        let first = self.get("first_name").trim();
        let last = self.get("last_name").trim();
        let name = match (first.is_empty(), last.is_empty()) {
            (false, false) => format!("{first} {last}"),
            (false, true) => first.to_owned(),
            (true, false) => last.to_owned(),
            (true, true) => self.get("organisation").trim().to_owned(),
        };

        if name.is_empty() {
            format!("New {}", self.kind.short_label().to_lowercase())
        } else {
            name
        }
    }
}

#[derive(Clone, Debug)]
pub struct IntakeSummary {
    pub id: String,
    pub kind: IntakeKind,
    pub name: String,
    pub current_step: usize,
    pub updated_at: i64,
}

#[derive(Clone, Debug)]
pub struct CompletedIntake {
    pub contact_id: String,
    pub name: String,
    pub kind: IntakeKind,
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
